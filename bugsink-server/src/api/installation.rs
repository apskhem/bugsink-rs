//! The installation: users admin, auth tokens, settings, counts (superusers, but tokens), system warnings, home.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, ApiState, created};
use crate::accounts::{CurrentUser, User};
use crate::installation::{self, AuthTokenForm, Deleted};

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/users", get(users))
        .route("/api/v1/users/{user_id}", patch(edit_user).delete(delete_user))
        .route("/api/v1/users/{user_id}/activate", post(activate))
        .route("/api/v1/users/{user_id}/deactivate", post(deactivate))
        .route("/api/v1/auth-tokens", get(auth_tokens).post(create_auth_token))
        .route("/api/v1/auth-tokens/new", get(new_auth_token))
        .route("/api/v1/auth-tokens/{token_id}", delete(revoke_auth_token))
        .route("/api/v1/counts", get(counts))
        .route("/api/v1/settings", get(settings))
        .route("/api/v1/system-warnings", get(system_warnings))
        .route("/api/v1/system-warnings/email/silence", post(silence_email_warning))
        .route("/api/v1/home", get(home))
        .route("/health/ready", get(|| async { "OK" }))
}

/// `user_passes_test(lambda u: u.is_superuser)`.
fn require_superuser(user: &User) -> Result<(), ApiError> {
    if user.is_superuser { Ok(()) } else { Err(ApiError::Forbidden("Superusers only".into())) }
}

async fn users(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
) -> ApiResult<Vec<installation::UserRow>> {
    require_superuser(&user)?;
    Ok(Json(installation::users(&state.pool).await?))
}

