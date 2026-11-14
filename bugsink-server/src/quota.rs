//! Ingestion quotas, per project and installation-wide. A port of upstream Bugsink 2.6.3's `ingest/digestion.py`
//! (`count_installation_periods_and_act_on_it`, `count_project_periods_and_act_on_it`, `is_quota_still_exceeded`) and
//! `ingest/event_counter.py`.
//!
//! Digest counts events over rolling windows (5 minutes, an hour, a month). When a window reaches its threshold,
//! `quota_exceeded_until` is set on the project or installation row; until then ingest answers 429 and digest drops
//! events. The count itself is amortized: it only runs again once `next_quota_check` digested events are reached.
//!
//! Counting is `crate::event_counter`'s.

use bugsink_db::{PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::event_counter::{Counted, Period, Threshold, ThresholdState, check_for_thresholds};
use crate::grouping::py::json_dumps;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limit(pub Option<i64>);

/// Upstream's `int_or_none`: anything that isn't an integer means "no limit".
pub(crate) fn int_or_none(s: &str) -> Result<Limit, String> {
    Ok(Limit(s.trim().parse().ok()))
}

/// Upstream's settings of the same names (`MAX_EVENTS_PER_*`, and the retention caps), with upstream's defaults.
#[derive(clap::Args, Clone, Copy, Debug)]
pub struct QuotaSettings {
    #[arg(long, env = "MAX_EVENTS_PER_PROJECT_PER_5_MINUTES", default_value_t = 1_000)]
    pub max_events_per_project_per_5_minutes: i64,
    #[arg(long, env = "MAX_EVENTS_PER_PROJECT_PER_HOUR", default_value_t = 5_000)]
    pub max_events_per_project_per_hour: i64,
    #[arg(long, env = "MAX_EVENTS_PER_PROJECT_PER_MONTH", default_value_t = 1_000_000)]
    pub max_events_per_project_per_month: i64,
    #[arg(long, env = "MAX_EVENTS_PER_5_MINUTES", default_value_t = 1_000)]
    pub max_events_per_5_minutes: i64,
    #[arg(long, env = "MAX_EVENTS_PER_HOUR", default_value_t = 5_000)]
    pub max_events_per_hour: i64,
    #[arg(long, env = "MAX_EVENTS_PER_MONTH", default_value_t = 1_000_000)]
    pub max_events_per_month: i64,
    /// Caps every project's retention (`get_retention_max_event_count`), also below what it is set to.
    #[arg(long, env = "MAX_RETENTION_PER_PROJECT_EVENT_COUNT", default_value = "None", value_parser = int_or_none)]
    pub max_retention_per_project_event_count: Limit,
    /// The most all projects' retention settings may add up to (checked when a project's retention is set).
    #[arg(long, env = "MAX_RETENTION_EVENT_COUNT", default_value = "None", value_parser = int_or_none)]
    pub max_retention_event_count: Limit,
}

impl Default for QuotaSettings {
    fn default() -> Self {
        Self {
            max_events_per_project_per_5_minutes: 1_000,
            max_events_per_project_per_hour: 5_000,
            max_events_per_project_per_month: 1_000_000,
            max_events_per_5_minutes: 1_000,
            max_events_per_hour: 5_000,
            max_events_per_month: 1_000_000,
            max_retention_per_project_event_count: Limit(None),
            max_retention_event_count: Limit(None),
        }
    }
}

/// Whose events are counted (`QUOTA_THRESHOLDS`' keys).
#[derive(Clone, Copy, Debug)]
pub enum Scope {
    Installation,
    Project(i64),
}

impl QuotaSettings {
    /// `Project.get_retention_max_event_count`.
    pub fn retention_max_event_count(&self, configured: i32) -> i32 {
        match self.max_retention_per_project_event_count.0 {
            Some(cap) => i64::from(configured).min(cap) as i32,
            None => configured,
        }
    }

    fn thresholds(&self, scope: Scope) -> [Threshold; 3] {
        let (per_5_minutes, per_hour, per_month) = match scope {
            Scope::Installation => (self.max_events_per_5_minutes, self.max_events_per_hour, self.max_events_per_month),
            Scope::Project(_) => (
                self.max_events_per_project_per_5_minutes,
                self.max_events_per_project_per_hour,
                self.max_events_per_project_per_month,
            ),
        };
        [
            Threshold { period: Period::Minute, nr_of_periods: 5, gte: per_5_minutes },
            Threshold { period: Period::Hour, nr_of_periods: 1, gte: per_hour },
            Threshold { period: Period::Month, nr_of_periods: 1, gte: per_month },
        ]
    }
}

/// The quota columns shared by `projects_project` and `phonehome_installation`.
#[derive(Debug)]
pub(crate) struct QuotaState {
    pub next_quota_check: i32,
    pub exceeded_until: Option<DateTime<Utc>>,
    /// JSON: `null` or the tripped threshold as `["hour", 1, 5000]`.
    pub exceeded_reason: String,
}

/// `is_quota_still_exceeded`: whether to refuse events now. A quota tripped under a threshold that has since been
/// raised no longer counts.
pub fn is_still_exceeded(
    settings: &QuotaSettings,
    scope: Scope,
    exceeded_until: Option<DateTime<Utc>>,
    exceeded_reason: &str,
    now: DateTime<Utc>,
) -> bool {
    if exceeded_until.is_none_or(|until| now >= until) {
        return false;
    }
    let Ok((period, nr_of_periods, gte)) = serde_json::from_str::<(String, u32, i64)>(exceeded_reason) else {
        return true; // upstream would fail here; refusing until `exceeded_until` is the safe reading
    };
    let current = settings
        .thresholds(scope)
        .into_iter()
        .find(|t| t.period.name() == period && t.nr_of_periods == nr_of_periods);
    !current.is_some_and(|t| t.gte > gte)
}

/// For ingest: the installation's quota, read without locking (upstream: `Installation.objects.get()`).
pub async fn installation_exceeded(pool: &PgPool, settings: &QuotaSettings, now: DateTime<Utc>) -> sqlx::Result<bool> {
    let row: Option<(Option<DateTime<Utc>>, String)> =
        sqlx::query_as("SELECT quota_exceeded_until, quota_exceeded_reason FROM phonehome_installation LIMIT 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some_and(|(until, reason)| is_still_exceeded(settings, Scope::Installation, until, &reason, now)))
}

/// `count_installation_periods_and_act_on_it`: false if the event must be dropped. Called before the project's own
/// count, with the project row locked but not yet updated.
pub(crate) async fn count_installation_periods_and_act_on_it(
    tx: &mut Transaction<'_, Postgres>,
    settings: &QuotaSettings,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let Some(row) = sqlx::query(
        "SELECT id, next_quota_check, quota_exceeded_until, quota_exceeded_reason FROM phonehome_installation LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(true);
    };
    let id: Uuid = row.get("id");
    let next_quota_check = i64::from(row.get::<i32, _>("next_quota_check"));
    let scope = Scope::Installation;
    if is_still_exceeded(settings, scope, row.get("quota_exceeded_until"), row.get("quota_exceeded_reason"), now) {
        return Ok(false);
    }

    // No per-event bookkeeping on the installation row (that would serialize all digestion): the count is the sum over
    // projects, +1 for the event about to be added.
    let digested_event_count: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(digested_event_count), 0)::bigint FROM projects_project")
            .fetch_one(&mut **tx)
            .await?;
    let digested_event_count = digested_event_count + 1;

    let thresholds = settings.thresholds(scope);
    if needs_check(&thresholds, digested_event_count, next_quota_check) {
        let states = check_for_thresholds(tx, Counted::Installation, now, &thresholds, 1).await?;
        let (until, reason) = exceeded_until_and_reason(&states);
        let check_again_after = states.iter().map(|s| s.check_again_after).min().unwrap_or(1).max(1);
        sqlx::query(
            "UPDATE phonehome_installation
             SET quota_exceeded_until = $2, quota_exceeded_reason = $3, next_quota_check = $4 WHERE id = $1",
        )
        .bind(id)
        .bind(until)
        .bind(reason)
        .bind(to_i32(digested_event_count + check_again_after))
        .execute(&mut **tx)
        .await?;
    }
    Ok(true)
}

/// `count_project_periods_and_act_on_it`: false if the event must be dropped. Otherwise increments
/// `digested_event_count` and, when due, recounts; the caller saves both.
pub(crate) async fn count_project_periods_and_act_on_it(
    tx: &mut Transaction<'_, Postgres>,
    settings: &QuotaSettings,
    project_id: i64,
    digested_event_count: &mut i32,
    quota: &mut QuotaState,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let scope = Scope::Project(project_id);
    // Also checked at ingest, but events already in the queue when the quota tripped still arrive here.
    if is_still_exceeded(settings, scope, quota.exceeded_until, &quota.exceeded_reason, now) {
        return Ok(false);
    }

    *digested_event_count += 1;

    let thresholds = settings.thresholds(scope);
    if needs_check(&thresholds, i64::from(*digested_event_count), i64::from(quota.next_quota_check)) {
        // Thresholds are "gte", so the event that reaches a quota is still accepted; the door closes after it. The
        // event isn't stored yet: +1.
        let states = check_for_thresholds(tx, Counted::Project(project_id), now, &thresholds, 1).await?;
        let (until, reason) = exceeded_until_and_reason(&states);
        // Unlike the installation's, not floored at 1 (as upstream).
        let check_again_after = states.iter().map(|s| s.check_again_after).min().unwrap_or(1);
        quota.exceeded_until = until;
        quota.exceeded_reason = reason;
        quota.next_quota_check = to_i32(i64::from(*digested_event_count) + check_again_after);
    }
    Ok(true)
}

/// Recount when due, or when `next_quota_check` is further away than the smallest threshold (a lowered setting).
fn needs_check(thresholds: &[Threshold], digested_event_count: i64, next_quota_check: i64) -> bool {
    let min_threshold = thresholds.iter().map(|t| t.gte).min().expect("thresholds");
    digested_event_count >= next_quota_check || next_quota_check - digested_event_count > min_threshold
}

fn to_i32(n: i64) -> i32 {
    i32::try_from(n).unwrap_or(if n < 0 { 0 } else { i32::MAX })
}

/// `max((below_from, threshold) for exceeded ones)`, with the threshold as upstream's JSON reason.
fn exceeded_until_and_reason(states: &[ThresholdState]) -> (Option<DateTime<Utc>>, String) {
    let key = |s: &&ThresholdState| (s.below_threshold_from, s.threshold.period.name(), s.threshold.nr_of_periods, s.threshold.gte);
    match states.iter().filter(|s| s.exceeded).max_by(|a, b| key(a).cmp(&key(b))) {
        None => (None, "null".into()),
        Some(s) => {
            let t = s.threshold;
            (s.below_threshold_from, json_dumps(&json!([t.period.name(), t.nr_of_periods, t.gte])))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeDelta, TimeZone};

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn still_exceeded() {
        let settings = QuotaSettings::default();
        let now = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
        let later = Some(now + TimeDelta::minutes(1));
        let scope = Scope::Project(1);
        assert!(!is_still_exceeded(&settings, scope, None, "null", now));
        assert!(!is_still_exceeded(&settings, scope, Some(now), r#"["hour", 1, 5000]"#, now));
        assert!(is_still_exceeded(&settings, scope, later, r#"["hour", 1, 5000]"#, now));
        assert!(is_still_exceeded(&settings, scope, later, r#"["hour", 1, 6000]"#, now));
        // the setting was raised since the quota tripped
        assert!(!is_still_exceeded(&settings, scope, later, r#"["hour", 1, 10]"#, now));
    }

    #[test]
    fn reason_is_python_json() {
        let t = Threshold { period: Period::Minute, nr_of_periods: 5, gte: 1000 };
        let state = |exceeded, from| ThresholdState { exceeded, below_threshold_from: from, check_again_after: 0, threshold: t };
        assert_eq!(exceeded_until_and_reason(&[state(false, None)]), (None, "null".to_string()));
        let from = Some(at("2026-11-01T12:00:00Z"));
        assert_eq!(exceeded_until_and_reason(&[state(true, from)]), (from, r#"["minute", 5, 1000]"#.to_string()));
    }
}
