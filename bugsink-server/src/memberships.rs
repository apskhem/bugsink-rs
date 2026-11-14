//! Team and project memberships, as upstream Bugsink 2.6.3's `teams/` and `projects/` views and models: who may do
//! what (admins, `TEAM_CREATION`, `USER_REGISTRATION`, `SINGLE_USER`/`SINGLE_TEAM`), inviting people by email address
//! (creating not-yet-active users for new addresses), invitation emails or links to pass on when email isn't set up
//! (`email_backend_delivers_mail`), and the email-verification tokens new users set their password with.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bugsink_db::{PgPool, Postgres, Transaction};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::accounts::User;
use crate::alerts::py_bool;
use crate::alerts::{AlertSettings, email, template};
use crate::tasks;

pub const ROLE_MEMBER: i32 = 0;
pub const ROLE_ADMIN: i32 = 1;

/// Upstream's settings for users, teams and projects, with its Docker image's defaults.
#[derive(clap::Args, Clone, Debug)]
pub struct UserSettings {
    /// One user only: implies SINGLE_TEAM, and nobody registers users or creates teams.
    #[arg(long, env = "SINGLE_USER", default_value = "false", value_parser = py_bool)]
    pub single_user: bool,
    /// All projects in one team, created with the first project.
    #[arg(long, env = "SINGLE_TEAM", default_value = "false", value_parser = py_bool)]
    pub single_team: bool,
    /// Who may add users (by inviting them): CB_ANYBODY, CB_MEMBERS, CB_ADMINS or CB_NOBODY.
    #[arg(long, env = "USER_REGISTRATION", default_value = "CB_MEMBERS")]
    pub user_registration: String,
    /// Seconds an email verification (invitation, password reset) link stays valid.
    #[arg(long, env = "USER_REGISTRATION_VERIFY_EMAIL_EXPIRY", default_value_t = 7 * 24 * 60 * 60)]
    pub user_registration_verify_email_expiry: i64,
    /// New users signing up have to confirm their email address before they can log in.
    #[arg(long, env = "USER_REGISTRATION_VERIFY_EMAIL", default_value = "true", value_parser = py_bool)]
    pub user_registration_verify_email: bool,
    /// Who may create teams: CB_ANYBODY, CB_MEMBERS, CB_ADMINS or CB_NOBODY.
    #[arg(long, env = "TEAM_CREATION", default_value = "CB_MEMBERS")]
    pub team_creation: String,
}

impl UserSettings {
    pub fn single_team(&self) -> bool {
        self.single_user || self.single_team
    }

    fn user_registration(&self) -> &str {
        if self.single_user { "CB_NOBODY" } else { &self.user_registration }
    }

    /// Signing up is for `USER_REGISTRATION=CB_ANYBODY` only.
    pub fn signup_allowed(&self) -> bool {
        self.user_registration() == "CB_ANYBODY"
    }

    /// `user_can_create_team`.
    pub fn user_can_create_team(&self, user: &User) -> bool {
        let creation = if self.single_user { "CB_NOBODY" } else { &self.team_creation };
        matches!(creation, "CB_ANYBODY" | "CB_MEMBERS") || (user.is_superuser && creation == "CB_ADMINS")
    }

    /// Invitations to addresses without a user are refused unless the inviter may register users.
    fn invited_user_must_exist(&self, inviter: &User) -> bool {
        match self.user_registration() {
            "CB_ANYBODY" | "CB_MEMBERS" => false,
            "CB_ADMINS" => !inviter.is_superuser,
            _ => true,
        }
    }
}

/// What the membership endpoints need besides the database.
pub struct Context {
    pub pool: PgPool,
    pub users: UserSettings,
    pub alerts: Arc<AlertSettings>,
}

/// A team or a project: the two things people are members of, with the same membership columns.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Team(Uuid),
    Project(i64),
}

impl Group {
    fn table(self) -> &'static str {
        match self {
            Self::Team(_) => "teams_teammembership",
            Self::Project(_) => "projects_projectmembership",
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Team(_) => "team_id",
            Self::Project(_) => "project_id",
        }
    }

    /// The group's id as an SQL literal (from typed ids only).
    fn id_sql(self) -> String {
        match self {
            Self::Team(id) => format!("'{id}'::uuid"),
            Self::Project(id) => id.to_string(),
        }
    }

    fn path(self) -> String {
        match self {
            Self::Team(id) => format!("/teams/{id}"),
            Self::Project(id) => format!("/projects/{id}"),
        }
    }

    /// Upstream's `<team|project>_members_accept` URL (also the frontend's).
    pub fn accept_url(self, base_url: &str) -> String {
        format!("{base_url}{}/members/accept/", self.path())
    }

    /// `<team|project>_members_accept_new_user`: where a new user sets their password first.
    pub fn accept_new_user_url(self, base_url: &str, token: &str) -> String {
        format!("{base_url}{}/members/accept/{token}/", self.path())
    }
}

