//! `tags/utils.py::deduce_tags` and `tags/models.py::digest_tags`.

use bugsink_db::{
  Postgres,
  Transaction,
};
use serde_json::{
  Map,
  Value,
};
use uuid::Uuid;

use super::StoredEvent;
use crate::grouping::{
  get_path,
  main_exception,
};
use crate::py::{
  py_str,
  take_chars,
  truthy,
};
use crate::user_agent::contexts_enriched_with_user_agent;

const MAX_EVENT_TAGS: usize = 100;

/// `value not in [None, ""]`
fn present(v: Option<&Value>) -> Option<&Value> {
  v.filter(|v| !v.is_null() && v.as_str() != Some(""))
}

/// `_convert_non_strings`: booleans as lowercase strings.
fn convert(v: &Value) -> Value {
  match v {
    Value::Bool(b) => Value::String(b.to_string()),
    other => other.clone(),
  }
}

fn deduce_tags(data: &Value) -> Map<String, Value> {
  let mut tags = match data.get("tags") {
    Some(Value::Object(o)) => o.clone(),
    Some(Value::Array(pairs)) => {
      pairs
        .iter()
        .filter_map(|p| {
          match p.as_array().map(Vec::as_slice) {
            Some([k, v]) => Some((py_str(k), v.clone())),
            _ => None,
          }
        })
        .collect()
    }
    _ => Map::new(),
  };

  for key in ["server_name", "release", "environment", "transaction"] {
    if let Some(v) = present(get_path(data, &[key])) {
      tags.insert(key.into(), convert(v));
    }
  }
  if let Some(url) = present(get_path(data, &["request", "url"])) {
    let url = py_str(url);
    let without = url.split('#').next().unwrap_or("").split('?').next().unwrap_or("");
    tags.insert("url".into(), Value::String(without.into()));
  }
  if let Some(v) = main_exception(data)
    .ok()
    .flatten()
    .and_then(|e| present(get_path(e, &["mechanism", "handled"])))
  {
    tags.insert("handled".into(), convert(v));
  }

  let contexts = &contexts_enriched_with_user_agent(data);
  for (tag, keys) in [
    ("trace", ["trace", "trace_id"]),
    ("trace.span", ["trace", "span_id"]),
    ("browser.name", ["browser", "name"]),
    ("browser.version", ["browser", "version"]),
    ("os.name", ["os", "name"]),
    ("os.version", ["os", "version"]),
  ] {
    if let Some(v) = present(get_path(contexts, &keys)) {
      tags.insert(tag.into(), convert(v));
    }
  }
  for (combined, a, b, sep) in [
    ("trace.ctx", "trace", "trace.span", "."),
    ("browser", "browser.name", "browser.version", " "),
    ("os", "os.name", "os.version", " "),
  ] {
    if let (Some(x), Some(y)) = (tags.get(a), tags.get(b)) {
      let value = format!("{}{sep}{}", py_str(x), py_str(y));
      tags.insert(combined.into(), Value::String(value));
    }
  }

  if let Some(user) = data.get("user").filter(|u| truthy(u)).and_then(Value::as_object) {
    let mut user_tags = Map::new();
    for key in ["id", "username", "email", "ip_address"] {
      if let Some(v) = user.get(key).filter(|v| truthy(v)) {
        user_tags.insert(format!("user.{key}"), v.clone());
        if !user_tags.contains_key("user") {
          user_tags.insert("user".into(), v.clone());
        }
      }
    }
    tags.extend(user_tags);
  }
  tags
}

fn is_mostly_unique(key: &str) -> bool {
  key.starts_with("user")
    || key.starts_with("trace")
    || matches!(key, "browser.version" | "browser" | "os.version" | "os")
}

