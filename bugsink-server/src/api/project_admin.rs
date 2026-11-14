//! Projects: the list (mine/teams/other), creating, settings, deleting, joining, leaving and the SDK setup page's DSN.
//! Members: `members.rs`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{ApiError, ApiResult, ApiState, require_project_access};
use crate::accounts::{CurrentUser, User};
use crate::alerts::{messaging, setup};
use crate::issues::pages;
use crate::memberships::{self, Group};
use crate::projects::{self, ProjectForm, ProjectListItem, ProjectSettings};

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/projects", get(list).post(create))
        .route("/api/v1/projects/new", get(new_options))
        .route("/api/v1/projects/{project_id}", get(settings).patch(edit).delete(delete))
        .route("/api/v1/projects/{project_id}/join", post(join))
        .route("/api/v1/projects/{project_id}/leave", post(leave))
        .route("/api/v1/projects/{project_id}/sdk-setup", get(sdk_setup))
        .route("/api/v1/projects/{project_id}/messaging-services", get(services).post(add_service))
        .route(
            "/api/v1/projects/{project_id}/messaging-services/{service_id}",
            axum::routing::patch(edit_service).delete(remove_service),
        )
        .route("/api/v1/projects/{project_id}/messaging-services/{service_id}/test", post(test_service))
}

#[derive(Deserialize)]
struct TabQuery {
    tab: Option<String>,
}

#[derive(Serialize)]
struct ListItem {
    #[serde(flatten)]
    project: ProjectListItem,
    /// Events in each of the last 24 hours, the current one last.
    sparkline: Vec<i32>,
}

#[derive(Serialize)]
struct ProjectList {
    /// "mine", "teams" or "other": the one asked for, or upstream's default.
    tab: &'static str,
    can_create: bool,
    items: Vec<ListItem>,
}

async fn list(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Query(query): Query<TabQuery>,
) -> ApiResult<ProjectList> {
    let (tab, items) = projects::projects(&state.pool, &user, query.tab.as_deref()).await?;
    let ids: Vec<i64> = items.iter().map(|p| p.id).collect();
    let mut sparklines = pages::project_list_sparklines(&state.pool, &ids, Utc::now()).await?;
    let items = items
        .into_iter()
        .map(|project| ListItem { sparkline: sparklines.remove(&project.id).unwrap_or_default(), project })
        .collect();
    Ok(Json(ProjectList { tab, can_create: projects::can_create_project(&state.pool, &user).await?, items }))
}

#[derive(Serialize)]
struct Team {
    id: Uuid,
    name: String,
}

/// What the new-project form offers: the teams it can go in, and the initial retention.
#[derive(Serialize)]
struct NewOptions {
    teams: Vec<Team>,
    retention_max_event_count: i64,
    grouping_mechanism: &'static str,
}

async fn require_can_create(state: &ApiState, user: &User) -> Result<(), ApiError> {
    if !projects::can_create_project(&state.pool, user).await? {
        return Err(ApiError::Forbidden("You need to be a team admin to create a project".into()));
    }
    Ok(())
}

async fn new_options(State(state): State<ApiState>, CurrentUser { user, .. }: CurrentUser) -> ApiResult<NewOptions> {
    require_can_create(&state, &user).await?;
    let teams = projects::teams_for_new_project(&state.pool, &state.settings.users, &user).await?;
    Ok(Json(NewOptions {
        teams: teams.into_iter().map(|(id, name)| Team { id, name }).collect(),
        retention_max_event_count: projects::default_retention(&state.settings.quotas),
        grouping_mechanism: crate::grouping::CURRENT_MECHANISM,
    }))
}

async fn create(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(form): Json<ProjectForm>,
) -> Result<Response, ApiError> {
    require_can_create(&state, &user).await?;
    let teams = projects::teams_for_new_project(&state.pool, &state.settings.users, &user).await?;
    let id = projects::create_project(&state.pool, &state.settings.quotas, &user, &teams, &form)
        .await?
        .map_err(ApiError::Invalid)?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))).into_response())
}

/// `_check_project_admin` on a project that isn't deleted.
pub(super) async fn require_project_admin(state: &ApiState, user: &User, project_id: i64) -> Result<(), ApiError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM projects_project WHERE id = $1 AND NOT is_deleted)")
            .bind(project_id)
            .fetch_one(&state.pool)
            .await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    if !memberships::is_project_admin(&state.pool, user, project_id).await? {
        return Err(ApiError::Forbidden("You are not an admin of this project".into()));
    }
    Ok(())
}

