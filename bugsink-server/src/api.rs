//! `/api/v1/...`: the JSON API the frontend is built on.
//!
//! This replaces upstream's server-rendered Django views, so its shape follows the pages in `frontend/README.md`
//! rather than upstream's DRF "canonical" API (which can be added later under `/api/canonical/0/`).
//!
//! Every endpoint except health and login needs a session (`accounts::CurrentUser`) and checks access like upstream's
//! views: superusers see everything; issues and events need an accepted membership of their project.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bugsink_db::PgPool;
use bugsink_db::models::{Event, EventSummary, IssueSummary};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::QueryBuilder;
use uuid::Uuid;

mod issue_pages;
mod members;
mod project_admin;
mod teams;

use crate::accounts::{self, CurrentUser, User};
use crate::issues::pages;
use crate::issues::{self, Action};
use crate::search::{Search, Searched};
use crate::{Settings, memberships, projects};

#[derive(Clone)]
pub struct ApiState {
    pub pool: PgPool,
    /// `BASE_URL`'s scheme://host[:port], for the CSRF origin check.
    pub origin: String,
    /// Session cookies are `Secure` when `BASE_URL` is https (upstream: `BEHIND_HTTPS_PROXY`).
    pub secure_cookie: bool,
    pub settings: Arc<Settings>,
}

impl ApiState {
    pub fn new(pool: PgPool, settings: Settings) -> anyhow::Result<Self> {
        let base_url = settings.alerts.base_url();
        let url = url::Url::parse(base_url).map_err(|e| anyhow::anyhow!("BASE_URL {base_url:?}: {e}"))?;
        Ok(Self {
            pool,
            origin: url.origin().ascii_serialization(),
            secure_cookie: url.scheme() == "https",
            settings: Arc::new(settings),
        })
    }

    pub fn memberships(&self) -> memberships::Context {
        memberships::Context {
            pool: self.pool.clone(),
            users: self.settings.users.clone(),
            alerts: Arc::new(self.settings.alerts.clone()),
        }
    }
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/login", post(accounts::login))
        .route("/api/v1/auth/logout", post(accounts::logout))
        .route("/api/v1/auth/reset-password/{token}", post(accounts::reset_password))
        .route("/api/v1/me", get(accounts::me))
        .route("/api/v1/projects/{project_id}/issues", get(list_issues))
        .route("/api/v1/projects/{project_id}/issues/actions", post(project_issue_actions))
        .route("/api/v1/issues", get(global_issue_list))
        .route("/api/v1/issues/actions", post(global_issue_actions))
        .route("/api/v1/issues/{issue_id}/actions", post(issue_action))
        .route("/api/v1/issues/{issue_id}/events", get(list_events))
        .route("/api/v1/events/{event_id}", get(get_event))
        .merge(issue_pages::routes())
        .merge(teams::routes())
        .merge(project_admin::routes())
        .merge(members::routes())
        .with_state(state)
}

pub enum ApiError {
    BadRequest(String),
    /// A form's errors by field, with Django's messages.
    Invalid(projects::FieldErrors),
    Conflict(String),
    Unauthorized,
    CsrfFailed,
    Forbidden(String),
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
            Self::Invalid(errors) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "detail": "invalid", "errors": errors }))).into_response()
            }
            Self::Conflict(detail) => (StatusCode::CONFLICT, Json(json!({ "detail": detail }))).into_response(),
            Self::Unauthorized => {
                (StatusCode::UNAUTHORIZED, Json(json!({ "detail": "authentication required" }))).into_response()
            }
            Self::CsrfFailed => (StatusCode::FORBIDDEN, Json(json!({ "detail": "CSRF check failed" }))).into_response(),
            Self::Forbidden(detail) => (StatusCode::FORBIDDEN, Json(json!({ "detail": detail }))).into_response(),
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

pub(crate) type ApiResult<T> = Result<Json<T>, ApiError>;

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
pub(crate) async fn require_project_access(pool: &PgPool, user: &User, project_id: i64) -> Result<(), ApiError> {
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
    if !member {
        return Err(ApiError::Forbidden("You don't have permission to access this project".into()));
    }
    Ok(())
}

const ISSUE_COLUMNS: &str = "id, project_id, digest_order, first_seen, last_seen, digested_event_count,
    stored_event_count, calculated_type, calculated_value, transaction, is_resolved, is_muted";

/// The issue lists' tabs (`_filter_issue_list_by_state`) and sorts (`ISSUE_LIST_SORTS`, falling back to last seen).
#[derive(Deserialize)]
struct IssueListQuery {
    #[serde(default = "default_state")]
    state: String,
    #[serde(default)]
    sort: String,
    /// Search (`crate::search`); project issue lists only, as upstream.
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
}