/// `user_is_team_admin`: superusers, and accepted admins of the team (of none: nobody but superusers).
pub async fn is_team_admin(pool: &PgPool, user: &User, team_id: Option<Uuid>) -> sqlx::Result<bool> {
    if user.is_superuser {
        return Ok(true);
    }
    let Some(team_id) = team_id else { return Ok(false) };
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM teams_teammembership
                        WHERE team_id = $1 AND user_id = $2 AND role = $3 AND accepted)",
    )
    .bind(team_id)
    .bind(user.id)
    .bind(ROLE_ADMIN)
    .fetch_one(pool)
    .await
}

/// `user_is_project_admin`: superusers, accepted project admins, and admins of the project's team.
pub async fn is_project_admin(pool: &PgPool, user: &User, project_id: i64) -> sqlx::Result<bool> {
    if user.is_superuser {
        return Ok(true);
    }
    let (admin, team_id): (bool, Option<Uuid>) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM projects_projectmembership
                        WHERE project_id = p.id AND user_id = $2 AND role = $3 AND accepted), p.team_id
         FROM projects_project p WHERE p.id = $1",
    )
    .bind(project_id)
    .bind(user.id)
    .bind(ROLE_ADMIN)
    .fetch_one(pool)
    .await?;
    Ok(admin || is_team_admin(pool, user, team_id).await?)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Member {
    pub user_id: i64,
    pub email: String,
    pub username: String,
    /// False until a newly invited user has set a password.
    pub is_active: bool,
    pub role: i32,
    pub accepted: bool,
    /// Null: as per the team's or the user's setting.
    pub send_email_alerts: Option<bool>,
}

pub async fn members(pool: &PgPool, group: Group) -> sqlx::Result<Vec<Member>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT m.user_id, u.email, u.username, u.is_active, m.role, m.accepted, m.send_email_alerts
         FROM {} m JOIN auth_user u ON u.id = m.user_id WHERE m.{} = {} ORDER BY m.id",
        group.table(),
        group.column(),
        group.id_sql()
    )))
    .fetch_all(pool)
    .await
}

/// A membership's (role, accepted), if any.
pub async fn membership(pool: &PgPool, group: Group, user_id: i64) -> sqlx::Result<Option<(i32, bool)>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT role, accepted FROM {} WHERE {} = {} AND user_id = $1",
        group.table(),
        group.column(),
        group.id_sql()
    )))
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Creates a membership unless there is one; true when created.
pub async fn add_member<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    group: Group,
    user_id: i64,
    role: i32,
    accepted: bool,
) -> sqlx::Result<bool> {
    let created: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {} ({}, user_id, role, accepted, send_email_alerts) VALUES ({}, $1, $2, $3, NULL)
         ON CONFLICT ({}, user_id) DO NOTHING RETURNING id",
        group.table(),
        group.column(),
        group.id_sql(),
        group.column()
    )))
    .bind(user_id)
    .bind(role)
    .bind(accepted)
    .fetch_optional(executor)
    .await?;
    Ok(created.is_some())
}

pub async fn remove_member(pool: &PgPool, group: Group, user_id: i64) -> sqlx::Result<bool> {
    let deleted = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM {} WHERE {} = {} AND user_id = $1",
        group.table(),
        group.column(),
        group.id_sql()
    )))
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(deleted.rows_affected() > 0)
}

/// Sets what is given of (role, send_email_alerts).
pub async fn update_member(
    pool: &PgPool,
    group: Group,
    user_id: i64,
    role: Option<i32>,
    send_email_alerts: Option<Option<bool>>,
) -> sqlx::Result<()> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {} SET role = coalesce($2, role),
             send_email_alerts = CASE WHEN $3 THEN $4 ELSE send_email_alerts END
         WHERE {} = {} AND user_id = $1",
        group.table(),
        group.column(),
        group.id_sql()
    )))
    .bind(user_id)
    .bind(role)
    .bind(send_email_alerts.is_some())
    .bind(send_email_alerts.flatten())
    .execute(pool)
    .await?;
    Ok(())
}

/// `<team|project>_members_accept`: accept or decline an invitation. False when there is no membership.
pub async fn respond(pool: &PgPool, group: Group, user_id: i64, accept: bool) -> sqlx::Result<bool> {
    let sql = if accept {
        "UPDATE {table} SET accepted = true WHERE {column} = {id} AND user_id = $1"
    } else {
        "DELETE FROM {table} WHERE {column} = {id} AND user_id = $1 AND NOT accepted"
    };
    let sql =
        sql.replace("{table}", group.table()).replace("{column}", group.column()).replace("{id}", &group.id_sql());
    let done = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(user_id).execute(pool).await?;
    Ok(done.rows_affected() > 0)
}

