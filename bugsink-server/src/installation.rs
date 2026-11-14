//! The installation's admin pages, as upstream Bugsink 2.6.3: the users admin (`users/views.py::user_list`,
//! `user_edit`), auth tokens (`bsmain/views.py`, `AuthTokenForm`, `AuthToken.clean`), the counts page, the banners of
//! system warnings (`useful_settings_processor`) and where "home" goes.

use std::collections::BTreeSet;

use bugsink_db::PgPool;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::accounts::User;
use crate::alerts::AlertSettings;
use crate::grouping::py::strip;
use crate::projects::FieldErrors;
use crate::quota::{self, QuotaSettings, Scope};

// --- users ------------------------------------------------------------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub is_active: bool,
    pub is_superuser: bool,
    pub is_staff: bool,
    pub date_joined: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
}

/// `user_list`: all users by username.
pub async fn users(pool: &PgPool) -> sqlx::Result<Vec<UserRow>> {
    sqlx::query_as(
        "SELECT id, username, email, first_name, last_name, is_active, is_superuser, is_staff, date_joined, last_login
         FROM auth_user ORDER BY username",
    )
    .fetch_all(pool)
    .await
}

/// "activate"/"deactivate". False: no such user.
pub async fn set_active(pool: &PgPool, user_id: i64, active: bool) -> sqlx::Result<bool> {
    let done = sqlx::query("UPDATE auth_user SET is_active = $2 WHERE id = $1")
        .bind(user_id)
        .bind(active)
        .execute(pool)
        .await?;
    Ok(done.rows_affected() > 0)
}

pub enum Deleted {
    Deleted,
    NoSuchUser,
    /// Upstream refuses to delete active users.
    Active,
}

/// "delete": what Django's `user.delete()` cascades to: memberships, email verifications and admin log entries go,
/// turning points and auth tokens lose their user (login sessions go with the user).
pub async fn delete_user(pool: &PgPool, user_id: i64) -> sqlx::Result<Deleted> {
    let mut tx = pool.begin().await?;
    let Some(active): Option<bool> = sqlx::query_scalar("SELECT is_active FROM auth_user WHERE id = $1 FOR UPDATE")
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
    else {
        return Ok(Deleted::NoSuchUser);
    };
    if active {
        return Ok(Deleted::Active);
    }
    for sql in [
        "DELETE FROM teams_teammembership WHERE user_id = $1",
        "DELETE FROM projects_projectmembership WHERE user_id = $1",
        "DELETE FROM users_emailverification WHERE user_id = $1",
        "DELETE FROM django_admin_log WHERE user_id = $1",
        "DELETE FROM auth_user_groups WHERE user_id = $1",
        "DELETE FROM auth_user_user_permissions WHERE user_id = $1",
        "UPDATE issues_turningpoint SET user_id = NULL WHERE user_id = $1",
        "UPDATE bsmain_authtoken SET user_id = NULL WHERE user_id = $1",
        "DELETE FROM auth_user WHERE id = $1",
    ] {
        sqlx::query(sql).bind(user_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Deleted::Deleted)
}

/// `UserEditForm`: the user's email address (also their username). None: no such user.
pub async fn edit_user(pool: &PgPool, user_id: i64, email: &str) -> sqlx::Result<Option<Result<(), FieldErrors>>> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM auth_user WHERE id = $1)")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    if !exists {
        return Ok(None);
    }
    let username = strip(email).to_owned();
    let error = |message: String| Ok(Some(Err(FieldErrors::from([("email", vec![message])]))));
    if username.is_empty() {
        return error("This field is required.".into());
    }
    if crate::accounts::validation::clean_email(&username).is_err() {
        return error("Enter a valid email address.".into());
    }
    let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM auth_user WHERE username = $1 AND id <> $2)")
        .bind(&username)
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    if taken {
        return error("This email is already registered by another user.".into());
    }
    // the model's own checks on the username
    let mut messages = Vec::new();
    if !regex::Regex::new(r"^[\w.@+-]+\z").expect("valid").is_match(&username) {
        messages.push(
            "Enter a valid username. This value may contain only letters, numbers, and @/./+/-/_ characters.".into(),
        );
    }
    let len = username.chars().count();
    if len > 150 {
        messages.push(format!("Ensure this value has at most 150 characters (it has {len})."));
    }
    if !messages.is_empty() {
        return Ok(Some(Err(FieldErrors::from([("email", messages)]))));
    }
    sqlx::query("UPDATE auth_user SET username = $2, email = $2 WHERE id = $1")
        .bind(user_id)
        .bind(&username)
        .execute(pool)
        .await?;
    Ok(Some(Ok(())))
}

