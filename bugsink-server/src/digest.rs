//! Digestion: the second half of ingestion, run as a background job. A port of upstream Bugsink 2.6.3's
//! `ingest/tasks.py::digest_event`, writing exactly the rows (and counters) upstream would, so that upstream can read
//! and continue from a database we wrote, and vice versa.
//!
//! Ported: grouping (incl. mechanism transition window and legacy attach), issue/project counters and digest orders,
//! event rows incl. retention irrelevance, hourly counts, releases (incl. "resolved by next release"), regressions +
//! reopen, time-based unmute, FIRST_SEEN/REGRESSED/UNMUTED/NEXT_MATERIALIZED turning points, tags, eviction (incl.
//! zero retention), installation/project quotas (`crate::quota`), alerts (enqueued for `crate::alerts`), volume-based
//! unmute, browser/OS tags from the User-Agent header.
//!
//! SKIPPED(parity) (optional extras, see the README): minidumps, `VALIDATE_ON_DIGEST`, file-based event storage.

mod counts;
pub(crate) mod releases;
pub mod retention;
mod tags;
mod user_agent;

/// For `tools/parity/user_agents.sh`.
#[doc(hidden)]
pub use tags::contexts_enriched_with_user_agent;

use bugsink_db::{PgPool, Postgres, Transaction, TurningPointKind};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::alerts::{self, Alert};
use crate::event_counter::{Counted, Period, Threshold, check_for_thresholds};
use crate::grouping::py::{isoformat, json_dumps, py_str, take_chars, truthy};
use crate::grouping::{self, GroupingError, KeyWithMechanism};
use crate::protocol::event::parse_timestamp;
use crate::quota::{self, QuotaSettings, QuotaState};

pub const JOB_KIND: &str = "digest";

const GROUPING_TRANSITION_PERIOD: TimeDelta = TimeDelta::days(30);

