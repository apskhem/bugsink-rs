//! Eviction: keeping a project's stored events under `retention_max_event_count`. A port of upstream's
//! `events/retention.py` (`should_evict`, `evict_for_max_events` and helpers) and of
//! `events/tasks.py::delete_by_age_until_under_retention_max` (the zero-retention cleanup job).
//!
//! An event's total irrelevance is its stored `irrelevance_for_retention` (fixed at creation, see
//! `digest::random_irrelevance`) plus an age-based irrelevance of `log4(age_in_hours + 1)`. Eviction lowers a cut-off
//! on the total until enough events are above it, newest epochs first, sparing `never_evict` events unless nothing else
//! is left.
//!
//! SKIPPED(parity) (optional extras, see the README): events in file-based storage (`storage_backend`) are deleted
//! from the table but not from storage.

use std::collections::HashMap;

use bugsink_db::{PgPool, Postgres, Transaction};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Project;
use crate::quota::QuotaSettings;
use crate::tasks;

const DELETE_OLD_EVENTS_BATCH_SIZE: i64 = 500;

#[derive(Debug, Default)]
pub(crate) struct EvictionCounts {
    pub total: i32,
    pub per_issue: HashMap<Uuid, i32>,
}

impl EvictionCounts {
    fn add(&mut self, other: EvictionCounts) {
        self.total += other.total;
        for (issue_id, count) in other.per_issue {
            *self.per_issue.entry(issue_id).or_default() += count;
        }
    }

    pub fn for_issue(&self, issue_id: Uuid) -> i32 {
        self.per_issue.get(&issue_id).copied().unwrap_or(0)
    }
}

/// (lower inclusive, upper exclusive) epoch bounds; `None` is unbounded.
type EpochBounds = (Option<i64>, Option<i64>);

/// Hours since 1970, the unit of age for eviction (`get_epoch`; same float arithmetic as Python's).
fn epoch(t: DateTime<Utc>) -> i64 {
    (t.timestamp_micros() as f64 / 1e6 / 3600.0) as i64
}

fn datetime_for_epoch(epoch: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(epoch * 3600, 0).single().expect("epoch in range")
}

pub(crate) fn should_evict(project: &Project, stored_event_count: i32) -> bool {
    stored_event_count > project.retention_max_event_count
}

/// Inverse of the age-based irrelevance `log4(age + 1)`: the age (in epochs) at which it reaches `irrelevance`.
fn age_for_irrelevance(irrelevance: u32) -> i64 {
    4i64.pow(irrelevance) - 1
}

/// Epoch bounds, newest first, paired with their age-based irrelevance (`get_epoch_bounds_with_irrelevance`, given the
/// oldest event's and the current epoch).
fn epoch_bounds_with_irrelevance(first_epoch: i64, current_epoch: i64) -> Vec<(EpochBounds, i32)> {
    let difference = current_epoch - first_epoch;
    let epochs = (0..).map(age_for_irrelevance).take_while(|age| *age < difference).map(|age| current_epoch - age);
    let points: Vec<Option<i64>> = std::iter::once(None).chain(epochs.map(Some)).chain(std::iter::once(None)).collect();
    points.windows(2).enumerate().map(|(irrelevance, w)| ((w[1], w[0]), irrelevance as i32)).collect()
}

/// How many events to evict at once (`eviction_target`): enough to not have to evict again right away, few enough to
/// not hold the project lock for long.
fn eviction_target(max_event_count: i32, stored_event_count: i32) -> i32 {
    ((f64::from(max_event_count) * 0.05) as i32).max(stored_event_count - max_event_count).min(500)
}

fn never_evict_filter(include_never_evict: bool) -> &'static str {
    if include_never_evict { "" } else { " AND never_evict = false" }
}