#[derive(Debug)]
pub enum InviteError {
    /// For the `email` field.
    Email(&'static str),
    Db(sqlx::Error),
    Other(anyhow::Error),
}

impl From<sqlx::Error> for InviteError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl From<anyhow::Error> for InviteError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

#[derive(Debug, Serialize)]
pub struct Invited {
    pub user_id: i64,
    /// False when the address was invited before (nothing changes about the membership then).
    pub membership_created: bool,
    /// An invitation email is on its way (`email_backend_delivers_mail`).
    pub emailed: bool,
    /// Without email: the link to pass on, if there is one to give (not for someone invited before who still has to
    /// set a password).
    pub invite_link: Option<String>,
}

/// `<team|project>_members_invite`.
pub async fn invite(
    ctx: &Context,
    group: Group,
    inviter: &User,
    email: &str,
    role: i32,
) -> Result<Invited, InviteError> {
    let email = crate::accounts::validation::clean_email(email).map_err(InviteError::Email)?;
    let mut tx = ctx.pool.begin().await?;
    let existing: Option<(i64, bool)> =
        sqlx::query_as("SELECT id, is_active FROM auth_user WHERE email = $1 ORDER BY id LIMIT 1")
            .bind(&email)
            .fetch_optional(&mut *tx)
            .await?;
    if existing.is_none() && ctx.users.invited_user_must_exist(inviter) {
        return Err(InviteError::Email("No user with this email address in the system."));
    }
    let (user_id, is_active, created) = match existing {
        Some((id, active)) => (id, active, false),
        None => (create_invited_user(&mut tx, &email).await?, false, true),
    };

    let base_url = ctx.alerts.base_url();
    let emailed = ctx.alerts.email.backend_delivers_mail();
    let invite_link = if emailed {
        send_invite(&mut tx, group, user_id, is_active, &email).await?;
        None
    } else if is_active {
        Some(group.accept_url(base_url))
    } else if created {
        let token = create_email_verification(&mut tx, user_id).await?;
        Some(group.accept_new_user_url(base_url, &token))
    } else {
        None
    };
    let membership_created = add_member(&mut *tx, group, user_id, role, false).await?;
    tx.commit().await?;
    Ok(Invited { user_id, membership_created, emailed, invite_link })
}

/// `User.objects.get_or_create(email=email, defaults={'username': email, 'is_active': False})`, the model's defaults
/// spelled out.
async fn create_invited_user(tx: &mut Transaction<'_, Postgres>, email: &str) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "INSERT INTO auth_user (password, last_login, is_superuser, username, first_name, last_name, email, is_staff,
             is_active, date_joined, send_email_alerts, theme_preference, language)
         VALUES ('', NULL, false, $1, '', '', $1, false, false, now(), true, 'system', 'auto') RETURNING id",
    )
    .bind(email)
    .fetch_one(&mut **tx)
    .await
}

/// `EmailVerification.objects.create(user=user, email=user.username)` with `secrets.token_urlsafe()`.
pub(crate) async fn create_email_verification(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
) -> sqlx::Result<String> {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    sqlx::query(
        "INSERT INTO users_emailverification (email, token, created_at, user_id)
         SELECT username, $1, now(), id FROM auth_user WHERE id = $2",
    )
    .bind(&token)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(token)
}

/// `_send_<team|project>_invite_email`: a new (not yet active) user gets a fresh token to set their password with.
async fn send_invite(
    tx: &mut Transaction<'_, Postgres>,
    group: Group,
    user_id: i64,
    is_active: bool,
    email: &str,
) -> anyhow::Result<()> {
    let token = if is_active { None } else { Some(create_email_verification(tx, user_id).await?) };
    tasks::enqueue(&mut **tx, INVITE_JOB_KIND, &InviteJob { email: email.to_owned(), group, token }).await
}

