//! `projects/api_views.py`, `teams/api_views.py` and `releases/api_views.py`: listing, creating, reading and
//! updating projects and teams; a project's event counts; a project's releases.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use bugsink_db::Postgres;
use chrono::{DateTime, SubsecRound, Utc};
use serde_json::{Map, Value, json};
use sqlx::QueryBuilder;
use uuid::Uuid;

use super::fields::{self, Errors, optional, present, required};
use super::pagination::{self, Ordering};
use super::{
    CountsOf, Ctx, Drf, PROJECT_VISIBLE, Reply, Request, Route, TEAM_VISIBLE, enforce_user_boundness_for_team,
    guard_project, guarded_project, object_body, ok, render, serve, uuid_pk,
};
use crate::api::ApiState;
use crate::py::{drf_datetime, py_uuid, str_datetime};

const PROJECT_VISIBILITIES: &[(&str, i32)] = &[("joinable", 1), ("discoverable", 10), ("team_members", 99)];
const TEAM_VISIBILITIES: &[(&str, i32)] = &[("joinable", 1), ("discoverable", 10), ("hidden", 99)];

fn visibility_name(choices: &[(&'static str, i32)], value: i32) -> Result<&'static str, Drf> {
    choices
        .iter()
        .find(|(_, v)| *v == value)
        .map(|(name, _)| *name)
        .ok_or_else(|| Drf::internal(format!("visibility {value} is not a choice")))
}

/// `?expand=`: the fields asked for (a single comma-separated value is split).
fn expand(req: &Request) -> Vec<String> {
    let values = req.query.get_list("expand");
    match values.as_slice() {
        [one] if one.contains(',') => one.split(',').map(str::to_owned).collect(),
        _ => values.into_iter().map(str::to_owned).collect(),
    }
}

/// `ExpandViewSetMixin` on a serializer without expandable fields.
fn refuse_expand(req: &Request) -> Result<(), Drf> {
    if expand(req).is_empty() {
        Ok(())
    } else {
        Err(Drf::invalid(json!({"expand": ["Expansions are not supported on this endpoint."]})))
    }
}

// --- projects -----------------------------------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id: i64,
    team_id: Option<Uuid>,
    name: String,
    slug: String,
    sentry_key: Uuid,
    digested_event_count: i32,
    stored_event_count: i32,
    alert_on_new_issue: bool,
    alert_on_regression: bool,
    alert_on_unmute: bool,
    visibility: i32,
    retention_max_event_count: i32,
    grouping_mechanism: String,
}

const PROJECT_COLUMNS: &str = "p.id, p.team_id, p.name, p.slug, p.sentry_key, p.digested_event_count,
    p.stored_event_count, p.alert_on_new_issue, p.alert_on_regression, p.alert_on_unmute, p.visibility,
    p.retention_max_event_count, p.grouping_mechanism";

/// `ProjectListSerializer` / `ProjectDetailSerializer`.
fn project_json(c: &Ctx, p: &ProjectRow) -> Result<Map<String, Value>, Drf> {
    let Value::Object(map) = json!({
        "id": p.id,
        "team": p.team_id,
        "name": p.name,
        "slug": p.slug,
        "dsn": crate::projects::dsn(c.state.settings.alerts.base_url(), p.id, p.sentry_key),
        "digested_event_count": p.digested_event_count,
        "stored_event_count": p.stored_event_count,
        "alert_on_new_issue": p.alert_on_new_issue,
        "alert_on_regression": p.alert_on_regression,
        "alert_on_unmute": p.alert_on_unmute,
        "visibility": visibility_name(PROJECT_VISIBILITIES, p.visibility)?,
        "retention_max_event_count": p.retention_max_event_count,
        "grouping_mechanism": p.grouping_mechanism,
    }) else {
        unreachable!()
    };
    Ok(map)
}

/// `ProjectCreateSerializer` / `ProjectUpdateSerializer`: what was written (the id as a string: a `UUIDField`).
fn written_project_json(p: &ProjectRow) -> Result<Value, Drf> {
    Ok(json!({
        "id": p.id.to_string(),
        "team": p.team_id,
        "name": p.name,
        "visibility": visibility_name(PROJECT_VISIBILITIES, p.visibility)?,
        "alert_on_new_issue": p.alert_on_new_issue,
        "alert_on_regression": p.alert_on_regression,
        "alert_on_unmute": p.alert_on_unmute,
        "retention_max_event_count": p.retention_max_event_count,
    }))
}

async fn project_row(c: &Ctx, id: i64) -> sqlx::Result<ProjectRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {PROJECT_COLUMNS} FROM projects_project p WHERE p.id = $1")))
        .bind(id)
        .fetch_one(c.pool())
        .await
}

static PROJECTS: Route = Route {
    allow: "GET, POST, HEAD, OPTIONS",
    name: "Project List",
    methods: &[("GET", Some("projects:read")), ("POST", Some("projects:manage"))],
};

pub async fn projects(State(state): State<ApiState>, req: Request) -> Response {
    serve(state, req, &PROJECTS, |c| async move {
        if c.method() == "POST" { create_project(&c).await } else { list_projects(&c).await }
    })
    .await
}

/// The projects (not deleted) the token reaches, by name; `?team=` for one team's.
async fn list_projects(c: &Ctx) -> Reply {
    let mut query = QueryBuilder::<Postgres>::new(format!(
        "SELECT {PROJECT_COLUMNS} FROM projects_project p WHERE NOT p.is_deleted"
    ));
    if let Some(bound) = c.token.project_id {
        query.push(" AND p.id = ").push_bind(bound);
    }
    if let Some(user) = c.token.user.as_ref().filter(|u| !u.is_superuser) {
        // the user's id in place of PROJECT_VISIBLE's $1 (this query numbers its own binds)
        query.push(format!(" AND {}", PROJECT_VISIBLE.replace("$1", &user.id.to_string())));
    }
    if let Some(team) = c.req.query.get("team").filter(|t| !t.is_empty()) {
        // upstream filters on the string as is: a malformed UUID is a 500
        let team = py_uuid(team).ok_or_else(|| Drf::internal(format!("“{team}” is not a valid UUID.")))?;
        query.push(" AND p.team_id = ").push_bind(team);
    }
    let ordering = Ordering::asc_desc(&c.req, &["p.name"], "text", false)?;
    let page = pagination::paginate(c.pool(), &c.req, query, &ordering, |p: &ProjectRow| p.name.clone()).await?;
    refuse_expand(&c.req)?;
    let results = page.items.iter().map(|p| project_json(c, p).map(Value::Object)).collect::<Result<_, _>>()?;
    ok(page.response(&c.req, results))
}

/// The fields a project is created or updated with (`ProjectCreateSerializer`), validated; on update, only those
/// given. Some(team) on create.
struct ProjectFields {
    team: Option<Uuid>,
    name: Option<String>,
    visibility: Option<i32>,
    alerts: [Option<bool>; 3],
    retention: Option<i64>,
}

const ALERTS: [&str; 3] = ["alert_on_new_issue", "alert_on_regression", "alert_on_unmute"];

/// `None` for the project being updated.
async fn project_fields(
    c: &Ctx,
    data: &Map<String, Value>,
    instance: Option<(i64, Option<Uuid>)>,
) -> Result<ProjectFields, Drf> {
    let creating = instance.is_none();
    let mut errors = Errors::default();
    let mut team = None;
    if creating {
        match required(data, "team", fields::uuid_pk) {
            Err(messages) => errors.add("team", messages),
            Ok(id) => {
                let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM teams_team WHERE id = $1)")
                    .bind(id)
                    .fetch_one(c.pool())
                    .await?;
                if exists {
                    team = Some(id);
                } else {
                    errors.add("team", fields::does_not_exist(&data["team"]));
                }
            }
        }
    }
    let name = optional(data, "name", creating, |v| fields::char_field(v, false, Some(255)));
    let name = errors.take("name", name).flatten();
    let visibility = optional(data, "visibility", false, |v| fields::choice_field(v, PROJECT_VISIBILITIES));
    let visibility = errors.take("visibility", visibility).flatten();
    let mut alerts = [None; 3];
    for (i, field) in ALERTS.iter().enumerate() {
        alerts[i] = errors.take(field, optional(data, field, false, fields::bool_field)).flatten();
    }
    let retention =
        optional(data, "retention_max_event_count", false, |v| fields::int_field(v, Some(0), Some(2147483647)));
    let mut retention = errors.take("retention_max_event_count", retention).flatten();
    // `validate_retention_max_event_count`
    if let Some(value) = retention {
        let quotas = &c.state.settings.quotas;
        let project_id = instance.map(|(id, _)| id);
        let within = crate::projects::retention_within_budget(c.pool(), quotas, value, project_id, None).await?;
        if let Err(message) = within {
            errors.add("retention_max_event_count", vec![message]);
            retention = None;
        }
    }
    if !errors.is_empty() {
        return Err(Drf::invalid(errors.into_value()));
    }
    // the (team, name) `UniqueTogetherValidator`; no check without a team
    let (unique_team, unique_name) = match instance {
        None => (team, name.clone()),
        Some((id, instance_team)) => {
            let current: String = sqlx::query_scalar("SELECT name FROM projects_project WHERE id = $1")
                .bind(id)
                .fetch_one(c.pool())
                .await?;
            (instance_team, Some(name.clone().unwrap_or(current)))
        }
    };
    if let (Some(team), Some(name)) = (unique_team, unique_name) {
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM projects_project
                            WHERE team_id = $1 AND name = $2 AND id IS DISTINCT FROM $3)",
        )
        .bind(team)
        .bind(&name)
        .bind(instance.map(|(id, _)| id))
        .fetch_one(c.pool())
        .await?;
        if taken {
            return Err(Drf::invalid(json!({"non_field_errors": ["The fields team, name must make a unique set."]})));
        }
    }
    Ok(ProjectFields { team, name, visibility, alerts, retention })
}

