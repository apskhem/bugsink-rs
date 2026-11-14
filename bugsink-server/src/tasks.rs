//! A small job queue on Postgres, replacing upstream's SQLite-based "snappea".
//!
//! - Enqueue inside the caller's transaction, so a job exists iff the work that produced it committed.
//! - Workers claim one job at a time with `FOR UPDATE SKIP LOCKED`, so any number of workers (and processes) can share
//!   the table.
//! - `NOTIFY jobs` on enqueue wakes idle workers immediately; polling is only a fallback.
//! - Delivery is at-least-once: a worker can die after doing the work but before deleting the job. Handlers must be
//!   idempotent (digest checks `events (project_id, event_id)` for this).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bugsink_db::PgPool;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::postgres::PgListener;
use sqlx::{PgExecutor, Row};
use tokio::sync::watch;

const CHANNEL: &str = "jobs";
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_ATTEMPTS: i32 = 5;

/// A job's payload, by the kind it's stored and dispatched as.
pub trait Job: Serialize + DeserializeOwned + Send + 'static {
    const KIND: &str;
}

pub async fn enqueue<'e, J: Job>(executor: impl PgExecutor<'e>, job: &J) -> anyhow::Result<()> {
    sqlx::query(
        "WITH new_job AS (INSERT INTO bsrs_job (kind, payload) VALUES ($1, $2::json) RETURNING id)
         SELECT pg_notify($3, '') FROM new_job",
    )
    .bind(J::KIND)
    .bind(serde_json::to_string(job)?)
    .bind(CHANNEL)
    .execute(executor)
    .await?;
    Ok(())
}

type Handler = Arc<dyn Fn(PgPool, Value) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send + Sync>;

#[derive(Clone, Default)]
pub struct Registry {
    handlers: HashMap<&'static str, Handler>,
}

impl Registry {
    pub fn register<J: Job, Fut>(self, handler: impl Fn(PgPool, J) -> Fut + Send + Sync + 'static) -> Self
    where
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.register_with((), move |pool, job, ()| handler(pool, job))
    }

    /// A handler that is also given (a clone of) `state`.
    pub fn register_with<J: Job, S, Fut>(
        mut self,
        state: S,
        handler: impl Fn(PgPool, J, S) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        S: Clone + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        self.handlers.insert(
            J::KIND,
            Arc::new(move |pool, payload| {
                let (handler, state) = (handler.clone(), state.clone());
                Box::pin(async move { handler(pool, serde_json::from_value(payload)?, state).await })
            }),
        );
        self
    }
}

struct Claimed {
    id: i64,
    kind: String,
    payload: Value,
    attempts: i32,
}

/// Runs `concurrency` workers until `shutdown` flips to true; in-flight jobs are finished first.
pub async fn run_workers(pool: PgPool, registry: Registry, concurrency: usize, shutdown: watch::Receiver<bool>) {
    let registry = Arc::new(registry);
    let workers: Vec<_> = (0..concurrency)
        .map(|n| tokio::spawn(worker(n, pool.clone(), registry.clone(), shutdown.clone())))
        .collect();
    for w in workers {
        let _ = w.await;
    }
}

/// Runs ready jobs in the calling task until none are left (failed ones are retried later, so this ends). For the
/// parity harness, which needs jobs done before the next step.
pub async fn run_until_idle(pool: &PgPool, registry: &Registry) -> sqlx::Result<()> {
    while let Some(job) = claim(pool).await? {
        run_one(pool, registry, job).await;
    }
    Ok(())
}

async fn worker(n: usize, pool: PgPool, registry: Arc<Registry>, mut shutdown: watch::Receiver<bool>) {
    let mut listener = match PgListener::connect_with(&pool).await {
        Ok(mut l) => match l.listen(CHANNEL).await {
            Ok(()) => Some(l),
            Err(e) => {
                tracing::warn!(worker = n, error = %e, "LISTEN failed; falling back to polling");
                None
            }
        },
        Err(e) => {
            tracing::warn!(worker = n, error = %e, "listener connect failed; falling back to polling");
            None
        }
    };

    while !*shutdown.borrow() {
        match claim(&pool).await {
            Ok(Some(job)) => run_one(&pool, &registry, job).await,
            Ok(None) => {
                let notified = async {
                    match listener.as_mut() {
                        Some(l) => drop(l.recv().await),
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    _ = notified => {}
                    _ = tokio::time::sleep(POLL_INTERVAL) => {}
                    _ = shutdown.changed() => {}
                }
            }
            Err(e) => {
                tracing::error!(worker = n, error = %e, "claiming job failed");
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }
}

async fn claim(pool: &PgPool) -> sqlx::Result<Option<Claimed>> {
    // A job locked for longer than 10 minutes is assumed to belong to a dead worker and is handed out again.
    let row = sqlx::query(
        "UPDATE bsrs_job SET locked_at = now(), attempts = attempts + 1
         WHERE id = (
             SELECT id FROM bsrs_job
             WHERE failed_at IS NULL AND run_at <= now()
               AND (locked_at IS NULL OR locked_at < now() - interval '10 minutes')
             ORDER BY run_at, id
             FOR UPDATE SKIP LOCKED
             LIMIT 1)
         RETURNING id, kind, payload, attempts",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Claimed {
        id: r.get("id"),
        kind: r.get("kind"),
        payload: r.get::<sqlx::types::Json<Value>, _>("payload").0,
        attempts: r.get("attempts"),
    }))
}

async fn run_one(pool: &PgPool, registry: &Registry, job: Claimed) {
    let result = match registry.handlers.get(job.kind.as_str()) {
        Some(handler) => handler(pool.clone(), job.payload).await,
        None => Err(anyhow::anyhow!("no handler registered for job kind {:?}", job.kind)),
    };

    let outcome = match &result {
        Ok(()) => sqlx::query("DELETE FROM bsrs_job WHERE id = $1").bind(job.id).execute(pool).await,
        Err(e) => {
            tracing::warn!(job = job.id, kind = %job.kind, attempt = job.attempts, error = %e, "job failed");
            // Exponential backoff: 2s, 4s, 8s, ... ; give up after MAX_ATTEMPTS.
            sqlx::query(
                "UPDATE bsrs_job SET locked_at = NULL, last_error = $2,
                     run_at = now() + make_interval(secs => power(2, attempts)),
                     failed_at = CASE WHEN attempts >= $3 THEN now() END
                 WHERE id = $1",
            )
            .bind(job.id)
            .bind(format!("{e:#}"))
            .bind(MAX_ATTEMPTS)
            .execute(pool)
            .await
        }
    };
    if let Err(e) = outcome {
        tracing::error!(job = job.id, error = %e, "recording job outcome failed");
    }
}