/// What ingest hands to digest, via the job queue (upstream: snappea task args + the event file).
#[derive(Debug, Serialize, Deserialize)]
pub struct DigestJob {
    pub project_id: i64,
    pub event_id: Uuid,
    pub ingested_at: DateTime<Utc>,
    pub remote_addr: Option<String>,
    pub event: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum DigestError {
    /// Input upstream can't digest either; upstream drops such events (the task raises), and so do we.
    #[error("dropping event: {0}")]
    Drop(String),
    #[error(transparent)]
    Retry(#[from] anyhow::Error),
}

impl From<sqlx::Error> for DigestError {
    fn from(e: sqlx::Error) -> Self {
        Self::Retry(e.into())
    }
}

impl From<GroupingError> for DigestError {
    fn from(e: GroupingError) -> Self {
        Self::Drop(e.to_string())
    }
}

/// Job handler, for `crate::tasks::Registry::register`.
pub async fn handle_job(pool: PgPool, payload: Value, quotas: QuotaSettings) -> anyhow::Result<()> {
    let job: DigestJob = serde_json::from_value(payload)?;
    let event_id = job.event_id;
    match digest(&pool, job, Utc::now(), &quotas).await {
        Ok(_) => Ok(()),
        Err(DigestError::Drop(reason)) => {
            tracing::warn!(%event_id, %reason, "event dropped in digest");
            Ok(())
        }
        Err(DigestError::Retry(e)) => Err(e),
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Digested { issue_id: Uuid, issue_created: bool },
    ProjectGone,
    ZeroRetention,
    OverQuota,
    Duplicate,
}

pub(crate) struct Project {
    pub id: i64,
    pub digested_event_count: i32,
    pub stored_event_count: i32,
    pub issue_count: i32,
    pub has_releases: bool,
    retention_max_event_count: i32,
    quota: QuotaState,
    alert_on_new_issue: bool,
    alert_on_regression: bool,
    alert_on_unmute: bool,
    grouping_mechanism: String,
    previous_grouping_mechanism: Option<String>,
    grouping_mechanism_upgraded_at: Option<DateTime<Utc>>,
}

pub(crate) struct Issue {
    pub id: Uuid,
    last_seen: DateTime<Utc>,
    digested_event_count: i32,
    stored_event_count: i32,
    calculated_type: String,
    calculated_value: String,
    is_resolved: bool,
    is_resolved_unconditionally: bool,
    pub is_resolved_by_next_release: bool,
    pub fixed_at: String,
    events_at: String,
    is_muted: bool,
    unmute_on_volume_based_conditions: String,
    unmute_after: Option<DateTime<Utc>>,
    next_unmute_check: i32,
}

enum GroupingPath {
    Found { grouping_id: i64, issue_id: Uuid },
    Attach { key: KeyWithMechanism, issue_id: Uuid },
    New { key: KeyWithMechanism },
}

pub async fn digest(
    pool: &PgPool,
    job: DigestJob,
    digested_at: DateTime<Utc>,
    quotas: &QuotaSettings,
) -> Result<Outcome, DigestError> {
    let mut tx = pool.begin().await?;

    // Serializes digestion per project (upstream: one global write lock): counters below are read-modify-write.
    let Some(mut project) = lock_project(&mut tx, job.project_id).await? else {
        return Ok(Outcome::ProjectGone);
    };
    // `get_retention_max_event_count` (only read below, never written back)
    project.retention_max_event_count = quotas.retention_max_event_count(project.retention_max_event_count);
    if project.retention_max_event_count == 0 {
        // `handle_zero_retention`: evict-before-add can't get below 1, so stored events are deleted by a job instead.
        if project.stored_event_count > 0 {
            retention::enqueue_delete_by_age(&mut tx, project.id).await?;
            tx.commit().await?;
        }
        return Ok(Outcome::ZeroRetention);
    }
    // Upstream relies on the (project, event_id) unique constraint and rolls back; checking first is equivalent.
    let duplicate: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM events_event WHERE project_id = $1 AND event_id = $2)")
            .bind(project.id)
            .bind(job.event_id)
            .fetch_one(&mut *tx)
            .await?;
    if duplicate {
        return Ok(Outcome::Duplicate);
    }

    // Installation first, then the project (whose count this increments). Over quota, the event is dropped but the
    // bookkeeping is kept.
    if !quota::count_installation_periods_and_act_on_it(&mut tx, quotas, digested_at).await?
        || !quota::count_project_periods_and_act_on_it(
            &mut tx,
            quotas,
            project.id,
            &mut project.digested_event_count,
            &mut project.quota,
            digested_at,
        )
        .await?
    {
        tx.commit().await?;
        return Ok(Outcome::OverQuota);
    }

    let data = &job.event;
    let denormalized = grouping::denormalized_fields(data)?;
    let (calculated_type, calculated_value) = grouping::display_type_and_value(data)?;

    let (mut issue, grouping_id, issue_created) = match grouping_path(&mut tx, &project, data, digested_at).await? {
        GroupingPath::Found { grouping_id, issue_id } => (load_issue(&mut tx, issue_id).await?, grouping_id, false),
        GroupingPath::Attach { key, issue_id } => {
            let grouping_id = create_grouping(&mut tx, project.id, &key, issue_id).await?;
            (load_issue(&mut tx, issue_id).await?, grouping_id, false)
        }
        GroupingPath::New { key } => {
            let issue = create_issue(&mut tx, &project, &job, &calculated_type, &calculated_value, &denormalized).await?;
            let grouping_id = create_grouping(&mut tx, project.id, &key, issue.id).await?;
            (issue, grouping_id, true)
        }
    };
    if !issue_created {
        issue.last_seen = job.ingested_at;
        issue.digested_event_count += 1;
        issue.calculated_type = calculated_type.clone();
        issue.calculated_value = calculated_value.clone();
    }

    // Evict before adding: the +1 is the event about to be stored.
    let project_stored_event_count = project.stored_event_count + 1;
    let evicted = if retention::should_evict(&project, project_stored_event_count) {
        let evicted = retention::evict_for_max_events(&mut tx, &project, digested_at, project_stored_event_count).await?;
        // This issue's count is updated in memory and saved below.
        let others = evicted.per_issue.iter().filter(|(id, _)| **id != issue.id).map(|(id, n)| (*id, *n));
        retention::update_issue_counts(&mut tx, others).await?;
        evicted
    } else {
        retention::EvictionCounts::default()
    };
    issue.stored_event_count += 1 - evicted.for_issue(issue.id);
    project.stored_event_count = project_stored_event_count - evicted.total;
    if issue_created {
        project.issue_count += 1;
    }
    sqlx::query(
        "UPDATE projects_project SET digested_event_count = $2, stored_event_count = $3, issue_count = $4,
             next_quota_check = $5, quota_exceeded_until = $6, quota_exceeded_reason = $7
         WHERE id = $1",
    )
    .bind(project.id)
    .bind(project.digested_event_count)
    .bind(project.stored_event_count)
    .bind(project.issue_count)
    .bind(project.quota.next_quota_check)
    .bind(project.quota.exceeded_until)
    .bind(&project.quota.exceeded_reason)
    .execute(&mut *tx)
    .await?;

    let event = insert_event(&mut tx, &job, &project, &issue, grouping_id, digested_at, &calculated_type, &calculated_value, &denormalized)
        .await?;

    counts::record_event_counts(&mut tx, project.id, issue.id, digested_at, event.digest_order).await?;

    let (release_version, _) = releases::create_release_if_needed(
        &mut tx,
        project.id,
        &mut project.has_releases,
        &event.release,
        job.ingested_at,
        Some(&mut issue),
    )
    .await?;

    let mut never_evict = false;
    if issue_created {
        insert_turning_point(&mut tx, project.id, issue.id, Some(event.id), job.ingested_at, TurningPointKind::FirstSeen, "{}")
            .await?;
        never_evict = true;
        if project.alert_on_new_issue {
            alerts::enqueue(&mut tx, issue.id, Alert::NewIssue).await?;
        }
    } else {
        if releases::issue_is_regression(&mut tx, &project, &issue, &event.release).await? {
            insert_turning_point(&mut tx, project.id, issue.id, Some(event.id), job.ingested_at, TurningPointKind::Regressed, "{}")
                .await?;
            never_evict = true;
            if project.alert_on_regression {
                alerts::enqueue(&mut tx, issue.id, Alert::Regression).await?;
            }
            issue.reopen();
        }
        if let Some(unmute_after) = issue.unmute_after.filter(|after| issue.is_muted && digested_at > *after) {
            issue.unmute();
            let metadata = json!({"mute_for": {"unmute_after": isoformat(&unmute_after)}});
            insert_turning_point(&mut tx, project.id, issue.id, Some(event.id), job.ingested_at, TurningPointKind::Unmuted, &json_dumps(&metadata))
                .await?;
            never_evict = true; // events that trigger a turning point are kept
            if project.alert_on_unmute {
                let unmute_reason = alerts::unmute_reason_mute_for(unmute_after);
                alerts::enqueue(&mut tx, issue.id, Alert::Unmute { unmute_reason }).await?;
            }
        }
    }
    if let Some(condition) = volume_based_unmute(&mut tx, &mut issue, digested_at).await?
        && issue.is_muted
    {
        issue.unmute();
        let metadata = json!({"mute_until": condition});
        insert_turning_point(&mut tx, project.id, issue.id, Some(event.id), job.ingested_at, TurningPointKind::Unmuted, &json_dumps(&metadata))
            .await?;
        never_evict = true;
        if project.alert_on_unmute {
            let unmute_reason = alerts::unmute_reason_mute_until(&condition);
            alerts::enqueue(&mut tx, issue.id, Alert::Unmute { unmute_reason }).await?;
        }
    }

    if never_evict {
        sqlx::query("UPDATE events_event SET never_evict = true WHERE id = $1").bind(event.id).execute(&mut *tx).await?;
    }
    if !issue.events_at.contains(&format!("{release_version}\n")) {
        issue.events_at.push_str(&release_version);
        issue.events_at.push('\n');
    }
    save_issue(&mut tx, &issue).await?;

    tags::digest_tags(&mut tx, data, &event, issue.id).await?;

    tx.commit().await?;
    Ok(Outcome::Digested { issue_id: issue.id, issue_created })
}

async fn lock_project(tx: &mut Transaction<'_, Postgres>, project_id: i64) -> sqlx::Result<Option<Project>> {
    let row = sqlx::query(
        "SELECT id, digested_event_count, stored_event_count, issue_count, has_releases, retention_max_event_count,
                next_quota_check, quota_exceeded_until, quota_exceeded_reason,
                alert_on_new_issue, alert_on_regression, alert_on_unmute,
                grouping_mechanism, previous_grouping_mechanism, grouping_mechanism_upgraded_at
         FROM projects_project WHERE id = $1 AND NOT is_deleted FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|r| Project {
        id: r.get("id"),
        digested_event_count: r.get("digested_event_count"),
        stored_event_count: r.get("stored_event_count"),
        issue_count: r.get("issue_count"),
        has_releases: r.get("has_releases"),
        retention_max_event_count: r.get("retention_max_event_count"),
        quota: QuotaState {
            next_quota_check: r.get("next_quota_check"),
            exceeded_until: r.get("quota_exceeded_until"),
            exceeded_reason: r.get("quota_exceeded_reason"),
        },
        alert_on_new_issue: r.get("alert_on_new_issue"),
        alert_on_regression: r.get("alert_on_regression"),
        alert_on_unmute: r.get("alert_on_unmute"),
        grouping_mechanism: r.get("grouping_mechanism"),
        previous_grouping_mechanism: r.get("previous_grouping_mechanism"),
        grouping_mechanism_upgraded_at: r.get("grouping_mechanism_upgraded_at"),
    }))
}

/// `get_grouping_path_for_event`.
async fn grouping_path(
    tx: &mut Transaction<'_, Postgres>,
    project: &Project,
    data: &Value,
    digested_at: DateTime<Utc>,
) -> Result<GroupingPath, DigestError> {
    let current = grouping::key_with_mechanism(data, &project.grouping_mechanism)?;
    if let Some((grouping_id, issue_id)) = existing_grouping(tx, project.id, &current.key, &current.mechanism).await? {
        return Ok(GroupingPath::Found { grouping_id, issue_id });
    }

    if current.mechanism == grouping::MECHANISM_INDEPENDENT {
        // Fingerprint-only keys created before mechanisms were recorded are stored as v1.
        return Ok(match existing_grouping(tx, project.id, &current.key, grouping::BUGSINK_V1).await? {
            Some((_, issue_id)) => GroupingPath::Attach { key: current, issue_id },
            None => GroupingPath::New { key: current },
        });
    }

    let transition_active = match (&project.previous_grouping_mechanism, project.grouping_mechanism_upgraded_at) {
        (Some(_), Some(upgraded_at)) => digested_at <= upgraded_at + GROUPING_TRANSITION_PERIOD,
        _ => false,
    };
    if !transition_active {
        return Ok(GroupingPath::New { key: current });
    }

    let previous_mechanism = project.previous_grouping_mechanism.as_deref().expect("checked above");
    let previous = grouping::key_with_mechanism(data, previous_mechanism)?;
    Ok(match existing_grouping(tx, project.id, &previous.key, &previous.mechanism).await? {
        Some((_, issue_id)) => GroupingPath::Attach { key: current, issue_id },
        None => GroupingPath::New { key: current },
    })
}

async fn existing_grouping(
    tx: &mut Transaction<'_, Postgres>,
    project_id: i64,
    key: &str,
    mechanism: &str,
) -> sqlx::Result<Option<(i64, Uuid)>> {
    let hash = KeyWithMechanism { key: key.into(), mechanism: mechanism.into() }.hash();
    sqlx::query_as(
        "SELECT id, issue_id FROM issues_grouping
         WHERE project_id = $1 AND grouping_key = $2 AND grouping_key_hash = $3 AND grouping_mechanism = $4
         ORDER BY id LIMIT 1",
    )
    .bind(project_id)
    .bind(key)
    .bind(hash)
    .bind(mechanism)
    .fetch_optional(&mut **tx)
    .await
}

async fn create_grouping(
    tx: &mut Transaction<'_, Postgres>,
    project_id: i64,
    key: &KeyWithMechanism,
    issue_id: Uuid,
) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "INSERT INTO issues_grouping (project_id, grouping_key, grouping_key_hash, grouping_mechanism, issue_id)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(project_id)
    .bind(&key.key)
    .bind(key.hash())
    .bind(&key.mechanism)
    .bind(issue_id)
    .fetch_one(&mut **tx)
    .await
}

