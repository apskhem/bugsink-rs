//! What users do to issues: resolve (unconditionally, in a release, by the next release), reopen, mute
//! (unconditionally, for a time, until a volume of events), unmute and delete. A port of upstream Bugsink 2.6.3's
//! `apply_issue_action` with `IssueStateManager` (one issue, from the issue's own page) and
//! `IssueQuerysetStateManager` (bulk, from the issue lists), writing the rows and turning points upstream would.
//!
//! The two paths differ upstream, and so they do here. In particular, a bulk action re-applies its validity filter on
//! every UPDATE it does, so the follow-up updates after the first one usually match nothing: bulk-resolving a muted
//! issue leaves it muted, and a bulk "mute for" doesn't set `unmute_after` (the digest's volume-based and time-based
//! unmuting then see what upstream would have left behind).

pub mod deletion;

use bugsink_db::{PgPool, Postgres, Transaction, TurningPointKind};
use chrono::{DateTime, SubsecRound, Utc};
use serde_json::json;
use uuid::Uuid;

use crate::event_counter::Period;
use crate::grouping::py::{isoformat, json_dumps};

/// An action as upstream's forms post it, e.g. `resolve`, `resolved_release:1.2.0`, `mute_for:day,1,None`,
/// `mute_until:hour,24,5`.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Resolve,
    ResolvedRelease(String),
    ResolvedNext,
    Reopen,
    Mute,
    MuteFor { period: Period, nr_of_periods: u32 },
    MuteUntil { period: Period, nr_of_periods: u32, volume: u32, gte_threshold: String },
    Unmute,
    Delete,
}

impl Action {
    /// None for anything upstream would fail on (it answers those with a 500).
    pub fn parse(s: &str) -> Option<Self> {
        let period = |name: &str| Period::from_name(name).filter(|p| *p != Period::Total);
        Some(match s {
            "resolve" => Self::Resolve,
            "resolved_next" => Self::ResolvedNext,
            "reopen" => Self::Reopen,
            "mute" => Self::Mute,
            "unmute" => Self::Unmute,
            "delete" => Self::Delete,
            _ => {
                let (name, params) = s.split_once(':')?;
                match name {
                    "resolved_release" => Self::ResolvedRelease(params.to_owned()),
                    "mute_for" => {
                        let [period_name, nr_of_periods, _] = params.split(',').collect::<Vec<_>>()[..] else {
                            return None;
                        };
                        Self::MuteFor { period: period(period_name)?, nr_of_periods: nr_of_periods.parse().ok()? }
                    }
                    "mute_until" => {
                        let [period_name, nr_of_periods, gte] = params.split(',').collect::<Vec<_>>()[..] else {
                            return None;
                        };
                        Self::MuteUntil {
                            period: period(period_name)?,
                            nr_of_periods: nr_of_periods.parse().ok()?,
                            volume: gte.parse().ok()?,
                            gte_threshold: gte.to_owned(),
                        }
                    }
                    _ => return None,
                }
            }
        })
    }

    fn is_mute(&self) -> bool {
        matches!(self, Self::Mute | Self::MuteFor { .. } | Self::MuteUntil { .. })
    }

    /// `is_valid_issue_action`: strict, so an action that would be a no-op is not valid either.
    fn is_valid_for(&self, issue: &IssueState) -> bool {
        match self {
            Self::Delete => true,
            Self::Reopen => issue.is_resolved,
            _ if issue.is_resolved => false,
            _ if self.is_mute() => !issue.is_muted,
            Self::Unmute => issue.is_muted,
            _ => true,
        }
    }