/// `evict_for_max_events`: evicts `eviction_target` events, `never_evict` ones only if nothing else is left.
pub(crate) async fn evict_for_max_events(
    tx: &mut Transaction<'_, Postgres>,
    project: &Project,
    now: DateTime<Utc>,
    stored_event_count: i32,
) -> sqlx::Result<EvictionCounts> {
    let mut evicted = evict_pass(tx, project, now, stored_event_count, false).await?;
    if let Pass::Exhausted = evicted.1 {
        // Everything that remains is never_evict: "never say never" and evict those as a last measure.
        let (more, outcome) = evict_pass(tx, project, now, stored_event_count - evicted.0.total, true).await?;
        if let Pass::Exhausted = outcome {
            // Upstream: "should not happen"; most likely the project's stored_event_count is too high.
            tracing::error!(
                project_id = project.id,
                evicted = evicted.0.total + more.total,
                max = project.retention_max_event_count,
                stored = stored_event_count,
                "failed to evict enough events"
            );
        }
        evicted.0.add(more);
    }
    Ok(evicted.0)
}

enum Pass {
    TargetReached,
    Exhausted,
}

/// One `evict_for_max_events` call for a given `include_never_evict`, minus the recursion.
async fn evict_pass(
    tx: &mut Transaction<'_, Postgres>,
    project: &Project,
    now: DateTime<Utc>,
    stored_event_count: i32,
    include_never_evict: bool,
) -> sqlx::Result<(EvictionCounts, Pass)> {
    let filter = never_evict_filter(include_never_evict);

    let oldest: Option<DateTime<Utc>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT MIN(digested_at) FROM events_event WHERE project_id = $1{filter}"
    )))
    .bind(project.id)
    .fetch_one(&mut **tx)
    .await?;
    let bounds = epoch_bounds_with_irrelevance(epoch(oldest.unwrap_or(now)), epoch(now));

    // Per epoch: (age-based irrelevance, max observed event irrelevance).
    let mut pairs = Vec::with_capacity(bounds.len());
    for &((lower, upper), age_irrelevance) in &bounds {
        let max: Option<i32> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT MAX(irrelevance_for_retention) FROM events_event
             WHERE project_id = $1{filter} AND ($2::timestamptz IS NULL OR digested_at >= $2)
               AND ($3::timestamptz IS NULL OR digested_at < $3)"
        )))
        .bind(project.id)
        .bind(lower.map(datetime_for_epoch))
        .bind(upper.map(datetime_for_epoch))
        .fetch_one(&mut **tx)
        .await?;
        pairs.push(age_irrelevance + max.unwrap_or(0));
    }

    let mut max_total_irrelevance = *pairs.iter().max().expect("at least one epoch");
    let target = eviction_target(project.retention_max_event_count, stored_event_count);
    let mut evicted = EvictionCounts::default();
    while evicted.total < target {
        // Decrement first: the observed max is exactly what gets evicted first (eviction is for "greater than").
        max_total_irrelevance -= 1;

        // `filter_for_work`: skip epochs where nothing exceeds the cut-off.
        let with_work = bounds.iter().zip(&pairs).filter(|(_, sum)| **sum > max_total_irrelevance).map(|(b, _)| *b);
        let more =
            evict_for_irrelevance(tx, project.id, max_total_irrelevance, with_work, include_never_evict, target - evicted.total)
                .await?;
        evicted.add(more);

        if evicted.total < target && max_total_irrelevance <= -1 {
            // The cut-off is below every possible irrelevance: nothing more can be evicted in this pass.
            return Ok((evicted, Pass::Exhausted));
        }
    }
    Ok((evicted, Pass::TargetReached))
}

