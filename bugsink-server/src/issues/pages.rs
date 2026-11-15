//! What upstream Bugsink 2.6.3's issue pages show, as data: the issue's header, tag summaries
//! (`Issue._get_issue_tags`), groupings (`issue_grouping`), history and comments (`issue_history`,
//! `history_comment_*`), navigation between events (`_get_event`, `_first_last`, `_event_count`) and event counts per
//! hour (`events/sparklines.py`).
//! Presentation (labels, percentages of bar heights, buckets aligned on local days) is left to the frontend.

use std::collections::HashMap;

use bugsink_db::{
  PgPool,
  TurningPointKind,
};
use chrono::{
  DateTime,
  DurationRound,
  TimeDelta,
  Utc,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{
  Digest,
  Sha256,
};
use sqlx::{
  QueryBuilder,
  Row,
};
use uuid::Uuid;

use super::NewTurningPoint;
use crate::accounts::display_name;
use crate::grouping::{
  BUGSINK_V1,
  BUGSINK_V2,
  LOG_MESSAGE_TYPE,
  MECHANISM_INDEPENDENT,
};
use crate::py::{
  splitlines,
  strip,
};
use crate::search::{
  Search,
  Searched,
};

/// `get_title_for_exception_type_and_value`.
pub fn title(ty: &str, value: &str) -> String {
  if value.is_empty() {
    return ty.to_owned();
  }
  let first_line = splitlines(value).first().copied().unwrap_or("");
  if ty == LOG_MESSAGE_TYPE {
    first_line.to_owned()
  } else {
    format!("{ty}: {first_line}")
  }
}

/// The issue page's header: what `issues/base.html` shows beyond the list's fields.
#[derive(Debug, Serialize)]
pub struct IssueDetail {
  /// `friendly_id`: the project's slug in capitals and the issue's number in the project.
  pub friendly_id: String,
  pub title: String,
  pub last_frame_filename: String,
  pub last_frame_module: String,
  pub last_frame_function: String,
  pub is_resolved_unconditionally: bool,
  pub is_resolved_by_next_release: bool,
  /// Releases it was marked as resolved in.
  pub fixed_at: Vec<String>,
  /// Releases it has events in ("" is the no-release release).
  pub events_at: Vec<String>,
  pub unmute_after: Option<DateTime<Utc>>,
  /// The "mute until" conditions: `[{"period", "nr_of_periods", "volume"}]`.
  pub unmute_on_volume_based_conditions: Value,
  pub project_has_releases: bool,
  /// The version "resolved in latest release" uses.
  pub latest_release: Option<String>,
}

pub async fn issue_detail(pool: &PgPool, issue_id: Uuid) -> sqlx::Result<IssueDetail> {
  let r = sqlx::query(
    "SELECT i.project_id, i.digest_order, i.calculated_type, i.calculated_value, i.last_frame_filename,
                i.last_frame_module, i.last_frame_function, i.is_resolved_unconditionally,
                i.is_resolved_by_next_release, i.fixed_at, i.events_at, i.unmute_after,
                i.unmute_on_volume_based_conditions, p.slug, p.has_releases
         FROM issues_issue i JOIN projects_project p ON p.id = i.project_id WHERE i.id = $1",
  )
  .bind(issue_id)
  .fetch_one(pool)
  .await?;
  let lines = |s: String| s.split_terminator('\n').map(str::to_owned).collect();
  let slug: String = r.get("slug");
  Ok(IssueDetail {
    friendly_id: super::friendly_id(&slug, r.get("digest_order")),
    title: title(r.get("calculated_type"), r.get("calculated_value")),
    last_frame_filename: r.get("last_frame_filename"),
    last_frame_module: r.get("last_frame_module"),
    last_frame_function: r.get("last_frame_function"),
    is_resolved_unconditionally: r.get("is_resolved_unconditionally"),
    is_resolved_by_next_release: r.get("is_resolved_by_next_release"),
    fixed_at: lines(r.get("fixed_at")),
    events_at: lines(r.get("events_at")),
    unmute_after: r.get("unmute_after"),
    unmute_on_volume_based_conditions: serde_json::from_str(r.get("unmute_on_volume_based_conditions"))
      .unwrap_or(Value::Array(vec![])),
    project_has_releases: r.get("has_releases"),
    latest_release: crate::releases::latest_release_version(pool, r.get("project_id")).await?,
  })
}

/// One tag key of an issue with its most frequent values.
#[derive(Debug, Serialize)]
pub struct KeyTags {
  pub key: String,
  pub values: Vec<TagCount>,
  /// The values that didn't make the cut, together.
  pub other: Option<TagCount>,
}

#[derive(Debug, Serialize)]
pub struct TagCount {
  /// None for `other`.
  pub value: Option<String>,
  pub count: i64,
  /// Of the key's events, rounded down.
  pub pct: i64,
}

/// Upstream's cut-offs: the header's summary and the tags tab.
pub const TAGS_SUMMARY_CUTOFF: usize = 4;
pub const TAGS_ALL_CUTOFF: usize = 25;

/// `_get_issue_tags`: per key (by name), its values by count, at most `cutoff` (the last place going to "other" when
/// there are more). Keys whose values are mostly unique are left out, unless the issue has at most `cutoff` events.
pub async fn tags(
  pool: &PgPool,
  issue_id: Uuid,
  digested_event_count: i32,
  cutoff: usize,
) -> sqlx::Result<Vec<KeyTags>> {
  let keys: Vec<(i64, String, i64)> = sqlx::query_as(
    "SELECT it.key_id, k.key, SUM(it.count)::bigint FROM tags_issuetag it JOIN tags_tagkey k ON k.id = it.key_id
         WHERE it.issue_id = $1 AND ($2 OR NOT k.mostly_unique)
         GROUP BY it.key_id, k.key ORDER BY k.key",
  )
  .bind(issue_id)
  .bind(digested_event_count as usize <= cutoff)
  .fetch_all(pool)
  .await?;
  let mut result = Vec::with_capacity(keys.len());
  for (key_id, key, total) in keys {
    // ties broken by id (upstream leaves them to the database)
    let mut values: Vec<(String, i32)> = sqlx::query_as(
      "SELECT v.value, it.count FROM tags_issuetag it JOIN tags_tagvalue v ON v.id = it.value_id
             WHERE it.issue_id = $1 AND it.key_id = $2 ORDER BY it.count DESC, it.id LIMIT $3",
    )
    .bind(issue_id)
    .bind(key_id)
    .bind(cutoff as i64 + 1)
    .fetch_all(pool)
    .await?;
    if values.len() > cutoff {
      values.truncate(cutoff - 1);
    }
    let pct = |count: i64| (count as f64 / total as f64 * 100.0) as i64;
    let seen: i64 = values.iter().map(|(_, c)| i64::from(*c)).sum();
    let values = values
      .into_iter()
      .map(|(value, count)| {
        TagCount {
          value: Some(value),
          count: count.into(),
          pct: pct(count.into()),
        }
      })
      .collect();
    let other = (seen < total).then(|| {
      TagCount {
        value: None,
        count: total - seen,
        pct: pct(total - seen),
      }
    });
    result.push(KeyTags { key, values, other });
  }
  Ok(result)
}

#[derive(Debug, Serialize)]
pub struct Grouping {
  pub grouping_mechanism: String,
  pub grouping_key: String,
}

/// `issue_grouping`'s order: mechanism-independent first, then newest mechanism first, then by key.
pub async fn groupings(pool: &PgPool, issue_id: Uuid) -> sqlx::Result<Vec<Grouping>> {
  let mut groupings: Vec<(String, String)> =
    sqlx::query_as("SELECT grouping_mechanism, grouping_key FROM issues_grouping WHERE issue_id = $1")
      .bind(issue_id)
      .fetch_all(pool)
      .await?;
  let by_age = [MECHANISM_INDEPENDENT, BUGSINK_V2, BUGSINK_V1];
  let order = |m: &str| by_age.iter().position(|a| *a == m).unwrap_or(999);
  groupings.sort_by(|(ma, ka), (mb, kb)| (order(ma), ka).cmp(&(order(mb), kb)));
  Ok(
    groupings
      .into_iter()
      .map(|(grouping_mechanism, grouping_key)| {
        Grouping {
          grouping_mechanism,
          grouping_key,
        }
      })
      .collect(),
  )
}

#[derive(Debug, Serialize)]
pub struct TurningPoint {
  pub id: i64,
  /// `TurningPointKind`: 1 first seen, 2 resolved, 3 muted, 4 regressed, 5 unmuted, 6 reopened, 10 release info
  /// added, 100 comment.
  pub kind: i32,
  pub timestamp: DateTime<Utc>,
  /// Parsed `metadata`, e.g. `{"mute_for": {...}}`, `{"resolved_release": "1.2"}`.
  pub metadata: Value,
  pub comment: String,
  /// None: Bugsink itself.
  pub user: Option<UserRef>,
  pub triggering_event: Option<EventRef>,
}

#[derive(Debug, Serialize)]
pub struct UserRef {
  pub id: i64,
  /// `best_displayname`: full name, else username.
  pub display_name: String,
  /// `gravatar_sha`: for an avatar without exposing the address.
  pub gravatar_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct EventRef {
  pub id: Uuid,
  pub event_id: Uuid,
  pub digest_order: i32,
}

/// `turningpoint_set_all()`: newest first.
pub async fn history(pool: &PgPool, issue_id: Uuid) -> sqlx::Result<Vec<TurningPoint>> {
  let rows = sqlx::query(
    "SELECT t.id, t.kind, t.timestamp, t.metadata, t.comment, t.user_id, u.first_name, u.last_name, u.username,
                u.email, e.id AS event_pk, e.event_id, e.digest_order
         FROM issues_turningpoint t
         LEFT JOIN auth_user u ON u.id = t.user_id
         LEFT JOIN events_event e ON e.id = t.triggering_event_id
         WHERE t.issue_id = $1 ORDER BY t.timestamp DESC, t.id DESC",
  )
  .bind(issue_id)
  .fetch_all(pool)
  .await?;
  Ok(
    rows
      .iter()
      .map(|r| {
        TurningPoint {
          id: r.get("id"),
          kind: r.get("kind"),
          timestamp: r.get("timestamp"),
          metadata: serde_json::from_str(r.get("metadata")).unwrap_or(Value::Null),
          comment: r.get("comment"),
          user: r.get::<Option<i64>, _>("user_id").map(|id| {
            let email: String = r.get("email");
            UserRef {
              id,
              display_name: display_name(r.get("first_name"), r.get("last_name"), r.get("username")),
              gravatar_sha256: format!("{:x}", Sha256::digest(strip(&email.to_lowercase()).as_bytes())),
            }
          }),
          triggering_event: r.get::<Option<Uuid>, _>("event_pk").map(|id| {
            EventRef {
              id,
              event_id: r.get("event_id"),
              digest_order: r.get("digest_order"),
            }
          }),
        }
      })
      .collect(),
  )
}

/// `CommentForm`: Django's form field strips surrounding whitespace.
pub fn clean_comment(comment: &str) -> &str {
  strip(comment)
}

/// `history_comment_new`: an empty comment is ignored (None).
pub async fn add_comment(
  pool: &PgPool,
  project_id: i64,
  issue_id: Uuid,
  user_id: i64,
  comment: &str,
  now: DateTime<Utc>,
) -> sqlx::Result<Option<i64>> {
  let comment = clean_comment(comment);
  if comment.is_empty() {
    return Ok(None);
  }
  Ok(Some(
    insert_comment(pool, project_id, issue_id, Some(user_id), comment, now).await?,
  ))
}

/// A `MANUAL_ANNOTATION` turning point; its id.
pub(crate) async fn insert_comment(
  pool: &PgPool,
  project_id: i64,
  issue_id: Uuid,
  user_id: Option<i64>,
  comment: &str,
  now: DateTime<Utc>,
) -> sqlx::Result<i64> {
  let turning_point = NewTurningPoint {
    project_id,
    issue_id,
    kind: TurningPointKind::ManualAnnotation,
    timestamp: now,
    triggering_event_id: None,
    user_id,
    metadata: "{}",
    comment,
  };
  turning_point.insert(pool).await
}

/// Navigation among an issue's events, within a search (`?q=`): an issue page's "first / previous / next / last".
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Nav {
  pub first: Option<i32>,
  pub prev: Option<i32>,
  pub next: Option<i32>,
  pub last: Option<i32>,
  /// `_event_count`: the matching events with a search, else the issue's stored events.
  pub count: i64,
}

/// The digest orders of the issue's events that match `search`, as a query to add conditions to.
fn matching(issue_id: Uuid, search: &Search, select: &str) -> QueryBuilder<sqlx::Postgres> {
  let mut qb = QueryBuilder::new(format!("SELECT {select} FROM events_event WHERE issue_id = "));
  qb.push_bind(issue_id);
  search.push_conditions(&mut qb, Searched::EventsOf(issue_id));
  qb
}

/// `_first_last`.
pub async fn bounds(pool: &PgPool, issue_id: Uuid, search: &Search) -> sqlx::Result<(Option<i32>, Option<i32>)> {
  let (first, last) = matching(issue_id, search, "MIN(digest_order), MAX(digest_order)")
    .build_query_as::<(Option<i32>, Option<i32>)>()
    .fetch_one(pool)
    .await?;
  Ok((first, last))
}

/// `_get_event`'s prev/next: the nearest matching event before or after `digest_order`.
pub async fn neighbour(
  pool: &PgPool,
  issue_id: Uuid,
  search: &Search,
  digest_order: i32,
  next: bool,
) -> sqlx::Result<Option<i32>> {
  let mut qb = matching(
    issue_id,
    search,
    if next { "MIN(digest_order)" } else { "MAX(digest_order)" },
  );
  qb.push(if next {
    " AND digest_order > "
  } else {
    " AND digest_order < "
  });
  qb.push_bind(digest_order);
  qb.build_query_scalar().fetch_one(pool).await
}

/// Navigation around `digest_order` (which needn't match the search itself).
pub async fn nav(
  pool: &PgPool,
  issue_id: Uuid,
  stored_event_count: i32,
  search: &Search,
  searching: bool,
  digest_order: i32,
) -> sqlx::Result<Nav> {
  let (first, last) = bounds(pool, issue_id, search).await?;
  let count = if searching {
    matching(issue_id, search, "COUNT(*)")
      .build_query_scalar()
      .fetch_one(pool)
      .await?
  } else {
    i64::from(stored_event_count)
  };
  Ok(Nav {
    first,
    prev: neighbour(pool, issue_id, search, digest_order, false).await?,
    next: neighbour(pool, issue_id, search, digest_order, true).await?,
    last,
    count,
  })
}

/// An hour of an issue's events: how many, and the last one's `digest_order` (to open it).
#[derive(Debug, PartialEq, Serialize, sqlx::FromRow)]
pub struct HourCount {
  pub bucket: DateTime<Utc>,
  pub count: i64,
  pub digest_order: Option<i32>,
}

/// `hour_bucket`
pub fn hour_bucket(t: DateTime<Utc>) -> DateTime<Utc> {
  t.duration_trunc(TimeDelta::hours(1)).expect("in range")
}

/// The issue page's sparkline data (`get_issue_event_sparkline`'s queries): the issue's hourly counts in
/// `[since, until)` (incl. evicted events), and with a search the matching stored events per hour.
pub async fn hourly_counts(
  pool: &PgPool,
  issue_id: Uuid,
  since: DateTime<Utc>,
  until: DateTime<Utc>,
  search: Option<&Search>,
) -> sqlx::Result<(Vec<HourCount>, Option<Vec<HourCount>>)> {
  let all = sqlx::query_as(
    "SELECT bucket, count::bigint, digest_order FROM events_issueeventcountsperhour
         WHERE issue_id = $1 AND bucket >= $2 AND bucket < $3 ORDER BY bucket",
  )
  .bind(issue_id)
  .bind(since)
  .bind(until)
  .fetch_all(pool)
  .await?;
  let matching_counts = match search {
    None => None,
    Some(search) => {
      let select = "date_trunc('hour', digested_at, 'UTC') AS bucket, COUNT(*) AS count,
                          MAX(digest_order) AS digest_order";
      let mut qb = matching(issue_id, search, select);
      qb.push(" AND digested_at >= ");
      qb.push_bind(since);
      qb.push(" AND digested_at < ");
      qb.push_bind(until);
      qb.push(" GROUP BY 1 ORDER BY 1");
      Some(qb.build_query_as().fetch_all(pool).await?)
    }
  };
  Ok((all, matching_counts))
}

/// The issue and project lists' sparklines (`_get_list_sparklines`): counts for the 24 hours up to and including the
/// current one, oldest first. `table`/`column`: `events_issueeventcountsperhour`/`issue_id` or the project ones.
async fn list_sparklines<K>(
  pool: &PgPool,
  table: &str,
  column: &str,
  ids: &[K],
  now: DateTime<Utc>,
) -> sqlx::Result<HashMap<K, Vec<i32>>>
where
  K: Clone + Eq + std::hash::Hash + Send + Unpin,
  K: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
  Vec<K>: for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
  let current = hour_bucket(now);
  let start = current - TimeDelta::hours(23);
  let mut result: HashMap<K, Vec<i32>> = ids.iter().map(|id| (id.clone(), vec![0; 24])).collect();
  let rows: Vec<(K, DateTime<Utc>, i32)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
    "SELECT {column}, bucket, count FROM {table} WHERE {column} = ANY($1) AND bucket >= $2 AND bucket < $3"
  )))
  .bind(ids.to_vec())
  .bind(start)
  .bind(current + TimeDelta::hours(1))
  .fetch_all(pool)
  .await?;
  for (id, bucket, count) in rows {
    if let Some(counts) = result.get_mut(&id) {
      counts[(bucket - start).num_hours() as usize] = count;
    }
  }
  Ok(result)
}

pub async fn issue_list_sparklines(
  pool: &PgPool,
  issue_ids: &[Uuid],
  now: DateTime<Utc>,
) -> sqlx::Result<HashMap<Uuid, Vec<i32>>> {
  list_sparklines(pool, "events_issueeventcountsperhour", "issue_id", issue_ids, now).await
}

pub async fn project_list_sparklines(
  pool: &PgPool,
  project_ids: &[i64],
  now: DateTime<Utc>,
) -> sqlx::Result<HashMap<i64, Vec<i32>>> {
  list_sparklines(pool, "events_projecteventcountsperhour", "project_id", project_ids, now).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn titles_like_upstream() {
    assert_eq!(title("ValueError", ""), "ValueError");
    assert_eq!(title("ValueError", "bad\nsecond"), "ValueError: bad");
    assert_eq!(title(LOG_MESSAGE_TYPE, "disk full\r\nmore"), "disk full");
    assert_eq!(title("KeyError", "\n"), "KeyError: ");
  }
}