    /// `q_for_invalid_issue_action`, as SQL over `issues_issue`.
    fn invalid_sql(&self) -> &'static str {
        match self {
            Self::Delete => "false",
            Self::Reopen => "NOT is_resolved",
            _ if self.is_mute() => "(is_resolved OR is_muted)",
            Self::Unmute => "(is_resolved OR NOT is_muted)",
            _ => "is_resolved",
        }
    }

    /// `make_issue_history`'s kind and metadata; None for delete (no history for an issue about to go).
    fn turning_point(&self, now: DateTime<Utc>) -> Option<(TurningPointKind, String)> {
        let (kind, metadata) = match self {
            Self::Delete => return None,
            Self::Resolve => (TurningPointKind::Resolved, json!({"resolved_unconditionally": true})),
            Self::ResolvedRelease(version) => (TurningPointKind::Resolved, json!({"resolved_release": version})),
            Self::ResolvedNext => (TurningPointKind::Resolved, json!({"resolve_by_next": true})),
            Self::Reopen => (TurningPointKind::Reopened, json!({})),
            Self::Mute => (TurningPointKind::Muted, json!({"mute_unconditionally": true})),
            Self::MuteFor { period, nr_of_periods } => (
                TurningPointKind::Muted,
                json!({"mute_for": {
                    "period_name": period.name(),
                    "nr_of_periods": nr_of_periods,
                    "unmute_after": isoformat(&period.add(now, *nr_of_periods)),
                }}),
            ),
            Self::MuteUntil { period, nr_of_periods, gte_threshold, .. } => (
                TurningPointKind::Muted,
                json!({"mute_until": {
                    "period_name": period.name(), "nr_of_periods": nr_of_periods, "gte_threshold": gte_threshold,
                }}),
            ),
            Self::Unmute => (TurningPointKind::Unmuted, json!({})),
        };
        Some((kind, json_dumps(&metadata)))
    }

    /// What a mute writes: `unmute_on_volume_based_conditions` and, for "mute for", `unmute_after`.
    fn mute_params(&self, now: DateTime<Utc>) -> (String, Option<DateTime<Utc>>) {
        match self {
            Self::MuteFor { period, nr_of_periods } => ("[]".into(), Some(period.add(now, *nr_of_periods))),
            Self::MuteUntil { period, nr_of_periods, volume, .. } => (
                json_dumps(&json!([{"period": period.name(), "nr_of_periods": nr_of_periods, "volume": volume}])),
                None,
            ),
            _ => ("[]".into(), None),
        }
    }
}

/// The fields an action reads and writes.
#[derive(sqlx::FromRow)]
struct IssueState {
    project_id: i64,
    is_resolved: bool,
    is_resolved_unconditionally: bool,
    is_resolved_by_next_release: bool,
    fixed_at: String,
    is_muted: bool,
    unmute_on_volume_based_conditions: String,
    unmute_after: Option<DateTime<Utc>>,
    next_unmute_check: i32,
    is_deleted: bool,
}

impl IssueState {
    fn resolve_flags(&mut self, unconditionally: bool, by_next_release: bool) {
        self.is_resolved = true;
        self.is_resolved_unconditionally = unconditionally;
        self.is_resolved_by_next_release = by_next_release;
        self.unmute();
    }

    /// `IssueStateManager.unmute` without a triggering event (user actions never have one).
    fn unmute(&mut self) {
        if self.is_muted {
            self.is_muted = false;
            self.unmute_on_volume_based_conditions = "[]".into();
            self.unmute_after = None;
        }
    }
}

