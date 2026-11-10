//! Replays digest steps (JSONL on stdin, see tools/parity/make_digest_steps.py) through this crate's digest. Jobs that
//! digest schedules (zero-retention deletion) run right after each step, like upstream's on-commit tasks in eager mode.
//! Usage: DATABASE_URL=... cargo run -p bugsink-server --example replay < steps.jsonl

use std::io::BufRead;

use bugsink_server::digest::{DigestError, DigestJob, digest, retention};
use bugsink_server::tasks;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Executor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = bugsink_db::connect(&std::env::var("DATABASE_URL")?, 2).await?;
    bugsink_db::prepare(&pool, false).await?;
    let registry = tasks::Registry::default()
        .register(retention::DELETE_BY_AGE_JOB_KIND, retention::handle_delete_by_age_job);
    for line in std::io::stdin().lock().lines() {
        let step: Value = serde_json::from_str(&line?)?;
        if let Some(sql) = step["sql"].as_str() {
            pool.execute(sqlx::AssertSqlSafe(sql.to_string())).await?;
            continue;
        }
        let job = DigestJob {
            project_id: step["project_id"].as_i64().unwrap(),
            event_id: step["event_id"].as_str().unwrap().parse()?,
            ingested_at: step["ingested_at"].as_str().unwrap().parse::<DateTime<Utc>>()?,
            remote_addr: step["remote_addr"].as_str().map(String::from),
            event: step["event"].clone(),
        };
        let digested_at = step["digested_at"].as_str().unwrap().parse::<DateTime<Utc>>()?;
        match digest(&pool, job, digested_at).await {
            Ok(_) => {}
            Err(DigestError::Drop(reason)) => println!("step {}: dropped: {reason}", step["n"]),
            Err(DigestError::Retry(e)) => anyhow::bail!("step {}: {e:#}", step["n"]),
        }
        tasks::run_until_idle(&pool, &registry).await?;
    }
    Ok(())
}