/// A project in a team (by a team admin, for a personal token). Nobody becomes a member.
async fn create_project(c: &Ctx) -> Reply {
    let data = c.req.data()?;
    refuse_expand(&c.req)?;
    let fields = project_fields(c, object_body(&data)?, None).await?;
    let team = fields.team.expect("required on create");
    enforce_user_boundness_for_team(c, "projects:manage", team).await?;
    let name = fields.name.expect("required on create");
    let mut tx = c.pool().begin().await?;
    let slug = crate::projects::unique_slug(&mut tx, &name).await?;
    // the model's defaults for what wasn't given
    let new = crate::projects::NewProject {
        team_id: Some(team),
        name: &name,
        slug: &slug,
        alerts: fields.alerts.map(|a| a.unwrap_or(true)),
        visibility: fields.visibility.unwrap_or(bugsink_db::PROJECT_VISIBILITY_TEAM_MEMBERS),
        retention_max_event_count: fields.retention.unwrap_or(10_000) as i32,
        grouping_mechanism: crate::grouping::BUGSINK_V2,
    };
    let (id, _) = crate::projects::insert_project(&mut tx, &new).await?;
    tx.commit().await?;
    Ok(render(StatusCode::CREATED, &written_project_json(&project_row(c, id).await?)?))
}

