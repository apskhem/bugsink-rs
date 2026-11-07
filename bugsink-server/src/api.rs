//! `/api/v1/...`: the JSON API the frontend is built on.
//!
//! This replaces upstream's server-rendered Django views, so its shape follows the pages in `frontend/README.md`
//! rather than upstream's DRF "canonical" API (which can be added later under `/api/canonical/0/`).
//!
//! TODO(before exposing beyond localhost): session auth (login/logout, `sessions` table, cookie + CSRF), and
//! per-project/team membership checks on every handler. Until then everything here is readable by anyone who can
//! reach the port.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use bugsink_db::PgPool;
use bugsink_db::models::{Event, EventSummary, IssueSummary, Project};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/projects", get(list_projects))
        .route("/api/v1/projects/{project_id}/issues", get(list_issues))
        .route("/api/v1/issues/{issue_id}", get(get_issue))
        .route("/api/v1/issues/{issue_id}/events", get(list_events))
        .route("/api/v1/events/{event_id}", get(get_event))
        .with_state(pool)
}

pub enum ApiError {
    NotFound,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, Json(json!({ "detail": "not found" }))).into_response(),
            Self::Db(e) => {
                tracing::error!(error = %e, "api query failed");
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

async fn health(State(pool): State<PgPool>) -> ApiResult<serde_json::Value> {
    sqlx::query("SELECT 1").execute(&pool).await?;
    Ok(Json(json!({ "status": "ok" })))
}

async fn list_projects(State(pool): State<PgPool>) -> ApiResult<Vec<Project>> {
    let projects = sqlx::query_as(
        "SELECT id, team_id, name, slug, sentry_key, digested_event_count, stored_event_count, issue_count, has_releases,
                retention_max_event_count
         FROM projects_project WHERE NOT is_deleted ORDER BY name",
    )
    .fetch_all(&pool)
    .await?;
    Ok(Json(projects))
}

const ISSUE_COLUMNS: &str = "id, project_id, digest_order, first_seen, last_seen, digested_event_count,
    stored_event_count, calculated_type, calculated_value, transaction, is_resolved, is_muted";

async fn list_issues(
    State(pool): State<PgPool>,
    Path(project_id): Path<i64>,
    Query(page): Query<Page>,
) -> ApiResult<Paged<IssueSummary>> {
    // TODO(parity): open/resolved/muted/all tabs, tag search (`q=`), sort options.
    let items = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ISSUE_COLUMNS} FROM issues_issue WHERE project_id = $1 AND NOT is_deleted
         ORDER BY last_seen DESC OFFSET $2 LIMIT $3"
    )))
    .bind(project_id)
    .bind(page.offset)
    .bind(page.limit())
    .fetch_all(&pool)
    .await?;
    Ok(Json(Paged { items, offset: page.offset, limit: page.limit() }))
}

async fn get_issue(State(pool): State<PgPool>, Path(issue_id): Path<Uuid>) -> ApiResult<IssueSummary> {
    let issue = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ISSUE_COLUMNS} FROM issues_issue WHERE id = $1 AND NOT is_deleted"
    )))
    .bind(issue_id)
    .fetch_optional(&pool)
    .await?;
    issue.map(Json).ok_or(ApiError::NotFound)
}

const EVENT_SUMMARY_COLUMNS: &str = "id, event_id, issue_id, digest_order, ingested_at, timestamp, level, release,
    environment, calculated_type, calculated_value";

async fn list_events(
    State(pool): State<PgPool>,
    Path(issue_id): Path<Uuid>,
    Query(page): Query<Page>,
) -> ApiResult<Paged<EventSummary>> {
    let items = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {EVENT_SUMMARY_COLUMNS} FROM events_event WHERE issue_id = $1
         ORDER BY digest_order DESC OFFSET $2 LIMIT $3"
    )))
    .bind(issue_id)
    .bind(page.offset)
    .bind(page.limit())
    .fetch_all(&pool)
    .await?;
    Ok(Json(Paged { items, offset: page.offset, limit: page.limit() }))
}

async fn get_event(State(pool): State<PgPool>, Path(event_id): Path<Uuid>) -> ApiResult<Event> {
    // TODO(parity): server-side sourcemap application and stacktrace shaping (upstream `events/utils.py`), so the
    // frontend renders frames rather than interpreting raw SDK payloads.
    let event = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {EVENT_SUMMARY_COLUMNS}, platform, server_name, sdk_name, sdk_version,
                CASE WHEN storage_backend IS NULL THEN data::json END AS data
         FROM events_event WHERE id = $1"
    )))
    .bind(event_id)
    .fetch_optional(&pool)
    .await?;
    event.map(Json).ok_or(ApiError::NotFound)
}
