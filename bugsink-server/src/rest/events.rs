//! `events/api_views.py`: an issue's events, one event (with its payload and stack trace as Markdown), looking one up
//! by the SDK's event id, and the stack trace alone.

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use bugsink_db::Postgres;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::QueryBuilder;
use uuid::Uuid;

use crate::py::{drf_datetime, py_uuid};
use super::pagination::{self, Ordering};
use super::{Ctx, Drf, Request, Route, guard_project, guarded_issue, guarded_project, ok, serve, uuid_pk};
use crate::api::ApiState;
use crate::py::strip;
use crate::search::{Search, Searched};

#[derive(sqlx::FromRow)]
struct EventRow {
    id: Uuid,
    ingested_at: DateTime<Utc>,
    digested_at: DateTime<Utc>,
    issue_id: Uuid,
    grouping_id: i64,
    event_id: Uuid,
    project_id: i64,
    timestamp: DateTime<Utc>,
    digest_order: i32,
}

const EVENT_COLUMNS: &str =
    "id, ingested_at, digested_at, issue_id, grouping_id, event_id, project_id, timestamp, digest_order";

/// `EventListSerializer`.
fn event_json(e: &EventRow) -> serde_json::Map<String, Value> {
    let Value::Object(map) = json!({
        "id": e.id,
        "ingested_at": drf_datetime(&e.ingested_at),
        "digested_at": drf_datetime(&e.digested_at),
        "issue": e.issue_id,
        "grouping": e.grouping_id,
        "event_id": e.event_id,
        "project": e.project_id,
        "timestamp": drf_datetime(&e.timestamp),
        "digest_order": e.digest_order,
    }) else {
        unreachable!()
    };
    map
}

static LIST: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Event List", methods: &[("GET", Some("events:read"))] };

/// An issue's events (`?issue=`, required), searched with `?q=`, newest first by default; without payloads.
pub async fn list(State(state): State<ApiState>, req: Request) -> Response {
    serve(state, req, &LIST, |c| async move {
        let identifier = super::required_param(&c.req, "issue")?;
        let (issue_id, project_id) = guarded_issue(&c, identifier, "events:read").await?;
        let q = strip(c.req.query.get("q").unwrap_or(""));
        let search = Search::prepare(c.pool(), project_id, q).await?;
        let mut query =
            QueryBuilder::<Postgres>::new(format!("SELECT {EVENT_COLUMNS} FROM events_event WHERE issue_id = "));
        query.push_bind(issue_id);
        search.push_conditions(&mut query, Searched::EventsOf(issue_id));
        let ordering = Ordering::asc_desc(&c.req, &["digest_order"], "integer", true)?;
        let page =
            pagination::paginate(c.pool(), &c.req, query, &ordering, |e: &EventRow| e.digest_order.to_string()).await?;
        let results = page.items.iter().map(|e| Value::Object(event_json(e))).collect();
        ok(page.response(&c.req, results))
    })
    .await
}

async fn event_row(c: &Ctx, sql: &str, id: Uuid) -> sqlx::Result<Option<EventRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {EVENT_COLUMNS} FROM events_event WHERE {sql}")))
        .bind(id)
        .fetch_optional(c.pool())
        .await
}

/// The event in the URL (any, even of a deleted issue), checked for `events:read`.
async fn guarded_event(c: &Ctx, pk: &str) -> Result<EventRow, Drf> {
    let event = event_row(c, "id = $1", uuid_pk(pk)?).await?.ok_or_else(|| Drf::no_match("Event"))?;
    guard_project(c, "events:read", event.project_id).await?;
    Ok(event)
}

/// `EventDetailSerializer`: the list's fields, the payload, and the stack trace as Markdown.
async fn detail_json(c: &Ctx, event: &EventRow) -> Result<Value, Drf> {
    let (data, platform, calculated_type, calculated_value): (Option<String>, String, String, String) = sqlx::query_as(
        "SELECT CASE WHEN storage_backend IS NULL THEN data END, platform, calculated_type, calculated_value
         FROM events_event WHERE id = $1",
    )
    .bind(event.id)
    .fetch_one(c.pool())
    .await?;
    let data = data.ok_or_else(|| Drf::internal("the event's payload is in upstream's file storage"))?;
    let data: Value = serde_json::from_str(&data).map_err(Drf::internal)?;
    let title = crate::issues::pages::title(&calculated_type, &calculated_value);
    let markdown = super::markdown::render(&data, &platform, &title);
    let mut map = event_json(event);
    map.insert("data".into(), data);
    map.insert("stacktrace_md".into(), Value::String(markdown));
    Ok(Value::Object(map))
}

static DETAIL: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Event Instance", methods: &[("GET", Some("events:read"))] };

pub async fn retrieve(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &DETAIL, |c| async move {
        let event = guarded_event(&c, &pk).await?;
        ok(detail_json(&c, &event).await?)
    })
    .await
}

static LOOKUP: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Lookup by event id", methods: &[("GET", Some("events:read"))] };

/// An event by project (`?project=`) and the SDK's `event_id`; not of a deleted issue.
pub async fn lookup(State(state): State<ApiState>, req: Request) -> Response {
    serve(state, req, &LOOKUP, |c| async move {
        let project_id = guarded_project(&c, super::required_param(&c.req, "project")?, true, "events:read").await?;
        let event_id = super::required_param(&c.req, "event_id")?;
        let event_id = py_uuid(event_id).ok_or_else(Drf::not_found)?;
        let event: Option<EventRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {EVENT_COLUMNS} FROM events_event e
             WHERE project_id = $1 AND event_id = $2
               AND NOT EXISTS (SELECT 1 FROM issues_issue i WHERE i.id = e.issue_id AND i.is_deleted)"
        )))
        .bind(project_id)
        .bind(event_id)
        .fetch_optional(c.pool())
        .await?;
        let event = event.ok_or_else(|| Drf::no_match("Event"))?;
        ok(detail_json(&c, &event).await?)
    })
    .await
}

static STACKTRACE: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Stacktrace", methods: &[("GET", Some("events:read"))] };

/// The stack trace alone, as `text/markdown`.
pub async fn stacktrace(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &STACKTRACE, |c| async move {
        let event = guarded_event(&c, &pk).await?;
        let detail = detail_json(&c, &event).await?;
        let markdown = detail["stacktrace_md"].as_str().unwrap_or("").to_owned();
        Ok((StatusCode::OK, [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], markdown).into_response())
    })
    .await
}
