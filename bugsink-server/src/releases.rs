//! `releases/models.py`: releases are created on first sight; ordering is semver-based
//! when versions are semver, by date otherwise ("sort epochs" separate runs of the two).

use std::cmp::Ordering;

use bugsink_db::{
  Postgres,
  Transaction,
  TurningPointKind,
};
use chrono::{
  DateTime,
  Utc,
};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::digest::Issue;
use crate::issues::NewTurningPoint;
use crate::py::{
  json_dumps,
  take_chars,
};

pub(crate) struct Release {
  id: Uuid,
  pub version: String,
  date_released: DateTime<Utc>,
  semver: String,
  is_semver: bool,
  sort_epoch: i32,
}

/// Sentry's release-name restrictions, sanitize-don't-raise.
fn sanitize_version(version: &str) -> String {
  let stripped: String = version
    .chars()
    .filter(|c| !matches!(c, '\n' | '\t' | '/' | '\\'))
    .collect();
  let stripped = if matches!(stripped.as_str(), "." | ".." | " ") {
    "sanitized".to_string()
  } else {
    stripped
  };
  take_chars(&stripped, 200)
}

/// `package@1.2.3` -> `1.2.3` (after the last `@`).
fn version_part(full_version: &str) -> &str {
  full_version.rsplit_once('@').map_or(full_version, |(_, v)| v)
}

fn parse_semver(v: &str) -> Option<semver::Version> {
  semver::Version::parse(v).ok()
}

/// `release_sort_key`: (sort_epoch, semver precedence | date_released).
fn cmp_releases(a: &Release, b: &Release) -> Ordering {
  a.sort_epoch.cmp(&b.sort_epoch).then_with(|| {
    match (a.is_semver, b.is_semver) {
      (true, true) => {
        match (parse_semver(&a.semver), parse_semver(&b.semver)) {
          (Some(va), Some(vb)) => va.cmp_precedence(&vb),
          _ => Ordering::Equal,
        }
      }
      _ => a.date_released.cmp(&b.date_released),
    }
  })
}

pub(crate) async fn ordered_releases<'e>(
  executor: impl sqlx::PgExecutor<'e>,
  project_id: i64,
) -> sqlx::Result<Vec<Release>> {
  let mut releases: Vec<Release> = sqlx::query(
    "SELECT id, version, date_released, semver, is_semver, sort_epoch FROM releases_release WHERE project_id = $1",
  )
  .bind(project_id)
  .fetch_all(executor)
  .await?
  .iter()
  .map(|r| {
    Release {
      id: r.get("id"),
      version: r.get("version"),
      date_released: r.get("date_released"),
      semver: r.get("semver"),
      is_semver: r.get("is_semver"),
      sort_epoch: r.get("sort_epoch"),
    }
  })
  .collect();
  releases.sort_by(cmp_releases);
  Ok(releases)
}

/// `Project.get_latest_release().version` (None before the project's first event).
pub(crate) async fn latest_release_version(pool: &bugsink_db::PgPool, project_id: i64) -> sqlx::Result<Option<String>> {
  Ok(ordered_releases(pool, project_id).await?.pop().map(|r| r.version))
}

