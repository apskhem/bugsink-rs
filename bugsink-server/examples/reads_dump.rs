//! Prints this crate's answers for the inputs of `tools/parity/upstream_reads.py` (JSONL on stdin, its `input`
//! objects), in the same format, for diffing against upstream.
//! Usage: DATABASE_URL=... cargo run -p bugsink-server --example reads_dump < inputs.jsonl

use std::io::BufRead;

use bugsink_db::PgPool;
use bugsink_server::event_display;
use bugsink_server::grouping::py::isoformat;
use bugsink_server::issues::pages;
use bugsink_server::search::{Search, Searched, parse_query};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use sqlx::QueryBuilder;
use uuid::Uuid;

/// (id, digested_event_count, stored_event_count) of a project's issue by number.
async fn issue(pool: &PgPool, input: &Value) -> anyhow::Result<(Uuid, i32, i32)> {
    Ok(sqlx::query_as(
        "SELECT id, digested_event_count, stored_event_count FROM issues_issue
         WHERE project_id = $1 AND digest_order = $2",
    )
    .bind(input["project_id"].as_i64().unwrap())
    .bind(input["issue"].as_i64().unwrap() as i32)
    .fetch_one(pool)
    .await?)
}

fn time(v: &Value) -> DateTime<Utc> {
    v.as_str().unwrap().parse().unwrap()
}

fn hours(rows: &[pages::HourCount]) -> Value {
    rows.iter().map(|h| json!([isoformat(&h.bucket), h.count, h.digest_order])).collect()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = bugsink_db::connect(&std::env::var("DATABASE_URL")?, 2).await?;
    for line in std::io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line?)?;
        let q = input["q"].as_str().unwrap_or("");
        let parsed = parse_query(q);
        // upstream fails on these (see `bugsink_server::search`); we match everything
        let upstream_fails = !q.is_empty() && parsed.tags.is_empty() && parsed.plain_text.is_empty();
        let project_id = input["project_id"].as_i64().unwrap_or(0);
        let search = || Search::prepare(&pool, project_id, q);
        let result = match input["kind"].as_str().unwrap() {
            "parse" => json!({"tags": parsed.tags, "plain_text": parsed.plain_text}),
            "issues" | "events" | "nav" if upstream_fails => json!("error: empty list of Q objects"),
            "issues" => {
                let search = search().await?;
                let mut qb = QueryBuilder::new("SELECT digest_order FROM issues_issue WHERE project_id = ");
                qb.push_bind(project_id);
                qb.push(" AND NOT is_deleted");
                search.push_conditions(&mut qb, Searched::Issues);
                qb.push(" ORDER BY digest_order");
                json!(qb.build_query_scalar::<i32>().fetch_all(&pool).await?)
            }
            "events" => {
                let (issue_id, _, _) = issue(&pool, &input).await?;
                let search = search().await?;
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
            "nav" => {
                let (issue_id, _, stored) = issue(&pool, &input).await?;
                let search = search().await?;
                let mut around = Map::new();
                let mut nav = pages::Nav::default();
                for d in input["around"].as_array().unwrap() {
                    let d = d.as_i64().unwrap() as i32;
                    nav = pages::nav(&pool, issue_id, stored, &search, !q.is_empty(), d).await?;
                    around.insert(d.to_string(), json!([nav.prev, nav.next]));
                }
                json!({"first": nav.first, "last": nav.last, "count": nav.count, "around": around})
            }
            "hourly" => {
                let (issue_id, _, _) = issue(&pool, &input).await?;
                let search = if q.is_empty() { None } else { Some(search().await?) };
                let (all, matching) =
                    pages::hourly_counts(&pool, issue_id, time(&input["since"]), time(&input["until"]), search.as_ref())
                        .await?;
                json!({"hours": hours(&all), "matching": matching.as_deref().map(hours)})
            }
            "detail" => {
                let (issue_id, _, _) = issue(&pool, &input).await?;
                let d = pages::issue_detail(&pool, issue_id).await?;
                json!({"friendly_id": d.friendly_id, "title": d.title, "fixed_at": d.fixed_at, "events_at": d.events_at,
                       "latest_release": d.latest_release})
            }
            "tags" => {
                let (issue_id, digested, _) = issue(&pool, &input).await?;
                let cutoff = input["cutoff"].as_u64().unwrap() as usize;
                let tags = pages::tags(&pool, issue_id, digested, cutoff).await?;
                tags.iter()
                    .map(|k| {
                        json!({
                            "key": k.key,
                            "values": k.values.iter().map(|v| json!([v.value, v.count, v.pct])).collect::<Vec<_>>(),
                            "other": k.other.as_ref().map(|o| json!([o.count, o.pct])),
                        })
                    })
                    .collect()
            }
            "groupings" => {
                let (issue_id, _, _) = issue(&pool, &input).await?;
                let groupings = pages::groupings(&pool, issue_id).await?;
                groupings.iter().map(|g| json!([g.grouping_mechanism, g.grouping_key])).collect()
            }
            "history" => {
                let (issue_id, _, _) = issue(&pool, &input).await?;
                pages::history(&pool, issue_id)
                    .await?
                    .iter()
                    .map(|t| {
                        json!([
                            t.id,
                            t.kind,
                            isoformat(&t.timestamp),
                            t.metadata,
                            t.comment,
                            t.user.as_ref().map(|u| json!([u.id, u.display_name, u.gravatar_sha256])),
                            t.triggering_event.as_ref().map(|e| e.event_id.simple().to_string()),
                        ])
                    })
                    .collect()
            }
            "list_sparklines" => {
                let ids: Vec<(Uuid, i32)> = sqlx::query_as(
                    "SELECT id, digest_order FROM issues_issue WHERE project_id = $1 AND NOT is_deleted",
                )
                .bind(project_id)
                .fetch_all(&pool)
                .await?;
                let uuids: Vec<Uuid> = ids.iter().map(|(id, _)| *id).collect();
                let sparklines = pages::issue_list_sparklines(&pool, &uuids, time(&input["now"])).await?;
                ids.iter().map(|(id, order)| (order.to_string(), json!(sparklines[id]))).collect::<Map<_, _>>().into()
            }
            "project_sparklines" => {
                let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM projects_project").fetch_all(&pool).await?;
                let sparklines = pages::project_list_sparklines(&pool, &ids, time(&input["now"])).await?;
                ids.iter().map(|id| (id.to_string(), json!(sparklines[id]))).collect::<Map<_, _>>().into()
            }
            "display" => {
                let id: Uuid = input["event"].as_str().unwrap().parse()?;
                let d = event_display::for_event(&pool, id).await?.expect("stored in the database");
                let s = &d.stacktrace;
                json!({
                    "stacktrace": {"entries": s.entries, "thread_stacktraces": s.thread_stacktraces,
                                   "stack_of_plates": s.stack_of_plates,
                                   "multiple_thread_stacktraces": s.multiple_thread_stacktraces,
                                   "request_repr": d.request_repr},
                    "details": {"key_info": d.key_info, "logentry_info": d.logentry_info,
                                "deployment_info": d.deployment_info, "contexts": d.contexts},
                    "breadcrumbs": d.breadcrumbs,
                })
            }
            kind => anyhow::bail!("unknown kind {kind}"),
        };
        println!("{}", json!({"input": input, "result": result}));
    }
    Ok(())
}
