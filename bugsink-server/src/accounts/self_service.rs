//! What users do for their own account, as upstream Bugsink 2.6.3's `users/views.py`: signing up (only with
//! `USER_REGISTRATION=CB_ANYBODY`; with `USER_REGISTRATION_VERIFY_EMAIL` the address is confirmed by email first),
//! confirming the address, asking for the confirmation again, asking for a password reset (setting it:
//! `accounts::reset_password`), preferences and changing the password. The emails (`send_confirm_email`,
//! `send_reset_email`) are jobs, with upstream's templates; their links use upstream's paths.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use super::validation::{self, UserAttributes};
use super::{CurrentUser, password, same_origin};
use crate::alerts::{AlertSettings, email, template};
use crate::api::{ApiError, ApiState};
use crate::grouping::py::strip;
use crate::memberships;
use crate::projects::FieldErrors;
use crate::tasks;

fn invalid(field: &'static str, message: impl Into<String>) -> ApiError {
    ApiError::Invalid(FieldErrors::from([(field, vec![message.into()])]))
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
        errors.insert("password", vec!["This field is required.".into()]);
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
    let encoded = tokio::task::spawn_blocking(move || password::make(&password)).await.map_err(ApiError::internal)?;
    let mut tx = state.pool.begin().await?;
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO auth_user (password, last_login, is_superuser, username, first_name, last_name, email, is_staff,
             is_active, date_joined, send_email_alerts, theme_preference, language)
         VALUES ($1, NULL, false, $2, '', '', $2, false, $3, now(), true, 'system', 'auto') RETURNING id",
    )
    .bind(encoded)
    .bind(&username)
    .bind(!verify)
    .fetch_one(&mut *tx)
    .await?;
    if !verify {
        return super::start_session(&state, tx, &headers, user_id).await;
    }
    let token = memberships::create_email_verification(&mut tx, user_id).await?;
    tasks::enqueue(&mut *tx, JOB_KIND, &AccountEmail { kind: Kind::Confirm, email: username.clone(), token })
        .await
        .map_err(ApiError::internal)?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "confirmation_sent_to": username }))).into_response())
}

/// The signup form's username: `UsernameField` (stripped, NFKC), `EmailValidator`, upstream's check for addresses
/// waiting for their confirmation, then the model's `UnicodeUsernameValidator`, length and uniqueness.
async fn clean_username(state: &ApiState, value: &str) -> sqlx::Result<Result<String, Vec<String>>> {
    let value = strip(value);
    if value.is_empty() {
        return Ok(Err(vec!["This field is required.".into()]));
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
    let mut errors = Vec::new();
    static USERNAME: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    if !USERNAME.get_or_init(|| Regex::new(r"^[\w.@+-]+\z").unwrap()).is_match(&username) {
        errors.push(
            "Enter a valid username. This value may contain only letters, numbers, and @/./+/-/_ characters.".into(),
        );
    }
    let len = username.chars().count();
    if len > 150 {
        errors.push(format!("Ensure this value has at most 150 characters (it has {len})."));
    }
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

/// `confirm_email` GET: whether the token is (still) valid.
pub async fn check_confirm_email(
    State(state): State<ApiState>,
    Path(token): Path<String>,
) -> Result<StatusCode, ApiError> {
    memberships::email_verification(&state.pool, &state.settings.users, &token).await?.ok_or(ApiError::NotFound)?;
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
        memberships::email_verification(&state.pool, &state.settings.users, &token).await?.ok_or(ApiError::NotFound)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE auth_user SET is_active = true WHERE id = $1").bind(user_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM users_emailverification WHERE id = $1").bind(verification_id).execute(&mut *tx).await?;
    super::start_session(&state, tx, &headers, user_id).await
}

/// `reset_password` GET: whether the token is (still) valid.
pub async fn check_reset_password(
    State(state): State<ApiState>,
    Path(token): Path<String>,
) -> Result<StatusCode, ApiError> {
    check_confirm_email(State(state), Path(token)).await
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
    let token = memberships::create_email_verification(&mut tx, user_id).await?;
    tasks::enqueue(&mut *tx, JOB_KIND, &AccountEmail { kind, email, token }).await.map_err(ApiError::internal)?;
    tx.commit().await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct PreferencesRequest {
    send_email_alerts: Option<bool>,
    theme_preference: Option<String>,
    language: Option<String>,
}

/// Upstream's `THEME_CHOICES` and `language_choices()` (its `LANGUAGES`).
const THEMES: [&str; 3] = ["system", "light", "dark"];
pub const LANGUAGES: [&str; 3] = ["auto", "en", "zh-hans"];

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
        errors.insert("old_password", vec!["This field is required.".into()]);
    } else {
        let verified = tokio::task::spawn_blocking(move || password::verify(&old, &encoded))
            .await
            .map_err(ApiError::internal)?;
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
    let encoded = tokio::task::spawn_blocking(move || password::make(&new)).await.map_err(ApiError::internal)?;
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

pub const JOB_KIND: &str = "account_email";

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Confirm,
    Reset,
}

#[derive(Serialize, Deserialize)]
struct AccountEmail {
    kind: Kind,
    email: String,
    token: String,
}

/// `send_confirm_email` / `send_reset_email`.
pub async fn handle_job(pool: bugsink_db::PgPool, payload: Value, settings: Arc<AlertSettings>) -> anyhow::Result<()> {
    let job: AccountEmail = serde_json::from_value(payload)?;
    let base_url = settings.base_url();
    let base_url_slash = format!("{base_url}/");
    let (subject, templates, url_key, url) = match job.kind {
        Kind::Confirm => (
            "Confirm your email address",
            (
                include_str!("../alerts/templates/confirm_email.html"),
                include_str!("../alerts/templates/confirm_email.txt"),
            ),
            "confirm_url",
            format!("{base_url}/accounts/confirm-email/{}/", job.token),
        ),
        Kind::Reset => (
            "Reset your password",
            (
                include_str!("../alerts/templates/reset_password_email.html"),
                include_str!("../alerts/templates/reset_password_email.txt"),
            ),
            "reset_url",
            format!("{base_url}/accounts/reset-password/{}/", job.token),
        ),
    };
    let ctx: template::Context = [
        ("site_title", settings.site_title.as_str()),
        ("base_url", base_url_slash.as_str()),
        (url_key, url.as_str()),
    ]
    .into_iter()
    .collect();
    email::send_rendered_email(&pool, &settings, subject, &job.email, templates, &ctx).await
}
