//! `delete_issue_deps`: deletes a deleted issue's rows in batches of at most 500, then the issue itself; the job
//! re-enqueues itself until done. Same order as upstream's `get_model_topography_with_issue_override`, everything via
//! `issue_id` (so an event's tags and turning points are gone before the event).

use bugsink_db::PgPool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::tasks;

pub const JOB_KIND: &str = "delete_issue_deps";

/// `DELETE_ISSUE_DEPS_BATCH_SIZE`
const BATCH_SIZE: i64 = 500;

/// Tables with an `issue_id`, in deletion order.
const DEPENDENTS: [&str; 6] = [
    "issues_turningpoint",
    "events_issueeventcountsperhour",
    "tags_eventtag",
    "events_event",
    "issues_grouping",
    "tags_issuetag",
];

#[derive(Serialize, Deserialize)]
pub struct DeleteIssueDepsJob {
    pub project_id: i64,
    pub issue_id: Uuid,
}

pub(crate) async fn enqueue<'e>(executor: impl PgExecutor<'e>, project_id: i64, issue_id: Uuid) -> anyhow::Result<()> {
    tasks::enqueue(executor, JOB_KIND, &DeleteIssueDepsJob { project_id, issue_id }).await
}

pub async fn handle_job(pool: PgPool, payload: Value) -> anyhow::Result<()> {
    let job: DeleteIssueDepsJob = serde_json::from_value(payload)?;
    if delete_batch(&pool, job.project_id, job.issue_id).await? {
        enqueue(&pool, job.project_id, job.issue_id).await?;
    }
    Ok(())
}

/// `delete_marked_issues_sync`: finishes deleting every issue marked as deleted, in this process (`bugsink vacuum`).
pub async fn delete_marked_issues(pool: &PgPool) -> anyhow::Result<()> {
    loop {
        let marked: Vec<(Uuid, i64)> =
            sqlx::query_as("SELECT id, project_id FROM issues_issue WHERE is_deleted ORDER BY id LIMIT 250")
                .fetch_all(pool)
                .await?;
        if marked.is_empty() {
            return Ok(());
        }
        for (issue_id, project_id) in marked {
            while delete_batch(pool, project_id, issue_id).await? {}
        }
    }
}

/// `delete_issue_deps_batch`: true when the budget ran out and another batch is needed.
async fn delete_batch(pool: &PgPool, project_id: i64, issue_id: Uuid) -> anyhow::Result<bool> {
    let job = DeleteIssueDepsJob { project_id, issue_id };
    let mut tx = pool.begin().await?;
    // digest's lock: events_event deletions change the project's stored_event_count
    sqlx::query("SELECT id FROM projects_project WHERE id = $1 FOR UPDATE")
        .bind(job.project_id)
        .execute(&mut *tx)
        .await?;

    let mut num_deleted = 0;
    for table in DEPENDENTS {
        // `delete_deps_with_budget`: the first `budget` rows by (issue_id, pk)
        let value_id = if table == "tags_issuetag" { "value_id" } else { "NULL::bigint" };
        let value_ids: Vec<Option<i64>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE id IN (SELECT id FROM {table} WHERE issue_id = $1 ORDER BY id LIMIT $2)
             RETURNING {value_id}"
        )))
        .bind(job.issue_id)
        .bind(BATCH_SIZE - num_deleted)
        .fetch_all(&mut *tx)
        .await?;
        let deleted = value_ids.len() as i64;
        if deleted == 0 {
            continue;
        }
        if table == "events_event" {
            // `do_pre_delete` (the issue's own count doesn't matter anymore)
            sqlx::query("UPDATE projects_project SET stored_event_count = stored_event_count - $2 WHERE id = $1")
                .bind(job.project_id)
                .bind(deleted as i32)
                .execute(&mut *tx)
                .await?;
        }
        if table == "tags_issuetag" {
            // `prune_orphans`
            let value_ids: Vec<i64> = value_ids.into_iter().flatten().collect();
            crate::vacuum::prune_tagvalues(&mut tx, &value_ids).await?;
        }
        num_deleted += deleted;
        if num_deleted >= BATCH_SIZE {
            break;
        }
    }

    let more = num_deleted >= BATCH_SIZE;
    if !more {
        sqlx::query("DELETE FROM issues_issue WHERE id = $1").bind(job.issue_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(more)
}