async fn settings(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
) -> ApiResult<ProjectSettings> {
    require_project_admin(&state, &user, project_id).await?;
    projects::project_settings(&state.pool, state.settings.alerts.base_url(), project_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn edit(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
    Json(form): Json<ProjectForm>,
) -> Result<StatusCode, ApiError> {
    require_project_admin(&state, &user, project_id).await?;
    projects::edit_project(&state.pool, &state.settings.quotas, project_id, &form).await?.map_err(ApiError::Invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    require_project_admin(&state, &user, project_id).await?;
    projects::delete_project(&state.pool, project_id).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn join(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    match projects::join_project(&state.pool, &user, project_id).await? {
        None => Err(ApiError::NotFound),
        Some(true) => Ok(StatusCode::NO_CONTENT),
        Some(false) => match memberships::membership(&state.pool, Group::Project(project_id), user.id).await? {
            Some(_) => Err(ApiError::Conflict("You are already a member of this project".into())),
            None => Err(ApiError::Forbidden("This project is not joinable".into())),
        },
    }
}

/// Removes your membership (or invitation).
async fn leave(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    memberships::remove_member(&state.pool, Group::Project(project_id), user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct SdkSetup {
    dsn: String,
    /// Upstream's setup page polls this to see the first event arrive.
    digested_event_count: i32,
}

async fn sdk_setup(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
) -> ApiResult<SdkSetup> {
    let Some((key, digested_event_count)): Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT sentry_key, digested_event_count FROM projects_project WHERE id = $1 AND NOT is_deleted",
    )
    .bind(project_id)
    .fetch_optional(&state.pool)
    .await?
    else {
        return Err(ApiError::NotFound);
    };
    require_project_access(&state.pool, &user, project_id).await?;
    Ok(Json(SdkSetup {
        dsn: projects::dsn(state.settings.alerts.base_url(), project_id, key),
        digested_event_count,
    }))
}

#[derive(Serialize)]
struct Kind {
    kind: &'static str,
    label: &'static str,
}

#[derive(Serialize)]
struct Services {
    /// What can be added; the config fields per kind: `webhook_url` (all but Telegram), `channel` (Mattermost),
    /// `bot_token`, `chat_id`, `message_thread_id` (Telegram).
    kinds: Vec<Kind>,
    services: Vec<setup::Service>,
}

/// `project_alerts_setup`: the project's messaging services, with how their last message went.
async fn services(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
) -> ApiResult<Services> {
    require_project_admin(&state, &user, project_id).await?;
    Ok(Json(Services {
        kinds: setup::KINDS.iter().map(|(kind, label)| Kind { kind, label }).collect(),
        services: setup::services(&state.pool, project_id).await?,
    }))
}

async fn add_service(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<i64>,
    Json(form): Json<setup::ServiceForm>,
) -> Result<Response, ApiError> {
    require_project_admin(&state, &user, project_id).await?;
    let id = setup::add_service(&state.pool, &state.settings.alerts.webhooks, project_id, &form)
        .await?
        .map_err(ApiError::Invalid)?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))).into_response())
}

async fn edit_service(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((project_id, service_id)): Path<(i64, i64)>,
    Json(form): Json<setup::ServiceForm>,
) -> Result<StatusCode, ApiError> {
    require_project_admin(&state, &user, project_id).await?;
    setup::edit_service(&state.pool, &state.settings.alerts.webhooks, project_id, service_id, &form)
        .await?
        .ok_or(ApiError::NotFound)?
        .map_err(ApiError::Invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_service(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((project_id, service_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    require_project_admin(&state, &user, project_id).await?;
    setup::remove_service(&state.pool, project_id, service_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Sent in the background; its outcome shows on the service (`last_failure_*`).
async fn test_service(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((project_id, service_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    require_project_admin(&state, &user, project_id).await?;
    let Some((kind, config, display_name, project_name)): Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT s.kind, s.config, s.display_name, p.name FROM alerts_messagingserviceconfig s
         JOIN projects_project p ON p.id = s.project_id WHERE s.id = $1 AND s.project_id = $2",
    )
    .bind(service_id)
    .bind(project_id)
    .fetch_optional(&state.pool)
    .await?
    else {
        return Err(ApiError::NotFound);
    };
    let config = serde_json::from_str(&config).map_err(ApiError::internal)?;
    let job = messaging::TestJob { service_config_id: service_id, kind, config, project_name, display_name };
    crate::tasks::enqueue(&state.pool, messaging::TEST_JOB_KIND, &job).await.map_err(ApiError::internal)?;
    Ok(StatusCode::ACCEPTED)
}
