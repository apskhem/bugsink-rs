//! Phoning home, as upstream Bugsink 2.6.3's `phonehome` app: at most once an hour, a message about the installation
//! goes to bugsink.com (`PHONEHOME`, on by default), and the answer's "remote messages" (e.g. "a security fix is
//! out") become system warnings for superusers until silenced or no longer applying to this version.
//!
//! Upstream checks whether a message is due on visits to some pages (home, the project and issue lists, the issue
//! pages) and on start (`prestart`), and sends it from a task; so do we, from a job (`SendIfDue`).
//!
//! The message reports what upstream's does, as far as it applies: the version is the upstream version whose schema
//! we run on (remote messages are about that version), there is no Python version (null), and the runtime is ours.

use std::sync::Arc;
use std::time::Duration;

use bugsink_db::PgPool;
use chrono::{
  DateTime,
  TimeDelta,
  Utc,
};
use serde::{
  Deserialize,
  Serialize,
};
use serde_json::{
  Value,
  json,
};
use sqlx::Row;

use crate::Settings;
use crate::py::py_str;
use crate::tasks::{
  self,
  Job,
};

const URL: &str = "https://www.bugsink.com/phonehome/v1/";

/// `INTERVAL`: phone home at most once an hour.
const INTERVAL: TimeDelta = TimeDelta::hours(1);

#[derive(clap::Args, Clone, Debug)]
pub struct PhonehomeSettings {
  /// Send bugsink.com a message about this installation once an hour, and show the messages it answers with.
  #[arg(long, env = "PHONEHOME", default_value = "true", value_parser = crate::py::py_bool)]
  pub phonehome: bool,
  /// Where to send it instead (for testing).
  #[arg(long, env = "BUGSINK_PHONEHOME_URL", default_value = URL, hide = true)]
  pub phonehome_url: String,
}

/// `send_if_due`.
#[derive(Serialize, Deserialize)]
pub struct SendIfDue {}

impl Job for SendIfDue {
  const KIND: &str = "phonehome.send_if_due";
}

/// `phone_home_is_due`: nothing was attempted in the last hour.
async fn is_due<'e>(executor: impl sqlx::PgExecutor<'e>, now: DateTime<Utc>) -> sqlx::Result<bool> {
  sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM phonehome_outboundmessage WHERE attempted_at >= $1)")
    .bind(now - INTERVAL)
    .fetch_one(executor)
    .await
}

/// The `phone_home` decorator's check, on the pages upstream decorates: if a message is due, have a worker send it.
/// In the background, so the page doesn't wait for it.
pub fn kick_off(pool: &PgPool, settings: &PhonehomeSettings) {
  if !settings.phonehome {
    return; // upstream's task would return right away
  }
  let pool = pool.clone();
  tokio::spawn(async move {
    let result = async {
      if is_due(&pool, Utc::now()).await? {
        tasks::enqueue(&pool, &SendIfDue {}).await?;
      }
      anyhow::Ok(())
    };
    if let Err(e) = result.await {
      tracing::warn!(error = %e, "checking whether to phone home failed");
    }
  });
}

/// Upstream's `prestart`: one more kick-off, on start.
pub async fn kick_off_on_start(pool: &PgPool) -> anyhow::Result<()> {
  tasks::enqueue(pool, &SendIfDue {}).await
}

