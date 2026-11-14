//! Members of a team or a project (the same endpoints under `/api/v1/teams/{id}` and `/api/v1/projects/{id}`): the
//! list, inviting, removing, re-sending an invitation or showing its link, member settings, and answering an
//! invitation (also as a new user with the emailed token).

use axum::extract::{MatchedPath, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{ApiError, ApiResult, ApiState};
use crate::accounts::{CurrentUser, User};
use crate::memberships::{self, Group, InviteError, Invited, Member, ROLE_ADMIN};
use crate::projects::{self, FieldErrors};

pub fn routes() -> Router<ApiState> {
    let mut router = Router::new();
    for kind in ["teams", "projects"] {
        router = router
            .route(&format!("/api/v1/{kind}/{{id}}/members"), get(list).post(invite))
            .route(&format!("/api/v1/{kind}/{{id}}/members/accept-new-user"), post(accept_new_user))
            .route(&format!("/api/v1/{kind}/{{id}}/members/{{user_id}}"), patch(update).delete(remove))
            .route(&format!("/api/v1/{kind}/{{id}}/members/{{user_id}}/reinvite"), post(reinvite))
            .route(&format!("/api/v1/{kind}/{{id}}/members/{{user_id}}/invite-link"), get(invite_link))
            .route(&format!("/api/v1/{kind}/{{id}}/accept"), post(accept))
            .route(&format!("/api/v1/{kind}/{{id}}/decline"), post(decline));
    }
    router
}

/// The team or project the route is about; 404 for a malformed id.
fn group(path: &MatchedPath, id: &str) -> Result<Group, ApiError> {
    if path.as_str().starts_with("/api/v1/teams/") {
        Uuid::try_parse(id).map(Group::Team).map_err(|_| ApiError::NotFound)
    } else {
        id.parse().map(Group::Project).map_err(|_| ApiError::NotFound)
    }
}

/// The team exists, or the project exists and isn't deleted.
async fn require_exists(state: &ApiState, group: Group) -> Result<(), ApiError> {
    let exists: bool = match group {
        Group::Team(id) => sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM teams_team WHERE id = $1)").bind(id),
        Group::Project(id) => sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM projects_project WHERE id = $1 AND NOT is_deleted)",
        )
        .bind(id),
    }
    .fetch_one(&state.pool)
    .await?;
    if exists { Ok(()) } else { Err(ApiError::NotFound) }
}

async fn require_admin(state: &ApiState, user: &User, group: Group) -> Result<(), ApiError> {
    match group {
        Group::Team(id) => super::teams::require_team_admin(state, user, id).await,
        Group::Project(id) => super::project_admin::require_project_admin(state, user, id).await,
    }
}

async fn list(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    matched: MatchedPath,
    Path(id): Path<String>,
) -> ApiResult<MemberList> {
    let group = group(&matched, &id)?;
    require_admin(&state, &user, group).await?;
    Ok(Json(MemberList {
        members: memberships::members(&state.pool, group).await?,
        email_delivers: state.settings.alerts.email.backend_delivers_mail(),
    }))
}

#[derive(Serialize)]
struct MemberList {
    members: Vec<Member>,
    /// Whether invitations are emailed (re-send them) or passed on by hand (copy their links).
    email_delivers: bool,
}

#[derive(Deserialize)]
struct InviteRequest {
    email: Option<String>,
    role: Option<i64>,
}

/// `<team|project>_members_invite`.
async fn invite(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    matched: MatchedPath,
    Path(id): Path<String>,
    Json(req): Json<InviteRequest>,
) -> ApiResult<Invited> {
    let group = group(&matched, &id)?;
    require_admin(&state, &user, group).await?;
    let mut errors = FieldErrors::new();
    let role = match req.role {
        None => {
            errors.insert("role", vec!["This field is required.".into()]);
            0
        }
        Some(r @ (0 | 1)) => r as i32,
        Some(r) => {
            errors.insert("role", vec![format!("Select a valid choice. {r} is not one of the available choices.")]);
            0
        }
    };
    let email = req.email.unwrap_or_default();
    if let Err(message) = crate::accounts::validation::clean_email(&email) {
        errors.insert("email", vec![message.into()]);
    }
    if !errors.is_empty() {
        return Err(ApiError::Invalid(errors));
    }
    match memberships::invite(&state.memberships(), group, &user, &email, role).await {
        Ok(invited) => Ok(Json(invited)),
        Err(InviteError::Email(message)) => {
            Err(ApiError::Invalid(FieldErrors::from([("email", vec![message.into()])])))
        }
        Err(InviteError::Db(e)) => Err(e.into()),
        Err(InviteError::Other(e)) => Err(ApiError::internal(e)),
    }
}

