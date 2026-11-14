//! `issues/api_views.py`: issues (list, detail, tags, groupings, event counts, deleting, triaging), comments and
//! history.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use bugsink_db::Postgres;
use chrono::{DateTime, SubsecRound, Utc};
use serde_json::{Value, json};
use sqlx::QueryBuilder;
use uuid::Uuid;

use super::fields::{self, Errors, required};
use super::pagination::{self, Ordering};
use super::{
    CountsOf, Ctx, Drf, Request, Route, guard_project, guarded_issue, guarded_project, issue_key, object_body, ok,
    render, serve,
};
use crate::api::ApiState;
use crate::issues::{self as triage, Action};
use crate::py::{drf_datetime, str_datetime, strip};
use crate::releases::parse_lines;
use crate::search::{ParsedQuery, Search, Searched, parse_query};

#[derive(sqlx::FromRow)]
pub struct IssueRow {
    id: Uuid,
    slug: String,
    project_id: i64,
    digest_order: i32,
    last_seen: DateTime<Utc>,
    first_seen: DateTime<Utc>,
    digested_event_count: i32,
    stored_event_count: i32,
    calculated_type: String,
    calculated_value: String,
    transaction: String,
    is_resolved: bool,
    is_resolved_unconditionally: bool,
    is_resolved_by_next_release: bool,
    fixed_at: String,
    events_at: String,
    is_muted: bool,
    unmute_after: Option<DateTime<Utc>>,
    unmute_on_volume_based_conditions: String,
}

const ISSUE_SELECT: &str = "SELECT i.id, p.slug, i.project_id, i.digest_order, i.last_seen, i.first_seen,
    i.digested_event_count, i.stored_event_count, i.calculated_type, i.calculated_value, i.transaction, i.is_resolved,
    i.is_resolved_unconditionally, i.is_resolved_by_next_release, i.fixed_at, i.events_at, i.is_muted, i.unmute_after,
    i.unmute_on_volume_based_conditions
    FROM issues_issue i JOIN projects_project p ON p.id = i.project_id";

/// `IssueSerializer`.
fn issue_json(r: &IssueRow) -> Value {
    let int = |v: &Value| match v {
        Value::String(s) => json!(s.trim().parse::<i64>().unwrap_or(0)),
        other => other.clone(),
    };
    let conditions: Vec<Value> = serde_json::from_str::<Vec<Value>>(&r.unmute_on_volume_based_conditions)
        .unwrap_or_default()
        .iter()
        .map(|c| {
            json!({
                "period": fields::str_of(&c["period"]),
                "nr_of_periods": int(&c["nr_of_periods"]),
                "volume": int(&c["volume"]),
            })
        })
        .collect();
    json!({
        "id": r.id,
        "friendly_id": triage::friendly_id(&r.slug, r.digest_order),
        "project": r.project_id,
        "digest_order": r.digest_order,
        "last_seen": drf_datetime(&r.last_seen),
        "first_seen": drf_datetime(&r.first_seen),
        "digested_event_count": r.digested_event_count,
        "stored_event_count": r.stored_event_count,
        "calculated_type": r.calculated_type,
        "calculated_value": r.calculated_value,
        "transaction": r.transaction,
        "is_resolved": r.is_resolved,
        "is_resolved_unconditionally": r.is_resolved_unconditionally,
        "is_resolved_by_next_release": r.is_resolved_by_next_release,
        "fixed_at": parse_lines(&r.fixed_at),
        "events_at": parse_lines(&r.events_at),
        "is_muted": r.is_muted,
        "unmute_after": fields::opt_datetime(&r.unmute_after),
        "unmute_on_volume_based_conditions": conditions,
    })
}

async fn issue_row(c: &Ctx, id: Uuid) -> sqlx::Result<IssueRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("{ISSUE_SELECT} WHERE i.id = $1"))).bind(id).fetch_one(c.pool()).await
}


static LIST: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Issue List", methods: &[("GET", Some("issues:read"))] };

