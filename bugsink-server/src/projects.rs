//! Teams and projects themselves, as upstream Bugsink 2.6.3's `teams/` and `projects/` views and forms: the lists
//! (with their mine/teams/other tabs), creating, editing (with the forms' validation and messages) and deleting
//! (projects in the background: `delete_project_deps`). Memberships: `crate::memberships`.

use bugsink_db::{
  PROJECT_VISIBILITY_TEAM_MEMBERS,
  PgPool,
  Postgres,
  Transaction,
};
use chrono::{
  DateTime,
  TimeDelta,
  Utc,
};
use serde::{
  Deserialize,
  Serialize,
};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::accounts::User;
use crate::forms::{
  FieldErrors,
  REQUIRED,
  char_field,
  choice_field,
  error,
};
use crate::memberships::{
  self,
  Group,
  ROLE_ADMIN,
  UserSettings,
};
use crate::quota::QuotaSettings;
use crate::tasks;

// --- teams ------------------------------------------------------------------------------------------------------

/// `TeamVisibility`: joinable, discoverable, hidden.
const TEAM_VISIBILITIES: [i64; 3] = [1, 10, 99];
const TEAM_HIDDEN: i32 = 99;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TeamListItem {
  pub id: Uuid,
  pub name: String,
  pub visibility: i32,
  /// Incl. projects being deleted (as upstream counts them).
  pub project_count: i64,
  pub member_count: i64,
  /// Your membership, if any (an invitation: not accepted yet).
  pub my_role: Option<i32>,
  pub my_accepted: Option<bool>,
  pub my_send_email_alerts: Option<bool>,
  /// Whether you can join it without an invitation.
  pub joinable: bool,
}

/// `team_list`: "mine" (teams you're a member of or invited to), "other" (visible teams you're not in). Without a tab,
/// upstream's choice: mine if you have any, or if there's nothing else.
pub async fn teams(pool: &PgPool, user: &User, tab: Option<&str>) -> sqlx::Result<(&'static str, Vec<TeamListItem>)> {
  let query = |mine: bool| {
    sqlx::query_as::<_, TeamListItem>(
      "SELECT t.id, t.name, t.visibility,
                    (SELECT COUNT(*) FROM projects_project p WHERE p.team_id = t.id) AS project_count,
                    (SELECT COUNT(*) FROM teams_teammembership m WHERE m.team_id = t.id AND m.accepted) AS member_count,
                    my.role AS my_role, my.accepted AS my_accepted, my.send_email_alerts AS my_send_email_alerts,
                    t.visibility <= 1 OR $1 AS joinable
             FROM teams_team t LEFT JOIN teams_teammembership my ON my.team_id = t.id AND my.user_id = $2
             WHERE ($1 OR my.id IS NOT NULL OR t.visibility < $3) AND (my.id IS NOT NULL) = $4
             ORDER BY t.name",
    )
    .bind(user.is_superuser)
    .bind(user.id)
    .bind(TEAM_HIDDEN)
    .bind(mine)
    .fetch_all(pool)
  };
  let tab = match tab {
    Some("mine") => "mine",
    Some("other") => "other",
    _ => {
      let mine = query(true).await?;
      if !mine.is_empty() || query(false).await?.is_empty() {
        return Ok(("mine", mine));
      }
      "other"
    }
  };
  Ok((tab, query(tab == "mine").await?))
}

#[derive(Debug, Deserialize)]
pub struct TeamForm {
  pub name: Option<String>,
  pub visibility: Option<i64>,
}

