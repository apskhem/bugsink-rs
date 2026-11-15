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

  for (key, value) in tags {
    let key_id: i64 = sqlx::query_scalar(
      "INSERT INTO tags_tagkey (project_id, key, mostly_unique) VALUES ($1, $2, $3)
             ON CONFLICT (project_id, key) DO UPDATE SET mostly_unique = EXCLUDED.mostly_unique RETURNING id",
    )
    .bind(event.project_id)
    .bind(&key)
    .bind(is_mostly_unique(&key))
    .fetch_one(&mut **tx)
    .await?;
    let value_id: i64 = sqlx::query_scalar(
      "INSERT INTO tags_tagvalue (project_id, key_id, value) VALUES ($1, $2, $3)
             ON CONFLICT (key_id, value) DO UPDATE SET project_id = EXCLUDED.project_id RETURNING id",
    )
    .bind(event.project_id)
    .bind(key_id)
    .bind(&value)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
      "INSERT INTO tags_eventtag (project_id, value_id, event_id, issue_id, digest_order) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (value_id, event_id) DO NOTHING",
    )
    .bind(event.project_id)
    .bind(value_id)
    .bind(event.id)
    .bind(issue_id)
    .bind(event.digest_order)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
      "INSERT INTO tags_issuetag (project_id, key_id, value_id, issue_id, count) VALUES ($1, $2, $3, $4, 0)
             ON CONFLICT (value_id, issue_id) DO NOTHING",
    )
    .bind(event.project_id)
    .bind(key_id)
    .bind(value_id)
    .bind(issue_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE tags_issuetag SET count = count + 1 WHERE value_id = $1 AND issue_id = $2")
      .bind(value_id)
      .bind(issue_id)
      .execute(&mut **tx)
      .await?;
  }
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