/// The issues of the accessible projects (one with `?project=`), searched (`?q=`), by state, sorted and paginated.
pub async fn list(State(state): State<ApiState>, req: Request) -> Response {
    serve(state, req, &LIST, |c| async move {
        let mut project = c.token.project_id;
        if c.req.query.contains("project") {
            let id = guarded_project(&c, c.req.query.get("project").unwrap_or(""), false, "issues:read").await?;
            project = Some(id);
        }

        let state_filter = match c.req.query.get("state").unwrap_or("all") {
            "all" => "",
            "open" => " AND NOT i.is_resolved AND NOT i.is_muted",
            "unresolved" => " AND NOT i.is_resolved",
            "resolved" => " AND i.is_resolved AND NOT i.is_muted",
            "muted" => " AND NOT i.is_resolved AND i.is_muted",
            _ => {
                let message = "Must be 'all', 'open', 'unresolved', 'resolved', or 'muted'.";
                return Err(Drf::invalid(json!({ "state": [message] })));
            }
        };
        let mut query = QueryBuilder::<Postgres>::new(format!(
            "{ISSUE_SELECT} WHERE NOT i.is_deleted AND NOT p.is_deleted{state_filter}"
        ));
        match (project, &c.token.user) {
            (Some(id), _) => {
                query.push(" AND i.project_id = ").push_bind(id);
            }
            // `get_issue_accessible_project_ids`
            (None, Some(user)) if !user.is_superuser => {
                query.push(
                    " AND i.project_id IN (SELECT m.project_id FROM projects_projectmembership m WHERE m.accepted
                      AND m.user_id = ",
                );
                query.push_bind(user.id).push(")");
            }
            _ => {}
        }
        let q = strip(c.req.query.get("q").unwrap_or("")).to_owned();
        search_issues(&c, project, &q, &mut query).await?;

        // `IssuesCursorPagination`
        let single = project.is_some();
        let sort = c.req.query.get("sort").unwrap_or(if single { "digest_order" } else { "last_seen" });
        if !["digest_order", "last_seen", "digested_event_count"].contains(&sort) {
            let message = "Must be 'digest_order', 'last_seen', or 'digested_event_count'.";
            return Err(Drf::invalid(json!({ "sort": [message] })));
        }
        let desc = match c.req.query.get("order").unwrap_or(if single { "asc" } else { "desc" }) {
            "asc" => false,
            "desc" => true,
            _ => return Err(Drf::invalid(json!({"order": ["Must be 'asc' or 'desc'."]}))),
        };
        let (fields, cast): (&[&'static str], _) = match sort {
            "digest_order" if !single => {
                return Err(Drf::invalid(json!({"sort": ["digest_order requires a single project."]})));
            }
            "digest_order" => (&["i.digest_order"], "integer"),
            "last_seen" => (&["i.last_seen", "i.id"], "timestamptz"),
            _ => (&["i.digested_event_count", "i.last_seen", "i.id"], "integer"),
        };
        let ordering = Ordering { fields: fields.iter().map(|f| (*f, desc)).collect(), cast };
        let page = pagination::paginate(c.pool(), &c.req, query, &ordering, |r: &IssueRow| match sort {
            "digest_order" => r.digest_order.to_string(),
            "last_seen" => str_datetime(&r.last_seen),
            _ => r.digested_event_count.to_string(),
        })
        .await?;
        let results = page.items.iter().map(issue_json).collect();
        ok(page.response(&c.req, results))
    })
    .await
}

/// `search_issues`: with a project, its tag values (none: no issues); across projects, tags by key and value.
async fn search_issues(
    c: &Ctx,
    project: Option<i64>,
    q: &str,
    query: &mut QueryBuilder<Postgres>,
) -> Result<(), Drf> {
    if q.is_empty() {
        return Ok(());
    }
    if let Some(project_id) = project {
        let search = Search::prepare(c.pool(), project_id, q).await?;
        // `push_conditions` works on unqualified columns
        query.push(" AND i.id IN (SELECT id FROM issues_issue WHERE true");
        search.push_conditions(query, Searched::Issues);
        query.push(")");
        return Ok(());
    }
    let ParsedQuery { tags, plain_text } = parse_query(q);
    for (key, value) in tags {
        query.push(
            " AND i.id IN (SELECT it.issue_id FROM tags_issuetag it JOIN tags_tagvalue v ON v.id = it.value_id
              JOIN tags_tagkey k ON k.id = v.key_id WHERE k.key = ",
        );
        query.push_bind(key).push(" AND v.value = ").push_bind(value).push(")");
    }
    if !plain_text.is_empty() {
        crate::search::push_icontains(query, "i.", &crate::search::contains_pattern(&plain_text));
    }
    Ok(())
}

static DETAIL: Route = Route {
    allow: "GET, DELETE, HEAD, OPTIONS",
    name: "Issue Instance",
    methods: &[("GET", Some("issues:read")), ("DELETE", Some("issues:delete"))],
};

pub async fn retrieve(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &DETAIL, |c| async move {
        if c.method() == "DELETE" {
            let (id, _) = guarded_issue(&c, &pk, "issues:delete").await?;
            // `delete_deferred`
            triage::apply_to_issue(c.pool(), id, &Action::Delete, None, Utc::now()).await.map_err(Drf::internal)?;
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let (id, _) = guarded_issue(&c, &pk, "issues:read").await?;
        ok(issue_json(&issue_row(&c, id).await?))
    })
    .await
}

static TAGS: Route = Route { allow: "GET, HEAD, OPTIONS", name: "Tags", methods: &[("GET", Some("events:read"))] };

#[derive(sqlx::FromRow)]
struct TagRow {
    id: i64,
    key: String,
    value: String,
    count: i32,
}

/// The issue's tag values with their counts, by id (`?key=` for one key).
pub async fn tags(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &TAGS, |c| async move {
        let (id, _) = guarded_issue(&c, &pk, "events:read").await?;
        let mut query = QueryBuilder::<Postgres>::new(
            "SELECT it.id, k.key, v.value, it.count FROM tags_issuetag it JOIN tags_tagvalue v ON v.id = it.value_id
             JOIN tags_tagkey k ON k.id = it.key_id WHERE it.issue_id = ",
        );
        query.push_bind(id);
        if let Some(key) = c.req.query.get("key") {
            query.push(" AND k.key = ").push_bind(key.to_owned());
        }
        let ordering = Ordering { fields: vec![("it.id", false)], cast: "bigint" };
        let page = pagination::paginate(c.pool(), &c.req, query, &ordering, |r: &TagRow| r.id.to_string()).await?;
        let results =
            page.items.iter().map(|t| json!({"id": t.id, "key": t.key, "value": t.value, "count": t.count})).collect();
        ok(page.response(&c.req, results))
    })
    .await
}

