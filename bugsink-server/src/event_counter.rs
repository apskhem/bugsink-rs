//! Counting events over rolling windows, for quotas (`crate::quota`) and volume-based unmuting (digest). A port of
//! upstream Bugsink 2.6.3's `ingest/event_counter.py` (and `check_for_thresholds_on_installation`).
//!
//! The count in a window is `last - first + 1` of `project_digest_order`, so evicted events still count. Like
//! upstream, this is also how an issue's events are counted, which makes it the number of the *project's* events
//! between the issue's first and last one in the window.

use bugsink_db::{Postgres, Transaction};
use chrono::{DateTime, Months, TimeDelta, TimeZone, Utc};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Period {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    /// No window: all events.
    Total,
}

impl Period {
    pub fn name(self) -> &'static str {
        match self {
            Self::Year => "year",
            Self::Month => "month",
            Self::Week => "week",
            Self::Day => "day",
            Self::Hour => "hour",
            Self::Minute => "minute",
            Self::Total => "total",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Year, Self::Month, Self::Week, Self::Day, Self::Hour, Self::Minute, Self::Total]
            .into_iter()
            .find(|p| p.name() == name)
    }

    /// `sub_periods_from_datetime` (dateutil's relativedelta: months and years clamp to the end of the month). None
    /// for `Total`.
    fn sub(self, dt: DateTime<Utc>, n: u32) -> Option<DateTime<Utc>> {
        let n64 = i64::from(n);
        Some(match self {
            Self::Year => dt.checked_sub_months(Months::new(12 * n))?,
            Self::Month => dt.checked_sub_months(Months::new(n))?,
            Self::Week => dt - TimeDelta::weeks(n64),
            Self::Day => dt - TimeDelta::days(n64),
            Self::Hour => dt - TimeDelta::hours(n64),
            Self::Minute => dt - TimeDelta::minutes(n64),
            Self::Total => return None,
        })
    }

    /// `add_periods_to_datetime`; for `Total`, upstream's "never" (9999-12-31 23:59).
    pub fn add(self, dt: DateTime<Utc>, n: u32) -> DateTime<Utc> {
        let n64 = i64::from(n);
        match self {
            Self::Year => dt.checked_add_months(Months::new(12 * n)).expect("date in range"),
            Self::Month => dt.checked_add_months(Months::new(n)).expect("date in range"),
            Self::Week => dt + TimeDelta::weeks(n64),
            Self::Day => dt + TimeDelta::days(n64),
            Self::Hour => dt + TimeDelta::hours(n64),
            Self::Minute => dt + TimeDelta::minutes(n64),
            Self::Total => Utc.with_ymd_and_hms(9999, 12, 31, 23, 59, 0).unwrap(),
        }
    }
}

/// `(period_name, nr_of_periods, gte_threshold)`: exceeded once the window holds at least `gte` events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Threshold {
    pub period: Period,
    pub nr_of_periods: u32,
    pub gte: i64,
}

/// Whose events are counted.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Counted {
    Installation,
    Project(i64),
    Issue(Uuid),
}

pub(crate) struct ThresholdState {
    pub exceeded: bool,
    /// When the count drops below the threshold again (if exceeded).
    pub below_threshold_from: Option<DateTime<Utc>>,
    /// How many more events before the threshold can possibly be reached.
    pub check_again_after: i64,
    pub threshold: Threshold,
}

/// `check_for_thresholds`. `add_for_current`: 1 when the event being digested is not stored yet.
pub(crate) async fn check_for_thresholds(
    tx: &mut Transaction<'_, Postgres>,
    counted: Counted,
    now: DateTime<Utc>,
    thresholds: &[Threshold],
    add_for_current: i64,
) -> sqlx::Result<Vec<ThresholdState>> {
    let mut states = Vec::with_capacity(thresholds.len());
    for &threshold in thresholds {
        let since = threshold.period.sub(now, threshold.nr_of_periods);
        let total = add_for_current + events_in_period(tx, counted, since).await?;
        let exceeded = total >= threshold.gte;
        let below_threshold_from = if exceeded {
            // The window's oldest event plus the window: the moment the count drops below the threshold again.
            let start = oldest_in_period(tx, counted, since).await?.unwrap_or(now);
            Some(threshold.period.add(start, threshold.nr_of_periods))
        } else {
            None
        };
        states.push(ThresholdState { exceeded, below_threshold_from, check_again_after: threshold.gte - total, threshold });
    }
    Ok(states)
}