static PROJECT: Route = Route {
    allow: "GET, PATCH, HEAD, OPTIONS",
    name: "Project Instance",
    methods: &[("GET", Some("projects:read")), ("PATCH", Some("projects:manage"))],
};

pub async fn project(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &PROJECT, |c| async move {
        let capability = if c.method() == "PATCH" { "projects:manage" } else { "projects:read" };
        let id = guarded_project(&c, &pk, true, capability).await?;
        if c.method() == "PATCH" {
            return update_project(&c, id).await;
        }
        let project = project_row(&c, id).await?;
        let mut map = project_json(&c, &project)?;
        let expand = expand(&c.req);
        if !expand.is_empty() {
            let unknown: Vec<String> =
                expand.iter().filter(|f| *f != "team").map(|f| format!("Unknown field: {f}")).collect();
            if !unknown.is_empty() {
                return Err(Drf::invalid(json!({ "expand": unknown })));
            }
            // `TeamDetailSerializer(None).data` for a project without a team: its fields' initial values
            let team = match project.team_id {
                None => json!({"name": "", "visibility": null}),
                Some(team) => team_json(&team_row(&c, team).await?)?,
            };
            map.insert("team".into(), team);
        }
        ok(Value::Object(map))
    })
    .await
}

/// The given fields (the team stays).
async fn update_project(c: &Ctx, id: i64) -> Reply {
    let data = c.req.data()?;
    refuse_expand(&c.req)?;
    let current = project_row(c, id).await?;
    let fields = project_fields(c, object_body(&data)?, Some((id, current.team_id))).await?;
    let [new_issue, regression, unmute] = fields.alerts;
    sqlx::query(
        "UPDATE projects_project SET name = coalesce($2, name), visibility = coalesce($3, visibility),
             alert_on_new_issue = coalesce($4, alert_on_new_issue),
             alert_on_regression = coalesce($5, alert_on_regression),
             alert_on_unmute = coalesce($6, alert_on_unmute),
             retention_max_event_count = coalesce($7, retention_max_event_count)
         WHERE id = $1",
    )
    .bind(id)
    .bind(fields.name)
    .bind(fields.visibility)
    .bind(new_issue)
    .bind(regression)
    .bind(unmute)
    .bind(fields.retention.map(|r| r as i32))
    .execute(c.pool())
    .await?;
    ok(written_project_json(&project_row(c, id).await?)?)
}