pub async fn handle_job(pool: PgPool, _job: SendIfDue, settings: Arc<Settings>) -> anyhow::Result<()> {
  if !settings.phonehome.phonehome {
    return Ok(());
  }
  if !is_due(&pool, Utc::now()).await? {
    return Ok(());
  }

  // Again, holding off anyone else doing the same, before recording the attempt.
  let mut tx = pool.begin().await?;
  sqlx::query("LOCK TABLE phonehome_outboundmessage IN EXCLUSIVE MODE")
    .execute(&mut *tx)
    .await?;
  if !is_due(&mut *tx, Utc::now()).await? {
    return Ok(());
  }
  let body = message_body(&pool, &settings).await?;
  let message_id: i64 =
    sqlx::query_scalar("INSERT INTO phonehome_outboundmessage (attempted_at, message) VALUES ($1, $2) RETURNING id")
      .bind(Utc::now())
      .bind(body.to_string())
      .fetch_one(&mut *tx)
      .await?;
  tx.commit().await?;

  // Upstream's tasks fail loudly when sending fails (when not run inline); the attempt stays recorded, so the next
  // try is an hour later.
  let response = send(&settings.phonehome.phonehome_url, &body).await?;
  let remote_messages = match serde_json::from_slice(&response)
    .map_err(|e| e.to_string())
    .and_then(|r: Value| validate_remote_messages(&r))
  {
    Ok(messages) => Some(messages),
    Err(e) => {
      tracing::error!(error = %e, "Invalid phonehome response");
      None
    }
  };

  let mut tx = pool.begin().await?;
  if let Some(messages) = remote_messages {
    store_remote_messages(&mut tx, &messages).await?;
  }
  sqlx::query("UPDATE phonehome_outboundmessage SET sent_at = $2 WHERE id = $1")
    .bind(message_id)
    .bind(Utc::now())
    .execute(&mut *tx)
    .await?;
  tx.commit().await?;
  Ok(())
}

async fn send(url: &str, body: &Value) -> anyhow::Result<bytes::Bytes> {
  let client = reqwest::Client::builder()
    .connect_timeout(Duration::from_secs(5))
    .read_timeout(Duration::from_secs(5))
    .build()?;
  let response = client
    .post(url)
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(body.to_string())
    .send()
    .await?
    .error_for_status()?;
  Ok(response.bytes().await?)
}

/// `_make_message_body`.
pub async fn message_body(pool: &PgPool, settings: &Settings) -> sqlx::Result<Value> {
  let installation_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM phonehome_installation LIMIT 1")
    .fetch_one(pool)
    .await?;
  let counts = sqlx::query(
    "SELECT (SELECT count(*) FROM auth_user) AS user_count,
            (SELECT count(*) FROM auth_user WHERE is_active) AS active_user_count,
            (SELECT count(*) FROM projects_project) AS project_count,
            (SELECT count(*) FROM teams_team) AS team_count,
            current_setting('server_version_num')::int AS server_version_num",
  )
  .fetch_one(pool)
  .await?;
  let server_version_num: i32 = counts.get("server_version_num");
  let email = &settings.alerts.email;
  Ok(json!({
      "installation_id": installation_id.to_string(),
      "version": bugsink_db::UPSTREAM_VERSION,
      "python_version": null,
      "settings": {
          "BASE_URL": settings.alerts.base_url(),
          "SITE_TITLE": settings.alerts.site_title,
          "DEFAULT_FROM_EMAIL": email.default_from_email,
          "SINGLE_USER": settings.users.single_user,
          "SINGLE_TEAM": settings.users.single_team,
          "EMAIL_BACKEND": email.backend_name(),
          // jobs run on workers, never inline
          "TASK_ALWAYS_EAGER": false,
          "IS_DOCKER": std::path::Path::new("/.dockerenv").exists(),
          "DATABASE_ENGINE": "django.db.backends.postgresql",
      },
      "runtime": {
          "database_vendor": "postgresql",
          // Django's `get_database_version()` on Postgres: (major, minor)
          "database_version": [server_version_num / 10000, server_version_num % 10000],
          "machine": std::env::consts::ARCH,
      },
      "usage": {
          "user_count": counts.get::<i64, _>("user_count"),
          "active_user_count": counts.get::<i64, _>("active_user_count"),
          "project_count": counts.get::<i64, _>("project_count"),
          "team_count": counts.get::<i64, _>("team_count"),
      },
  }))
}

// --- remote messages --------------------------------------------------------------------------------------------

