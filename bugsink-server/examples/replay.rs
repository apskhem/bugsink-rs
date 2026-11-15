//! Replays digest steps (JSONL on stdin, see tools/parity/make_digest_steps.py) through this crate's digest, issue
//! actions and vacuuming. Jobs that steps schedule (zero-retention deletion, alerts, issue deletion) run right after
//! each step, like upstream's on-commit tasks in eager mode.
//! Usage: DATABASE_URL=... cargo run -p bugsink-server --example replay < steps.jsonl
//! Settings (quotas, alerts) come from the same environment variables as `bugsink` (and upstream).

use std::io::BufRead;

use bugsink_server::digest::{
  DigestError,
  DigestJob,
  digest,
};
use bugsink_server::issues::{
  self,
  Action,
};
use bugsink_server::{
  Settings,
  job_registry,
  tasks,
  vacuum,
};
use chrono::{
  DateTime,
  Utc,
};
use clap::Parser;
use serde_json::Value;
use sqlx::Executor;

#[derive(Parser)]
struct Args {
  #[command(flatten)]
  settings: Settings,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let settings = Args::parse().settings;
  let pool = bugsink_db::connect(&std::env::var("DATABASE_URL")?, 2).await?;
  bugsink_db::prepare(&pool, false).await?;
  let registry = job_registry(&settings);
  for line in std::io::stdin().lock().lines() {
    let step: Value = serde_json::from_str(&line?)?;
    if let Some(sql) = step["sql"].as_str() {
      pool.execute(sqlx::AssertSqlSafe(sql.to_string())).await?;
      continue;
    }
    if step["vacuum"].as_bool() == Some(true) {
      let all = vacuum::What {
        eventless_issuetags: true,
        tags: true,
        deleted_issues: true,
      };
      vacuum::run(&pool, all).await?;
      continue;
    }
    if let Some(action) = step["action"].as_str() {
      let action = Action::parse(action).unwrap();
      let issue_ids: Vec<uuid::Uuid> =
        sqlx::query_scalar(sqlx::AssertSqlSafe(step["issues"].as_str().unwrap().to_string()))
          .fetch_all(&pool)
          .await?;
      let user_id = step["user_id"].as_i64().unwrap();
      let now = step["now"].as_str().unwrap().parse::<DateTime<Utc>>()?;
      if step["bulk"].as_bool().unwrap() {
        let project_id = step["project_id"].as_i64().unwrap();
        issues::apply_to_issues(&pool, &[project_id], &issue_ids, &action, user_id, now).await?;
      } else {
        for issue_id in issue_ids {
          issues::apply_to_issue(&pool, issue_id, &action, Some(user_id), now).await?;
        }
      }
      tasks::run_until_idle(&pool, &registry).await?;
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
    match digest(&pool, job, digested_at, &settings.quotas).await {
      Ok(()) => {}
      Err(DigestError::Drop(reason)) => println!("step {}: dropped: {reason}", step["n"]),
      Err(DigestError::Retry(e)) => anyhow::bail!("step {}: {e:#}", step["n"]),
    }
    tasks::run_until_idle(&pool, &registry).await?;
  }
  Ok(())
}