/// `evict_for_irrelevance`: evicts events whose total irrelevance exceeds `max_total_irrelevance`, up to
/// `max_event_count`, newest epochs first.
async fn evict_for_irrelevance(
    tx: &mut Transaction<'_, Postgres>,
    project_id: i64,
    max_total_irrelevance: i32,
    bounds: impl Iterator<Item = (EpochBounds, i32)>,
    include_never_evict: bool,
    max_event_count: i32,
) -> sqlx::Result<EvictionCounts> {
    let mut evicted = EvictionCounts::default();
    for ((_, upper), age_irrelevance) in bounds {
        let max_item_irrelevance = max_total_irrelevance - age_irrelevance;
        let limit = max_event_count - evicted.total;
        let more = evict_for_epoch_and_irrelevance(tx, project_id, upper, max_item_irrelevance, limit, include_never_evict)
            .await?;
        evicted.add(more);

        // With no lower bound on time, -1 already evicted everything evictable in this and all older epochs.
        if max_item_irrelevance <= -1 || evicted.total >= max_event_count {
            break;
        }
    }
    Ok(evicted)
}

/// `evict_for_epoch_and_irrelevance`: evicts up to `limit` events older than `max_epoch` (exclusive) with an item
/// irrelevance above `max_irrelevance`.
async fn evict_for_epoch_and_irrelevance(
    tx: &mut Transaction<'_, Postgres>,
    project_id: i64,
    max_epoch: Option<i64>,
    max_irrelevance: i32,
    limit: i32,
    include_never_evict: bool,
) -> sqlx::Result<EvictionCounts> {
    let filter = format!(
        "project_id = $1 AND irrelevance_for_retention > $2{}{}",
        never_evict_filter(include_never_evict),
        if max_epoch.is_some() { " AND digested_at < $3" } else { "" },
    );
    let max_digested_at = max_epoch.map(datetime_for_epoch);

    if include_never_evict {
        // never_evict events are the ones turning points point at. Like upstream this clears the references for every
        // matching event, not just the `limit` deleted below.
        let mut q = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE issues_turningpoint SET triggering_event_id = NULL
             WHERE triggering_event_id IN (SELECT id FROM events_event WHERE {filter})"
        )))
        .bind(project_id)
        .bind(max_irrelevance);
        if let Some(t) = max_digested_at {
            q = q.bind(t);
        }
        q.execute(&mut **tx).await?;
    }

    if limit <= 0 {
        return Ok(EvictionCounts::default());
    }
    // Ties on digest_order are broken however the plan happens to read the rows (as upstream's). Not preparing the
    // statement gets the same per-call plan as upstream's literal SQL rather than a generic one.
    let mut q = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM events_event WHERE {filter} ORDER BY digest_order LIMIT {limit}"
    )))
    .persistent(false)
    .bind(project_id)
    .bind(max_irrelevance);
    if let Some(t) = max_digested_at {
        q = q.bind(t);
    }
    let ids: Vec<Uuid> = q.fetch_all(&mut **tx).await?;
    delete_events(tx, &ids).await
}

/// Deletes events and their tags; turning points must no longer point at them.
async fn delete_events(tx: &mut Transaction<'_, Postgres>, ids: &[Uuid]) -> sqlx::Result<EvictionCounts> {
    if ids.is_empty() {
        return Ok(EvictionCounts::default());
    }
    let per_issue: Vec<(Uuid, i64)> =
        sqlx::query_as("SELECT issue_id, COUNT(*) FROM events_event WHERE id = ANY($1) GROUP BY issue_id")
            .bind(ids)
            .fetch_all(&mut **tx)
            .await?;
    sqlx::query("DELETE FROM tags_eventtag WHERE event_id = ANY($1)").bind(ids).execute(&mut **tx).await?;
    let deleted = sqlx::query("DELETE FROM events_event WHERE id = ANY($1)").bind(ids).execute(&mut **tx).await?;
    Ok(EvictionCounts {
        total: deleted.rows_affected() as i32,
        per_issue: per_issue.into_iter().map(|(issue_id, n)| (issue_id, n as i32)).collect(),
    })
}