static GROUPING: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Grouping", methods: &[("GET", Some("events:read"))] };

#[derive(sqlx::FromRow)]
struct GroupingRow {
    id: i64,
    grouping_key: String,
    grouping_mechanism: Option<String>,
}

/// The issue's grouping keys, by id.
pub async fn grouping(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &GROUPING, |c| async move {
        let (id, _) = guarded_issue(&c, &pk, "events:read").await?;
        let mut query = QueryBuilder::<Postgres>::new(
            "SELECT id, grouping_key, grouping_mechanism FROM issues_grouping WHERE issue_id = ",
        );
        query.push_bind(id);
        let ordering = Ordering { fields: vec![("id", false)], cast: "bigint" };
        let page = pagination::paginate(c.pool(), &c.req, query, &ordering, |r: &GroupingRow| r.id.to_string()).await?;
        let results = page
            .items
            .iter()
            .map(|g| json!({"id": g.id, "grouping_key": g.grouping_key, "grouping_mechanism": g.grouping_mechanism}))
            .collect();
        ok(page.response(&c.req, results))
    })
    .await
}

static COUNTS: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Event counts", methods: &[("GET", Some("issues:read"))] };

pub async fn event_counts(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &COUNTS, |c| async move {
        let (id, _) = guarded_issue(&c, &pk, "issues:read").await?;
        super::event_counts(&c, CountsOf::Issue(id)).await
    })
    .await
}

/// The triage actions by their URL's name, and their views' names.
static TRIAGE: [(&str, Route); 8] = [
    ("resolve", triage_route("Resolve")),
    ("resolve-next", triage_route("Resolve next")),
    ("resolve-latest", triage_route("Resolve latest")),
    ("reopen", triage_route("Reopen")),
    ("mute", triage_route("Mute")),
    ("mute-for", triage_route("Mute for")),
    ("mute-until", triage_route("Mute until")),
    ("unmute", triage_route("Unmute")),
];