fn default_state() -> String {
    "open".into()
}

impl IssueListQuery {
    fn page(&self) -> Page {
        Page { offset: self.offset, limit: self.limit }
    }

    fn state_sql(&self) -> Result<&'static str, ApiError> {
        Ok(match self.state.as_str() {
            "open" => "NOT is_resolved AND NOT is_muted",
            "unresolved" => "NOT is_resolved",
            "resolved" => "is_resolved AND NOT is_muted",
            "muted" => "NOT is_resolved AND is_muted",
            "all" => "true",
            _ => {
                return Err(ApiError::BadRequest("state must be one of open, unresolved, resolved, muted, all".into()));
            }
        })
    }

    fn order_sql(&self) -> &'static str {
        // id last: a stable order for paging through ties
        match self.sort.as_str() {
            "events" => "digested_event_count DESC, last_seen DESC, id",
            _ => "last_seen DESC, id",
        }
    }

    async fn fetch(&self, pool: &PgPool, project_ids: &[i64], search: &Search) -> ApiResult<Paged<IssueListItem>> {
        let page = self.page();
        let mut qb = QueryBuilder::new(format!("SELECT {ISSUE_COLUMNS} FROM issues_issue WHERE project_id = ANY("));
        qb.push_bind(project_ids.to_vec());
        qb.push(format_args!(") AND NOT is_deleted AND {}", self.state_sql()?));
        search.push_conditions(&mut qb, Searched::Issues);
        qb.push(format_args!(" ORDER BY {} OFFSET ", self.order_sql()));
        qb.push_bind(page.offset);
        qb.push(" LIMIT ");
        qb.push_bind(page.limit());
        let issues: Vec<IssueSummary> = qb.build_query_as().fetch_all(pool).await?;
        let ids: Vec<Uuid> = issues.iter().map(|i| i.id).collect();
        let mut sparklines = pages::issue_list_sparklines(pool, &ids, Utc::now()).await?;
        let items = issues
            .into_iter()
            .map(|issue| IssueListItem { sparkline: sparklines.remove(&issue.id).unwrap_or_default(), issue })
            .collect();
        Ok(Json(Paged { items, offset: page.offset, limit: page.limit() }))
    }
}

#[derive(Serialize)]
struct IssueListItem {
    #[serde(flatten)]
    issue: IssueSummary,
    /// Events in each of the last 24 hours, the current one last.
    sparkline: Vec<i32>,
}

/// `project_membership_required`: the project is looked up first (deleted or not), then access is checked.
async fn accessible_project(pool: &PgPool, user: &User, project_id: i64) -> Result<(), ApiError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM projects_project WHERE id = $1)")
        .bind(project_id)
        .fetch_one(pool)
        .await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    require_project_access(pool, user, project_id).await
}

async fn list_issues(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
    Query(query): Query<IssueListQuery>,
) -> ApiResult<Paged<IssueListItem>> {
    accessible_project(&state.pool, &user, project_id).await?;
    let search = Search::prepare(&state.pool, project_id, &query.q).await?;
    query.fetch(&state.pool, &[project_id], &search).await
}

/// `get_issue_accessible_project_ids`: the projects whose issues show up in the global issue list.
async fn issue_accessible_project_ids(pool: &PgPool, user: &User) -> Result<Vec<i64>, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT p.id FROM projects_project p
         WHERE NOT p.is_deleted
           AND ($1 OR EXISTS (SELECT 1 FROM projects_projectmembership m
                              WHERE m.project_id = p.id AND m.user_id = $2 AND m.accepted))",
    )
    .bind(user.is_superuser)
    .bind(user.id)
    .fetch_all(pool)
    .await?)
}

async fn global_issue_list(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Query(query): Query<IssueListQuery>,
) -> ApiResult<Paged<IssueListItem>> {
    if !query.q.is_empty() {
        return Err(ApiError::BadRequest("search (q) is per project, as upstream's".into()));
    }
    let project_ids = issue_accessible_project_ids(&state.pool, &user).await?;
    query.fetch(&state.pool, &project_ids, &Search::Everything).await
}

#[derive(Deserialize)]
struct ActionRequest {
    /// Upstream's action strings: `resolve`, `resolved_release:<version>`, `resolved_next`, `reopen`, `mute`,
    /// `mute_for:<period>,<n>,None`, `mute_until:<period>,<n>,<volume>`, `unmute`, `delete`.
    action: String,
}

#[derive(Deserialize)]
struct BulkActionRequest {
    action: String,
    issue_ids: Vec<Uuid>,
}

