//! Prints the contexts this crate derives from each JSONL event's User-Agent header (stdin), one JSON line each, in
//! the same format as `tools/parity/upstream_user_agents.py`, for diffing against upstream.

use std::io::BufRead;

use bugsink_server::digest::contexts_enriched_with_user_agent;
use serde_json::Value;

fn main() {
    for line in std::io::stdin().lock().lines() {
        let data: Value = serde_json::from_str(&line.unwrap()).unwrap();
        println!("{}", contexts_enriched_with_user_agent(&data));
    }
}