/// `_handle_post` on an issue's page: applies the action if it is valid for the issue's current state (returns false
/// and changes nothing otherwise, like upstream, which then just shows the page again). The issue must exist and not
/// be deleted.
pub async fn apply_to_issue(
    pool: &PgPool,
    issue_id: Uuid,
    action: &Action,
    user_id: i64,
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    let now = now.trunc_subsecs(6); // what Postgres (and so upstream's metadata) keeps
    let mut tx = pool.begin().await?;
    // Same lock order as digest (project, then issue), so an action and a digest of the same issue serialize.
    let project_id: i64 = sqlx::query_scalar("SELECT project_id FROM issues_issue WHERE id = $1")
        .bind(issue_id)
        .fetch_one(&mut *tx)
        .await?;
    lock_projects(&mut tx, &[project_id]).await?;
    let mut issue: IssueState = sqlx::query_as(
        "SELECT project_id, is_resolved, is_resolved_unconditionally, is_resolved_by_next_release, fixed_at, is_muted,
                unmute_on_volume_based_conditions, unmute_after, next_unmute_check, is_deleted
         FROM issues_issue WHERE id = $1 FOR UPDATE",
    )
    .bind(issue_id)
    .fetch_one(&mut *tx)
    .await?;
    if issue.is_deleted || !action.is_valid_for(&issue) {
        return Ok(false);
    }

    if let Some((kind, metadata)) = action.turning_point(now) {
        sqlx::query(
            "INSERT INTO issues_turningpoint (project_id, issue_id, triggering_event_id, user_id, timestamp, kind,
                 metadata, comment)
             VALUES ($1, $2, NULL, $3, $4, $5, $6, '')",
        )
        .bind(issue.project_id)
        .bind(issue_id)
        .bind(user_id)
        .bind(now)
        .bind(kind as i32)
        .bind(metadata)
        .execute(&mut *tx)
        .await?;
    }

    match action {
        Action::Resolve => issue.resolve_flags(true, false),
        Action::ResolvedRelease(version) => {
            issue.resolve_flags(false, false);
            // `add_fixed_at`: fixed_at is a newline-terminated list of versions
            if !issue.fixed_at.split_terminator('\n').any(|v| v == version) {
                issue.fixed_at.push_str(version);
                issue.fixed_at.push('\n');
            }
        }
        Action::ResolvedNext => issue.resolve_flags(false, true),
        Action::Reopen => {
            issue.is_resolved = false;
            issue.is_resolved_unconditionally = false;
            issue.is_resolved_by_next_release = false;
            issue.unmute();
        }
        Action::Mute | Action::MuteFor { .. } | Action::MuteUntil { .. } => {
            let (conditions, unmute_after) = action.mute_params(now);
            issue.is_muted = true;
            issue.unmute_on_volume_based_conditions = conditions;
            issue.next_unmute_check = 0;
            if unmute_after.is_some() {
                issue.unmute_after = unmute_after;
            }
        }
        Action::Unmute => issue.unmute(),
        Action::Delete => {
            issue.is_deleted = true;
            mark_for_deletion(&mut tx, &[(issue_id, issue.project_id)]).await?;
        }
    }

    sqlx::query(
        "UPDATE issues_issue SET is_resolved = $2, is_resolved_unconditionally = $3, is_resolved_by_next_release = $4,
             fixed_at = $5, is_muted = $6, unmute_on_volume_based_conditions = $7, unmute_after = $8,
             next_unmute_check = $9, is_deleted = $10
         WHERE id = $1",
    )
    .bind(issue_id)
    .bind(issue.is_resolved)
    .bind(issue.is_resolved_unconditionally)
    .bind(issue.is_resolved_by_next_release)
    .bind(&issue.fixed_at)
    .bind(issue.is_muted)
    .bind(&issue.unmute_on_volume_based_conditions)
    .bind(issue.unmute_after)
    .bind(issue.next_unmute_check)
    .bind(issue.is_deleted)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// The bulk action of upstream's issue lists, on the issues among `issue_ids` that are in `project_ids` and not
