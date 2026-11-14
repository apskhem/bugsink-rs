//! Cleanup of stale rows, as upstream Bugsink 2.6.3's `vacuum` management command (`bsmain`) with its `sync` tasks
//! from `tags/tasks.py`. Like upstream, nothing runs it automatically: schedule `bugsink vacuum` (e.g. daily).
//!
//! - eventless issue tags: issue tags whose value no event of the issue has anymore (events are evicted with their
//!   event tags, but issue tags are kept until this runs), then their tag values if no other issue uses them
//! - tags: tag values no event or issue tag uses, then tag keys without values
//! - deleted issues: finishing the deletion of issues marked as deleted (normally done by background jobs)
//!
//! TODO(parity): the command's files, old-events (`MAX_EVENT_AGE_DAYS`), ingest-dir and orphaned-issues parts.

use bugsink_db::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// `VACUUM_TAGS_BATCH_SIZE`
const TAGS_BATCH_SIZE: i64 = 10_000;
/// `VACUUM_EVENTLESS_ISSUETAGS_BATCH_SIZE`
const EVENTLESS_ISSUETAGS_BATCH_SIZE: i64 = 2048;

#[derive(Clone, Copy, Debug)]
pub struct What {
    pub eventless_issuetags: bool,
    pub tags: bool,
    pub deleted_issues: bool,
}

/// In upstream's order: deleted issues, eventless issue tags, then tags (which picks up what the others orphaned).
pub async fn run(pool: &PgPool, what: What) -> anyhow::Result<()> {
    if what.deleted_issues {
        tracing::info!("vacuuming deleted issues");
        crate::issues::deletion::delete_marked_issues(pool).await?;
    }
    if what.eventless_issuetags {
        tracing::info!("vacuuming eventless issue tags");
        let mut min_id = 0;
        while let Some(next) = eventless_issuetags_batch(pool, min_id).await? {
            min_id = next;
        }
    }
    if what.tags {
        tracing::info!("vacuuming tags");
        let mut min_id = 0;
        while let Some(next) = tagvalues_batch(pool, min_id).await? {
            min_id = next;
        }
        let mut min_id = 0;
        while let Some(next) = tagkeys_batch(pool, min_id).await? {
            min_id = next;
        }
    }
    Ok(())
}

/// `vacuum_tagvalues_batch`: the next batch of tag values by id; deletes the unused ones and returns the last id
/// checked (None when there were none left).
async fn tagvalues_batch(pool: &PgPool, min_id: i64) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let last: Option<i64> = sqlx::query_scalar(
        "WITH batch AS (SELECT id FROM tags_tagvalue WHERE id > $1 ORDER BY id LIMIT $2),
              unused AS (DELETE FROM tags_tagvalue v USING batch b
                         WHERE v.id = b.id
                           AND NOT EXISTS (SELECT 1 FROM tags_eventtag WHERE value_id = b.id)
                           AND NOT EXISTS (SELECT 1 FROM tags_issuetag WHERE value_id = b.id))
         SELECT MAX(id) FROM batch",
    )
    .bind(min_id)
    .bind(TAGS_BATCH_SIZE)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(last)
}

/// `vacuum_tagkeys_batch`: as `tagvalues_batch`, for tag keys without values.
async fn tagkeys_batch(pool: &PgPool, min_id: i64) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let last: Option<i64> = sqlx::query_scalar(
        "WITH batch AS (SELECT id FROM tags_tagkey WHERE id > $1 ORDER BY id LIMIT $2),
              unused AS (DELETE FROM tags_tagkey k USING batch b
                         WHERE k.id = b.id AND NOT EXISTS (SELECT 1 FROM tags_tagvalue WHERE key_id = b.id))
         SELECT MAX(id) FROM batch",
    )
    .bind(min_id)
    .bind(TAGS_BATCH_SIZE)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(last)
}

/// `vacuum_eventless_issuetags_batch`: deletes the batch's issue tags that no event tag of the same issue and value
/// backs anymore, then prunes their values (`prune_tagvalues`: those no issue tag uses).
async fn eventless_issuetags_batch(pool: &PgPool, min_id: i64) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let batch: Vec<(i64, Uuid, i64)> =
        sqlx::query_as("SELECT id, issue_id, value_id FROM tags_issuetag WHERE id > $1 ORDER BY id LIMIT $2")
            .bind(min_id)
            .bind(EVENTLESS_ISSUETAGS_BATCH_SIZE)
            .fetch_all(&mut *tx)
            .await?;
    let Some(&(last, _, _)) = batch.last() else { return Ok(None) };
    let ids: Vec<i64> = batch.iter().map(|(id, _, _)| *id).collect();
    let stale_values: Vec<i64> = sqlx::query_scalar(
        "DELETE FROM tags_issuetag it WHERE it.id = ANY($1)
           AND NOT EXISTS (SELECT 1 FROM tags_eventtag e WHERE e.issue_id = it.issue_id AND e.value_id = it.value_id)
         RETURNING it.value_id",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    prune_tagvalues(&mut tx, &stale_values).await?;
    tx.commit().await?;
    Ok(Some(last))
}

/// `prune_tagvalues`: deletes those of the values no issue tag uses anymore (an event tag implies an issue tag; tag
/// keys are left to `tagkeys_batch`).
pub(crate) async fn prune_tagvalues(tx: &mut Transaction<'_, Postgres>, value_ids: &[i64]) -> sqlx::Result<()> {
    if value_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "DELETE FROM tags_tagvalue v WHERE v.id = ANY($1)
           AND NOT EXISTS (SELECT 1 FROM tags_issuetag WHERE value_id = v.id)",
    )
    .bind(value_ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
