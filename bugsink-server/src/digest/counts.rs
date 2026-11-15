//! `events/usage.py::record_event_counts`: hourly event counts per installation, project and issue.

use bugsink_db::{
  Postgres,
  Transaction,
};
use chrono::{
  DateTime,
  DurationRound,
  TimeDelta,
  Utc,
};
use uuid::Uuid;

const MAX_AGE: TimeDelta = TimeDelta::days(90);

pub async fn record_event_counts(
  tx: &mut Transaction<'_, Postgres>,
  project_id: i64,
  issue_id: Uuid,
  digested_at: DateTime<Utc>,
  digest_order: i32,
) -> anyhow::Result<()> {
  let bucket = digested_at.duration_trunc(TimeDelta::hours(1))?;

  sqlx::query(
    "INSERT INTO events_projecteventcountsperhour (project_id, bucket, count) VALUES ($1, $2, 1)
         ON CONFLICT (project_id, bucket) DO UPDATE SET count = events_projecteventcountsperhour.count + 1",
  )
  .bind(project_id)
  .bind(bucket)
  .execute(&mut **tx)
  .await?;

  sqlx::query(
    "INSERT INTO events_issueeventcountsperhour (project_id, issue_id, bucket, count, digest_order)
         VALUES ($1, $2, $3, 1, $4)
         ON CONFLICT (issue_id, bucket) DO UPDATE
         SET count = events_issueeventcountsperhour.count + 1, digest_order = EXCLUDED.digest_order",
  )
  .bind(project_id)
  .bind(issue_id)
  .bind(bucket)
  .bind(digest_order)
  .execute(&mut **tx)
  .await?;

  // The installation's row last: every project's digestion updates it, so it stays locked until the commit.
  let created: bool = sqlx::query_scalar(
    "INSERT INTO events_installationeventcountsperhour (bucket, count) VALUES ($1, 1)
         ON CONFLICT (bucket) DO UPDATE SET count = events_installationeventcountsperhour.count + 1
         RETURNING xmax = 0",
  )
  .bind(bucket)
  .fetch_one(&mut **tx)
  .await?;
  if created {
    // Once per hour: drop buckets older than 90 days.
    let cutoff = bucket - MAX_AGE;
    for table in [
      "events_installationeventcountsperhour",
      "events_projecteventcountsperhour",
      "events_issueeventcountsperhour",
    ] {
      sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table} WHERE bucket < $1")))
        .bind(cutoff)
        .execute(&mut **tx)
        .await?;
    }
  }
  Ok(())
}