pub(crate) async fn digest_tags(
  tx: &mut Transaction<'_, Postgres>,
  data: &Value,
  event: &StoredEvent,
  issue_id: Uuid,
) -> sqlx::Result<()> {
  let mut tags: Vec<(String, String)> = deduce_tags(data)
    .iter()
    .map(|(k, v)| (k.clone(), take_chars(&py_str(v), 200)))
    .collect();

  for key in ["user.ip_address", "user"] {
    if let Some(pos) = tags.iter().position(|(k, v)| k == key && v == "{{auto}}") {
      match &event.remote_addr {
        Some(addr) => tags[pos].1 = addr.clone(),
        None => {
          tags.remove(pos);
        }
      }
    }
  }
  if tags.len() > MAX_EVENT_TAGS {
    tracing::warn!(
      "event has {} tags; storing {MAX_EVENT_TAGS} and dropping the rest",
      tags.len()
    );
    tags.truncate(MAX_EVENT_TAGS);
  }

  if tags.is_empty() {
    return Ok(());
  }

  // All tags in three statements (not a few per tag: digestion holds the project's lock, so round trips are its
  // throughput). Each statement sees the rows the ones before it wrote; rows are inserted in the tags' order. Existing
  // keys and values are only rewritten when the conflicting row differs.
  let (keys, values): (Vec<String>, Vec<String>) = tags.into_iter().unzip();
  let mostly_unique: Vec<bool> = keys.iter().map(|k| is_mostly_unique(k)).collect();
  sqlx::query(
    "INSERT INTO tags_tagkey (project_id, key, mostly_unique)
         SELECT $1, t.key, t.mostly_unique FROM unnest($2::text[], $3::bool[]) WITH ORDINALITY AS t(key, mostly_unique, n)
         ORDER BY t.n
         ON CONFLICT (project_id, key) DO UPDATE SET mostly_unique = EXCLUDED.mostly_unique
         WHERE tags_tagkey.mostly_unique IS DISTINCT FROM EXCLUDED.mostly_unique",
  )
  .bind(event.project_id)
  .bind(&keys)
  .bind(&mostly_unique)
  .execute(&mut **tx)
  .await?;
  sqlx::query(
    "INSERT INTO tags_tagvalue (project_id, key_id, value)
         SELECT $1, k.id, t.value FROM unnest($2::text[], $3::text[]) WITH ORDINALITY AS t(key, value, n)
         JOIN tags_tagkey k ON k.project_id = $1 AND k.key = t.key
         ORDER BY t.n
         ON CONFLICT (key_id, value) DO UPDATE SET project_id = EXCLUDED.project_id
         WHERE tags_tagvalue.project_id IS DISTINCT FROM EXCLUDED.project_id",
  )
  .bind(event.project_id)
  .bind(&keys)
  .bind(&values)
  .execute(&mut **tx)
  .await?;
  sqlx::query(
    "WITH tagged AS (
             SELECT t.n, k.id AS key_id, v.id AS value_id
             FROM unnest($2::text[], $3::text[]) WITH ORDINALITY AS t(key, value, n)
             JOIN tags_tagkey k ON k.project_id = $1 AND k.key = t.key
             JOIN tags_tagvalue v ON v.key_id = k.id AND v.value = t.value
         ), event_tags AS (
             INSERT INTO tags_eventtag (project_id, value_id, event_id, issue_id, digest_order)
             SELECT $1, value_id, $4, $5, $6 FROM tagged ORDER BY n
             ON CONFLICT (value_id, event_id) DO NOTHING
         )
         INSERT INTO tags_issuetag (project_id, key_id, value_id, issue_id, count)
         SELECT $1, key_id, value_id, $5, 1 FROM tagged ORDER BY n
         ON CONFLICT (value_id, issue_id) DO UPDATE SET count = tags_issuetag.count + 1",
  )
  .bind(event.project_id)
  .bind(&keys)
  .bind(&values)
  .bind(event.id)
  .bind(issue_id)
  .bind(event.digest_order)
  .execute(&mut **tx)
  .await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  #[test]
  fn deduces() {
    let data = json!({
        "tags": {"custom": "x"}, "release": "1.0", "environment": "", "request": {"url": "https://a/b?q#f"},
        "exception": {"values": [{"type": "E", "mechanism": {"handled": false}}]},
        "contexts": {"browser": {"name": "Firefox", "version": "120"}},
        "user": {"id": 7, "ip_address": "{{auto}}"},
    });
    let tags = deduce_tags(&data);
    let keys: Vec<&str> = tags.keys().map(String::as_str).collect();
    assert_eq!(
      keys,
      [
        "custom",
        "release",
        "url",
        "handled",
        "browser.name",
        "browser.version",
        "browser",
        "user.id",
        "user",
        "user.ip_address"
      ]
    );
    assert_eq!(tags["url"], "https://a/b");
    assert_eq!(tags["handled"], "false");
    assert_eq!(tags["browser"], "Firefox 120");
    assert_eq!(tags["user"], 7);
  }
}