async fn set_active(state: &ApiState, user: &User, user_id: i64, active: bool) -> Result<StatusCode, ApiError> {
    require_superuser(user)?;
    if !installation::set_active(&state.pool, user_id, active).await? {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn activate(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(user_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    set_active(&state, &user, user_id, true).await
}

/// Their sessions end with it.
async fn deactivate(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(user_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    set_active(&state, &user, user_id, false).await
}

/// Only inactive users can be deleted.
async fn delete_user(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(user_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    require_superuser(&user)?;
    match installation::delete_user(&state.pool, user_id).await? {
        Deleted::Deleted => Ok(StatusCode::NO_CONTENT),
        Deleted::NoSuchUser => Err(ApiError::NotFound),
        Deleted::Active => Err(ApiError::Conflict("Cannot delete an active user; deactivate them first".into())),
    }
}

#[derive(Deserialize)]
struct EditUser {
    email: Option<String>,
}

/// The user's email address, which is also their username.
async fn edit_user(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(user_id): Path<i64>,
    Json(req): Json<EditUser>,
) -> Result<StatusCode, ApiError> {
    require_superuser(&user)?;
    installation::edit_user(&state.pool, user_id, req.email.as_deref().unwrap_or(""))
        .await?
        .ok_or(ApiError::NotFound)?
        .map_err(ApiError::Invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn auth_tokens(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
) -> ApiResult<Vec<installation::AuthToken>> {
    Ok(Json(installation::auth_tokens(&state.pool, &user).await?))
}

#[derive(Serialize)]
struct NewAuthToken {
    /// (capability, description)
    capabilities: Vec<(&'static str, &'static str)>,
    /// What a token can be limited to: (id, name, team name).
    projects: Vec<(i64, String, Option<String>)>,
    /// Only superusers choose between a personal token and a service token (theirs are always personal).
    can_choose_user_bound: bool,
}

async fn new_auth_token(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
) -> ApiResult<NewAuthToken> {
    Ok(Json(NewAuthToken {
        capabilities: installation::CAPABILITIES.to_vec(),
        projects: installation::token_projects(&state.pool, &user).await?,
        can_choose_user_bound: user.is_superuser,
    }))
}

async fn create_auth_token(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(form): Json<AuthTokenForm>,
) -> Result<Response, ApiError> {
    let id = installation::create_auth_token(&state.pool, &user, &form).await?.map_err(ApiError::Invalid)?;
    Ok(created(id))
}

async fn revoke_auth_token(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(token_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if !installation::revoke_auth_token(&state.pool, &user, token_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn counts(State(state): State<ApiState>, CurrentUser { user, .. }: CurrentUser) -> ApiResult<Value> {
    require_superuser(&user)?;
    Ok(Json(installation::counts(&state.pool).await?))
}

/// `settings_view`: the settings this installation runs with (bugsink-rs's, under upstream's names), secrets masked.
async fn settings(State(state): State<ApiState>, CurrentUser { user, .. }: CurrentUser) -> ApiResult<Value> {
    require_superuser(&user)?;
    let s = &state.settings;
    if s.users.minimize_information_exposure {
        return Err(ApiError::Forbidden("This view is disabled because MINIMIZE_INFORMATION_EXPOSURE=True".into()));
    }
    let e = &s.alerts.email;
    let w = &s.alerts.webhooks;
    let masked = |v: &Option<String>| if v.as_deref().is_some_and(|v| !v.is_empty()) { "********" } else { "" };
    let q = &s.quotas;
    let settings: serde_json::Map<String, Value> = [
        // upstream's order (`bugsink.app_settings`), then ours
        ("BASE_URL", json!(s.alerts.base_url())),
        ("SITE_TITLE", json!(s.alerts.site_title)),
        ("SINGLE_USER", json!(s.users.single_user)),
        ("USER_REGISTRATION", json!(s.users.user_registration)),
        ("USER_REGISTRATION_VERIFY_EMAIL", json!(s.users.user_registration_verify_email)),
        ("USER_REGISTRATION_VERIFY_EMAIL_EXPIRY", json!(s.users.user_registration_verify_email_expiry)),
        ("SINGLE_TEAM", json!(s.users.single_team)),
        ("TEAM_CREATION", json!(s.users.team_creation)),
        ("MAX_EVENTS_PER_PROJECT_PER_5_MINUTES", json!(q.max_events_per_project_per_5_minutes)),
        ("MAX_EVENTS_PER_PROJECT_PER_HOUR", json!(q.max_events_per_project_per_hour)),
        ("MAX_EVENTS_PER_PROJECT_PER_MONTH", json!(q.max_events_per_project_per_month)),
        ("MAX_EVENTS_PER_5_MINUTES", json!(q.max_events_per_5_minutes)),
        ("MAX_EVENTS_PER_HOUR", json!(q.max_events_per_hour)),
        ("MAX_EVENTS_PER_MONTH", json!(q.max_events_per_month)),
        ("MAX_EMAILS_PER_HOUR", json!(e.max_emails_per_hour.0)),
        ("MAX_EMAILS_PER_MONTH", json!(e.max_emails_per_month.0)),
        ("MAX_RETENTION_PER_PROJECT_EVENT_COUNT", json!(q.max_retention_per_project_event_count.0)),
        ("MAX_RETENTION_EVENT_COUNT", json!(q.max_retention_event_count.0)),
        ("MINIMIZE_INFORMATION_EXPOSURE", json!(s.users.minimize_information_exposure)),
        ("ALERTS_WEBHOOK_OUTBOUND_MODE", json!(w.alerts_webhook_outbound_mode.as_str())),
        ("ALERTS_WEBHOOK_ALLOW_LIST", json!(w.alerts_webhook_allow_list.entries)),
        ("ALERTS_WEBHOOK_DENY_LIST", json!(w.alerts_webhook_deny_list.entries)),
        ("ALERTS_WEBHOOK_DENY_NON_GLOBAL", json!(w.alerts_webhook_deny_non_global)),
        ("BEHIND_HTTPS_PROXY", json!(s.proxy.behind_https_proxy)),
        ("BEHIND_PLAIN_HTTP_PROXY", json!(s.proxy.behind_plain_http_proxy)),
        ("EMAIL_TIMEOUT", json!(e.email_timeout)),
        ("ALLOWED_HOSTS", json!(e.allowed_hosts)),
        ("USE_X_REAL_IP", json!(s.proxy.use_x_real_ip)),
        ("USE_X_FORWARDED_FOR", json!(s.proxy.use_x_forwarded_for)),
        ("X_FORWARDED_FOR_PROXY_COUNT", json!(s.proxy.x_forwarded_for_proxy_count)),
        ("TIME_ZONE", json!("UTC")),
        ("EMAIL_BACKEND", json!(e.email_backend)),
        ("EMAIL_HOST", json!(e.email_host)),
        ("EMAIL_HOST_USER", json!(masked(&e.email_host_user))),
        ("EMAIL_HOST_PASSWORD", json!(masked(&e.email_host_password))),
        ("EMAIL_PORT", json!(e.email_port)),
        ("EMAIL_USE_TLS", json!(e.email_use_tls)),
        ("EMAIL_USE_SSL", json!(e.email_use_ssl)),
        ("DEFAULT_FROM_EMAIL", json!(e.default_from_email)),
    ]
    .into_iter()
    .map(|(k, v): (&str, Value)| (k.to_owned(), v))
    .collect();
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "upstream_version": bugsink_db::UPSTREAM_VERSION,
        "settings": settings,
    })))
}

/// The banners every page shows.
async fn system_warnings(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
) -> ApiResult<Vec<installation::SystemWarning>> {
    let s = &state.settings;
    Ok(Json(installation::system_warnings(&state.pool, &s.alerts, &s.quotas, &user, Utc::now()).await?))
}

/// `silence_email_system_warning`.
async fn silence_email_warning(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<StatusCode, ApiError> {
    require_superuser(&user)?;
    installation::silence_email_warning(&state.pool).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Where `/` takes you: `{"go": "issues", "project_id"}`, `{"go": "projects"}` or `{"go": "teams"}`.
async fn home(State(state): State<ApiState>, CurrentUser { user, .. }: CurrentUser) -> ApiResult<Value> {
    Ok(Json(installation::home(&state.pool, &user, state.settings.users.single_team()).await?))
}