#[derive(Serialize)]
struct ActionResult {
    /// False when the action doesn't apply to the issue's current state (e.g. already resolved); nothing changed.
    applied: bool,
    /// The issue afterwards; absent once deleted.
    issue: Option<IssueSummary>,
}

#[derive(Serialize)]
struct BulkActionResult {
    /// The selected issues the action did not apply to, because of their state.
    unapplied_issue_ids: Vec<Uuid>,
}

fn parse_action(action: &str) -> Result<Action, ApiError> {
    Action::parse(action).ok_or_else(|| ApiError::BadRequest(format!("unknown issue action {action:?}")))
}

/// An action from an issue's own page (upstream: a POST to any of its tabs).
async fn issue_action(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
    Json(req): Json<ActionRequest>,
) -> ApiResult<ActionResult> {
    accessible_issue(&state.pool, &user, issue_id).await?;
    let action = parse_action(&req.action)?;
    let applied = issues::apply_to_issue(&state.pool, issue_id, &action, user.id, Utc::now())
        .await
        .map_err(ApiError::internal)?;
    let issue = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ISSUE_COLUMNS} FROM issues_issue WHERE id = $1 AND NOT is_deleted"
    )))
    .bind(issue_id)
    .fetch_optional(&state.pool)
    .await?;
    Ok(Json(ActionResult { applied, issue }))
}

/// A bulk action from a project's issue list.
async fn project_issue_actions(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
    Json(req): Json<BulkActionRequest>,
) -> ApiResult<BulkActionResult> {
    accessible_project(&state.pool, &user, project_id).await?;
    let action = parse_action(&req.action)?;
    let unapplied_issue_ids =
        issues::apply_to_issues(&state.pool, &[project_id], &req.issue_ids, &action, user.id, Utc::now())
            .await
            .map_err(ApiError::internal)?;
    Ok(Json(BulkActionResult { unapplied_issue_ids }))
}

/// A bulk action from the global issue list, over the issues of every project the user has issue access to.
async fn global_issue_actions(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(req): Json<BulkActionRequest>,
) -> ApiResult<BulkActionResult> {
    let action = parse_action(&req.action)?;
    if matches!(action, Action::ResolvedRelease(_) | Action::ResolvedNext) {
        return Err(ApiError::Forbidden("Release-specific issue actions are project-scoped".into()));
    }
    let project_ids = issue_accessible_project_ids(&state.pool, &user).await?;
    let unapplied_issue_ids =
        issues::apply_to_issues(&state.pool, &project_ids, &req.issue_ids, &action, user.id, Utc::now())
            .await
            .map_err(ApiError::internal)?;
    Ok(Json(BulkActionResult { unapplied_issue_ids }))
}

/// `issue_membership_required`: a deleted issue is a 404.
pub(crate) async fn accessible_issue(pool: &PgPool, user: &User, issue_id: Uuid) -> Result<IssueSummary, ApiError> {
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

pub(crate) const EVENT_SUMMARY_COLUMNS: &str = "id, event_id, issue_id, digest_order, ingested_at, timestamp, level,
    release, environment, calculated_type, calculated_value";

#[derive(Deserialize)]
struct EventListQuery {
    /// Search (`crate::search`) within the issue's events.
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
}

async fn list_events(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
    Query(query): Query<EventListQuery>,
) -> ApiResult<Paged<EventSummary>> {
    let issue = accessible_issue(&state.pool, &user, issue_id).await?;
    let page = Page { offset: query.offset, limit: query.limit };
    let search = Search::prepare(&state.pool, issue.project_id, &query.q).await?;
    let mut qb = QueryBuilder::new(format!("SELECT {EVENT_SUMMARY_COLUMNS} FROM events_event WHERE issue_id = "));
    qb.push_bind(issue_id);
    search.push_conditions(&mut qb, Searched::EventsOf(issue_id));
    qb.push(" ORDER BY digest_order DESC OFFSET ");
    qb.push_bind(page.offset);
    qb.push(" LIMIT ");
    qb.push_bind(page.limit());
    let items = qb.build_query_as().fetch_all(&state.pool).await?;
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
    event_by_id(&state.pool, event_id).await.map(Json)
}

/// An event with its payload.
pub(crate) async fn event_by_id(pool: &PgPool, event_id: Uuid) -> Result<Event, ApiError> {
    // The payload as the SDK sent it; the issue event pages add how upstream shows it (`crate::event_display`).
    let event = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {EVENT_SUMMARY_COLUMNS}, platform, server_name, sdk_name, sdk_version,
                CASE WHEN storage_backend IS NULL THEN data::json END AS data
         FROM events_event WHERE id = $1"
    )))
    .bind(event_id)
    .fetch_optional(pool)
    .await?;
    event.ok_or(ApiError::NotFound)
}