static PROJECT_COUNTS: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Event counts", methods: &[("GET", Some("projects:read"))] };

pub async fn project_event_counts(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &PROJECT_COUNTS, |c| async move {
        let id = guarded_project(&c, &pk, true, "projects:read").await?;
        super::event_counts(&c, CountsOf::Project(id)).await
    })
    .await
}

// --- teams --------------------------------------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct TeamRow {
    id: Uuid,
    name: String,
    visibility: i32,
}

fn team_json(t: &TeamRow) -> Result<Value, Drf> {
    Ok(json!({"id": t.id, "name": t.name, "visibility": visibility_name(TEAM_VISIBILITIES, t.visibility)?}))
}

async fn team_row(c: &Ctx, id: Uuid) -> sqlx::Result<TeamRow> {
    sqlx::query_as("SELECT id, name, visibility FROM teams_team WHERE id = $1").bind(id).fetch_one(c.pool()).await
}

static TEAMS: Route = Route {
    allow: "GET, POST, HEAD, OPTIONS",
    name: "Team List",
    methods: &[("GET", Some("teams:read")), ("POST", Some("teams:manage"))],
};

pub async fn teams(State(state): State<ApiState>, req: Request) -> Response {
    serve(state, req, &TEAMS, |c| async move {
        if c.method() == "POST" {
            if let Some(user) = &c.token.user
                && !c.state.settings.users.user_can_create_team(user)
            {
                return Err(Drf::forbidden("The user bound to this token is not allowed to create teams."));
            }
            let data = c.req.data()?;
            let (name, visibility) = team_fields(&c, object_body(&data)?, None).await?;
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO teams_team (id, name, visibility) VALUES ($1, $2, $3)")
                .bind(id)
                .bind(name)
                .bind(visibility.unwrap_or(10))
                .execute(c.pool())
                .await?;
            return Ok(render(StatusCode::CREATED, &team_json(&team_row(&c, id).await?)?));
        }
        let mut query = QueryBuilder::<Postgres>::new("SELECT t.id, t.name, t.visibility FROM teams_team t WHERE true");
        if let Some(user) = c.token.user.as_ref().filter(|u| !u.is_superuser) {
            query.push(format!(" AND {}", TEAM_VISIBLE.replace("$1", &user.id.to_string())));
        }
        let ordering = Ordering::asc_desc(&c.req, &["t.name"], "text", false)?;
        let page = pagination::paginate(c.pool(), &c.req, query, &ordering, |t: &TeamRow| t.name.clone()).await?;
        let results = page.items.iter().map(team_json).collect::<Result<_, _>>()?;
        ok(page.response(&c.req, results))
    })
    .await
}