/// `TeamForm`: the cleaned (name, visibility).
async fn clean_team(
  pool: &PgPool,
  form: &TeamForm,
  instance: Option<Uuid>,
) -> sqlx::Result<Result<(String, i64), FieldErrors>> {
  let mut errors = FieldErrors::new();
  let name = char_field(&mut errors, "name", form.name.as_deref(), 255);
  let visibility = choice_field(&mut errors, "visibility", form.visibility, &TEAM_VISIBILITIES);
  if !errors.contains_key("name") {
    let taken: bool =
      sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM teams_team WHERE name = $1 AND id IS DISTINCT FROM $2)")
        .bind(&name)
        .bind(instance)
        .fetch_one(pool)
        .await?;
    if taken {
      errors
        .entry("name")
        .or_default()
        .push("Team with this Name already exists.".into());
    }
  }
  Ok(if errors.is_empty() {
    Ok((name, visibility))
  } else {
    Err(errors)
  })
}

/// `team_new`: the creator becomes its (accepted) admin.
pub async fn create_team(pool: &PgPool, user: &User, form: &TeamForm) -> sqlx::Result<Result<Uuid, FieldErrors>> {
  let (name, visibility) = match clean_team(pool, form, None).await? {
    Ok(cleaned) => cleaned,
    Err(errors) => return Ok(Err(errors)),
  };
  let mut tx = pool.begin().await?;
  let id = Uuid::new_v4();
  sqlx::query("INSERT INTO teams_team (id, name, visibility) VALUES ($1, $2, $3)")
    .bind(id)
    .bind(&name)
    .bind(visibility as i32)
    .execute(&mut *tx)
    .await?;
  memberships::add_member(&mut *tx, Group::Team(id), user.id, ROLE_ADMIN, true).await?;
  tx.commit().await?;
  Ok(Ok(id))
}

pub async fn edit_team(pool: &PgPool, id: Uuid, form: &TeamForm) -> sqlx::Result<Result<(), FieldErrors>> {
  let (name, visibility) = match clean_team(pool, form, Some(id)).await? {
    Ok(cleaned) => cleaned,
    Err(errors) => return Ok(Err(errors)),
  };
  sqlx::query("UPDATE teams_team SET name = $2, visibility = $3 WHERE id = $1")
    .bind(id)
    .bind(name)
    .bind(visibility as i32)
    .execute(pool)
    .await?;
  Ok(Ok(()))
}

/// `team_edit` "delete": upstream deletes the team's projects outright, which the database refuses as soon as a
/// project has anything (its creator's membership, events, ...): then nothing is deleted (upstream: a 500) and this
/// returns false.
pub async fn delete_team(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
  let mut tx = pool.begin().await?;
  sqlx::query("DELETE FROM projects_project WHERE team_id = $1")
    .bind(id)
    .execute(&mut *tx)
    .await?;
  sqlx::query("DELETE FROM teams_teammembership WHERE team_id = $1")
    .bind(id)
    .execute(&mut *tx)
    .await?;
  sqlx::query("DELETE FROM teams_team WHERE id = $1")
    .bind(id)
    .execute(&mut *tx)
    .await?;
  match tx.commit().await {
    Ok(()) => Ok(true),
    // deferred foreign keys: checked at commit
    Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23503") => Ok(false),
    Err(e) => Err(e),
  }
}

// --- projects ---------------------------------------------------------------------------------------------------

/// `ProjectVisibility`: joinable, discoverable, team members.
const PROJECT_VISIBILITIES: [i64; 3] = [1, 10, 99];
const GROUPING_MECHANISMS: [&str; 2] = [crate::grouping::BUGSINK_V1, crate::grouping::BUGSINK_V2];
const OPEN_ISSUE_COUNT_SHOW_THRESHOLD: i32 = 25_000;
const GROUPING_TRANSITION_PERIOD: TimeDelta = TimeDelta::days(30);

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProjectListItem {
  pub id: i64,
  pub name: String,
  pub slug: String,
  pub team_id: Option<Uuid>,
  pub team_name: Option<String>,
  pub visibility: i32,
  pub digested_event_count: i32,
  pub stored_event_count: i32,
  pub issue_count: i32,
  pub member_count: i64,
  /// Not counted for projects with more than 25,000 issues (null).
  pub open_issue_count: Option<i64>,
  pub my_role: Option<i32>,
  pub my_accepted: Option<bool>,
  pub my_send_email_alerts: Option<bool>,
  /// `is_joinable`: through your team, or open to anyone.
  pub joinable: bool,
}