/// deleted. Applies the action to those it is valid for and returns the others (`unapplied_issue_ids`).
pub async fn apply_to_issues(
    pool: &PgPool,
    project_ids: &[i64],
    issue_ids: &[Uuid],
    action: &Action,
    user_id: i64,
    now: DateTime<Utc>,
) -> anyhow::Result<Vec<Uuid>> {
    let now = now.trunc_subsecs(6);
    let mut tx = pool.begin().await?;
    let involved: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT project_id FROM issues_issue
         WHERE project_id = ANY($1) AND NOT is_deleted AND id = ANY($2)",
    )
    .bind(project_ids)
    .bind(issue_ids)
    .fetch_all(&mut *tx)
    .await?;
    lock_projects(&mut tx, &involved).await?;

    // `issue_qs` and `issue_qs.exclude(illegal_conditions)`; $1 project ids, $2 issue ids.
    let selected = "project_id = ANY($1) AND NOT is_deleted AND id = ANY($2)";
    let invalid = action.invalid_sql();
    let applicable = format!("{selected} AND NOT {invalid}");
    let unapplied: Vec<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM issues_issue WHERE {selected} AND {invalid}"
    )))
    .bind(project_ids)
    .bind(issue_ids)
    .fetch_all(&mut *tx)
    .await?;

    if let Some((kind, metadata)) = action.turning_point(now) {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO issues_turningpoint (project_id, issue_id, triggering_event_id, user_id, timestamp, kind,
                 metadata, comment)
             SELECT project_id, id, NULL, $3, $4, $5, $6, '' FROM issues_issue WHERE {applicable}"
        )))
        .bind(project_ids)
        .bind(issue_ids)
        .bind(user_id)
        .bind(now)
        .bind(kind as i32)
        .bind(metadata)
        .execute(&mut *tx)
        .await?;
    }

    // Each statement re-evaluates `applicable`, as each of upstream's `issue_qs.update(...)` calls does.
    let update = Update { applicable: &applicable, project_ids, issue_ids };
    let unmute = "is_muted = false, unmute_on_volume_based_conditions = '[]', unmute_after = NULL";
    match action {
        Action::Resolve => {
            update.run(&mut tx, RESOLVE_UNCONDITIONALLY, "", None).await?;
            update.run(&mut tx, unmute, "", None).await?;
        }
        Action::ResolvedRelease(version) => {
            // `_resolve_at`: `fixed_at__contains=version + "\n"`, a substring match unlike the single-issue path
            let marker = "strpos(fixed_at, $3 || E'\\n') > 0";
            update.run(&mut tx, RESOLVE_CONDITIONALLY, &format!("AND {marker}"), Some(version)).await?;
            let set = format!("{RESOLVE_CONDITIONALLY}, fixed_at = fixed_at || $3 || E'\\n'");
            update.run(&mut tx, &set, &format!("AND NOT {marker}"), Some(version)).await?;
            update.run(&mut tx, unmute, "", None).await?;
        }
        Action::ResolvedNext => {
            update.run(&mut tx, RESOLVE_BY_NEXT, "", None).await?;
            update.run(&mut tx, unmute, "", None).await?;
        }
        Action::Reopen => {
            let set = "is_resolved = false, is_resolved_unconditionally = false, is_resolved_by_next_release = false";
            update.run(&mut tx, set, "", None).await?;
            update.run(&mut tx, unmute, "", None).await?;
        }
        Action::Mute | Action::MuteFor { .. } | Action::MuteUntil { .. } => {
            let (conditions, unmute_after) = action.mute_params(now);
            let set = "is_muted = true, unmute_on_volume_based_conditions = $3, next_unmute_check = 0";
            update.run(&mut tx, set, "", Some(&conditions)).await?;
            if let Some(unmute_after) = unmute_after {
                update.run(&mut tx, "unmute_after = $3::timestamptz", "", Some(&unmute_after.to_rfc3339())).await?;
            }
        }
        Action::Unmute => update.run(&mut tx, unmute, "", None).await?,
        Action::Delete => {
            let pairs: Vec<(Uuid, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT id, project_id FROM issues_issue WHERE {applicable}"
            )))
            .bind(project_ids)
            .bind(issue_ids)
            .fetch_all(&mut *tx)
            .await?;
            mark_for_deletion(&mut tx, &pairs).await?;
        }
    }
    tx.commit().await?;
    Ok(unapplied)
}

const RESOLVE_UNCONDITIONALLY: &str =
    "is_resolved = true, is_resolved_unconditionally = true, is_resolved_by_next_release = false";
const RESOLVE_CONDITIONALLY: &str =
    "is_resolved = true, is_resolved_unconditionally = false, is_resolved_by_next_release = false";
const RESOLVE_BY_NEXT: &str =
    "is_resolved = true, is_resolved_unconditionally = false, is_resolved_by_next_release = true";

/// One `issue_qs.update(...)` of a bulk action: `$1`/`$2` select the issues, `$3` is the optional parameter.
struct Update<'a> {
    applicable: &'a str,
    project_ids: &'a [i64],
    issue_ids: &'a [Uuid],
}

