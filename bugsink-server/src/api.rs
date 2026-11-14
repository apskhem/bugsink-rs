//! `/api/v1/...`: the JSON API the frontend is built on.
//!
//! This replaces upstream's server-rendered Django views, so its shape follows the pages in `frontend/README.md`
//! rather than upstream's DRF "canonical" API (which can be added later under `/api/canonical/0/`).
//!
//! Every endpoint except health and login needs a session (`accounts::CurrentUser`) and checks access like upstream's
//! views: superusers see everything; issues and events need an accepted membership of their project.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bugsink_db::PgPool;
use bugsink_db::models::{Event, EventSummary, IssueSummary, Project};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::accounts::{self, CurrentUser, User};

#[derive(Clone)]
pub struct ApiState {
    pub pool: PgPool,
    /// `BASE_URL`'s scheme://host[:port], for the CSRF origin check.
    pub origin: String,
    /// Session cookies are `Secure` when `BASE_URL` is https (upstream: `BEHIND_HTTPS_PROXY`).
    pub secure_cookie: bool,
}

impl ApiState {
    pub fn new(pool: PgPool, base_url: &str) -> anyhow::Result<Self> {
        let url = url::Url::parse(base_url).map_err(|e| anyhow::anyhow!("BASE_URL {base_url:?}: {e}"))?;
        Ok(Self { pool, origin: url.origin().ascii_serialization(), secure_cookie: url.scheme() == "https" })
    }
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/login", post(accounts::login))
        .route("/api/v1/auth/logout", post(accounts::logout))
        .route("/api/v1/me", get(accounts::me))
        .route("/api/v1/projects", get(list_projects))
        .route("/api/v1/projects/{project_id}/issues", get(list_issues))
        .route("/api/v1/issues/{issue_id}", get(get_issue))
        .route("/api/v1/issues/{issue_id}/events", get(list_events))
        .route("/api/v1/events/{event_id}", get(get_event))
        .with_state(state)
}

pub enum ApiError {
    BadRequest(String),
    Unauthorized,
    CsrfFailed,
    Forbidden,
    NotFound,
    Db(sqlx::Error),
    Internal(String),
}

impl ApiError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(detail) => (StatusCode::BAD_REQUEST, Json(json!({ "detail": detail }))).into_response(),
            Self::Unauthorized => {
                (StatusCode::UNAUTHORIZED, Json(json!({ "detail": "authentication required" }))).into_response()
            }
            Self::CsrfFailed => (StatusCode::FORBIDDEN, Json(json!({ "detail": "CSRF check failed" }))).into_response(),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                Json(json!({ "detail": "You don't have permission to access this project" })),
            )
                .into_response(),
            Self::NotFound => (StatusCode::NOT_FOUND, Json(json!({ "detail": "not found" }))).into_response(),
            Self::Db(e) => {
                tracing::error!(error = %e, "api query failed");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": "internal error" }))).into_response()
            }
            Self::Internal(e) => {
                tracing::error!(error = %e, "api request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": "internal error" }))).into_response()
            }
        }
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

/// Keyset-free offset pagination for now; issue lists are short-lived views, not exports.
#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
}

impl Page {
    fn limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 250)
    }
}

#[derive(Serialize)]
struct Paged<T> {
    items: Vec<T>,
    offset: i64,
    limit: i64,
}

async fn health(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(Json(json!({ "status": "ok" })))
}

/// Upstream's `user_has_project_membership_access`, which `project/issue/event_membership_required` apply after
/// looking the object up (so a missing object is a 404 for everyone, and an inaccessible one a 403).
async fn require_project_access(pool: &PgPool, user: &User, project_id: i64) -> Result<(), ApiError> {
    if user.is_superuser {
        return Ok(());
    }
    let member: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM projects_projectmembership WHERE project_id = $1 AND user_id = $2 AND accepted)",
    )
    .bind(project_id)
    .bind(user.id)
    .fetch_one(pool)
    .await?;
    if member { Ok(()) } else { Err(ApiError::Forbidden) }
}