// --- auth tokens ------------------------------------------------------------------------------------------------

/// `CAPABILITIES`: (capability, description); the column is the name with `:` and `-` as `_`.
pub const CAPABILITIES: [(&str, &str); 12] = [
    ("issues:read", "View issues and their details."),
    ("events:read", "View events, including event data and stacktraces."),
    ("issues:comment", "Add comments to issues."),
    ("issues:triage", "Resolve, reopen, mute, and unmute issues."),
    ("issues:delete", "Delete issues."),
    ("releases:read", "View releases and their details."),
    ("releases:create", "Create releases."),
    ("debug-files:upload", "Upload and assemble debug files and artifact bundles."),
    ("projects:read", "View projects and their details."),
    ("projects:manage", "Create and update projects."),
    ("teams:read", "View teams and their details."),
    ("teams:manage", "Create and update teams."),
];

/// `CAPABILITIES_UNBOUNDABLE_BY_PROJECT`.
const NOT_FOR_ONE_PROJECT: [&str; 3] = ["projects:manage", "teams:read", "teams:manage"];

fn column(capability: &str) -> String {
    capability.replace([':', '-'], "_")
}

#[derive(Debug, Serialize)]
pub struct AuthToken {
    pub id: i64,
    pub token: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub is_user_bound: bool,
    pub user: Option<Value>,
    pub is_project_bound: bool,
    pub project: Option<Value>,
    pub capabilities: Vec<&'static str>,
    pub has_all_capabilities: bool,
}

/// `_auth_tokens_visible_to`: tokens that haven't expired (been revoked); all for superusers, else your own.
pub async fn auth_tokens(pool: &PgPool, user: &User) -> sqlx::Result<Vec<AuthToken>> {
    let columns: Vec<String> = CAPABILITIES.iter().map(|(c, _)| format!("t.{}", column(c))).collect();
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT t.id, t.token, t.description, t.created_at, t.is_user_bound, t.user_id, u.username,
                t.is_project_bound, t.project_id, p.name AS project_name, {}
         FROM bsmain_authtoken t LEFT JOIN auth_user u ON u.id = t.user_id
         LEFT JOIN projects_project p ON p.id = t.project_id
         WHERE (t.expires_at IS NULL OR t.expires_at > now()) AND ($1 OR (t.is_user_bound AND t.user_id = $2))
         ORDER BY t.created_at DESC",
        columns.join(", ")
    )))
    .bind(user.is_superuser)
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    use sqlx::Row;
    Ok(rows
        .iter()
        .map(|r| {
            let capabilities: Vec<&'static str> =
                CAPABILITIES.iter().filter(|(c, _)| r.get::<bool, _>(column(c).as_str())).map(|(c, _)| *c).collect();
            AuthToken {
                id: r.get("id"),
                token: r.get("token"),
                description: r.get("description"),
                created_at: r.get("created_at"),
                is_user_bound: r.get("is_user_bound"),
                user: r
                    .get::<Option<i64>, _>("user_id")
                    .map(|id| json!({"id": id, "username": r.get::<String, _>("username")})),
                is_project_bound: r.get("is_project_bound"),
                project: r
                    .get::<Option<i64>, _>("project_id")
                    .map(|id| json!({"id": id, "name": r.get::<Option<String>, _>("project_name")})),
                has_all_capabilities: capabilities.len() == CAPABILITIES.len(),
                capabilities,
            }
        })
        .collect())
}

/// The projects a token can be limited to: all (not deleted) for superusers, else the ones you can see.
pub async fn token_projects(pool: &PgPool, user: &User) -> sqlx::Result<Vec<(i64, String, Option<String>)>> {
    sqlx::query_as(
        "SELECT p.id, p.name, t.name FROM projects_project p LEFT JOIN teams_team t ON t.id = p.team_id
         WHERE NOT p.is_deleted
           AND ($1 OR p.visibility < $3
                OR EXISTS (SELECT 1 FROM projects_projectmembership m WHERE m.project_id = p.id AND m.user_id = $2)
                OR EXISTS (SELECT 1 FROM teams_teammembership tm
                           WHERE tm.team_id = p.team_id AND tm.user_id = $2 AND tm.accepted))
         ORDER BY p.name",
    )
    .bind(user.is_superuser)
    .bind(user.id)
    .bind(bugsink_db::PROJECT_VISIBILITY_TEAM_MEMBERS)
    .fetch_all(pool)
    .await
}