/// `TeamCreateUpdateSerializer`: (name, visibility), each None when not given on update.
async fn team_fields(
    c: &Ctx,
    data: &Map<String, Value>,
    instance: Option<Uuid>,
) -> Result<(Option<String>, Option<i32>), Drf> {
    let mut errors = Errors::default();
    let name = match present(data, "name", instance.is_none()) {
        Err(messages) => {
            errors.add("name", messages);
            None
        }
        Ok(None) => None,
        Ok(Some(v)) => {
            // the model's unique name (UniqueValidator first), then the field's own checks
            let mut messages = Vec::new();
            let text = match v {
                Value::String(s) => Some(crate::py::strip(s).to_owned()),
                Value::Number(_) => Some(fields::str_of(v)),
                _ => None,
            };
            let valid = fields::char_field(v, false, Some(255));
            if let (Ok(_), Some(text)) = (&valid, &text) {
                let taken: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM teams_team WHERE name = $1 AND id IS DISTINCT FROM $2)",
                )
                .bind(text)
                .bind(instance)
                .fetch_one(c.pool())
                .await?;
                if taken {
                    messages.push("team with this Name already exists.".to_owned());
                }
            }
            match valid {
                Err(field_messages) => {
                    errors.add("name", field_messages);
                    None
                }
                Ok(_) if !messages.is_empty() => {
                    errors.add("name", messages);
                    None
                }
                Ok(name) => Some(name),
            }
        }
    };
    let visibility = optional(data, "visibility", false, |v| fields::choice_field(v, TEAM_VISIBILITIES));
    let visibility = errors.take("visibility", visibility).flatten();
    if !errors.is_empty() {
        return Err(Drf::invalid(errors.into_value()));
    }
    Ok((name, visibility))
}

static TEAM: Route = Route {
    allow: "GET, PATCH, HEAD, OPTIONS",
    name: "Team Instance",
    methods: &[("GET", Some("teams:read")), ("PATCH", Some("teams:manage"))],
};

pub async fn team(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &TEAM, |c| async move {
        let capability = if c.method() == "PATCH" { "teams:manage" } else { "teams:read" };
        let id = uuid_pk(&pk)?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM teams_team WHERE id = $1)")
            .bind(id)
            .fetch_one(c.pool())
            .await?;
        if !exists {
            return Err(Drf::no_match("Team"));
        }
        enforce_user_boundness_for_team(&c, capability, id).await?;
        if c.method() == "PATCH" {
            let data = c.req.data()?;
            let (name, visibility) = team_fields(&c, object_body(&data)?, Some(id)).await?;
            sqlx::query(
                "UPDATE teams_team SET name = coalesce($2, name), visibility = coalesce($3, visibility) WHERE id = $1",
            )
            .bind(id)
            .bind(name)
            .bind(visibility)
            .execute(c.pool())
            .await?;
        }
        ok(team_json(&team_row(&c, id).await?)?)
    })
    .await
}

// --- releases -----------------------------------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ReleaseRow {
    id: Uuid,
    project_id: i64,
    version: String,
    date_released: DateTime<Utc>,
    semver: String,
    is_semver: bool,
    sort_epoch: i32,
}

fn release_json(r: &ReleaseRow, detail: bool) -> Value {
    let mut v = json!({
        "id": r.id,
        "project": r.project_id,
        "version": r.version,
        "date_released": drf_datetime(&r.date_released),
    });
    if detail {
        v["semver"] = json!(r.semver);
        v["is_semver"] = json!(r.is_semver);
        v["sort_epoch"] = json!(r.sort_epoch);
    }
    v
}

const RELEASE_SELECT: &str =
    "SELECT id, project_id, version, date_released, semver, is_semver, sort_epoch FROM releases_release";