/// `project_list`: "mine" (you're a member or invited), "teams" (your teams' other projects), "other" (visible, the
/// rest). Without a tab: the first of these that isn't empty, else mine.
pub async fn projects(
  pool: &PgPool,
  user: &User,
  tab: Option<&str>,
) -> sqlx::Result<(&'static str, Vec<ProjectListItem>)> {
  let query = |tab: &'static str| {
    sqlx::query_as::<_, ProjectListItem>(
      "WITH visible AS (
                 SELECT p.* FROM projects_project p
                 WHERE NOT p.is_deleted
                   AND ($1 OR p.visibility < $3
                        OR EXISTS (SELECT 1 FROM projects_projectmembership m
                                   WHERE m.project_id = p.id AND m.user_id = $2)
                        OR EXISTS (SELECT 1 FROM teams_teammembership t
                                   WHERE t.team_id = p.team_id AND t.user_id = $2 AND t.accepted))
             ), tabbed AS (
                 SELECT v.*,
                        EXISTS (SELECT 1 FROM projects_projectmembership m WHERE m.project_id = v.id AND m.user_id = $2)
                            AS mine,
                        EXISTS (SELECT 1 FROM teams_teammembership t
                                WHERE t.team_id = v.team_id AND t.user_id = $2 AND t.accepted) AS in_my_team
                 FROM visible v
             )
             SELECT p.id, p.name, p.slug, p.team_id, t.name AS team_name, p.visibility, p.digested_event_count,
                    p.stored_event_count, p.issue_count,
                    (SELECT COUNT(*) FROM projects_projectmembership m WHERE m.project_id = p.id AND m.accepted)
                        AS member_count,
                    CASE WHEN p.issue_count <= $5 THEN
                        (SELECT COUNT(*) FROM issues_issue i
                         WHERE i.project_id = p.id AND NOT i.is_deleted AND NOT i.is_resolved AND NOT i.is_muted)
                    END AS open_issue_count,
                    my.role AS my_role, my.accepted AS my_accepted, my.send_email_alerts AS my_send_email_alerts,
                    (p.in_my_team OR p.visibility <= 1) AS joinable
             FROM tabbed p
             LEFT JOIN teams_team t ON t.id = p.team_id
             LEFT JOIN projects_projectmembership my ON my.project_id = p.id AND my.user_id = $2
             WHERE CASE $4 WHEN 'mine' THEN p.mine
                           WHEN 'teams' THEN p.in_my_team AND NOT p.mine
                           ELSE NOT p.mine AND NOT p.in_my_team END
             ORDER BY p.name",
    )
    .bind(user.is_superuser)
    .bind(user.id)
    .bind(PROJECT_VISIBILITY_TEAM_MEMBERS)
    .bind(tab)
    .bind(OPEN_ISSUE_COUNT_SHOW_THRESHOLD)
    .fetch_all(pool)
  };
  let tab = match tab {
    Some("mine") => "mine",
    Some("teams") => "teams",
    Some("other") => "other",
    _ => {
      for tab in ["mine", "teams", "other"] {
        let found = query(tab).await?;
        if !found.is_empty() {
          return Ok((tab, found));
        }
      }
      "mine"
    }
  };
  Ok((tab, query(tab).await?))
}

/// `user_can_create_project`: superusers and team admins (invited ones too).
pub async fn can_create_project(pool: &PgPool, user: &User) -> sqlx::Result<bool> {
  if user.is_superuser {
    return Ok(true);
  }
  sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM teams_teammembership WHERE user_id = $1 AND role = $2)")
    .bind(user.id)
    .bind(ROLE_ADMIN)
    .fetch_one(pool)
    .await
}

