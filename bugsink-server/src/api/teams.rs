//! Teams: the list (mine/other), creating, editing, deleting, joining and leaving. Members: `members.rs`.

use axum::extract::{
  Path,
  Query,
  State,
};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{
  get,
  post,
};
use axum::{
  Json,
  Router,
};
use serde::Serialize;
use uuid::Uuid;

use super::{
  ApiError,
  ApiResult,
  ApiState,
  TabQuery,
  created,
  require_admin,
};
use crate::accounts::CurrentUser;
use crate::memberships::{
  self,
  Group,
  Join,
};
use crate::projects::{
  self,
  TeamForm,
  TeamListItem,
};

pub fn routes() -> Router<ApiState> {
  Router::new()
    .route("/api/v1/teams", get(list).post(create))
    .route("/api/v1/teams/{team_id}", get(detail).patch(edit).delete(delete))
    .route("/api/v1/teams/{team_id}/join", post(join))
    .route("/api/v1/teams/{team_id}/leave", post(leave))
}

#[derive(Serialize)]
struct TeamList {
  /// "mine" or "other": the one asked for, or upstream's default.
  tab: &'static str,
  /// `TEAM_CREATION`.
  can_create: bool,
  items: Vec<TeamListItem>,
}

async fn list(
  State(state): State<ApiState>,
  CurrentUser { user, .. }: CurrentUser,
  Query(query): Query<TabQuery>,
) -> ApiResult<TeamList> {
  let (tab, items) = projects::teams(&state.pool, &user, query.tab.as_deref()).await?;
  Ok(Json(TeamList {
    tab,
    can_create: state.settings.users.user_can_create_team(&user),
    items,
  }))
}

async fn create(
  State(state): State<ApiState>,
  CurrentUser { user, .. }: CurrentUser,
  Json(form): Json<TeamForm>,
) -> Result<Response, ApiError> {
  if !state.settings.users.user_can_create_team(&user) {
    return Err(ApiError::Forbidden("You are not allowed to create teams".into()));
  }
  let id = projects::create_team(&state.pool, &user, &form)
    .await?
    .map_err(ApiError::Invalid)?;
  Ok(created(id))
}

#[derive(Serialize, sqlx::FromRow)]
struct TeamDetail {
  id: Uuid,
  name: String,
  visibility: i32,
}

async fn detail(
  State(state): State<ApiState>,
  CurrentUser { user, .. }: CurrentUser,
  Path(team_id): Path<Uuid>,
) -> ApiResult<TeamDetail> {
  require_admin(&state, &user, Group::Team(team_id)).await?;
  let team = sqlx::query_as("SELECT id, name, visibility FROM teams_team WHERE id = $1")
    .bind(team_id)
    .fetch_one(&state.pool)
    .await?;
  Ok(Json(team))
}

async fn edit(
  State(state): State<ApiState>,
  CurrentUser { user, .. }: CurrentUser,
  Path(team_id): Path<Uuid>,
  Json(form): Json<TeamForm>,
) -> Result<StatusCode, ApiError> {
  require_admin(&state, &user, Group::Team(team_id)).await?;
  projects::edit_team(&state.pool, team_id, &form)
    .await?
    .map_err(ApiError::Invalid)?;
  Ok(StatusCode::NO_CONTENT)
}

async fn delete(
  State(state): State<ApiState>,
  CurrentUser { user, .. }: CurrentUser,
  Path(team_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  require_admin(&state, &user, Group::Team(team_id)).await?;
  if !projects::delete_team(&state.pool, team_id).await? {
    return Err(ApiError::Conflict(
      "The team's projects can't be deleted with it: delete them first (upstream fails on this too)".into(),
    ));
  }
  Ok(StatusCode::NO_CONTENT)
}

async fn join(
  State(state): State<ApiState>,
  CurrentUser { user, .. }: CurrentUser,
  Path(team_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  match memberships::join_team(&state.pool, &user, team_id).await? {
    None => Err(ApiError::NotFound),
    Some(Join::NotJoinable) => Err(ApiError::Forbidden("This team is not joinable".into())),
    Some(Join::AlreadyMember) => Err(ApiError::Conflict("You are already a member of this team".into())),
    Some(Join::Joined) => Ok(StatusCode::NO_CONTENT),
  }
}

/// Leaving needs an accepted membership; the last admin stays (promote someone else, or delete the team).
async fn leave(
  State(state): State<ApiState>,
  CurrentUser { user, .. }: CurrentUser,
  Path(team_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  match memberships::leave_team(&state.pool, user.id, team_id, true).await? {
    None => Err(ApiError::NotFound),
    Some(false) => Err(ApiError::Conflict(LAST_ADMIN.into())),
    Some(true) => Ok(StatusCode::NO_CONTENT),
  }
}

pub(super) const LAST_ADMIN: &str =
  "You are the last admin of this team. Promote another member or delete the team first.";