const fn triage_route(name: &'static str) -> Route {
    Route { allow: "POST, OPTIONS", name, methods: &[("POST", Some("issues:triage"))] }
}

/// The triage actions: `POST /issues/{id}/resolve/` etc. Each refuses what doesn't apply to the issue's state.
pub async fn action(
    State(state): State<ApiState>,
    Path((pk, action)): Path<(String, String)>,
    req: Request,
) -> Response {
    let Some((_, route)) = TRIAGE.iter().find(|(name, _)| *name == action) else {
        return Drf::not_found().into_response();
    };
    serve(state, req, route, |c| async move {
        let (id, project_id) = guarded_issue(&c, &pk, "issues:triage").await?;
        let issue = issue_row(&c, id).await?;
        let refuse = |detail: &str| Drf::invalid(json!({ "detail": detail }));
        let unresolved = || if issue.is_resolved { Err(refuse("Issue is already resolved.")) } else { Ok(()) };
        let unmuted = || if issue.is_muted { Err(refuse("Issue is already muted.")) } else { Ok(()) };
        let todo = match action.as_str() {
            "resolve" => {
                unresolved()?;
                Action::Resolve
            }
            "resolve-next" => {
                unresolved()?;
                Action::ResolvedNext
            }
            "resolve-latest" => {
                unresolved()?;
                let has_releases: bool = sqlx::query_scalar("SELECT has_releases FROM projects_project WHERE id = $1")
                    .bind(project_id)
                    .fetch_one(c.pool())
                    .await?;
                if !has_releases {
                    return Err(refuse("Project has no releases."));
                }
                let latest = crate::releases::latest_release_version(c.pool(), project_id).await?;
                Action::ResolvedRelease(latest.unwrap_or_default())
            }
            "reopen" => {
                if !issue.is_resolved {
                    return Err(refuse("Issue is not resolved."));
                }
                Action::Reopen
            }
            "mute" => {
                unresolved()?;
                unmuted()?;
                Action::Mute
            }
            "mute-for" | "mute-until" => {
                let data = c.req.data()?;
                let data = object_body(&data)?;
                let mut errors = Errors::default();
                const PERIODS: &[(&str, i32)] =
                    &[("year", 0), ("month", 1), ("week", 2), ("day", 3), ("hour", 4), ("minute", 5)];
                let positive = |v: &Value| fields::int_field(v, Some(1), None);
                let period = required(data, "period_name", |v| fields::choice_field(v, PERIODS));
                let period = errors.take("period_name", period);
                let nr = errors.take("nr_of_periods", required(data, "nr_of_periods", positive));
                let gte = if action == "mute-until" {
                    errors.take("gte_threshold", required(data, "gte_threshold", positive))
                } else {
                    Some(0)
                };
                let (Some(period), Some(nr), Some(gte)) = (period, nr, gte) else {
                    return Err(Drf::invalid(errors.into_value()));
                };
                unresolved()?;
                unmuted()?;
                let name = PERIODS[period as usize].0;
                let spec = if action == "mute-for" {
                    format!("mute_for:{name},{nr},")
                } else {
                    format!("mute_until:{name},{nr},{gte}")
                };
                Action::parse(&spec).ok_or_else(|| Drf::internal(format!("unsupported action {spec}")))?
            }
            _ => {
                unresolved()?;
                if !issue.is_muted {
                    return Err(refuse("Issue is not muted."));
                }
                Action::Unmute
            }
        };
        triage::apply_to_issue(c.pool(), id, &todo, None, Utc::now()).await.map_err(Drf::internal)?;
        ok(issue_json(&issue_row(&c, id).await?))
    })
    .await
}

static COMMENTS: Route =
    Route { allow: "POST, OPTIONS", name: "Issue Comment List", methods: &[("POST", Some("issues:comment"))] };