/// The teams a new project can go in: all for superusers, else the ones you're an (accepted) admin of. With
/// `SINGLE_TEAM`, upstream creates "Single Team" the first time.
pub async fn teams_for_new_project(
  pool: &PgPool,
  users: &UserSettings,
  user: &User,
) -> sqlx::Result<Vec<(Uuid, String)>> {
  if users.single_team() {
    sqlx::query(
      "INSERT INTO teams_team (id, name, visibility)
             SELECT $1, 'Single Team', 10 WHERE NOT EXISTS (SELECT 1 FROM teams_team)",
    )
    .bind(Uuid::new_v4())
    .execute(pool)
    .await?;
  }
  sqlx::query_as(
    "SELECT t.id, t.name FROM teams_team t
         WHERE $1 OR EXISTS (SELECT 1 FROM teams_teammembership m
                             WHERE m.team_id = t.id AND m.user_id = $2 AND m.role = $3 AND m.accepted)
         ORDER BY t.name",
  )
  .bind(user.is_superuser)
  .bind(user.id)
  .bind(ROLE_ADMIN)
  .fetch_all(pool)
  .await
}

/// `ProjectForm`'s initial retention: the per-project cap, or a fifth of the installation's, whichever is lower.
pub fn default_retention(quotas: &QuotaSettings) -> i64 {
  [
    quotas.max_retention_per_project_event_count.0,
    quotas.max_retention_event_count.0.map(|max| max / 5),
  ]
  .into_iter()
  .flatten()
  .min()
  .unwrap_or(10_000)
}

#[derive(Debug, Deserialize)]
pub struct ProjectForm {
  /// On creation only.
  pub team_id: Option<Uuid>,
  pub name: Option<String>,
  pub visibility: Option<i64>,
  pub retention_max_event_count: Option<i64>,
  pub grouping_mechanism: Option<String>,
}

struct CleanProject {
  team_id: Option<Uuid>,
  name: String,
  visibility: i64,
  retention_max_event_count: i64,
  grouping_mechanism: String,
}

/// `ProjectForm` for creating (`allowed_teams`) or editing (`instance`: id, team, current retention).
async fn clean_project(
  pool: &PgPool,
  quotas: &QuotaSettings,
  form: &ProjectForm,
  allowed_teams: Option<&[(Uuid, String)]>,
  instance: Option<(i64, Option<Uuid>, i64)>,
) -> sqlx::Result<Result<CleanProject, FieldErrors>> {
  let mut errors = FieldErrors::new();
  let team_id = match allowed_teams {
    None => instance.and_then(|(_, team, _)| team),
    Some(allowed) => {
      match form.team_id {
        None => {
          errors.entry("team").or_default().push(REQUIRED.into());
          None
        }
        Some(id) if !allowed.iter().any(|(t, _)| *t == id) => {
          errors
            .entry("team")
            .or_default()
            .push("Select a valid choice. That choice is not one of the available choices.".into());
          None
        }
        Some(id) => Some(id),
      }
    }
  };
  let name = char_field(&mut errors, "name", form.name.as_deref(), 255);
  let visibility = choice_field(&mut errors, "visibility", form.visibility, &PROJECT_VISIBILITIES);
  let retention = match form.retention_max_event_count {
    None => {
      errors
        .entry("retention_max_event_count")
        .or_default()
        .push(REQUIRED.into());
      0
    }
    Some(v) if v < 0 => {
      let message = "Ensure this value is greater than or equal to 0.";
      errors
        .entry("retention_max_event_count")
        .or_default()
        .push(message.into());
      v
    }
    Some(v) if v > i64::from(i32::MAX) => {
      errors
        .entry("retention_max_event_count")
        .or_default()
        .push(format!("Ensure this value is less than or equal to {}.", i32::MAX));
      v
    }
    Some(v) => {
      let (id, grace) = instance.map_or((None, 0), |(id, _, current)| (Some(id), current));
      if let Err(message) = retention_within_budget(pool, quotas, v, id, Some(grace)).await? {
        errors.entry("retention_max_event_count").or_default().push(message);
      }
      v
    }
  };
  let grouping_mechanism = form.grouping_mechanism.clone().unwrap_or_default();
  if grouping_mechanism.is_empty() {
    errors.entry("grouping_mechanism").or_default().push(REQUIRED.into());
  } else if !GROUPING_MECHANISMS.contains(&grouping_mechanism.as_str()) {
    errors.entry("grouping_mechanism").or_default().push(format!(
      "Select a valid choice. {grouping_mechanism} is not one of the available choices."
    ));
  }
  // `clean`: one name per team
  if !errors.contains_key("name") && !errors.contains_key("team") && team_id.is_some() {
    let taken: bool = sqlx::query_scalar(
      "SELECT EXISTS (SELECT 1 FROM projects_project
                            WHERE team_id = $1 AND name = $2 AND id IS DISTINCT FROM $3)",
    )
    .bind(team_id)
    .bind(&name)
    .bind(instance.map(|(id, _, _)| id))
    .fetch_one(pool)
    .await?;
    if taken {
      errors
        .entry("name")
        .or_default()
        .push("A project with this name already exists in this team.".into());
    }
  }
  if !errors.is_empty() {
    return Ok(Err(errors));
  }
  Ok(Ok(CleanProject {
    team_id,
    name,
    visibility,
    retention_max_event_count: retention,
    grouping_mechanism,
  }))
}

