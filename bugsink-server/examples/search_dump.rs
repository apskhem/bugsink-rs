//! Prints this crate's search results for the inputs of `tools/parity/upstream_search.py` (JSONL on stdin, its
//! `input` objects), in the same format, for diffing against upstream.
//! Usage: DATABASE_URL=... cargo run -p bugsink-server --example search_dump < inputs.jsonl

use std::io::BufRead;

use bugsink_server::search::{Search, Searched, parse_query};
use serde_json::{Value, json};
use sqlx::QueryBuilder;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = bugsink_db::connect(&std::env::var("DATABASE_URL")?, 2).await?;
    for line in std::io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line?)?;
        let q = input["q"].as_str().unwrap();
        let parsed = parse_query(q);
        let result = match input["kind"].as_str().unwrap() {
            "parse" => json!({"tags": parsed.tags, "plain_text": parsed.plain_text}),
            // upstream fails on these (see `crate::search`); we match everything
            _ if !q.is_empty() && parsed.tags.is_empty() && parsed.plain_text.is_empty() => {
                json!("error: empty list of Q objects")
            }
            kind => {
                let project_id = input["project_id"].as_i64().unwrap();
                let search = Search::prepare(&pool, project_id, q).await?;
                if kind == "issues" {
                    let mut qb = QueryBuilder::new("SELECT digest_order FROM issues_issue WHERE project_id = ");
                    qb.push_bind(project_id);
                    qb.push(" AND NOT is_deleted");
                    search.push_conditions(&mut qb, Searched::Issues);
                    qb.push(" ORDER BY digest_order");
                    let orders: Vec<i32> = qb.build_query_scalar().fetch_all(&pool).await?;
                    json!(orders)
                } else {
                    let issue_id: Uuid = sqlx::query_scalar(
                        "SELECT id FROM issues_issue WHERE project_id = $1 AND digest_order = $2",
                    )
                    .bind(project_id)
                    .bind(input["issue"].as_i64().unwrap() as i32)
                    .fetch_one(&pool)
                    .await?;
                    let mut qb = QueryBuilder::new("SELECT event_id FROM events_event WHERE issue_id = ");
                    qb.push_bind(issue_id);
                    search.push_conditions(&mut qb, Searched::EventsOf(issue_id));
                    let mut ids: Vec<String> = qb
                        .build_query_scalar::<Uuid>()
                        .fetch_all(&pool)
                        .await?
                        .into_iter()
                        .map(|id| id.simple().to_string())
                        .collect();
                    ids.sort();
                    json!(ids)
                }
            }
        };
        println!("{}", json!({"input": input, "result": result}));
    }
    Ok(())
}