static RELEASES: Route = Route {
    allow: "GET, POST, HEAD, OPTIONS",
    name: "Release List",
    methods: &[("GET", Some("releases:read")), ("POST", Some("releases:create"))],
};

pub async fn releases(State(state): State<ApiState>, req: Request) -> Response {
    serve(state, req, &RELEASES, |c| async move {
        if c.method() == "POST" {
            return create_release(&c).await;
        }
        let project = super::required_param(&c.req, "project")?;
        let project_id = guarded_project(&c, project, true, "releases:read").await?;
        let mut query = QueryBuilder::<Postgres>::new(format!("{RELEASE_SELECT} WHERE project_id = "));
        query.push_bind(project_id);
        let ordering = Ordering::asc_desc(&c.req, &["date_released"], "timestamptz", true)?;
        let page = pagination::paginate(c.pool(), &c.req, query, &ordering, |r: &ReleaseRow| {
            str_datetime(&r.date_released)
        })
        .await?;
        let results = page.items.iter().map(|r| release_json(r, false)).collect();
        ok(page.response(&c.req, results))
    })
    .await
}

/// `ReleaseCreateSerializer`: a release that doesn't exist yet (on a project the token administers).
async fn create_release(c: &Ctx) -> Reply {
    let data = c.req.data()?;
    let data = object_body(&data)?;
    let mut errors = Errors::default();
    let mut project = None;
    match required(data, "project", fields::int_pk) {
        Err(messages) => errors.add("project", messages),
        Ok(id) => {
            let found: Option<i64> = match id {
                Some(id) => sqlx::query_scalar("SELECT id FROM projects_project WHERE id = $1")
                    .bind(id)
                    .fetch_optional(c.pool())
                    .await?,
                None => None,
            };
            match found {
                Some(id) => project = Some(id),
                None => errors.add("project", fields::does_not_exist(&data["project"])),
            }
        }
    }
    let version = errors.take("version", required(data, "version", |v| fields::char_field(v, true, None)));
    let timestamp = errors.take("timestamp", optional(data, "timestamp", false, fields::datetime_field));
    let (Some(project_id), Some(version), Some(timestamp)) = (project, version, timestamp) else {
        return Err(Drf::invalid(errors.into_value()));
    };
    guard_project(c, "releases:create", project_id).await?;
    let mut tx = c.pool().begin().await?;
    let mut has_releases: bool =
        sqlx::query_scalar("SELECT has_releases FROM projects_project WHERE id = $1 FOR UPDATE")
            .bind(project_id)
            .fetch_one(&mut *tx)
            .await?;
    let timestamp = timestamp.unwrap_or_else(|| Utc::now().trunc_subsecs(6));
    let (version, created) = crate::releases::create_release_if_needed(
        &mut tx,
        project_id,
        &mut has_releases,
        &version,
        timestamp,
        None,
    )
    .await?;
    if !created {
        return Err(Drf::invalid(json!({"version": ["Release with this version already exists for the project."]})));
    }
    tx.commit().await?;
    let release: ReleaseRow =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{RELEASE_SELECT} WHERE project_id = $1 AND version = $2")))
            .bind(project_id)
            .bind(&version)
            .fetch_one(c.pool())
            .await?;
    Ok(render(StatusCode::CREATED, &release_json(&release, true)))
}

static RELEASE: Route =
    Route { allow: "GET, HEAD, OPTIONS", name: "Release Instance", methods: &[("GET", Some("releases:read"))] };

pub async fn release(State(state): State<ApiState>, Path(pk): Path<String>, req: Request) -> Response {
    serve(state, req, &RELEASE, |c| async move {
        let id = uuid_pk(&pk)?;
        let release: ReleaseRow = sqlx::query_as(sqlx::AssertSqlSafe(format!("{RELEASE_SELECT} WHERE id = $1")))
            .bind(id)
            .fetch_optional(c.pool())
            .await?
            .ok_or_else(|| Drf::no_match("Release"))?;
        guard_project(&c, "releases:read", release.project_id).await?;
        ok(release_json(&release, true))
    })
    .await
}