/// The retention caps for a project (None: a new one): the form's `clean_retention_max_event_count` with grace for
/// what the project already has (so lowering is always allowed), or the REST API's
/// `validate_retention_max_event_count` without.
pub(crate) async fn retention_within_budget(
  pool: &PgPool,
  quotas: &QuotaSettings,
  value: i64,
  project_id: Option<i64>,
  grace: Option<i64>,
) -> sqlx::Result<Result<(), String>> {
  let with_grace = |limit: i64| grace.map_or(limit, |grace| limit.max(grace));
  if let Some(cap) = quotas.max_retention_per_project_event_count.0
    && value > with_grace(cap)
  {
    return Ok(Err(format!(
      "The maximum allowed retention per project is {cap} events."
    )));
  }
  if let Some(total) = quotas.max_retention_event_count.0 {
    let others: i64 = sqlx::query_scalar(
      "SELECT coalesce(SUM(retention_max_event_count), 0)::bigint FROM projects_project
             WHERE id IS DISTINCT FROM $1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let budget_left = with_grace((total - others).max(0));
    if value > budget_left {
      return Ok(Err(format!(
        "The maximum allowed retention for this project is {budget_left} events (based on the \
                 installation-wide max of {total} events)."
      )));
    }
  }
  Ok(Ok(()))
}

/// Django's `slugify`: ASCII (accents dropped), lower case, word characters, single dashes.
fn slugify(value: &str) -> String {
  let ascii: String = value.nfkd().filter(char::is_ascii).collect::<String>().to_lowercase();
  let kept: String = ascii
    .chars()
    .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || is_ascii_space(*c))
    .collect();
  let mut slug = String::new();
  let mut in_run = false;
  for c in kept.chars() {
    if c == '-' || is_ascii_space(c) {
      if !in_run {
        slug.push('-');
      }
      in_run = true;
    } else {
      slug.push(c);
      in_run = false;
    }
  }
  slug.trim_matches(|c| c == '-' || c == '_').to_owned()
}

/// Python's `\s` on ASCII text: space, \t\n\v\f\r and the separators \x1c-\x1f.
fn is_ascii_space(c: char) -> bool {
  matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\x1c'..='\x1f')
}

/// What a new project is created with; everything else starts as upstream's model defaults.
pub struct NewProject<'a> {
  pub team_id: Option<Uuid>,
  pub name: &'a str,
  pub slug: &'a str,
  /// On new issues, regressions and unmutes.
  pub alerts: [bool; 3],
  pub visibility: i32,
  pub retention_max_event_count: i32,
  pub grouping_mechanism: &'a str,
}

