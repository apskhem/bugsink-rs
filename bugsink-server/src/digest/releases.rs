//! `releases/models.py` and `issues/regressions.py`: releases are created on first sight; ordering is semver-based
//! when versions are semver, by date otherwise ("sort epochs" separate runs of the two).

use std::cmp::Ordering;

use bugsink_db::{Postgres, Transaction, TurningPointKind};
use crate::grouping::py::{json_dumps, take_chars};
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use super::{DigestError, Issue, Project, insert_turning_point};

struct Release {
    id: Uuid,
    version: String,
    date_released: DateTime<Utc>,
    semver: String,
    is_semver: bool,
    sort_epoch: i32,
}

/// Sentry's release-name restrictions, sanitize-don't-raise.
fn sanitize_version(version: &str) -> String {
    let stripped: String = version.chars().filter(|c| !matches!(c, '\n' | '\t' | '/' | '\\')).collect();
    let stripped = if matches!(stripped.as_str(), "." | ".." | " ") { "sanitized".to_string() } else { stripped };
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
    a.sort_epoch.cmp(&b.sort_epoch).then_with(|| match (a.is_semver, b.is_semver) {
        (true, true) => match (parse_semver(&a.semver), parse_semver(&b.semver)) {
            (Some(va), Some(vb)) => va.cmp_precedence(&vb),
            _ => Ordering::Equal,
        },
        _ => a.date_released.cmp(&b.date_released),
    })
}

async fn ordered_releases(tx: &mut Transaction<'_, Postgres>, project_id: i64) -> sqlx::Result<Vec<Release>> {
    let mut releases: Vec<Release> = sqlx::query(
        "SELECT id, version, date_released, semver, is_semver, sort_epoch FROM releases_release WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await?
    .iter()
    .map(|r| Release {
        id: r.get("id"),
        version: r.get("version"),
        date_released: r.get("date_released"),
        semver: r.get("semver"),
        is_semver: r.get("is_semver"),
        sort_epoch: r.get("sort_epoch"),
    })
    .collect();
    releases.sort_by(cmp_releases);
    Ok(releases)
}

/// `create_release_if_needed`; returns the (sanitized) version.
pub(crate) async fn create_release_if_needed(
    tx: &mut Transaction<'_, Postgres>,
    project: &mut Project,
    version: &str,
    timestamp: DateTime<Utc>,
    issue: &mut Issue,
) -> sqlx::Result<String> {
    let version = sanitize_version(version);
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM releases_release WHERE project_id = $1 AND version = $2)")
            .bind(project.id)
            .bind(&version)
            .fetch_one(&mut **tx)
            .await?;
    if exists {
        return Ok(version);
    }

    let semver_part = version_part(&version);
    let is_semver = parse_semver(semver_part).is_some();
    let last_epoch: Option<(i32, bool)> = sqlx::query_as(
        "SELECT sort_epoch, is_semver FROM releases_release WHERE project_id = $1 ORDER BY sort_epoch DESC LIMIT 1",
    )
    .bind(project.id)
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
    .bind(project.id)
    .bind(&version)
    .bind(timestamp)
    .bind(if is_semver { semver_part } else { "" })
    .bind(is_semver)
    .bind(sort_epoch)
    .execute(&mut **tx)
    .await?;

    if version.is_empty() {
        return Ok(version); // the empty release exists only to remember its date
    }
    if !project.has_releases {
        project.has_releases = true;
        sqlx::query("UPDATE projects_project SET has_releases = true WHERE id = $1")
            .bind(project.id)
            .execute(&mut **tx)
            .await?;
    }

    let latest = ordered_releases(tx, project.id).await?.pop().map(|r| r.id);
    if latest == Some(release_id) {
        // Issues "resolved by next release" are now resolved in this one.
        let metadata = json_dumps(&json!({"actual_release": version}));
        let waiting: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM issues_issue WHERE project_id = $1 AND is_resolved_by_next_release",
        )
        .bind(project.id)
        .fetch_all(&mut **tx)
        .await?;
        for waiting_issue in &waiting {
            insert_turning_point(tx, project.id, *waiting_issue, None, timestamp, TurningPointKind::NextMaterialized, &metadata)
                .await?;
        }
        sqlx::query(
            "UPDATE issues_issue SET fixed_at = fixed_at || $2, is_resolved_by_next_release = false
             WHERE project_id = $1 AND is_resolved_by_next_release",
        )
        .bind(project.id)
        .bind(format!("{version}\n"))
        .execute(&mut **tx)
        .await?;
        if issue.is_resolved_by_next_release {
            issue.fixed_at.push_str(&version);
            issue.fixed_at.push('\n');
            issue.is_resolved_by_next_release = false;
        }
    }
    Ok(version)
}

/// `"a\nb\n"` -> `["a", "b"]` (newline-terminated lists, upstream's `parse_lines`).
fn parse_lines(s: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split('\n').collect();
    parts.pop();
    parts
}

/// `issue_is_regression`: given a new event at `current_release`, was the issue resolved for that release?
pub(crate) async fn issue_is_regression(
    tx: &mut Transaction<'_, Postgres>,
    project: &Project,
    issue: &Issue,
    current_release: &str,
) -> Result<bool, DigestError> {
    if !issue.is_resolved || issue.is_resolved_by_next_release {
        return Ok(false);
    }
    if issue.is_resolved_unconditionally || !project.has_releases {
        return Ok(true);
    }
    let sorted = ordered_releases(tx, project.id).await?;
    let fixed_at = parse_lines(&issue.fixed_at);
    let events_at = parse_lines(&issue.events_at);

    let mut marked_as_resolved = false;
    for release in &sorted {
        if fixed_at.contains(&release.version.as_str()) {
            marked_as_resolved = true;
        } else if events_at.contains(&release.version.as_str()) {
            marked_as_resolved = false;
        }
        if release.version == current_release {
            return Ok(marked_as_resolved);
        }
    }
    // Upstream raises here too (e.g. a release name that sanitizing changed).
    Err(DigestError::Drop(format!("can't find release {current_release:?}")))
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
