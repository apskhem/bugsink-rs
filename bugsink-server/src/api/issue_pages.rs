//! The issue pages: header, tags, groupings, history and comments, an event with navigation, hourly counts.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use axum::routing::{get, patch, post};
use bugsink_db::TurningPointKind;
use bugsink_db::models::{Event, IssueSummary};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{ApiError, ApiResult, ApiState, accessible_issue};
use crate::accounts::CurrentUser;
use crate::issues::pages::{self, IssueDetail, Nav};
use crate::search::Search;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/issues/{issue_id}", get(get_issue))
        .route("/api/v1/issues/{issue_id}/tags", get(issue_tags))
        .route("/api/v1/issues/{issue_id}/groupings", get(issue_groupings))
        .route("/api/v1/issues/{issue_id}/history", get(issue_history))
        .route("/api/v1/issues/{issue_id}/comments", post(add_comment))
        .route("/api/v1/issues/{issue_id}/comments/{comment_id}", patch(edit_comment).delete(delete_comment))
        .route("/api/v1/issues/{issue_id}/counts", get(issue_counts))
        .route("/api/v1/issues/{issue_id}/events/{selector}", get(issue_event))
        .route("/api/v1/issues/{issue_id}/events/{digest_order}/{nav}", get(issue_event_nav))
}

#[derive(Serialize)]
struct IssuePage {
    #[serde(flatten)]
    summary: IssueSummary,
    #[serde(flatten)]
    detail: IssueDetail,
}

async fn get_issue(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
) -> ApiResult<IssuePage> {
    let summary = accessible_issue(&state.pool, &user, issue_id).await?;
    let detail = pages::issue_detail(&state.pool, issue_id).await?;
    Ok(Json(IssuePage { summary, detail }))
}

#[derive(Deserialize)]
struct TagsQuery {
    /// The tags tab (25 values per key) rather than the header's summary (4).
    #[serde(default)]
    all: bool,
}

async fn issue_tags(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
    Query(query): Query<TagsQuery>,
) -> ApiResult<Vec<pages::KeyTags>> {
    let issue = accessible_issue(&state.pool, &user, issue_id).await?;
    let cutoff = if query.all { pages::TAGS_ALL_CUTOFF } else { pages::TAGS_SUMMARY_CUTOFF };
    Ok(Json(pages::tags(&state.pool, issue_id, issue.digested_event_count, cutoff).await?))
}

async fn issue_groupings(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
) -> ApiResult<Vec<pages::Grouping>> {
    accessible_issue(&state.pool, &user, issue_id).await?;
    Ok(Json(pages::groupings(&state.pool, issue_id).await?))
}

async fn issue_history(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
) -> ApiResult<Vec<pages::TurningPoint>> {
    accessible_issue(&state.pool, &user, issue_id).await?;
    Ok(Json(pages::history(&state.pool, issue_id).await?))
}

#[derive(Deserialize)]
struct CommentRequest {
    comment: String,
}

fn check_comment(comment: &str) -> Result<(), ApiError> {
    // Django's form fields refuse NUL characters (upstream then fails with a 500)
    if comment.contains('\0') {
        return Err(ApiError::BadRequest("Null characters are not allowed.".into()));
    }
    Ok(())
}

/// `history_comment_new`: 201 with the comment's id; an empty comment is ignored (200, no id).
async fn add_comment(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
    Json(req): Json<CommentRequest>,
) -> Result<Response, ApiError> {
    let issue = accessible_issue(&state.pool, &user, issue_id).await?;
    check_comment(&req.comment)?;
    let id = pages::add_comment(&state.pool, issue.project_id, issue_id, user.id, &req.comment, Utc::now()).await?;
    let status = if id.is_some() { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(json!({ "id": id }))).into_response())
}

