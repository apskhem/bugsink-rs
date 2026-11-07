//! Prints this crate's grouping outputs for JSONL events on stdin, in the same format as
//! `tools/parity/upstream_grouping.py`, for diffing against upstream.

use std::io::BufRead;

use bugsink_server::grouping::{BUGSINK_V1, BUGSINK_V2, denormalized_fields, display_type_and_value, key_with_mechanism};
use serde_json::{Value, json};

fn main() {
    for line in std::io::stdin().lock().lines() {
        let data: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let err = |_| json!({"error": "error"});
        let mut out = serde_json::Map::new();
        for mechanism in [BUGSINK_V1, BUGSINK_V2] {
            let k = key_with_mechanism(&data, mechanism).map(|k| json!([k.key, k.mechanism]));
            out.insert(mechanism.into(), k.unwrap_or_else(err));
        }
        out.insert("display".into(), display_type_and_value(&data).map(|(t, v)| json!([t, v])).unwrap_or_else(err));
        let d = denormalized_fields(&data).map(|d| {
            json!({"transaction": d.transaction, "last_frame_filename": d.last_frame_filename,
                   "last_frame_module": d.last_frame_module, "last_frame_function": d.last_frame_function})
        });
        out.insert("denormalized".into(), d.unwrap_or_else(err));
        println!("{}", Value::Object(out));
    }
}