/// `<team|project>_members` "reinvite": email the invitation again (only when email is set up).
pub async fn reinvite(pool: &PgPool, group: Group, user_id: i64) -> anyhow::Result<Option<String>> {
    let mut tx = pool.begin().await?;
    let Some((email, is_active)): Option<(String, bool)> =
        sqlx::query_as("SELECT email, is_active FROM auth_user WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?
    else {
        return Ok(None);
    };
    send_invite(&mut tx, group, user_id, is_active, &email).await?;
    tx.commit().await?;
    Ok(Some(email))
}

/// `<team|project>_members` "copy_invite_link" (only without email): for an active user the accept page; an invited
/// user who still has to set a password gets no new link this way (None).
pub fn invite_link(base_url: &str, group: Group, is_active: bool) -> Option<String> {
    is_active.then(|| group.accept_url(base_url))
}

pub const INVITE_JOB_KIND: &str = "invite_email";

#[derive(Serialize, Deserialize)]
pub struct InviteJob {
    pub email: String,
    pub group: Group,
    /// For a new user: the token to set their password with.
    pub token: Option<String>,
}

/// `send_<team|project>_invite_email[_new_user]`.
pub async fn handle_invite_job(pool: PgPool, payload: Value, settings: Arc<AlertSettings>) -> anyhow::Result<()> {
    let job: InviteJob = serde_json::from_value(payload)?;
    let name: Option<String> = match job.group {
        Group::Team(id) => {
            sqlx::query_scalar("SELECT name FROM teams_team WHERE id = $1").bind(id).fetch_optional(&pool)
        }
        Group::Project(id) => {
            sqlx::query_scalar("SELECT name FROM projects_project WHERE id = $1").bind(id).fetch_optional(&pool)
        }
    }
    .await?;
    let Some(name) = name else {
        tracing::warn!(group = ?job.group, "invited to something that no longer exists; not sending");
        return Ok(());
    };
    let base_url = settings.base_url();
    let url = match &job.token {
        None => job.group.accept_url(base_url),
        Some(token) => job.group.accept_new_user_url(base_url, token),
    };
    let templates = match (job.group, job.token.is_some()) {
        (Group::Team(_), false) => TEAM_INVITE,
        (Group::Team(_), true) => TEAM_INVITE_NEW_USER,
        (Group::Project(_), false) => PROJECT_INVITE,
        (Group::Project(_), true) => PROJECT_INVITE_NEW_USER,
    };
    let base_url_slash = format!("{base_url}/");
    let name_key = match job.group {
        Group::Team(_) => "team_name",
        Group::Project(_) => "project_name",
    };
    let ctx: template::Context = [
        ("site_title", settings.site_title.as_str()),
        ("base_url", base_url_slash.as_str()),
        (name_key, name.as_str()),
        ("url", url.as_str()),
    ]
    .into_iter()
    .collect();
    let subject = format!("You have been invited to join \"{name}\"");
    email::send_rendered_email(&pool, &settings, &subject, &job.email, templates, &ctx).await
}

const PROJECT_INVITE: (&str, &str) = (
    include_str!("alerts/templates/project_membership_invite.html"),
    include_str!("alerts/templates/project_membership_invite.txt"),
);
const PROJECT_INVITE_NEW_USER: (&str, &str) = (
    include_str!("alerts/templates/project_membership_invite_new_user.html"),
    include_str!("alerts/templates/project_membership_invite_new_user.txt"),
);
const TEAM_INVITE: (&str, &str) = (
    include_str!("alerts/templates/team_membership_invite.html"),
    include_str!("alerts/templates/team_membership_invite.txt"),
);
const TEAM_INVITE_NEW_USER: (&str, &str) = (
    include_str!("alerts/templates/team_membership_invite_new_user.html"),
    include_str!("alerts/templates/team_membership_invite_new_user.txt"),
);

/// Deletes email verifications older than `USER_REGISTRATION_VERIFY_EMAIL_EXPIRY` (upstream does this on every
/// request that looks one up), then finds the token's user: (verification id, user id, is_active, password).
pub async fn email_verification(
    pool: &PgPool,
    users: &UserSettings,
    token: &str,
) -> sqlx::Result<Option<(i64, i64, bool, String)>> {
    sqlx::query("DELETE FROM users_emailverification WHERE created_at < $1")
        .bind(Utc::now() - chrono::TimeDelta::seconds(users.user_registration_verify_email_expiry))
        .execute(pool)
        .await?;
    sqlx::query_as(
        "SELECT v.id, u.id, u.is_active, u.password FROM users_emailverification v JOIN auth_user u ON u.id = v.user_id
         WHERE v.token = $1",
    )
    .bind(token)
    .fetch_optional(pool)
    .await
}

/// `<team|project>_members_accept_new_user`: true when the invited user must set a password first (with the token,
/// `accounts::reset_password`); otherwise the token is used up and the invitation can be accepted after logging in.
pub async fn accept_new_user(pool: &PgPool, users: &UserSettings, token: &str) -> sqlx::Result<Option<bool>> {
    let Some((verification_id, _, is_active, password)) = email_verification(pool, users, token).await? else {
        return Ok(None);
    };
    // `has_usable_password`: not marked unusable ("!...")
    if password.starts_with('!') || !is_active {
        return Ok(Some(true));
    }
    sqlx::query("DELETE FROM users_emailverification WHERE id = $1").bind(verification_id).execute(pool).await?;
    Ok(Some(false))
}