/// `Project.objects.create`: (id, sentry_key). Django model defaults are not database defaults: every NOT NULL column
/// is spelled out, with upstream's values.
pub async fn insert_project(tx: &mut Transaction<'_, Postgres>, p: &NewProject<'_>) -> sqlx::Result<(i64, Uuid)> {
  let [new_issue, regression, unmute] = p.alerts;
  sqlx::query_as(
    "INSERT INTO projects_project (
             team_id, name, slug, sentry_key, is_deleted, has_releases, issue_count, digested_event_count,
             stored_event_count, alert_on_new_issue, alert_on_regression, alert_on_unmute, visibility,
             quota_exceeded_until, quota_exceeded_reason, next_quota_check, retention_max_event_count,
             grouping_mechanism, previous_grouping_mechanism, grouping_mechanism_upgraded_at)
         VALUES ($1, $2, $3, gen_random_uuid(), false, false, 0, 0, 0, $4, $5, $6, $7, NULL, 'null', 0, $8, $9, NULL,
                 NULL)
         RETURNING id, sentry_key",
  )
  .bind(p.team_id)
  .bind(p.name)
  .bind(p.slug)
  .bind(new_issue)
  .bind(regression)
  .bind(unmute)
  .bind(p.visibility)
  .bind(p.retention_max_event_count)
  .bind(p.grouping_mechanism)
  .fetch_one(&mut **tx)
  .await
}

/// `Project.save`'s slug: the name's, or with -0, -1, ... appended while taken by a slug starting with it.
pub async fn unique_slug(
  tx: &mut bugsink_db::Transaction<'_, bugsink_db::Postgres>,
  name: &str,
) -> sqlx::Result<String> {
  let base = slugify(name);
  let similar: Vec<String> = sqlx::query_scalar("SELECT slug FROM projects_project WHERE starts_with(slug, $1)")
    .bind(&base)
    .fetch_all(&mut **tx)
    .await?;
  let mut slug = base.clone();
  let mut i = 0;
  while similar.contains(&slug) {
    slug = format!("{base}-{i}");
    i += 1;
  }
  Ok(slug)
}

/// `project_new`: the creator becomes its (accepted) admin. A slug longer than the column fits is refused (upstream:
/// a 500).
pub async fn create_project(
  pool: &PgPool,
  quotas: &QuotaSettings,
  user: &User,
  allowed_teams: &[(Uuid, String)],
  form: &ProjectForm,
) -> sqlx::Result<Result<i64, FieldErrors>> {
  let p = match clean_project(pool, quotas, form, Some(allowed_teams), None).await? {
    Ok(p) => p,
    Err(errors) => return Ok(Err(errors)),
  };
  let mut tx = pool.begin().await?;
  let slug = unique_slug(&mut tx, &p.name).await?;
  if slug.chars().count() > 50 {
    return Ok(Err(error("name", "This name makes a slug longer than 50 characters.")));
  }
  let new = NewProject {
    team_id: p.team_id,
    name: &p.name,
    slug: &slug,
    alerts: [true; 3],
    visibility: p.visibility as i32,
    retention_max_event_count: p.retention_max_event_count as i32,
    grouping_mechanism: &p.grouping_mechanism,
  };
  let (id, _) = insert_project(&mut tx, &new).await?;
  memberships::add_member(&mut *tx, Group::Project(id), user.id, ROLE_ADMIN, true).await?;
  tx.commit().await?;
  Ok(Ok(id))
}

/// A project's settings page (`project_edit`'s form): everything editable, plus the read-only slug and DSN.
#[derive(Debug, Serialize)]
pub struct ProjectSettings {
  pub id: i64,
  pub team_id: Option<Uuid>,
  pub name: String,
  pub slug: String,
  pub dsn: String,
  pub visibility: i32,
  pub retention_max_event_count: i32,
  pub grouping_mechanism: String,
  /// While switching grouping mechanisms (30 days), events are also matched with the previous one's keys.
  pub previous_grouping_mechanism: Option<String>,
  pub grouping_transition_ends_at: Option<DateTime<Utc>>,
  pub alert_on_new_issue: bool,
  pub alert_on_regression: bool,
  pub alert_on_unmute: bool,
}