/// `history_comment_edit`/`_delete`: the turning point (owner, kind); 404 unless it is on the issue.
async fn turning_point_on(state: &ApiState, issue_id: Uuid, comment_id: i64) -> Result<(Option<i64>, i32), ApiError> {
    sqlx::query_as("SELECT user_id, kind FROM issues_turningpoint WHERE id = $1 AND issue_id = $2")
        .bind(comment_id)
        .bind(issue_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Any of the user's own turning points can carry a comment (e.g. why they resolved), not just comments.
async fn edit_comment(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((issue_id, comment_id)): Path<(Uuid, i64)>,
    Json(req): Json<CommentRequest>,
) -> Result<StatusCode, ApiError> {
    accessible_issue(&state.pool, &user, issue_id).await?;
    let (owner, _) = turning_point_on(&state, issue_id, comment_id).await?;
    if owner != Some(user.id) {
        return Err(ApiError::Forbidden("You can only edit your own comments".into()));
    }
    check_comment(&req.comment)?;
    sqlx::query("UPDATE issues_turningpoint SET comment = $2 WHERE id = $1")
        .bind(comment_id)
        .bind(pages::clean_comment(&req.comment))
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Only comments can be deleted, not the turning points of actions.
async fn delete_comment(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((issue_id, comment_id)): Path<(Uuid, i64)>,
) -> Result<StatusCode, ApiError> {
    accessible_issue(&state.pool, &user, issue_id).await?;
    let (owner, kind) = turning_point_on(&state, issue_id, comment_id).await?;
    if owner != Some(user.id) {
        return Err(ApiError::Forbidden("You can only delete your own comments".into()));
    }
    if kind != TurningPointKind::ManualAnnotation as i32 {
        return Err(ApiError::Forbidden("You can only delete manual annotations".into()));
    }
    sqlx::query("DELETE FROM issues_turningpoint WHERE id = $1").bind(comment_id).execute(&state.pool).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

#[derive(Serialize)]
struct EventPage {
    event: Event,
    /// Within the search, around this event.
    nav: Nav,
}

async fn event_by_digest_order(
    state: &ApiState,
    issue_id: Uuid,
    digest_order: Option<i32>,
) -> Result<Event, ApiError> {
    let digest_order = digest_order.ok_or(ApiError::NotFound)?;
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM events_event WHERE issue_id = $1 AND digest_order = $2")
        .bind(issue_id)
        .bind(digest_order)
        .fetch_optional(&state.pool)
        .await?;
    super::event_by_id(&state.pool, id.ok_or(ApiError::NotFound)?).await
}

async fn event_page(
    state: &ApiState,
    issue: &IssueSummary,
    search: &Search,
    q: &str,
    event: Event,
) -> ApiResult<EventPage> {
    let digest_order = event.summary.digest_order;
    let nav = pages::nav(&state.pool, issue.id, issue.stored_event_count, search, !q.is_empty(), digest_order).await?;
    Ok(Json(EventPage { event, nav }))
}

/// `/events/first` and `/events/last` (within the search), `/events/<digest_order>`, `/events/<id>` (our id, or the
/// SDK's event_id). Direct lookups don't have to match the search, as upstream.
async fn issue_event(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((issue_id, selector)): Path<(Uuid, String)>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<EventPage> {
    let issue = accessible_issue(&state.pool, &user, issue_id).await?;
    let search = Search::prepare(&state.pool, issue.project_id, &query.q).await?;
    let event = match selector.as_str() {
        "first" | "last" => {
            let (first, last) = pages::bounds(&state.pool, issue_id, &search).await?;
            event_by_digest_order(&state, issue_id, if selector == "first" { first } else { last }).await?
        }
        _ => {
            if let Ok(digest_order) = selector.parse::<i32>() {
                event_by_digest_order(&state, issue_id, Some(digest_order)).await?
            } else {
                // our id first, then the SDK's (`_get_event`)
                let id = Uuid::try_parse(&selector).map_err(|_| ApiError::NotFound)?;
                let found: Option<Uuid> = sqlx::query_scalar(
                    "SELECT id FROM events_event WHERE issue_id = $1 AND (id = $2 OR event_id = $2)
                     ORDER BY id <> $2 LIMIT 1",
                )
                .bind(issue_id)
                .bind(id)
                .fetch_optional(&state.pool)
                .await?;
                super::event_by_id(&state.pool, found.ok_or(ApiError::NotFound)?).await?
            }
        }
    };
    event_page(&state, &issue, &search, &query.q, event).await
}

/// `/events/<digest_order>/prev` and `/next`: the nearest matching event.
async fn issue_event_nav(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((issue_id, digest_order, nav)): Path<(Uuid, i32, String)>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<EventPage> {
    let issue = accessible_issue(&state.pool, &user, issue_id).await?;
    let next = match nav.as_str() {
        "next" => true,
        "prev" => false,
        _ => return Err(ApiError::NotFound),
    };
    let search = Search::prepare(&state.pool, issue.project_id, &query.q).await?;
    let found = pages::neighbour(&state.pool, issue_id, &search, digest_order, next).await?;
    let event = event_by_digest_order(&state, issue_id, found).await?;
    event_page(&state, &issue, &search, &query.q, event).await
}

#[derive(Serialize)]
struct Counts {
    since: DateTime<Utc>,
    until: DateTime<Utc>,
    /// Hours with events (incl. evicted ones), oldest first; `digest_order` is the hour's last event.
    hours: Vec<pages::HourCount>,
    /// With a search: per hour, the stored events that match.
    matching: Option<Vec<pages::HourCount>>,
}

/// The issue page's sparkline: hourly counts for the 28 days up to and including the current hour (upstream shows
/// them in buckets of 6, 12 or 24 hours aligned on the installation's local days; that is up to the frontend).
async fn issue_counts(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(issue_id): Path<Uuid>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Counts> {
    let issue = accessible_issue(&state.pool, &user, issue_id).await?;
    let until = pages::hour_bucket(Utc::now()) + TimeDelta::hours(1);
    let since = until - TimeDelta::days(28) - TimeDelta::hours(1);
    let search = if query.q.is_empty() {
        None
    } else {
        Some(Search::prepare(&state.pool, issue.project_id, &query.q).await?)
    };
    let (hours, matching) = pages::hourly_counts(&state.pool, issue_id, since, until, search.as_ref()).await?;
    Ok(Json(Counts { since, until, hours, matching }))
}
