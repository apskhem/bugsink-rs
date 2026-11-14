//! What users do for their own account, as upstream Bugsink 2.6.3's `users/views.py`: signing up (only with
//! `USER_REGISTRATION=CB_ANYBODY`; with `USER_REGISTRATION_VERIFY_EMAIL` the address is confirmed by email first),
//! confirming the address, asking for the confirmation again, asking for a password reset (setting it:
//! `accounts::reset_password`), preferences and changing the password. The emails (`send_confirm_email`,
//! `send_reset_email`) are jobs, with upstream's templates; their links use upstream's paths and carry an
//! `EmailVerification` token (also what invited new users set their password with).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bugsink_db::{PgPool, Postgres, Transaction};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use unicode_normalization::UnicodeNormalization;

use super::validation::{self, UserAttributes};
use super::{CurrentUser, password, same_origin};
use crate::alerts::AlertSettings;
use crate::api::{ApiError, ApiState};
use crate::forms::{FieldErrors, REQUIRED};
use crate::mail::{self, template};
use crate::memberships::UserSettings;
use crate::py::strip;
use crate::tasks;

fn invalid(field: &'static str, message: impl Into<String>) -> ApiError {
    ApiError::Invalid(crate::forms::error(field, message))
}

#[derive(Serialize)]
pub struct AccountSettings {
    /// Whether anybody can sign up (`USER_REGISTRATION=CB_ANYBODY`).
    signup: bool,
    site_title: String,
}

/// What the login page needs to know.
pub async fn settings(State(state): State<ApiState>) -> Json<AccountSettings> {
    Json(AccountSettings {
        signup: state.settings.users.signup_allowed(),
        site_title: state.settings.alerts.site_title.clone(),
    })
}

#[derive(Deserialize)]
pub struct SignupRequest {
    email: Option<String>,
    password: Option<String>,
}

/// `signup` with upstream's `UserCreationForm` (the email address is the username): Django's username and email
/// rules, an address that is waiting for its confirmation, and the password validators.
pub async fn signup(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<SignupRequest>,
) -> Result<Response, ApiError> {
    if !same_origin(&headers, &state.origin) {
        return Err(ApiError::CsrfFailed);
    }
    if !state.settings.users.signup_allowed() {
        return Err(ApiError::NotFound);
    }
    let mut errors = FieldErrors::new();
    let username = match clean_username(&state, req.email.as_deref().unwrap_or("")).await? {
        Ok(username) => Some(username),
        Err(messages) => {
            errors.insert("email", messages);
            None
        }
    };
    let password = req.password.unwrap_or_default();
    if password.is_empty() {
        errors.insert("password", vec![REQUIRED.into()]);
    } else {
        // the instance has the username by then, but not yet the email (upstream sets it on save)
        let attributes = UserAttributes {
            username: username.as_deref().unwrap_or(""),
            first_name: "",
            last_name: "",
            email: "",
        };
        let password_errors = validation::password_errors(&password, &attributes);
        if !password_errors.is_empty() {
            errors.insert("password", password_errors);
        }
    }
    let Some(username) = username.filter(|_| errors.is_empty()) else {
        return Err(ApiError::Invalid(errors));
    };

    let verify = state.settings.users.user_registration_verify_email;
    let encoded = password::hash(&password).await.map_err(ApiError::internal)?;
    let mut tx = state.pool.begin().await?;
    let user_id = super::insert_user(&mut *tx, &encoded, &username, &username, !verify, false).await?;
    if !verify {
        return super::start_session(&state, tx, &headers, user_id).await;
    }
    enqueue_email(&mut tx, user_id, Kind::Confirm, &username, "").await.map_err(ApiError::internal)?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "confirmation_sent_to": username }))).into_response())
}

