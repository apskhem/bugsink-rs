//! Users and login sessions for the UI API: upstream's `auth_user` table and password hashes (so accounts carry over
//! in both directions), our own `bsrs_session` table (so sessions don't: switching sides means logging in again).
//!
//! CSRF: every unsafe request (POST, PUT, PATCH, DELETE) with an `Origin` header must come from our own origin, and,
//! once logged in, must carry the session's token in `X-CSRFToken` (returned by login and `GET /api/v1/me`). The
//! login itself has no token yet; it is covered by the `Origin` check and by requiring a JSON body, which a
//! cross-site form cannot send without a CORS preflight that the UI API never allows.

pub mod password;

use axum::Json;
use axum::extract::{FromRequestParts, State};
use axum::http::header::{COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use bugsink_db::PgPool;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use unicode_normalization::UnicodeNormalization;

use crate::api::{ApiError, ApiState};

const COOKIE_NAME: &str = "bsrs_sessionid";
const CSRF_HEADER: &str = "x-csrftoken";
/// Django's `SESSION_COOKIE_AGE` (two weeks); like Django's default, not extended by use.
const SESSION_AGE: Duration = Duration::weeks(2);
/// Django's `AuthenticationForm` message for both "no such user" and "wrong password" (and inactive users, which
/// `ModelBackend` refuses to authenticate).
const INVALID_LOGIN: &str =
    "Please enter a correct username and password. Note that both fields may be case-sensitive.";

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub is_superuser: bool,
    pub is_staff: bool,
    pub send_email_alerts: bool,
    pub theme_preference: String,
    pub language: String,
}

const USER_COLUMNS: &str = "u.id, u.username, u.email, u.first_name, u.last_name, u.is_superuser, u.is_staff,
    u.send_email_alerts, u.theme_preference, u.language";

/// The logged-in user of a request. Extracting it answers 401 without a valid session and enforces CSRF on unsafe
/// methods, so a handler that takes it cannot forget either.
pub struct CurrentUser {
    pub user: User,
    key_hash: Vec<u8>,
    csrf_token: String,
}

impl FromRequestParts<ApiState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> Result<Self, ApiError> {
        let unsafe_method = !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE);
        if unsafe_method && !same_origin(&parts.headers, &state.origin) {
            return Err(ApiError::CsrfFailed);
        }
        let token = session_cookie(&parts.headers).ok_or(ApiError::Unauthorized)?;
        let key_hash = sha256(token.as_bytes());
        // A session ends when it expires, when its user is deactivated or deleted, and when the password changes (on
        // either side), like Django's session auth hash.
        let row: Option<SessionRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {USER_COLUMNS}, s.csrf_token
             FROM bsrs_session s JOIN auth_user u ON u.id = s.user_id
             WHERE s.key_hash = $1 AND s.expires_at > now() AND u.is_active
               AND s.auth_hash = sha256(convert_to(u.password, 'UTF8'))"
        )))
        .bind(&key_hash)
        .fetch_optional(&state.pool)
        .await?;
        let SessionRow { user, csrf_token } = row.ok_or(ApiError::Unauthorized)?;
        if unsafe_method {
            let sent = parts.headers.get(CSRF_HEADER).map(HeaderValue::as_bytes).unwrap_or_default();
            if !bool::from(sent.ct_eq(csrf_token.as_bytes())) {
                return Err(ApiError::CsrfFailed);
            }
        }
        Ok(Self { user, key_hash, csrf_token })
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    #[sqlx(flatten)]
    user: User,
    csrf_token: String,
}

/// What the frontend knows about the logged-in user, including the token for unsafe requests.
#[derive(Serialize)]
pub struct Me {
    #[serde(flatten)]
    user: User,
    csrf_token: String,
}

pub async fn me(current: CurrentUser) -> Json<Me> {
    Json(Me { user: current.user, csrf_token: current.csrf_token })
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

/// Django's `LoginView` with `ModelBackend`: exact username match, active users only, password rehashed when its
/// parameters are outdated, `last_login` updated.
pub async fn login(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    if !same_origin(&headers, &state.origin) {
        return Err(ApiError::CsrfFailed);
    }
    let row: Option<(i64, String, bool)> =
        sqlx::query_as("SELECT id, password, is_active FROM auth_user WHERE username = $1")
            .bind(&req.username)
            .fetch_optional(&state.pool)
            .await?;

    let password = req.password;
    let (user_id, encoded, is_active) = match row {
        Some(row) => row,
        None => {
            // Like Django: hash anyway, so that response times don't tell which usernames exist.
            tokio::task::spawn_blocking(move || password::make(&password)).await.map_err(ApiError::internal)?;
            return Err(ApiError::BadRequest(INVALID_LOGIN.into()));
        }
    };
    let (verified, password) = tokio::task::spawn_blocking(move || (password::verify(&password, &encoded), password))
        .await
        .map_err(ApiError::internal)?;
    if !verified.matches || !is_active {
        return Err(ApiError::BadRequest(INVALID_LOGIN.into()));
    }

    let new_hash = if verified.must_update {
        Some(tokio::task::spawn_blocking(move || password::make(&password)).await.map_err(ApiError::internal)?)
    } else {
        None
    };

    let token = random_hex(32);
    let csrf_token = random_hex(32);
    let mut tx = state.pool.begin().await?;
    if let Some(new_hash) = &new_hash {
        sqlx::query("UPDATE auth_user SET password = $1 WHERE id = $2")
            .bind(new_hash)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }
    let user: User = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE auth_user u SET last_login = now() WHERE u.id = $1 RETURNING {USER_COLUMNS}"
    )))
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if let Some(old) = session_cookie(&headers) {
        sqlx::query("DELETE FROM bsrs_session WHERE key_hash = $1")
            .bind(sha256(old.as_bytes()))
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM bsrs_session WHERE expires_at <= now()").execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO bsrs_session (key_hash, user_id, csrf_token, auth_hash, expires_at)
         SELECT $1, id, $2, sha256(convert_to(password, 'UTF8')), $3 FROM auth_user WHERE id = $4",
    )
    .bind(sha256(token.as_bytes()))
    .bind(&csrf_token)
    .bind(Utc::now() + SESSION_AGE)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let cookie = set_cookie(&token, SESSION_AGE.num_seconds(), state.secure_cookie);
    Ok(([(SET_COOKIE, cookie)], Json(Me { user, csrf_token })).into_response())
}