/// Teams: removing yourself is leaving (the last admin can't).
async fn remove(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    matched: MatchedPath,
    Path((id, user_id)): Path<(String, i64)>,
) -> Result<StatusCode, ApiError> {
    let group = group(&matched, &id)?;
    require_admin(&state, &user, group).await?;
    if let Group::Team(team_id) = group
        && user_id == user.id
    {
        return match projects::leave_team(&state.pool, user.id, team_id, false).await? {
            None => Err(ApiError::NotFound),
            Some(false) => Err(ApiError::Conflict(super::teams::LAST_ADMIN.into())),
            Some(true) => Ok(StatusCode::NO_CONTENT),
        };
    }
    memberships::remove_member(&state.pool, group, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// "reinvite": only when email is set up.
async fn reinvite(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    matched: MatchedPath,
    Path((id, user_id)): Path<(String, i64)>,
) -> ApiResult<serde_json::Value> {
    let group = group(&matched, &id)?;
    require_admin(&state, &user, group).await?;
    if !state.settings.alerts.email.backend_delivers_mail() {
        return Err(ApiError::Conflict("Email is not set up: pass on the invitation's link instead".into()));
    }
    let email = memberships::reinvite(&state.pool, group, user_id).await.map_err(ApiError::internal)?;
    let email = email.ok_or(ApiError::NotFound)?;
    Ok(Json(json!({ "email": email })))
}

/// "copy_invite_link": only without email, for an open invitation of someone who can log in.
async fn invite_link(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    matched: MatchedPath,
    Path((id, user_id)): Path<(String, i64)>,
) -> ApiResult<serde_json::Value> {
    let group = group(&matched, &id)?;
    require_admin(&state, &user, group).await?;
    if state.settings.alerts.email.backend_delivers_mail() {
        return Err(ApiError::Conflict("Email is set up: re-send the invitation instead".into()));
    }
    match memberships::membership(&state.pool, group, user_id).await? {
        Some((_, false)) => {}
        _ => return Err(ApiError::NotFound),
    }
    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM auth_user WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?;
    let link = memberships::invite_link(state.settings.alerts.base_url(), group, is_active)
        .ok_or_else(|| ApiError::Forbidden("Cannot show an account setup link for an existing inactive user".into()))?;
    Ok(Json(json!({ "invite_link": link })))
}

#[derive(Deserialize)]
struct MemberUpdate {
    role: Option<i64>,
    /// `null` sets "as per the team's or user's setting"; leave it out to keep it.
    #[serde(default, deserialize_with = "present")]
    send_email_alerts: Option<Option<bool>>,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<bool>>, D::Error> {
    Option::<bool>::deserialize(d).map(Some)
}

/// `<team|project>_member_settings`: your own membership's email alerts (and role, as an admin); someone else's
/// role, as an admin (for teams upstream requires being a superuser *and* a team admin for that).
async fn update(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    matched: MatchedPath,
    Path((id, user_id)): Path<(String, i64)>,
    Json(req): Json<MemberUpdate>,
) -> Result<StatusCode, ApiError> {
    let group = group(&matched, &id)?;
    require_exists(&state, group).await?;
    let noun = match group {
        Group::Team(_) => "team",
        Group::Project(_) => "project",
    };
    let Some((your_role, accepted)) = memberships::membership(&state.pool, group, user.id).await? else {
        return Err(ApiError::Forbidden(format!("You are not a member of this {noun}")));
    };
    if !accepted {
        return Err(ApiError::Conflict("Accept the invitation first".into()));
    }
    let role = match req.role {
        None => None,
        Some(r @ (0 | 1)) => Some(r as i32),
        Some(r) => {
            return Err(ApiError::Invalid(FieldErrors::from([(
                "role",
                vec![format!("Select a valid choice. {r} is not one of the available choices.")],
            )])));
        }
    };
    if user_id == user.id {
        let may_edit_role = your_role == ROLE_ADMIN || user.is_superuser;
        memberships::update_member(&state.pool, group, user_id, role.filter(|_| may_edit_role), req.send_email_alerts)
            .await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    let allowed = match group {
        Group::Team(_) => user.is_superuser && your_role == ROLE_ADMIN,
        Group::Project(id) => memberships::is_project_admin(&state.pool, &user, id).await?,
    };
    if !allowed {
        return Err(ApiError::Forbidden(format!("You are not an admin of this {noun}")));
    }
    if memberships::membership(&state.pool, group, user_id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    memberships::update_member(&state.pool, group, user_id, role, None).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Accepting an invitation you already accepted changes nothing.
async fn accept(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    matched: MatchedPath,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    respond(&state, &user, group(&matched, &id)?, true).await
}

/// Declining removes the invitation (not an accepted membership).
async fn decline(
    State(state): State<ApiState>,
    CurrentUser { user, .. }: CurrentUser,
    matched: MatchedPath,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    respond(&state, &user, group(&matched, &id)?, false).await
}

async fn respond(state: &ApiState, user: &User, group: Group, accept: bool) -> Result<StatusCode, ApiError> {
    require_exists(state, group).await?;
    if memberships::membership(&state.pool, group, user.id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    memberships::respond(&state.pool, group, user.id, accept).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TokenRequest {
    token: String,
}

/// `<team|project>_members_accept_new_user` (no login needed: the emailed token identifies the user).
/// `needs_password`: set one with `POST /api/v1/auth/reset-password/{token}` first; otherwise log in and accept.
async fn accept_new_user(
    State(state): State<ApiState>,
    Json(req): Json<TokenRequest>,
) -> ApiResult<serde_json::Value> {
    let needs_password = memberships::accept_new_user(&state.pool, &state.settings.users, &req.token)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({ "needs_password": needs_password })))
}