const ISSUE_STATE_COLUMNS: &str = "id, last_seen, digested_event_count, stored_event_count, calculated_type,
    calculated_value, is_resolved, is_resolved_unconditionally, is_resolved_by_next_release, fixed_at, events_at,
    is_muted, unmute_on_volume_based_conditions, unmute_after, next_unmute_check";

fn issue_from_row(r: &sqlx::postgres::PgRow) -> Issue {
    Issue {
        id: r.get("id"),
        last_seen: r.get("last_seen"),
        digested_event_count: r.get("digested_event_count"),
        stored_event_count: r.get("stored_event_count"),
        calculated_type: r.get("calculated_type"),
        calculated_value: r.get("calculated_value"),
        is_resolved: r.get("is_resolved"),
        is_resolved_unconditionally: r.get("is_resolved_unconditionally"),
        is_resolved_by_next_release: r.get("is_resolved_by_next_release"),
        fixed_at: r.get("fixed_at"),
        events_at: r.get("events_at"),
        is_muted: r.get("is_muted"),
        unmute_on_volume_based_conditions: r.get("unmute_on_volume_based_conditions"),
        unmute_after: r.get("unmute_after"),
        next_unmute_check: r.get("next_unmute_check"),
    }
}

async fn load_issue(tx: &mut Transaction<'_, Postgres>, issue_id: Uuid) -> sqlx::Result<Issue> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {ISSUE_STATE_COLUMNS} FROM issues_issue WHERE id = $1 FOR UPDATE")))
        .bind(issue_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(issue_from_row(&row))
}

async fn create_issue(
    tx: &mut Transaction<'_, Postgres>,
    project: &Project,
    job: &DigestJob,
    calculated_type: &str,
    calculated_value: &str,
    d: &grouping::Denormalized,
) -> sqlx::Result<Issue> {
    // Django model defaults are not database defaults: every column is spelled out. MAX + 1 is safe because the
    // project row is locked.
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO issues_issue (
             id, project_id, digest_order, first_seen, last_seen, digested_event_count, stored_event_count,
             calculated_type, calculated_value, transaction, last_frame_filename, last_frame_module, last_frame_function,
             is_resolved, is_resolved_by_next_release, is_resolved_unconditionally, fixed_at, events_at,
             is_muted, unmute_on_volume_based_conditions, unmute_after, next_unmute_check, is_deleted)
         SELECT $1, $2, COALESCE(MAX(digest_order), 0) + 1, $3, $3, 1, 0, $4, $5, $6, $7, $8, $9,
                false, false, false, '', '', false, '[]', NULL, 0, false
         FROM issues_issue WHERE project_id = $2
         RETURNING {ISSUE_STATE_COLUMNS}"
    )))
    .bind(Uuid::new_v4())
    .bind(project.id)
    .bind(job.ingested_at)
    .bind(calculated_type)
    .bind(calculated_value)
    .bind(&d.transaction)
    .bind(&d.last_frame_filename)
    .bind(&d.last_frame_module)
    .bind(&d.last_frame_function)
    .fetch_one(&mut **tx)
    .await?;
    Ok(issue_from_row(&row))
}