/// A comment on an issue (by UUID or friendly id); comments made with a token have no user.
pub async fn comment(State(state): State<ApiState>, req: Request) -> Response {
    serve(state, req, &COMMENTS, |c| async move {
        let data = c.req.data()?;
        let data = object_body(&data)?;
        let mut errors = Errors::default();
        let issue = match required(data, "issue", |v| fields::char_field(v, false, None)) {
            Err(messages) => {
                errors.add("issue", messages);
                None
            }
            Ok(identifier) => match issue_key(&identifier) {
                None => {
                    errors.add("issue", vec!["Invalid issue identifier.".into()]);
                    None
                }
                Some(key) => match super::find_issue(&c, &key).await? {
                    None => {
                        errors.add("issue", vec!["Issue not found.".into()]);
                        None
                    }
                    found => found,
                },
            },
        };
        let text = errors.take("comment", required(data, "comment", |v| fields::char_field(v, false, None)));
        let (Some((issue_id, project_id)), Some(text)) = (issue, text) else {
            return Err(Drf::invalid(errors.into_value()));
        };
        guard_project(&c, "issues:comment", project_id).await?;
        let now = Utc::now().trunc_subsecs(6);
        let id = triage::pages::insert_comment(c.pool(), project_id, issue_id, None, &text, now).await?;
        Ok(render(
            StatusCode::CREATED,
            &json!({
                "id": id,
                "issue": issue_id,
                "project": project_id,
                "timestamp": drf_datetime(&now),
                "comment": text,
                "user": null,
            }),
        ))
    })
    .await
}

static HISTORY: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Issue History List", methods: &[("GET", Some("issues:read"))] };

#[derive(sqlx::FromRow)]
struct TurningPointRow {
    id: i64,
    issue_id: Uuid,
    project_id: i64,
    triggering_event_id: Option<Uuid>,
    user_id: Option<i64>,
    first_name: Option<String>,
    last_name: Option<String>,
    username: Option<String>,
    timestamp: DateTime<Utc>,
    kind: i32,
    comment: Option<String>,
    metadata: String,
}

/// `TurningPointKind(kind).name.lower()`.
fn kind_name(kind: i32) -> &'static str {
    match kind {
        1 => "first_seen",
        2 => "resolved",
        3 => "muted",
        4 => "regressed",
        5 => "unmuted",
        6 => "reopened",
        10 => "next_materialized",
        100 => "manual_annotation",
        _ => "unknown",
    }
}

/// An issue's turning points (`?issue=`, required), newest first by default.
pub async fn history(State(state): State<ApiState>, req: Request) -> Response {
    serve(state, req, &HISTORY, |c| async move {
        let identifier = super::required_param(&c.req, "issue")?;
        let (issue_id, _) = guarded_issue(&c, identifier, "issues:read").await?;
        let ordering = Ordering::asc_desc(&c.req, &["tp.timestamp", "tp.id"], "timestamptz", true)?;
        let mut query = QueryBuilder::<Postgres>::new(
            "SELECT tp.id, tp.issue_id, tp.project_id, tp.triggering_event_id, tp.user_id, u.first_name, u.last_name,
                    u.username, tp.timestamp, tp.kind, tp.comment, tp.metadata
             FROM issues_turningpoint tp LEFT JOIN auth_user u ON u.id = tp.user_id WHERE tp.issue_id = ",
        );
        query.push_bind(issue_id);
        let position = |r: &TurningPointRow| str_datetime(&r.timestamp);
        let page = pagination::paginate(c.pool(), &c.req, query, &ordering, position).await?;
        let results = page
            .items
            .iter()
            .map(|t| {
                let display_name = t.username.as_ref().map(|username| {
                    let (first, last) = (t.first_name.as_deref().unwrap_or(""), t.last_name.as_deref().unwrap_or(""));
                    crate::accounts::display_name(first, last, username)
                });
                json!({
                    "id": t.id,
                    "issue": t.issue_id,
                    "project": t.project_id,
                    "triggering_event": t.triggering_event_id,
                    "user": t.user_id,
                    "user_display_name": display_name,
                    "timestamp": drf_datetime(&t.timestamp),
                    "kind": kind_name(t.kind),
                    "comment": t.comment,
                    "metadata": serde_json::from_str::<Value>(&t.metadata).unwrap_or(Value::Null),
                })
            })
            .collect();
        ok(page.response(&c.req, results))
    })
    .await
}