/// The signup form's username: `UsernameField` (stripped, NFKC), `EmailValidator`, upstream's check for addresses
/// waiting for their confirmation, then the model's `UnicodeUsernameValidator`, length and uniqueness.
async fn clean_username(state: &ApiState, value: &str) -> sqlx::Result<Result<String, Vec<String>>> {
    let value = strip(value);
    if value.is_empty() {
        return Ok(Err(vec![REQUIRED.into()]));
    }
    let username: String = if value.chars().count() > 150 { value.to_owned() } else { value.nfkc().collect() };
    if validation::clean_email(&username).is_err() {
        return Ok(Err(vec!["Enter a valid email address.".into()]));
    }
    let waiting: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM auth_user WHERE username = $1 AND NOT is_active)")
            .bind(&username)
            .fetch_one(&state.pool)
            .await?;
    if waiting {
        return Ok(Err(vec![
            "This email is already registered but not yet confirmed. Please check your email for the confirmation \
             link or request it again."
                .into(),
        ]));
    }
    let mut errors = validation::username_errors(&username);
    if errors.is_empty() {
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM auth_user WHERE username = $1)")
            .bind(&username)
            .fetch_one(&state.pool)
            .await?;
        if taken {
            errors.push("A user with that username already exists.".into());
        }
    }
    Ok(if errors.is_empty() { Ok(username) } else { Err(errors) })
}

/// `confirm_email` / `reset_password` GET: whether the token is (still) valid.
pub async fn check_token(
    State(state): State<ApiState>,
    Path(token): Path<String>,
) -> Result<StatusCode, ApiError> {
    email_verification(&state.pool, &state.settings.users, &token).await?.ok_or(ApiError::NotFound)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `confirm_email` POST: activates the user (the token proves the address) and logs them in.
pub async fn confirm_email(
    State(state): State<ApiState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !same_origin(&headers, &state.origin) {
        return Err(ApiError::CsrfFailed);
    }
    let (verification_id, user_id, _, _) =
        email_verification(&state.pool, &state.settings.users, &token).await?.ok_or(ApiError::NotFound)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE auth_user SET is_active = true WHERE id = $1").bind(user_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM users_emailverification WHERE id = $1").bind(verification_id).execute(&mut *tx).await?;
    super::start_session(&state, tx, &headers, user_id).await
}

#[derive(Deserialize)]
pub struct EmailRequest {
    email: Option<String>,
}