impl Issue {
    /// `IssueStateManager.reopen`.
    fn reopen(&mut self) {
        self.is_resolved = false;
        self.is_resolved_unconditionally = false;
        self.is_resolved_by_next_release = false;
        self.unmute();
    }

    /// `IssueStateManager.unmute` (the state part; the caller records the turning point).
    fn unmute(&mut self) {
        if self.is_muted {
            self.is_muted = false;
            self.unmute_on_volume_based_conditions = "[]".into();
            self.unmute_after = None;
        }
    }
}

async fn save_issue(tx: &mut Transaction<'_, Postgres>, issue: &Issue) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE issues_issue SET
             last_seen = $2, digested_event_count = $3, stored_event_count = $4, calculated_type = $5,
             calculated_value = $6, is_resolved = $7, is_resolved_unconditionally = $8,
             is_resolved_by_next_release = $9, fixed_at = $10, events_at = $11, is_muted = $12,
             unmute_on_volume_based_conditions = $13, unmute_after = $14, next_unmute_check = $15
         WHERE id = $1",
    )
    .bind(issue.id)
    .bind(issue.last_seen)
    .bind(issue.digested_event_count)
    .bind(issue.stored_event_count)
    .bind(&issue.calculated_type)
    .bind(&issue.calculated_value)
    .bind(issue.is_resolved)
    .bind(issue.is_resolved_unconditionally)
    .bind(issue.is_resolved_by_next_release)
    .bind(&issue.fixed_at)
    .bind(&issue.events_at)
    .bind(issue.is_muted)
    .bind(&issue.unmute_on_volume_based_conditions)
    .bind(issue.unmute_after)
    .bind(issue.next_unmute_check)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// `count_issue_periods_and_act_on_it`: when a check is due, counts the issue's events against its volume-based unmute