#[derive(Debug, Deserialize)]
pub struct AuthTokenForm {
    pub description: Option<String>,
    /// Superusers only: tie the token to themselves (else a service token); everyone else's tokens are their own.
    #[serde(default)]
    pub is_user_bound: bool,
    pub project_id: Option<i64>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// `auth_token_create` (`AuthTokenForm` + `AuthToken.clean`). The new token's id.
pub async fn create_auth_token(
    pool: &PgPool,
    user: &User,
    form: &AuthTokenForm,
) -> sqlx::Result<Result<i64, FieldErrors>> {
    let mut errors = FieldErrors::new();
    let description = strip(form.description.as_deref().unwrap_or("")).to_owned();
    let len = description.chars().count();
    if description.is_empty() {
        errors.insert("description", vec!["This field is required.".into()]);
    } else if len > 255 {
        errors.insert("description", vec![format!("Ensure this value has at most 255 characters (it has {len}).")]);
    }
    let project_id = match form.project_id {
        None => None,
        Some(id) if token_projects(pool, user).await?.iter().any(|(p, _, _)| *p == id) => Some(id),
        Some(_) => {
            errors.insert(
                "project",
                vec!["Select a valid choice. That choice is not one of the available choices.".into()],
            );
            None
        }
    };
    let mut capabilities = BTreeSet::new();
    for c in &form.capabilities {
        match CAPABILITIES.iter().find(|(name, _)| name == c) {
            Some((name, _)) => {
                capabilities.insert(*name);
            }
            None => {
                errors.entry("capabilities").or_default().push(format!("Unknown capability: {c}"));
            }
        }
    }
    if capabilities.is_empty() {
        errors.entry("__all__").or_default().push("Select at least one capability.".into());
    }
    // `AuthToken.clean`, for what the form leaves possible
    let forbidden: Vec<&str> = NOT_FOR_ONE_PROJECT.iter().copied().filter(|c| capabilities.contains(c)).collect();
    if project_id.is_some() && !forbidden.is_empty() {
        let mut forbidden = forbidden;
        forbidden.sort();
        let message = format!("Project-bound tokens cannot have capabilities: {}.", forbidden.join(", "));
        errors.insert("project", vec![message]);
    }
    if !errors.is_empty() {
        return Ok(Err(errors));
    }
    let is_user_bound = !user.is_superuser || form.is_user_bound;
    let mut token_bytes = [0u8; 20];
    rand::fill(&mut token_bytes);
    let token: String = token_bytes.iter().map(|b| format!("{b:02x}")).collect();
    let columns: Vec<String> = CAPABILITIES.iter().map(|(c, _)| column(c)).collect();
    let values: Vec<String> = CAPABILITIES.iter().map(|(c, _)| capabilities.contains(c).to_string()).collect();
    let id = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "INSERT INTO bsmain_authtoken (token, created_at, description, is_user_bound, user_id, is_project_bound,
             project_id, expires_at, {})
         VALUES ($1, now(), $2, $3, $4, $5, $6, NULL, {}) RETURNING id",
        columns.join(", "),
        values.join(", ")
    )))
    .bind(token)
    .bind(description)
    .bind(is_user_bound)
    .bind(is_user_bound.then_some(user.id))
    .bind(project_id.is_some())
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    Ok(Ok(id))
}