/// `resend_confirmation`: a new confirmation email for an address that isn't confirmed yet.
pub async fn resend_confirmation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<EmailRequest>,
) -> Result<StatusCode, ApiError> {
    if !same_origin(&headers, &state.origin) {
        return Err(ApiError::CsrfFailed);
    }
    let email = validation::clean_email(req.email.as_deref().unwrap_or("")).map_err(|m| invalid("email", m))?;
    let Some((user_id, is_active)): Option<(i64, bool)> =
        sqlx::query_as("SELECT id, is_active FROM auth_user WHERE username = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await?
    else {
        return Err(ApiError::NotFound); // upstream: a 500
    };
    if is_active {
        return Err(ApiError::NotFound); // "This email is already confirmed." (upstream: a 404 too)
    }
    send(&state, user_id, Kind::Confirm, email).await?;
    Ok(StatusCode::ACCEPTED)
}

/// `request_reset_password`: an email with a link to set a new password (upstream says when an address isn't
/// registered: signup tells anyway).
pub async fn request_reset_password(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<EmailRequest>,
) -> Result<StatusCode, ApiError> {
    if !same_origin(&headers, &state.origin) {
        return Err(ApiError::CsrfFailed);
    }
    let email = validation::clean_email(req.email.as_deref().unwrap_or("")).map_err(|m| invalid("email", m))?;
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM auth_user WHERE username = $1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await?;
    let user_id = user_id.ok_or_else(|| invalid("email", "This email is not registered."))?;
    send(&state, user_id, Kind::Reset, email).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn send(state: &ApiState, user_id: i64, kind: Kind, email: String) -> Result<(), ApiError> {
    let mut tx = state.pool.begin().await?;
    enqueue_email(&mut tx, user_id, kind, &email, "").await.map_err(ApiError::internal)?;
    tx.commit().await?;
    Ok(())
}

/// A fresh email verification for the user, and the job that emails its link.
async fn enqueue_email(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    kind: Kind,
    email: &str,
    reason: &str,
) -> anyhow::Result<()> {
    let token = create_email_verification(tx, user_id).await?;
    let job = AccountEmail { kind, email: email.to_owned(), token, reason: reason.to_owned() };
    tasks::enqueue(&mut **tx, &job).await
}

/// `EmailVerification.objects.create(user=user, email=user.username)` with `secrets.token_urlsafe()`.
pub(crate) async fn create_email_verification(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
) -> sqlx::Result<String> {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    sqlx::query(
        "INSERT INTO users_emailverification (email, token, created_at, user_id)
         SELECT username, $1, now(), id FROM auth_user WHERE id = $2",
    )
    .bind(&token)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(token)
}

/// Deletes email verifications older than `USER_REGISTRATION_VERIFY_EMAIL_EXPIRY` (upstream does this on every
/// request that looks one up), then finds the token's user: (verification id, user id, is_active, password).
pub(crate) async fn email_verification(
    pool: &PgPool,
    users: &UserSettings,
    token: &str,
) -> sqlx::Result<Option<(i64, i64, bool, String)>> {
    sqlx::query("DELETE FROM users_emailverification WHERE created_at < $1")
        .bind(Utc::now() - chrono::TimeDelta::seconds(users.user_registration_verify_email_expiry))
        .execute(pool)
        .await?;
    sqlx::query_as(
        "SELECT v.id, u.id, u.is_active, u.password FROM users_emailverification v JOIN auth_user u ON u.id = v.user_id
         WHERE v.token = $1",
    )
    .bind(token)
    .fetch_optional(pool)
    .await
}

#[derive(Deserialize)]
pub struct PreferencesRequest {
    send_email_alerts: Option<bool>,
    theme_preference: Option<String>,
    language: Option<String>,
}

/// Upstream's `THEME_CHOICES` and `language_choices()` (its `LANGUAGES`).
const THEMES: [&str; 3] = ["system", "light", "dark"];
const LANGUAGES: [&str; 3] = ["auto", "en", "zh-hans"];

/// `preferences`: what is given of email alerts, theme and language.
pub async fn update_preferences(
    State(state): State<ApiState>,
    current: CurrentUser,
    Json(req): Json<PreferencesRequest>,
) -> Result<StatusCode, ApiError> {
    let mut errors = FieldErrors::new();
    let choice = |errors: &mut FieldErrors, field: &'static str, value: &Option<String>, choices: &[&str]| {
        if let Some(v) = value
            && !choices.contains(&v.as_str())
        {
            errors.insert(field, vec![format!("Select a valid choice. {v} is not one of the available choices.")]);
        }
    };
    choice(&mut errors, "theme_preference", &req.theme_preference, &THEMES);
    choice(&mut errors, "language", &req.language, &LANGUAGES);
    if !errors.is_empty() {
        return Err(ApiError::Invalid(errors));
    }
    sqlx::query(
        "UPDATE auth_user SET send_email_alerts = coalesce($2, send_email_alerts),
             theme_preference = coalesce($3, theme_preference), language = coalesce($4, language)
         WHERE id = $1",
    )
    .bind(current.user.id)
    .bind(req.send_email_alerts)
    .bind(req.theme_preference)
    .bind(req.language)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    old_password: Option<String>,
    new_password: Option<String>,
}

/// `change_password` (Django's `PasswordChangeForm`): this session stays (`update_session_auth_hash`), the user's
/// other sessions end.
pub async fn change_password(
    State(state): State<ApiState>,
    current: CurrentUser,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    let user = &current.user;
    let mut errors = FieldErrors::new();
    let old = req.old_password.unwrap_or_default();
    let encoded: String = sqlx::query_scalar("SELECT password FROM auth_user WHERE id = $1")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?;
    if old.is_empty() {
        errors.insert("old_password", vec![REQUIRED.into()]);
    } else {
        let verified = password::check(&old, &encoded).await.map_err(ApiError::internal)?;
        if !verified.matches {
            errors.insert(
                "old_password",
                vec!["Your old password was entered incorrectly. Please enter it again.".into()],
            );
        }
    }
    let new = req.new_password.unwrap_or_default();
    let attributes = UserAttributes {
        username: &user.username,
        first_name: &user.first_name,
        last_name: &user.last_name,
        email: &user.email,
    };
    let new_errors = validation::password_errors(&new, &attributes);
    if !new_errors.is_empty() {
        errors.insert("new_password", new_errors);
    }
    if !errors.is_empty() {
        return Err(ApiError::Invalid(errors));
    }
    let encoded = password::hash(&new).await.map_err(ApiError::internal)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE auth_user SET password = $2 WHERE id = $1")
        .bind(user.id)
        .bind(encoded)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE bsrs_session s SET auth_hash = sha256(convert_to(u.password, 'UTF8'))
         FROM auth_user u WHERE u.id = s.user_id AND s.key_hash = $1",
    )
    .bind(&current.key_hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Confirm,
    Reset,
    Welcome,
}

#[derive(Serialize, Deserialize)]
pub struct AccountEmail {
    kind: Kind,
    email: String,
    token: String,
    /// Welcome emails: why they were added (the email's first line).
    #[serde(default)]
    reason: String,
}

impl tasks::Job for AccountEmail {
    const KIND: &str = "account_email";
}

/// Upstream's `send_welcome_email` command: an email to a user with a link to set their password.
pub async fn send_welcome_email(pool: &PgPool, email: &str, reason: &str) -> anyhow::Result<()> {
    let Some(user_id): Option<i64> = sqlx::query_scalar("SELECT id FROM auth_user WHERE email = $1")
        .bind(email)
        .fetch_optional(pool)
        .await?
    else {
        anyhow::bail!("no user with email address {email:?}");
    };
    let mut tx = pool.begin().await?;
    enqueue_email(&mut tx, user_id, Kind::Welcome, email, reason).await?;
    tx.commit().await?;
    Ok(())
}

/// `send_confirm_email` / `send_reset_email`.
pub async fn handle_job(pool: PgPool, job: AccountEmail, settings: Arc<AlertSettings>) -> anyhow::Result<()> {
    let base_url = settings.base_url();
    let base_url_slash = format!("{base_url}/");
    let (subject, templates, url_key, url) = match job.kind {
        Kind::Confirm => (
            "Confirm your email address",
            (
                include_str!("../mail/templates/confirm_email.html"),
                include_str!("../mail/templates/confirm_email.txt"),
            ),
            "confirm_url",
            format!("{base_url}/accounts/confirm-email/{}/", job.token),
        ),
        Kind::Reset => (
            "Reset your password",
            (
                include_str!("../mail/templates/reset_password_email.html"),
                include_str!("../mail/templates/reset_password_email.txt"),
            ),
            "reset_url",
            format!("{base_url}/accounts/reset-password/{}/", job.token),
        ),
        Kind::Welcome => (
            "Welcome to Bugsink",
            (
                include_str!("../mail/templates/welcome_email.html"),
                include_str!("../mail/templates/welcome_email.txt"),
            ),
            "reset_url",
            format!("{base_url}/accounts/reset-password/{}/", job.token),
        ),
    };
    let ctx: template::Context = [
        ("site_title", settings.site_title.as_str()),
        ("base_url", base_url_slash.as_str()),
        (url_key, url.as_str()),
        ("reason", job.reason.as_str()),
    ]
    .into_iter()
    .collect();
    mail::send_rendered_email(&pool, &settings, subject, &job.email, templates, &ctx).await
}