/// conditions (`[{"period": "hour", "nr_of_periods": 24, "volume": 5}]`) and returns the first one that is met. Runs
/// after the event is stored, so it counts itself.
async fn volume_based_unmute(
    tx: &mut Transaction<'_, Postgres>,
    issue: &mut Issue,
    now: DateTime<Utc>,
) -> Result<Option<Value>, DigestError> {
    let conditions: Vec<Value> = serde_json::from_str(&issue.unmute_on_volume_based_conditions)
        .map_err(|e| DigestError::Drop(format!("invalid unmute_on_volume_based_conditions: {e}")))?;
    if conditions.is_empty() || issue.digested_event_count < issue.next_unmute_check {
        return Ok(None);
    }
    let thresholds = conditions
        .iter()
        .map(|c| {
            Some(Threshold {
                period: Period::from_name(c.get("period")?.as_str()?)?,
                nr_of_periods: u32::try_from(c.get("nr_of_periods")?.as_u64()?).ok()?,
                gte: c.get("volume")?.as_i64()?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| DigestError::Drop(format!("invalid volume-based condition in {:?}", conditions)))?;

    let states = check_for_thresholds(tx, Counted::Issue(issue.id), now, &thresholds, 0).await?;
    let check_again_after = states.iter().map(|s| s.check_again_after).min().unwrap_or(1).max(1);
    issue.next_unmute_check =
        i32::try_from(i64::from(issue.digested_event_count) + check_again_after).unwrap_or(i32::MAX);

    // Several conditions met at once (not possible from the UI) unmute once, for the first.
    Ok(states.iter().zip(&conditions).find(|(state, _)| state.exceeded).map(|(_, c)| {
        json!({"period": c["period"], "nr_of_periods": c["nr_of_periods"], "volume": c["volume"]})
    }))
}

pub(crate) struct StoredEvent {
    pub id: Uuid,
    pub digest_order: i32,
    pub release: String,
    pub remote_addr: Option<String>,
    pub project_id: i64,
}

/// `maybe_empty(data.get(key))` as a string, truncated like upstream (`[:max]`).
fn field(data: &Value, path: &[&str], max: usize) -> String {
    let mut cur = Some(data);
    for key in path {
        cur = cur.and_then(|v| v.get(key));
    }
    cur.filter(|v| truthy(v)).map(|v| take_chars(&py_str(v), max)).unwrap_or_default()
}

/// `Event.from_ingested`.
#[allow(clippy::too_many_arguments)]
async fn insert_event(
    tx: &mut Transaction<'_, Postgres>,
    job: &DigestJob,
    project: &Project,
    issue: &Issue,
    grouping_id: i64,
    digested_at: DateTime<Utc>,
    calculated_type: &str,
    calculated_value: &str,
    d: &grouping::Denormalized,
) -> sqlx::Result<StoredEvent> {
    let data = &job.event;
    let timestamp = match data.get("timestamp") {
        None => job.ingested_at,
        // Upstream fails on unparseable timestamps (NOT NULL column); we fall back to ingestion time.
        Some(ts) => parse_timestamp(ts).unwrap_or(job.ingested_at),
    };
    let platform = match field(data, &["platform"], 64) {
        p if p.is_empty() => "other".to_string(),
        p => p,
    };
    let release = field(data, &["release"], 250);
    let id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO events_event (
             id, event_id, project_id, issue_id, grouping_id, ingested_at, digested_at, data, storage_backend,
             timestamp, platform, level, logger, transaction, server_name, release, dist, environment,
             sdk_name, sdk_version, calculated_type, calculated_value,
             last_frame_filename, last_frame_module, last_frame_function, remote_addr,
             digest_order, project_digest_order, irrelevance_for_retention, never_evict)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21,
                 $22, $23, $24, $25::inet, $26, $27, $28, false)",
    )
    .bind(id)
    .bind(job.event_id)
    .bind(project.id)
    .bind(issue.id)
    .bind(grouping_id)
    .bind(job.ingested_at)
    .bind(digested_at)
    .bind(json_dumps(data))
    .bind(timestamp)
    .bind(&platform)
    .bind(field(data, &["level"], 7)) // upstream doesn't truncate (and fails on longer levels)
    .bind(field(data, &["logger"], 64))
    .bind(&d.transaction)
    .bind(field(data, &["server_name"], 255))
    .bind(&release)
    .bind(field(data, &["dist"], 64))
    .bind(field(data, &["environment"], 64))
    .bind(field(data, &["sdk", "name"], 255))
    .bind(field(data, &["sdk", "version"], 255))
    .bind(calculated_type)
    .bind(calculated_value)
    .bind(&d.last_frame_filename)
    .bind(&d.last_frame_module)
    .bind(&d.last_frame_function)
    .bind(&job.remote_addr)
    .bind(issue.digested_event_count)
    .bind(project.digested_event_count)
    .bind(random_irrelevance(issue.stored_event_count))
    .execute(&mut **tx)
    .await?;

    Ok(StoredEvent {
        id,
        digest_order: issue.digested_event_count,
        release,
        remote_addr: job.remote_addr.clone(),
        project_id: project.id,
    })
}

/// `get_random_irrelevance`: the more events an issue has, the less relevant (more evictable) a new one is on
/// average. Fixed at creation; used by eviction.
fn random_irrelevance(stored_event_count: i32) -> i32 {
    let n = (rand::random::<f64>() * f64::from(stored_event_count) * 2.0).round_ties_even() as u64;
    nonzero_leading_bits(n)
}

/// Number of binary digits up to and including the last 1: 0b101000 -> 3.
fn nonzero_leading_bits(n: u64) -> i32 {
    if n == 0 { 0 } else { (64 - n.leading_zeros() - n.trailing_zeros()) as i32 }
}

pub(crate) async fn insert_turning_point(
    tx: &mut Transaction<'_, Postgres>,
    project_id: i64,
    issue_id: Uuid,
    triggering_event_id: Option<Uuid>,
    timestamp: DateTime<Utc>,
    kind: TurningPointKind,
    metadata: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO issues_turningpoint (project_id, issue_id, triggering_event_id, user_id, timestamp, kind, metadata, comment)
         VALUES ($1, $2, $3, NULL, $4, $5, $6, '')",
    )
    .bind(project_id)
    .bind(issue_id)
    .bind(triggering_event_id)
    .bind(timestamp)
    .bind(kind as i32)
    .bind(metadata)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_bits() {
        for (n, expected) in [(0, 0), (1, 1), (0b100000, 1), (0b101000, 3), (0b110001, 6)] {
            assert_eq!(nonzero_leading_bits(n), expected, "{n:b}");
        }
    }
}