/// `Project.dsn` (`build_dsn`): the key as user, BASE_URL's host and path, then the project id.
pub fn dsn(base_url: &str, project_id: i64, sentry_key: Uuid) -> String {
  match url::Url::parse(base_url) {
    Ok(url) => {
      let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
      let path = url.path().trim_end_matches('/');
      let host = url.host_str().unwrap_or("");
      format!(
        "{}://{}@{host}{port}{path}/{project_id}",
        url.scheme(),
        sentry_key.simple()
      )
    }
    Err(_) => String::new(),
  }
}

#[derive(sqlx::FromRow)]
struct SettingsRow {
  team_id: Option<Uuid>,
  name: String,
  slug: String,
  sentry_key: Uuid,
  visibility: i32,
  retention_max_event_count: i32,
  grouping_mechanism: String,
  previous_grouping_mechanism: Option<String>,
  grouping_mechanism_upgraded_at: Option<DateTime<Utc>>,
  alert_on_new_issue: bool,
  alert_on_regression: bool,
  alert_on_unmute: bool,
}

pub async fn project_settings(pool: &PgPool, base_url: &str, id: i64) -> sqlx::Result<Option<ProjectSettings>> {
  let row: Option<SettingsRow> = sqlx::query_as(
    "SELECT team_id, name, slug, sentry_key, visibility, retention_max_event_count, grouping_mechanism,
                previous_grouping_mechanism, grouping_mechanism_upgraded_at, alert_on_new_issue,
                alert_on_regression, alert_on_unmute
         FROM projects_project WHERE id = $1 AND NOT is_deleted",
  )
  .bind(id)
  .fetch_optional(pool)
  .await?;
  Ok(row.map(|r| {
    let transition_ends_at = r
      .grouping_mechanism_upgraded_at
      .map(|t| t + GROUPING_TRANSITION_PERIOD)
      .filter(|end| Utc::now() <= *end);
    ProjectSettings {
      id,
      team_id: r.team_id,
      name: r.name,
      slug: r.slug,
      dsn: dsn(base_url, id, r.sentry_key),
      visibility: r.visibility,
      retention_max_event_count: r.retention_max_event_count,
      grouping_mechanism: r.grouping_mechanism,
      previous_grouping_mechanism: transition_ends_at.and(r.previous_grouping_mechanism),
      grouping_transition_ends_at: transition_ends_at,
      alert_on_new_issue: r.alert_on_new_issue,
      alert_on_regression: r.alert_on_regression,
      alert_on_unmute: r.alert_on_unmute,
    }
  }))
}

/// `project_edit`: name, visibility, retention and grouping mechanism (switching starts a transition from the old
/// one); the team can't be changed.
pub async fn edit_project(
  pool: &PgPool,
  quotas: &QuotaSettings,
  id: i64,
  form: &ProjectForm,
) -> sqlx::Result<Result<(), FieldErrors>> {
  let Some((team_id, retention, grouping)): Option<(Option<Uuid>, i32, String)> =
    sqlx::query_as("SELECT team_id, retention_max_event_count, grouping_mechanism FROM projects_project WHERE id = $1")
      .bind(id)
      .fetch_optional(pool)
      .await?
  else {
    return Ok(Ok(()));
  };
  let p = match clean_project(pool, quotas, form, None, Some((id, team_id, i64::from(retention)))).await? {
    Ok(p) => p,
    Err(errors) => return Ok(Err(errors)),
  };
  sqlx::query(
    "UPDATE projects_project SET name = $2, visibility = $3, retention_max_event_count = $4,
             grouping_mechanism = $5,
             previous_grouping_mechanism = CASE WHEN $6 THEN $7 ELSE previous_grouping_mechanism END,
             grouping_mechanism_upgraded_at = CASE WHEN $6 THEN now() ELSE grouping_mechanism_upgraded_at END
         WHERE id = $1",
  )
  .bind(id)
  .bind(&p.name)
  .bind(p.visibility as i32)
  .bind(p.retention_max_event_count as i32)
  .bind(&p.grouping_mechanism)
  .bind(p.grouping_mechanism != grouping)
  .bind(&grouping)
  .execute(pool)
  .await?;
  Ok(Ok(()))
}