/// For a window's start of `None` (all events): earlier than any event.
fn window_start(since: Option<DateTime<Utc>>) -> DateTime<Utc> {
    since.unwrap_or_else(|| Utc.with_ymd_and_hms(1, 1, 1, 0, 0, 0).unwrap())
}

/// The condition on the counted project or issue (`$2`).
fn owner_filter(counted: Counted) -> &'static str {
    match counted {
        Counted::Installation => "",
        Counted::Project(_) => " AND project_id = $2",
        Counted::Issue(_) => " AND issue_id = $2",
    }
}

fn bind_owner<'q, O>(
    q: sqlx::query::QueryScalar<'q, Postgres, O, sqlx::postgres::PgArguments>,
    counted: Counted,
) -> sqlx::query::QueryScalar<'q, Postgres, O, sqlx::postgres::PgArguments> {
    match counted {
        Counted::Installation => q,
        Counted::Project(id) => q.bind(id),
        Counted::Issue(id) => q.bind(id),
    }
}

/// `get_total_events_in_period`, summed per project for the installation (`project_digest_order` is per project).
async fn events_in_period(
    tx: &mut Transaction<'_, Postgres>,
    counted: Counted,
    since: Option<DateTime<Utc>>,
) -> sqlx::Result<i64> {
    let sql = match counted {
        Counted::Issue(_) => "SELECT COALESCE(
                 (SELECT project_digest_order FROM events_event
                  WHERE issue_id = $2 AND digested_at >= $1 AND project_digest_order IS NOT NULL
                  ORDER BY digested_at DESC, project_digest_order DESC LIMIT 1)
                 - (SELECT project_digest_order FROM events_event
                    WHERE issue_id = $2 AND digested_at >= $1 AND project_digest_order IS NOT NULL
                    ORDER BY digested_at, project_digest_order LIMIT 1) + 1, 0)::bigint"
            .to_string(),
        _ => format!(
            "SELECT COALESCE(SUM(l.o - f.o + 1), 0)::bigint FROM projects_project p
             CROSS JOIN LATERAL (
                 SELECT project_digest_order AS o FROM events_event
                 WHERE project_id = p.id AND digested_at >= $1 AND project_digest_order IS NOT NULL
                 ORDER BY digested_at, project_digest_order LIMIT 1) f
             CROSS JOIN LATERAL (
                 SELECT project_digest_order AS o FROM events_event
                 WHERE project_id = p.id AND digested_at >= $1 AND project_digest_order IS NOT NULL
                 ORDER BY digested_at DESC, project_digest_order DESC LIMIT 1) l
             {}",
            if let Counted::Project(_) = counted { "WHERE p.id = $2" } else { "" }
        ),
    };
    let q = sqlx::query_scalar(sqlx::AssertSqlSafe(sql)).bind(window_start(since));
    bind_owner(q, counted).fetch_one(&mut **tx).await
}

async fn oldest_in_period(
    tx: &mut Transaction<'_, Postgres>,
    counted: Counted,
    since: Option<DateTime<Utc>>,
) -> sqlx::Result<Option<DateTime<Utc>>> {
    let sql = format!("SELECT MIN(digested_at) FROM events_event WHERE digested_at >= $1{}", owner_filter(counted));
    let q = sqlx::query_scalar(sqlx::AssertSqlSafe(sql)).bind(window_start(since));
    bind_owner(q, counted).fetch_one(&mut **tx).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn periods_like_relativedelta() {
        assert_eq!(Period::Month.sub(at("2026-03-31T10:00:00Z"), 1), Some(at("2026-02-28T10:00:00Z")));
        assert_eq!(Period::Month.add(at("2026-01-31T10:00:00Z"), 1), at("2026-02-28T10:00:00Z"));
        assert_eq!(Period::Year.sub(at("2028-02-29T10:00:00Z"), 1), Some(at("2027-02-28T10:00:00Z")));
        assert_eq!(Period::Minute.sub(at("2026-03-31T10:00:00Z"), 5), Some(at("2026-03-31T09:55:00Z")));
        assert_eq!(Period::Total.sub(at("2026-03-31T10:00:00Z"), 1), None);
        assert_eq!(Period::from_name("week"), Some(Period::Week));
        assert_eq!(Period::from_name("fortnight"), None);
    }
}