pub async fn logout(State(state): State<ApiState>, current: CurrentUser) -> Result<Response, ApiError> {
    sqlx::query("DELETE FROM bsrs_session WHERE key_hash = $1").bind(&current.key_hash).execute(&state.pool).await?;
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, set_cookie("", 0, state.secure_cookie))]).into_response())
}

/// Upstream's `CREATE_SUPERUSER` / `User.objects.create_superuser(username=email, email=email, password=...)`, with
/// the model defaults spelled out (Django's defaults are not database defaults).
pub async fn create_superuser(pool: &PgPool, email: &str, raw_password: &str) -> anyhow::Result<i64> {
    let username: String = email.nfkc().collect();
    let email = normalize_email(email);
    let raw_password = raw_password.to_owned();
    let encoded = tokio::task::spawn_blocking(move || password::make(&raw_password)).await?;
    let id = sqlx::query_scalar(
        "INSERT INTO auth_user (password, last_login, is_superuser, username, first_name, last_name, email, is_staff,
             is_active, date_joined, send_email_alerts, theme_preference, language)
         VALUES ($1, NULL, true, $2, '', '', $3, true, true, now(), true, 'system', 'auto')
         RETURNING id",
    )
    .bind(encoded)
    .bind(username)
    .bind(email)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Django's `BaseUserManager.normalize_email`: lowercase the domain part.
fn normalize_email(email: &str) -> String {
    match email.rsplit_once('@') {
        Some((local, domain)) => format!("{local}@{}", domain.to_lowercase()),
        None => email.to_owned(),
    }
}

/// No `Origin` (same-origin GETs, non-browser clients) passes; a browser sends one on every cross-origin POST. It must
/// be `BASE_URL`'s origin or match the `Host` the request was sent to (a dev server proxying to us, a second name).
fn same_origin(headers: &HeaderMap, own_origin: &str) -> bool {
    let Some(origin) = headers.get(ORIGIN) else { return true };
    let Ok(origin) = origin.to_str() else { return false };
    if origin.eq_ignore_ascii_case(own_origin) {
        return true;
    }
    let Ok(url) = url::Url::parse(origin) else { return false };
    let Some(host) = url.host_str() else { return false };
    let authority = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    headers.get(HOST).and_then(|h| h.to_str().ok()).is_some_and(|h| h.eq_ignore_ascii_case(&authority))
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, value)| *name == COOKIE_NAME && !value.is_empty())
        .map(|(_, value)| value.to_owned())
}

fn set_cookie(value: &str, max_age: i64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{COOKIE_NAME}={value}; Max-Age={max_age}; Path=/; HttpOnly; SameSite=Lax{secure}"))
        .expect("cookie value is hex")
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::fill(&mut buf[..]);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn origin_check() {
        let own = "https://bugs.example.org";
        assert!(same_origin(&headers(&[]), own));
        assert!(same_origin(&headers(&[("origin", "https://bugs.example.org")]), own));
        assert!(same_origin(&headers(&[("origin", "http://localhost:5173"), ("host", "localhost:5173")]), own));
        assert!(!same_origin(&headers(&[("origin", "https://evil.example"), ("host", "bugs.example.org")]), own));
        assert!(!same_origin(&headers(&[("origin", "http://localhost:5174"), ("host", "localhost:5173")]), own));
        assert!(!same_origin(&headers(&[("origin", "null"), ("host", "bugs.example.org")]), own));
    }

    #[test]
    fn reads_session_cookie() {
        assert_eq!(session_cookie(&headers(&[("cookie", "a=1; bsrs_sessionid=abc; b=2")])).as_deref(), Some("abc"));
        let two_headers = headers(&[("cookie", "a=1"), ("cookie", "bsrs_sessionid=x")]);
        assert_eq!(session_cookie(&two_headers).as_deref(), Some("x"));
        assert_eq!(session_cookie(&headers(&[("cookie", "bsrs_sessionid=; sessionid=abc")])), None);
    }

    #[test]
    fn normalizes_email_like_django() {
        assert_eq!(normalize_email("Foo@Example.COM"), "Foo@example.com");
        assert_eq!(normalize_email("nodomain"), "nodomain");
    }
}