/// `Project.delete_deferred`: gone from every list at once; its rows are deleted in the background.
pub async fn delete_project(pool: &PgPool, id: i64) -> anyhow::Result<()> {
  let mut tx = pool.begin().await?;
  sqlx::query("UPDATE projects_project SET is_deleted = true WHERE id = $1")
    .bind(id)
    .execute(&mut *tx)
    .await?;
  tasks::enqueue(&mut *tx, &DeleteProjectDepsJob { project_id: id }).await?;
  tx.commit().await?;
  Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct DeleteProjectDepsJob {
  pub project_id: i64,
}

impl tasks::Job for DeleteProjectDepsJob {
  const KIND: &str = "delete_project_deps";
}

/// `get_model_topography_with_project_override`'s order: what refers to other rows first.
const PROJECT_DEPENDENTS: [&str; 17] = [
  "tags_eventtag",
  "tags_issuetag",
  "tags_tagvalue",
  "tags_tagkey",
  "issues_turningpoint",
  "events_issueeventcountsperhour",
  "events_projecteventcountsperhour",
  "events_event",
  "issues_grouping",
  "alerts_messagingserviceconfig",
  "projects_projectmembership",
  "releases_release",
  "issues_issue",
  "bsmain_authtoken",
  "files_artifactbundleassembly",
  "files_difassembly",
  "files_filemetadata",
];

/// `delete_project_deps`: 500 rows per job (re-enqueued until done), then the project itself.
pub async fn handle_delete_job(pool: PgPool, job: DeleteProjectDepsJob) -> anyhow::Result<()> {
  const BUDGET: i64 = 500;
  let mut tx = pool.begin().await?;
  let mut deleted = 0;
  for table in PROJECT_DEPENDENTS {
    let n = sqlx::query(sqlx::AssertSqlSafe(format!(
      "DELETE FROM {table} WHERE id IN (SELECT id FROM {table} WHERE project_id = $1 ORDER BY id LIMIT $2)"
    )))
    .bind(job.project_id)
    .bind(BUDGET - deleted)
    .execute(&mut *tx)
    .await?
    .rows_affected() as i64;
    deleted += n;
    if deleted >= BUDGET {
      break;
    }
  }
  if deleted >= BUDGET {
    tasks::enqueue(&mut *tx, &job).await?;
  } else {
    sqlx::query("DELETE FROM projects_project WHERE id = $1")
      .bind(job.project_id)
      .execute(&mut *tx)
      .await?;
  }
  tx.commit().await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn slugs_like_django() {
    assert_eq!(slugify("My App"), "my-app");
    assert_eq!(slugify("  Héllo, Wörld!  "), "hello-world");
    assert_eq!(slugify("a -- b__c"), "a-b__c");
    assert_eq!(slugify("_-x-_"), "x");
    assert_eq!(slugify("日本"), "");
  }

  #[test]
  fn dsns_like_upstream() {
    let key: Uuid = "11111111-2222-3333-4444-555555555555".parse().unwrap();
    let k = "11111111222233334444555555555555";
    assert_eq!(
      dsn("https://bugs.example.org", 7, key),
      format!("https://{k}@bugs.example.org/7")
    );
    assert_eq!(
      dsn("http://localhost:8000/sub/", 7, key),
      format!("http://{k}@localhost:8000/sub/7")
    );
  }
}