/// `update_issue_counts`: decrements `stored_event_count` of issues that lost events, one query per distinct count.
pub(crate) async fn update_issue_counts(
    tx: &mut Transaction<'_, Postgres>,
    per_issue: impl IntoIterator<Item = (Uuid, i32)>,
) -> sqlx::Result<()> {
    let mut by_count: HashMap<i32, Vec<Uuid>> = HashMap::new();
    for (issue_id, count) in per_issue {
        by_count.entry(count).or_default().push(issue_id);
    }
    for (count, issue_ids) in by_count {
        sqlx::query("UPDATE issues_issue SET stored_event_count = stored_event_count - $2 WHERE id = ANY($1)")
            .bind(&issue_ids)
            .bind(count)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct DeleteByAgeJob {
    pub project_id: i64,
}

impl tasks::Job for DeleteByAgeJob {
    const KIND: &str = "delete_by_age_until_under_retention_max";
}

pub(crate) async fn enqueue_delete_by_age(tx: &mut Transaction<'_, Postgres>, project_id: i64) -> anyhow::Result<()> {
    tasks::enqueue(&mut **tx, &DeleteByAgeJob { project_id }).await
}

/// Job handler (`delete_by_age_until_under_retention_max`): deletes the oldest events,
/// 500 per job, until the project is under its retention maximum. Digest schedules it when retention is zero, where
/// evict-before-add can't work.
pub async fn handle_delete_by_age_job(pool: PgPool, job: DeleteByAgeJob, quotas: QuotaSettings) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    let Some((stored_event_count, max_event_count)): Option<(i32, i32)> = sqlx::query_as(
        "SELECT stored_event_count, retention_max_event_count FROM projects_project WHERE id = $1 FOR UPDATE",
    )
    .bind(job.project_id)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(());
    };
    let max_event_count = quotas.retention_max_event_count(max_event_count);
    let how_many_too_many = i64::from((stored_event_count - max_event_count).max(0));
    if how_many_too_many == 0 {
        return Ok(());
    }

    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM events_event WHERE project_id = $1 ORDER BY digested_at, id LIMIT $2")
            .bind(job.project_id)
            .bind(how_many_too_many.min(DELETE_OLD_EVENTS_BATCH_SIZE))
            .fetch_all(&mut *tx)
            .await?;
    sqlx::query("UPDATE issues_turningpoint SET triggering_event_id = NULL WHERE triggering_event_id = ANY($1)")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    let evicted = delete_events(&mut tx, &ids).await?;
    update_issue_counts(&mut tx, evicted.per_issue).await?;
    sqlx::query("UPDATE projects_project SET stored_event_count = stored_event_count - $2 WHERE id = $1")
        .bind(job.project_id)
        .bind(evicted.total)
        .execute(&mut *tx)
        .await?;

    if evicted.total > 0 {
        enqueue_delete_by_age(&mut tx, job.project_id).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The expected values below are upstream's (events/tests.py).

    #[test]
    fn target() {
        assert_eq!(eviction_target(100, 101), 5);
        assert_eq!(eviction_target(10_000, 10_001), 500);
        assert_eq!(eviction_target(100_000, 100_001), 500);
        assert_eq!(eviction_target(100, 10_001), 500);
        assert_eq!(eviction_target(6, 7), 1);
    }

    #[test]
    fn bounds() {
        let now = Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap();
        let current = epoch(now);
        assert_eq!(current, 455832);
        assert_eq!(epoch_bounds_with_irrelevance(current, current), vec![((None, None), 0)]);
        assert_eq!(
            epoch_bounds_with_irrelevance(epoch(now - chrono::TimeDelta::hours(1)), current),
            vec![((Some(455832), None), 0), ((None, Some(455832)), 1)]
        );
        assert_eq!(
            epoch_bounds_with_irrelevance(epoch(now - chrono::TimeDelta::days(1)), current),
            vec![
                ((Some(455832), None), 0),
                ((Some(455829), Some(455832)), 1),
                ((Some(455817), Some(455829)), 2),
                ((None, Some(455817)), 3),
            ]
        );
    }

    #[test]
    fn epoch_round_trip() {
        assert_eq!(datetime_for_epoch(455832), Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap());
        assert_eq!(epoch(Utc.with_ymd_and_hms(2022, 1, 1, 0, 59, 59).unwrap()), 455832);
    }
}