/// "revoke": it expires now. False: not one of the tokens you can see.
pub async fn revoke_auth_token(pool: &PgPool, user: &User, id: i64) -> sqlx::Result<bool> {
    let done = sqlx::query(
        "UPDATE bsmain_authtoken SET expires_at = now()
         WHERE id = $1 AND (expires_at IS NULL OR expires_at > now()) AND ($2 OR (is_user_bound AND user_id = $3))",
    )
    .bind(id)
    .bind(user.is_superuser)
    .bind(user.id)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

// --- counts -----------------------------------------------------------------------------------------------------

/// The counts page's apps and models (upstream's `interesting_apps`); its "snappea" queue is our job queue.
const COUNTED: [(&str, &[(&str, &str)]); 11] = [
    ("bsmain", &[("AuthToken", "bsmain_authtoken")]),
    (
        "events",
        &[
            ("Event", "events_event"),
            ("InstallationEventCountsPerHour", "events_installationeventcountsperhour"),
            ("ProjectEventCountsPerHour", "events_projecteventcountsperhour"),
            ("IssueEventCountsPerHour", "events_issueeventcountsperhour"),
        ],
    ),
    (
        "files",
        &[
            ("DifAssembly", "files_difassembly"),
            ("ArtifactBundleAssembly", "files_artifactbundleassembly"),
            ("Chunk", "files_chunk"),
            ("File", "files_file"),
            ("FileMetadata", "files_filemetadata"),
        ],
    ),
    ("ingest", &[("Envelope", "ingest_envelope")]),
    ("issues", &[("Issue", "issues_issue"), ("Grouping", "issues_grouping"), ("TurningPoint", "issues_turningpoint")]),
    ("projects", &[("Project", "projects_project"), ("ProjectMembership", "projects_projectmembership")]),
    ("releases", &[("Release", "releases_release")]),
    ("jobs", &[("Job", "bsrs_job")]),
    (
        "tags",
        &[
            ("TagKey", "tags_tagkey"),
            ("TagValue", "tags_tagvalue"),
            ("EventTag", "tags_eventtag"),
            ("IssueTag", "tags_issuetag"),
        ],
    ),
    ("teams", &[("Team", "teams_team"), ("TeamMembership", "teams_teammembership")]),
    ("users", &[("User", "auth_user"), ("EmailVerification", "users_emailverification")]),
];

/// `counts`: rows per model, per app.
pub async fn counts(pool: &PgPool) -> sqlx::Result<Value> {
    let mut apps = Map::new();
    for (app, models) in COUNTED {
        let mut counts = Map::new();
        for (model, table) in models {
            let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(pool)
                .await?;
            counts.insert((*model).into(), json!(n));
        }
        apps.insert(app.into(), Value::Object(counts));
    }
    Ok(Value::Object(apps))
}

// --- system warnings --------------------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SystemWarning {
    pub message: String,
    /// More about it.
    pub link: Option<&'static str>,
    /// Superusers can silence this one (`POST /api/v1/system-warnings/email/silence`).
    pub silenceable: bool,
}