/// Upstream's `projects_visible_to_user`: superusers see all; others their projects (accepted or not), their teams'
/// projects, and joinable/discoverable ones.
async fn list_projects(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
) -> ApiResult<Vec<Project>> {
    // TODO(parity): upstream's mine/teams/other tabs, member counts, join/leave.
    let projects = sqlx::query_as(
        "SELECT id, team_id, name, slug, sentry_key, digested_event_count, stored_event_count, issue_count,
                has_releases, retention_max_event_count
         FROM projects_project p
         WHERE NOT is_deleted
           AND ($1 OR visibility < $2
                OR EXISTS (SELECT 1 FROM projects_projectmembership m WHERE m.project_id = p.id AND m.user_id = $3)
                OR EXISTS (SELECT 1 FROM teams_teammembership t
                           WHERE t.team_id = p.team_id AND t.user_id = $3 AND t.accepted))
         ORDER BY name",
    )
    .bind(user.is_superuser)
    .bind(bugsink_db::PROJECT_VISIBILITY_TEAM_MEMBERS)
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(projects))
}

const ISSUE_COLUMNS: &str = "id, project_id, digest_order, first_seen, last_seen, digested_event_count,
    stored_event_count, calculated_type, calculated_value, transaction, is_resolved, is_muted";

async fn list_issues(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
    Query(page): Query<Page>,
) -> ApiResult<Paged<IssueSummary>> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM projects_project WHERE id = $1)")
        .bind(project_id)
        .fetch_one(&state.pool)
        .await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    require_project_access(&state.pool, &user, project_id).await?;
    // TODO(parity): open/resolved/muted/all tabs, tag search (`q=`), sort options.
    let items = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ISSUE_COLUMNS} FROM issues_issue WHERE project_id = $1 AND NOT is_deleted
         ORDER BY last_seen DESC OFFSET $2 LIMIT $3"
    )))
    .bind(project_id)
    .bind(page.offset)
    .bind(page.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(Paged { items, offset: page.offset, limit: page.limit() }))
}

/// `issue_membership_required`: a deleted issue is a 404.
async fn accessible_issue(pool: &PgPool, user: &User, issue_id: Uuid) -> Result<IssueSummary, ApiError> {
    let issue: IssueSummary = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ISSUE_COLUMNS} FROM issues_issue WHERE id = $1 AND NOT is_deleted"
    )))
    .bind(issue_id)
    .fetch_optional(pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    require_project_access(pool, user, issue.project_id).await?;
    Ok(issue)
}

async fn get_issue(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
) -> ApiResult<IssueSummary> {
    accessible_issue(&state.pool, &user, issue_id).await.map(Json)
}

const EVENT_SUMMARY_COLUMNS: &str = "id, event_id, issue_id, digest_order, ingested_at, timestamp, level, release,
    environment, calculated_type, calculated_value";

async fn list_events(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
    Query(page): Query<Page>,
) -> ApiResult<Paged<EventSummary>> {
    accessible_issue(&state.pool, &user, issue_id).await?;
    let items = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {EVENT_SUMMARY_COLUMNS} FROM events_event WHERE issue_id = $1
         ORDER BY digest_order DESC OFFSET $2 LIMIT $3"
    )))
    .bind(issue_id)
    .bind(page.offset)
    .bind(page.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(Paged { items, offset: page.offset, limit: page.limit() }))
}

async fn get_event(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(event_id): Path<Uuid>,
) -> ApiResult<Event> {
    // `event_membership_required`
    let project_id: i64 = sqlx::query_scalar("SELECT project_id FROM events_event WHERE id = $1")
        .bind(event_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    require_project_access(&state.pool, &user, project_id).await?;
    // TODO(parity): server-side sourcemap application and stacktrace shaping (upstream `events/utils.py`), so the
    // frontend renders frames rather than interpreting raw SDK payloads.
    let event = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {EVENT_SUMMARY_COLUMNS}, platform, server_name, sdk_name, sdk_version,
                CASE WHEN storage_backend IS NULL THEN data::json END AS data
         FROM events_event WHERE id = $1"
    )))
    .bind(event_id)
    .fetch_optional(&state.pool)
    .await?;
    event.map(Json).ok_or(ApiError::NotFound)
}