impl Update<'_> {
    async fn run(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        set: &str,
        and_where: &str,
        param: Option<&str>,
    ) -> sqlx::Result<()> {
        let sql = format!("UPDATE issues_issue SET {set} WHERE {} {and_where}", self.applicable);
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(self.project_ids).bind(self.issue_ids);
        if let Some(param) = param {
            q = q.bind(param);
        }
        q.execute(&mut **tx).await?;
        Ok(())
    }
}

/// Row locks on the projects, in id order (digest locks one project at a time).
async fn lock_projects(tx: &mut Transaction<'_, Postgres>, project_ids: &[i64]) -> sqlx::Result<()> {
    sqlx::query("SELECT id FROM projects_project WHERE id = ANY($1) ORDER BY id FOR UPDATE")
        .bind(project_ids)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// `mark_issues_for_deletion` plus scheduling `delete_issue_deps` per issue: the issues disappear from every list at
/// once and their groupings no longer match new events (which open a new issue instead); the rows themselves are
/// deleted in the background (`deletion`).
async fn mark_for_deletion(tx: &mut Transaction<'_, Postgres>, pairs: &[(Uuid, i64)]) -> anyhow::Result<()> {
    if pairs.is_empty() {
        return Ok(());
    }
    let issue_ids: Vec<Uuid> = pairs.iter().map(|(issue_id, _)| *issue_id).collect();
    let project_ids: Vec<i64> = pairs.iter().map(|(_, project_id)| *project_id).collect();
    sqlx::query("UPDATE issues_issue SET is_deleted = true WHERE id = ANY($1)")
        .bind(&issue_ids)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "UPDATE projects_project p SET issue_count = p.issue_count - d.n
         FROM (SELECT project_id, COUNT(*) AS n FROM unnest($1::bigint[]) AS project_id GROUP BY project_id) d
         WHERE p.id = d.project_id",
    )
    .bind(&project_ids)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE issues_grouping SET grouping_key_hash = NULL WHERE issue_id = ANY($1)")
        .bind(&issue_ids)
        .execute(&mut **tx)
        .await?;
    for &(issue_id, project_id) in pairs {
        deletion::enqueue(&mut **tx, project_id, issue_id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parses_upstream_actions() {
        assert_eq!(Action::parse("resolve"), Some(Action::Resolve));
        assert_eq!(Action::parse("resolved_release:pkg@1.0:x"), Some(Action::ResolvedRelease("pkg@1.0:x".into())));
        assert_eq!(
            Action::parse("mute_for:day,1,None"),
            Some(Action::MuteFor { period: Period::Day, nr_of_periods: 1 })
        );
        assert_eq!(
            Action::parse("mute_until:hour,24,5"),
            Some(Action::MuteUntil { period: Period::Hour, nr_of_periods: 24, volume: 5, gte_threshold: "5".into() })
        );
        let bad_actions =
            ["", "resolve:x", "mute_for:day,1", "mute_for:fortnight,1,None", "mute_until:hour,24,x", "frobnicate"];
        for bad in bad_actions {
            assert_eq!(Action::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn metadata_like_upstream() {
        let now = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
        let (_, m) = Action::parse("mute_for:hour,3,None").unwrap().turning_point(now).unwrap();
        assert_eq!(
            m,
            r#"{"mute_for": {"period_name": "hour", "nr_of_periods": 3, "unmute_after": "2026-11-01T15:00:00+00:00"}}"#
        );
        let (_, m) = Action::parse("mute_until:day,1,10").unwrap().turning_point(now).unwrap();
        assert_eq!(m, r#"{"mute_until": {"period_name": "day", "nr_of_periods": 1, "gte_threshold": "10"}}"#);
        let (conditions, _) = Action::parse("mute_until:day,1,10").unwrap().mute_params(now);
        assert_eq!(conditions, r#"[{"period": "day", "nr_of_periods": 1, "volume": 10}]"#);
        assert_eq!(Action::Delete.turning_point(now), None);
    }
}