/// A remote message as saved. None is a JSON null, which the NOT NULL columns refuse (upstream's task fails there).
#[derive(Debug, PartialEq)]
struct RemoteMessage {
  key: Option<String>,
  applies_until_version: Option<String>,
  /// (text, url): url "" for plain text.
  parts: Vec<(Option<String>, String)>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct RemotePart {
  pub text: String,
  /// "" for plain text.
  pub url: String,
}

/// Iterating a JSON value as Python would, where only an empty string or dict gives nothing (non-empty ones give
/// strings, on which the lookups that follow fail).
fn py_list(v: &Value) -> Result<&[Value], String> {
  match v {
    Value::Array(a) => Ok(a),
    Value::String(s) if s.is_empty() => Ok(&[]),
    Value::Object(o) if o.is_empty() => Ok(&[]),
    _ => Err("not a list".into()),
  }
}

/// Django's `str()` of what a CharField/TextField is given.
fn field_str(v: &Value) -> Option<String> {
  (!v.is_null()).then(|| py_str(v))
}

/// `urlsplit(url)`: the scheme is https and there is a host.
fn is_https_with_host(url: &str) -> bool {
  let Some((scheme, rest)) = url.split_once(':') else {
    return false;
  };
  let Some(rest) = rest.strip_prefix("//") else {
    return false;
  };
  scheme.eq_ignore_ascii_case("https") && !rest.starts_with(['/', '?', '#']) && !rest.is_empty()
}

/// `_validate_remote_messages`, plus the conversions saving the rows does.
fn validate_remote_messages(response: &Value) -> Result<Vec<RemoteMessage>, String> {
  let missing = |k: &str| format!("missing {k:?}");
  let messages = py_list(response.get("messages").ok_or_else(|| missing("messages"))?)?;
  messages
    .iter()
    .map(|message| {
      let Value::Object(message) = message else {
        return Err("message is not an object".into());
      };
      let key = message.get("id").ok_or_else(|| missing("id"))?;
      let applies_until_version = message
        .get("applies_until_version")
        .ok_or_else(|| missing("applies_until_version"))?;
      if crate::py::truthy(applies_until_version) {
        let v = applies_until_version
          .as_str()
          .ok_or("applies_until_version is not a string")?;
        semver::Version::parse(v).map_err(|e| format!("applies_until_version {v:?}: {e}"))?;
      }
      let parts = py_list(message.get("parts").ok_or_else(|| missing("parts"))?)?
        .iter()
        .map(|part| {
          let Value::Object(part) = part else {
            return Err("part is not an object".into());
          };
          let text = part.get("text").ok_or_else(|| missing("text"))?;
          let url = match part.get("url") {
            None => String::new(),
            Some(Value::String(url)) if is_https_with_host(url) => url.clone(),
            Some(_) => return Err("Invalid phonehome message URL".into()),
          };
          Ok((field_str(text), url))
        })
        .collect::<Result<_, String>>()?;
      Ok(RemoteMessage {
        key: field_str(key),
        applies_until_version: field_str(applies_until_version),
        parts,
      })
    })
    .collect()
}

/// The remote messages replace the active ones; a message seen before keeps whether it was silenced.
async fn store_remote_messages(
  tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
  messages: &[RemoteMessage],
) -> sqlx::Result<()> {
  sqlx::query("UPDATE phonehome_remotemessage SET active = false")
    .execute(&mut **tx)
    .await?;
  for (position, message) in messages.iter().enumerate() {
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO phonehome_remotemessage (key, applies_until_version, position, active, silenced)
             VALUES ($1, $2, $3, true, false)
             ON CONFLICT (key) DO UPDATE
             SET applies_until_version = excluded.applies_until_version, position = excluded.position, active = true
             RETURNING id",
    )
    .bind(&message.key)
    .bind(&message.applies_until_version)
    .bind(position as i32)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM phonehome_remotemessagepart WHERE message_id = $1")
      .bind(id)
      .execute(&mut **tx)
      .await?;
    for (position, (text, url)) in message.parts.iter().enumerate() {
      sqlx::query("INSERT INTO phonehome_remotemessagepart (message_id, position, text, url) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(position as i32)
        .bind(text)
        .bind(url)
        .execute(&mut **tx)
        .await?;
    }
  }
  Ok(())
}

/// `get_remote_warnings`: for superusers, the active messages that weren't silenced and apply to this version, as
/// (key, parts).
pub async fn remote_warnings(pool: &PgPool) -> sqlx::Result<Vec<(String, Vec<RemotePart>)>> {
  let messages: Vec<(i64, String, String)> = sqlx::query_as(
    "SELECT id, key, applies_until_version FROM phonehome_remotemessage
         WHERE active AND NOT silenced ORDER BY position, id",
  )
  .fetch_all(pool)
  .await?;
  let mut warnings = Vec::new();
  for (id, key, applies_until_version) in messages {
    if !applies_to_this_version(&applies_until_version) {
      continue;
    }
    let parts = sqlx::query_as::<_, (String, String)>(
      "SELECT text, url FROM phonehome_remotemessagepart WHERE message_id = $1 ORDER BY position, id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(text, url)| RemotePart { text, url })
    .collect();
    warnings.push((key, parts));
  }
  Ok(warnings)
}

/// No version given, or this one is older (by precedence: build metadata doesn't count). A stored version that
/// doesn't parse makes upstream's pages fail; we skip the message.
fn applies_to_this_version(applies_until_version: &str) -> bool {
  if applies_until_version.is_empty() {
    return true;
  }
  let current = semver::Version::parse(bugsink_db::UPSTREAM_VERSION).expect("UPSTREAM_VERSION is a version");
  semver::Version::parse(applies_until_version)
    .is_ok_and(|until| current.cmp_precedence(&until) == std::cmp::Ordering::Less)
}

/// `silence_remote_message`.
pub async fn silence(pool: &PgPool, key: &str) -> sqlx::Result<()> {
  sqlx::query("UPDATE phonehome_remotemessage SET silenced = true WHERE key = $1")
    .bind(key)
    .execute(pool)
    .await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  #[test]
  fn validates_like_upstream() {
    let ok = json!({"messages": [
        {"id": "sec-2-6-4", "applies_until_version": "2.6.4",
         "parts": [{"text": "Security fix:"}, {"text": "upgrade", "url": "https://www.bugsink.com/x"}]},
        {"id": 7, "applies_until_version": "", "parts": [{"text": 5}, {"text": null}]},
        {"id": "x", "applies_until_version": null, "parts": ""},
    ]});
    let s = |v: &str| Some(v.to_owned());
    assert_eq!(
      validate_remote_messages(&ok).unwrap(),
      vec![
        RemoteMessage {
          key: s("sec-2-6-4"),
          applies_until_version: s("2.6.4"),
          parts: vec![
            (s("Security fix:"), "".into()),
            (s("upgrade"), "https://www.bugsink.com/x".into())
          ],
        },
        RemoteMessage {
          key: s("7"),
          applies_until_version: s(""),
          parts: vec![(s("5"), "".into()), (None, "".into())],
        },
        RemoteMessage {
          key: s("x"),
          applies_until_version: None,
          parts: vec![],
        },
      ]
    );
    assert_eq!(validate_remote_messages(&json!({"messages": ""})).unwrap(), vec![]);
    let part = |p: Value| json!({"messages": [{"id": "x", "applies_until_version": "", "parts": [p]}]});
    for bad in [
      json!({}),
      json!({"messages": "x"}),
      json!({"messages": [{"applies_until_version": "", "parts": []}]}),
      json!({"messages": [{"id": "x", "parts": []}]}),
      json!({"messages": [{"id": "x", "applies_until_version": "2.6", "parts": []}]}),
      json!({"messages": [{"id": "x", "applies_until_version": 5, "parts": []}]}),
      json!({"messages": [{"id": "x", "applies_until_version": "", "parts": "p"}]}),
      part(json!({"url": "https://a.b/"})),
      part(json!("text")),
      part(json!({"text": "t", "url": "http://a.b/"})),
      part(json!({"text": "t", "url": "https:///p"})),
      part(json!({"text": "t", "url": "https:/a.b"})),
      part(json!({"text": "t", "url": 5})),
      part(json!({"text": "t", "url": null})),
    ] {
      assert!(validate_remote_messages(&bad).is_err(), "{bad}");
    }
    assert!(validate_remote_messages(&part(json!({"text": "t", "url": "HTTPS://a.b?x"}))).is_ok());
  }

  #[test]
  fn applies_until_newer_versions() {
    assert!(applies_to_this_version(""));
    assert!(applies_to_this_version("2.6.4"));
    assert!(applies_to_this_version("3.0.0-rc.1"));
    assert!(!applies_to_this_version("2.6.3"));
    assert!(!applies_to_this_version("2.6.3+build"));
    assert!(!applies_to_this_version("2.6.3-rc.1"));
    assert!(!applies_to_this_version("2.5.0"));
    assert!(!applies_to_this_version("not a version"));
  }
}