/// `useful_settings_processor`'s `system_warnings`: email not set up (until silenced), email quotas and failures, the
/// installation's ingest quota, and (instead of upstream's snappea) a backed up job queue.
pub async fn system_warnings(
    pool: &PgPool,
    alerts: &AlertSettings,
    quotas: &QuotaSettings,
    user: &User,
    now: DateTime<Utc>,
) -> sqlx::Result<Vec<SystemWarning>> {
    let mut warnings = Vec::new();
    type Row = (bool, String, String, Option<DateTime<Utc>>, String);
    let (silenced, quota_usage, diagnostics, exceeded_until, exceeded_reason): Row =
        sqlx::query_as(
            "SELECT silence_email_system_warning, email_quota_usage, email_sending_diagnostics, quota_exceeded_until,
                    quota_exceeded_reason
             FROM phonehome_installation LIMIT 1",
        )
        .fetch_one(pool)
        .await?;

    // upstream's QuietConsoleEmailBackend: no EMAIL_BACKEND and no EMAIL_HOST
    let email = &alerts.email;
    if email.email_backend.is_none() && email.email_host.is_none() && !silenced {
        warnings.push(SystemWarning {
            message: "Email is not set up, emails won't be sent. To get the most out of Bugsink, please set up email."
                .into(),
            link: Some("https://www.bugsink.com/docs/settings/#email"),
            silenceable: user.is_superuser,
        });
    }

    // `get_email_quota_warnings`
    let usage: Value = serde_json::from_str(&quota_usage).unwrap_or(Value::Null);
    let used =
        |period: &str, key: String| usage.get(period).and_then(|p| p.get(&key)).and_then(Value::as_i64).unwrap_or(0);
    if let Some(max) = email.max_emails_per_hour.0 {
        let n = used("per_hour", now.format("%Y-%m-%dT%H").to_string());
        if n >= max {
            warnings.push(SystemWarning {
                message: format!(
                    "Bugsink has sent {n} emails this hour, which is the maximum. No more emails will be sent until \
                     the next hour."
                ),
                link: None,
                silenceable: false,
            });
        }
    }
    if let Some(max) = email.max_emails_per_month.0 {
        let n = used("per_month", now.format("%Y-%m").to_string());
        if n >= max {
            warnings.push(SystemWarning {
                message: format!(
                    "Bugsink has sent {n} emails this month, which is the maximum. No more emails will be sent until \
                     the 1st of next month."
                ),
                link: None,
                silenceable: false,
            });
        }
    }

    // `get_email_failure_warnings`: most of at least 5 attempts in the last 5 minutes failed
    let diagnostics: Value = serde_json::from_str(&diagnostics).unwrap_or(Value::Null);
    let cutoff = now - chrono::TimeDelta::minutes(5);
    let recent: Vec<&Value> = diagnostics
        .get("attempts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| {
            a.get("at")
                .and_then(Value::as_str)
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .is_some_and(|at| at >= cutoff)
        })
        .collect();
    let failures = recent.iter().filter(|a| !a.get("ok").and_then(Value::as_bool).unwrap_or(false)).count();
    if recent.len() >= 5 && failures * 2 > recent.len() {
        warnings.push(SystemWarning {
            message: format!(
                "Email sending appears to be failing: {failures} of {} recent email attempts failed.",
                recent.len()
            ),
            link: None,
            silenceable: false,
        });
    }

    // the installation's ingest quota
    if quota::is_still_exceeded(quotas, Scope::Installation, exceeded_until, &exceeded_reason, now)
        && let Ok((period, n, gte)) = serde_json::from_str::<(String, i64, i64)>(&exceeded_reason)
    {
        let per = if n != 1 { format!("{n} {period}s") } else { period };
        let until = exceeded_until.map(django_date).unwrap_or_default();
        warnings.push(SystemWarning {
            message: format!(
                "Event ingestion stopped until {until}. Reason: installation quota ({gte} events per {per}) exceeded."
            ),
            link: None,
            silenceable: false,
        });
    }

    // the job queue (upstream: snappea's): many waiting, or some waiting too long
    let (count, oldest): (i64, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT count(*), min(run_at) FROM bsrs_job WHERE failed_at IS NULL AND run_at <= now()")
            .fetch_one(pool)
            .await?;
    let age = oldest.map(|o| (now - o).num_seconds()).unwrap_or(0);
    if count > 300 || (count > 0 && age > 5) {
        warnings.push(SystemWarning {
            message: format!(
                "{count} background jobs are waiting, the oldest for {age}s. The workers may be backlogged, not \
                 running, misconfigured or blocked."
            ),
            link: None,
            silenceable: false,
        });
    }
    Ok(warnings)
}

/// Django's `date:"j M G:i T"` in UTC (upstream's Docker default `TIME_ZONE`).
fn django_date(t: DateTime<Utc>) -> String {
    t.format("%-d %b %-H:%M UTC").to_string()
}

pub async fn silence_email_warning(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("UPDATE phonehome_installation SET silence_email_system_warning = true").execute(pool).await?;
    Ok(())
}

// --- home -------------------------------------------------------------------------------------------------------

/// `home`: your only project's issues, else the project list if you're in (or invited to) any project, else (and
/// with `SINGLE_TEAM`) the project list, else the team list. Deleted projects don't count (upstream sends you to one
/// anyway).
pub async fn home(pool: &PgPool, user: &User, single_team: bool) -> sqlx::Result<Value> {
    let accepted: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT m.project_id FROM projects_projectmembership m JOIN projects_project p ON p.id = m.project_id
         WHERE m.user_id = $1 AND m.accepted AND NOT p.is_deleted",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    if let [project_id] = accepted[..] {
        return Ok(json!({"go": "issues", "project_id": project_id}));
    }
    let any: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM projects_projectmembership m JOIN projects_project p ON p.id = m.project_id
                        WHERE m.user_id = $1 AND NOT p.is_deleted)",
    )
    .bind(user.id)
    .fetch_one(pool)
    .await?;
    Ok(json!({"go": if any || single_team { "projects" } else { "teams" }}))
}