/// `create_release_if_needed`: the (sanitized) version, and whether the release is new. `has_releases` is the
/// project's flag; `issue` the one being digested, if any.
pub(crate) async fn create_release_if_needed(
  tx: &mut Transaction<'_, Postgres>,
  project_id: i64,
  has_releases: &mut bool,
  version: &str,
  timestamp: DateTime<Utc>,
  issue: Option<&mut Issue>,
) -> sqlx::Result<(String, bool)> {
  let version = sanitize_version(version);
  let exists: bool =
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM releases_release WHERE project_id = $1 AND version = $2)")
      .bind(project_id)
      .bind(&version)
      .fetch_one(&mut **tx)
      .await?;
  if exists {
    return Ok((version, false));
  }

  let semver_part = version_part(&version);
  let is_semver = parse_semver(semver_part).is_some();
  let last_epoch: Option<(i32, bool)> = sqlx::query_as(
    "SELECT sort_epoch, is_semver FROM releases_release WHERE project_id = $1 ORDER BY sort_epoch DESC LIMIT 1",
  )
  .bind(project_id)
  .fetch_optional(&mut **tx)
  .await?;
  let sort_epoch = match last_epoch {
    None => 0,
    Some((epoch, last_is_semver)) if last_is_semver == is_semver => epoch,
    Some((epoch, _)) => epoch + 1,
  };
  let release_id = Uuid::new_v4();
  sqlx::query(
    "INSERT INTO releases_release (id, project_id, version, date_released, semver, is_semver, sort_epoch)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
  )
  .bind(release_id)
  .bind(project_id)
  .bind(&version)
  .bind(timestamp)
  .bind(if is_semver { semver_part } else { "" })
  .bind(is_semver)
  .bind(sort_epoch)
  .execute(&mut **tx)
  .await?;

  if version.is_empty() {
    return Ok((version, true)); // the empty release exists only to remember its date
  }
  if !*has_releases {
    *has_releases = true;
    sqlx::query("UPDATE projects_project SET has_releases = true WHERE id = $1")
      .bind(project_id)
      .execute(&mut **tx)
      .await?;
  }

  let latest = ordered_releases(&mut **tx, project_id).await?.pop().map(|r| r.id);
  if latest == Some(release_id) {
    // Issues "resolved by next release" are now resolved in this one.
    let metadata = json_dumps(&json!({"actual_release": version}));
    let waiting: Vec<Uuid> =
      sqlx::query_scalar("SELECT id FROM issues_issue WHERE project_id = $1 AND is_resolved_by_next_release")
        .bind(project_id)
        .fetch_all(&mut **tx)
        .await?;
    for waiting_issue in &waiting {
      let turning_point = NewTurningPoint {
        project_id,
        issue_id: *waiting_issue,
        kind: TurningPointKind::NextMaterialized,
        timestamp,
        triggering_event_id: None,
        user_id: None,
        metadata: &metadata,
        comment: "",
      };
      turning_point.insert(&mut **tx).await?;
    }
    sqlx::query(
      "UPDATE issues_issue SET fixed_at = fixed_at || $2, is_resolved_by_next_release = false
             WHERE project_id = $1 AND is_resolved_by_next_release",
    )
    .bind(project_id)
    .bind(format!("{version}\n"))
    .execute(&mut **tx)
    .await?;
    if let Some(issue) = issue
      && issue.is_resolved_by_next_release
    {
      issue.fixed_at.push_str(&version);
      issue.fixed_at.push('\n');
      issue.is_resolved_by_next_release = false;
    }
  }
  Ok((version, true))
}

/// `"a\nb\n"` -> `["a", "b"]` (newline-terminated lists, upstream's `parse_lines`).
pub(crate) fn parse_lines(s: &str) -> Vec<&str> {
  let mut parts: Vec<&str> = s.split('\n').collect();
  parts.pop();
  parts
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sanitizes() {
    assert_eq!(sanitize_version("a/b\tc\\d\n"), "abcd");
    assert_eq!(sanitize_version(".."), "sanitized");
    assert_eq!(sanitize_version(&"x".repeat(250)).len(), 200);
  }

  #[test]
  fn semver_detection() {
    assert_eq!(version_part("pkg@1.2.3"), "1.2.3");
    assert!(parse_semver(version_part("my@pkg@1.2.3-rc.1+build")).is_some());
    assert!(parse_semver("1.2").is_none());
    assert!(parse_semver("v1.2.3").is_none());
  }

  #[test]
  fn lines() {
    assert_eq!(parse_lines("a\nb\n"), ["a", "b"]);
    assert!(parse_lines("").is_empty());
  }
}
