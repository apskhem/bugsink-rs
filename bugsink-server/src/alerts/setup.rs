//! Setting up a project's messaging services, as upstream Bugsink 2.6.3's `project_alerts_setup` and
//! `project_messaging_service_add/_edit` views with each service's config form (`get_form_class`): Django's
//! `URLField` and `URLValidator`, then `validate_webhook_url` (the same checks as sending, incl. resolving the host),
//! and Telegram's chat id rules. The config is stored as upstream's `json.dumps(get_config())`.

use std::sync::OnceLock;

use bugsink_db::PgPool;
use fancy_regex::Regex;
use serde::{
  Deserialize,
  Serialize,
};
use serde_json::{
  Map,
  Value,
  json,
};

use super::WebhookSettings;
use crate::forms::{
  FieldErrors,
  REQUIRED,
  char_field,
};
use crate::py::{
  json_dumps,
  strip,
};

/// `get_alert_service_kind_choices`: (kind, label).
pub const KINDS: [(&str, &str); 7] = [
  ("discord", "Discord"),
  ("google_chat", "Google Chat"),
  ("mattermost", "Mattermost"),
  ("msteams", "Microsoft Teams"),
  ("slack", "Slack"),
  ("telegram", "Telegram"),
  ("custom", "Custom"),
];

/// A form field's value from JSON: strings as they are, numbers as Django would see them posted.
fn text(value: Option<&Value>) -> String {
  match value {
    None | Some(Value::Null) => String::new(),
    Some(Value::String(s)) => s.clone(),
    Some(other) => other.to_string(),
  }
}

/// `forms.URLField(assume_scheme="https", max_length)`: the cleaned URL, or every validator's message.
pub fn clean_url(value: Option<&Value>, max_length: Option<usize>) -> Result<String, Vec<String>> {
  let mut value = strip(&text(value)).to_owned();
  if value.is_empty() {
    return Err(vec![REQUIRED.into()]);
  }
  // `to_python`: no valid scheme, assume https
  let (scheme, has_colon) = match value.split_once(':') {
    Some((scheme, _)) => (scheme, true),
    None => ("", false),
  };
  let first = scheme.chars().next();
  if !has_colon || scheme.is_empty() || !first.is_some_and(|c| c.is_ascii_alphabetic()) || scheme.contains('/') {
    value = if value.starts_with("//") {
      format!("https:{value}")
    } else {
      format!("https://{value}")
    };
  }
  let mut errors = Vec::new();
  if !url_is_valid(&value) {
    errors.push("Enter a valid URL.".into());
  }
  let len = value.chars().count();
  if let Some(max) = max_length.filter(|max| len > *max) {
    errors.push(format!(
      "Ensure this value has at most {max} characters (it has {len})."
    ));
  }
  if errors.is_empty() { Ok(value) } else { Err(errors) }
}

/// `URLValidator` (http, https, ftp, ftps).
fn url_is_valid(value: &str) -> bool {
  if value.chars().count() > 2048 || value.contains(['\t', '\r', '\n']) {
    return false;
  }
  let scheme = value.split("://").next().unwrap_or("").to_lowercase();
  if !["http", "https", "ftp", "ftps"].contains(&scheme.as_str()) {
    return false;
  }
  let Some(netloc) = urlsplit_netloc(value) else {
    return false;
  };
  static URL: OnceLock<Regex> = OnceLock::new();
  let url_re = URL.get_or_init(|| {
    let ul = "\u{a1}-\u{ffff}";
    let octet = r"(?:0|25[0-5]|2[0-4][0-9]|1[0-9]?[0-9]?|[1-9][0-9]?)";
    let ipv4 = format!(r"{octet}(?:\.{octet}){{3}}");
    let ipv6 = r"\[[0-9a-f:.]+\]";
    let hostname = format!(r"[a-z{ul}0-9](?:[a-z{ul}0-9-]{{0,61}}[a-z{ul}0-9])?");
    let domain = format!(r"(?:\.(?!-)[a-z{ul}0-9-]{{1,63}}(?<!-))*");
    let tld = format!(r"\.(?!-)(?:[a-z{ul}-]{{2,63}}|xn--[a-z0-9]{{1,59}})(?<!-)\.?");
    Regex::new(&format!(
      concat!(
        r"(?i)^(?:[a-z0-9.+-]*)://(?:[^\s:@/]+(?::[^\s:@/]*)?@)?",
        r"(?:{ipv4}|{ipv6}|({hostname}{domain}{tld}|localhost))(?::[0-9]{{1,5}})?(?:[/?#][^\s]*)?\z",
      ),
      ipv4 = ipv4,
      ipv6 = ipv6,
      hostname = hostname,
      domain = domain,
      tld = tld
    ))
    .expect("valid")
  });
  if !url_re.is_match(value).unwrap_or(false) {
    return false;
  }
  static BRACKETED: OnceLock<Regex> = OnceLock::new();
  let bracketed = BRACKETED.get_or_init(|| Regex::new(r"^\[(.+)\](?::[0-9]{1,5})?$").expect("valid"));
  if let Ok(Some(c)) = bracketed.captures(&netloc)
    && c[1].parse::<std::net::Ipv6Addr>().is_err()
  {
    return false;
  }
  hostname(&netloc).is_some_and(|h| h.chars().count() <= 253)
}

/// Python's `urlsplit(value).netloc` for a value with a scheme; None where it raises.
fn urlsplit_netloc(value: &str) -> Option<String> {
  let value: String = value
    .trim_start_matches(|c: char| c <= ' ')
    .chars()
    .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
    .collect();
  let rest = match value.find(':') {
    Some(i)
      if i > 0
        && value.starts_with(|c: char| c.is_ascii_alphabetic())
        && value[..i]
          .chars()
          .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) =>
    {
      &value[i + 1..]
    }
    _ => value.as_str(),
  };
  let Some(after) = rest.strip_prefix("//") else {
    return Some(String::new());
  };
  let end = after.find(['/', '?', '#']).unwrap_or(after.len());
  let netloc = &after[..end];
  let (open, close) = (netloc.contains('['), netloc.contains(']'));
  if open != close {
    return None; // "Invalid IPv6 URL"
  }
  if open {
    let host_and_port = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let (before, bracketed) = host_and_port.split_once('[')?;
    if !before.is_empty() {
      return None;
    }
    let (host, port) = bracketed.split_once(']').unwrap_or((bracketed, ""));
    if !port.is_empty() && !port.starts_with(':') {
      return None;
    }
    static IPV_FUTURE: OnceLock<Regex> = OnceLock::new();
    let valid = if host.starts_with('v') {
      let ipv_future = IPV_FUTURE.get_or_init(|| Regex::new(r"\Av[a-fA-F0-9]+\..+\z").expect("valid"));
      ipv_future.is_match(host).unwrap_or(false)
    } else {
      host.parse::<std::net::Ipv6Addr>().is_ok()
    };
    if !valid {
      return None;
    }
  }
  // `_checknetloc`: characters that NFKC-normalize into URL delimiters
  if !netloc.is_ascii() {
    use unicode_normalization::UnicodeNormalization;
    let n: String = netloc.chars().filter(|c| !"@:#?".contains(*c)).collect();
    let normalized: String = n.nfkc().collect();
    if n != normalized && normalized.contains(['/', '?', '#', '@', ':']) {
      return None;
    }
  }
  Some(netloc.to_owned())
}

/// `SplitResult.hostname`: after the userinfo, without brackets and port, lower case; None if empty.
fn hostname(netloc: &str) -> Option<String> {
  let info = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
  let host = match info.split_once('[') {
    Some((_, bracketed)) => bracketed.split_once(']').map_or(bracketed, |(h, _)| h),
    None => info.split_once(':').map_or(info, |(h, _)| h),
  };
  if host.is_empty() {
    return None;
  }
  let (h, zone) = host.split_once('%').map_or((host, None), |(h, z)| (h, Some(z)));
  Some(match zone {
    Some(zone) => format!("{}%{zone}", h.to_lowercase()),
    None => h.to_lowercase(),
  })
}

/// The service's config form (`get_form_class()`): the config to store, or the form's errors.
pub async fn clean_config(
  webhooks: &WebhookSettings,
  kind: &str,
  data: &Map<String, Value>,
) -> Result<Value, FieldErrors> {
  let mut errors = FieldErrors::new();
  let webhook_url = |max| {
    async move {
      match clean_url(data.get("webhook_url"), max) {
        Err(messages) => Err(messages),
        Ok(url) => {
          match super::webhook::validate_webhook_url(webhooks, &url).await {
            Ok(()) => Ok(url),
            Err(message) => Err(vec![message]),
          }
        }
      }
    }
  };
  let config = match kind {
    "slack" | "discord" | "google_chat" | "custom" | "msteams" => {
      // Teams workflow URLs are long; the others have no limit but URLValidator's (2048)
      let max = (kind == "msteams").then_some(1000);
      match webhook_url(max).await {
        Ok(url) => json!({"webhook_url": url}),
        Err(messages) => {
          errors.insert("webhook_url", messages);
          Value::Null
        }
      }
    }
    "mattermost" => {
      let url = webhook_url(None).await;
      let channel = strip(&text(data.get("channel"))).to_owned();
      match url {
        Ok(url) => json!({"webhook_url": url, "channel": channel}),
        Err(messages) => {
          errors.insert("webhook_url", messages);
          Value::Null
        }
      }
    }
    "telegram" => {
      let bot_token = strip(&text(data.get("bot_token"))).to_owned();
      if bot_token.is_empty() {
        errors.insert("bot_token", vec![REQUIRED.into()]);
      } else if let Err(message) = super::webhook::validate_webhook_url(
        webhooks,
        &format!("https://api.telegram.org/bot{bot_token}/sendMessage"),
      )
      .await
      {
        errors.insert("bot_token", vec![message]);
      }
      let chat_id = strip(&text(data.get("chat_id"))).to_owned();
      if chat_id.is_empty() {
        errors.insert("chat_id", vec![REQUIRED.into()]);
      } else if let Some(name) = chat_id.strip_prefix('@') {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
          errors.insert(
            "chat_id",
            vec!["Channel username must be in the format @channelusername.".into()],
          );
        }
      } else {
        let digits = chat_id.strip_prefix('-').unwrap_or(&chat_id);
        // Python's \d: any Unicode decimal digit
        if digits.is_empty() || !digits.chars().all(|c| c.is_numeric()) {
          errors.insert(
            "chat_id",
            vec!["Chat ID must be a numeric chat ID or @channelusername.".into()],
          );
        }
      }
      // `IntegerField(required=False, min_value=1)`
      let thread = strip(&text(data.get("message_thread_id"))).to_owned();
      let message_thread_id = if thread.is_empty() {
        None
      } else {
        match thread.parse::<i64>() {
          Err(_) => {
            errors.insert("message_thread_id", vec!["Enter a whole number.".into()]);
            None
          }
          Ok(n) if n < 1 => {
            let message = "Ensure this value is greater than or equal to 1.";
            errors.insert("message_thread_id", vec![message.into()]);
            None
          }
          Ok(n) => Some(n),
        }
      };
      // `clean`: topics only with numeric chat ids (a chat id that failed its own checks isn't considered)
      if message_thread_id.is_some() && !errors.contains_key("chat_id") && chat_id.starts_with('@') {
        errors.insert(
          "message_thread_id",
          vec!["Topic ID can only be used with numeric chat IDs.".into()],
        );
      }
      json!({
          "bot_token": bot_token,
          "chat_id": chat_id,
          "message_thread_id": message_thread_id,
      })
    }
    _ => Value::Null,
  };
  if errors.is_empty() { Ok(config) } else { Err(errors) }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Service {
  pub id: i64,
  pub display_name: String,
  pub kind: String,
  /// The service's config, e.g. `{"webhook_url": ...}`.
  #[sqlx(json)]
  pub config: Value,
  pub last_failure_timestamp: Option<chrono::DateTime<chrono::Utc>>,
  pub last_failure_status_code: Option<i32>,
  pub last_failure_response_text: Option<String>,
  pub last_failure_is_json: Option<bool>,
  pub last_failure_error_type: Option<String>,
  pub last_failure_error_message: Option<String>,
}

pub async fn services(pool: &PgPool, project_id: i64) -> sqlx::Result<Vec<Service>> {
  sqlx::query_as(
    "SELECT id, display_name, kind, config::json AS config, last_failure_timestamp, last_failure_status_code,
                last_failure_response_text, last_failure_is_json, last_failure_error_type, last_failure_error_message
         FROM alerts_messagingserviceconfig WHERE project_id = $1 ORDER BY id",
  )
  .bind(project_id)
  .fetch_all(pool)
  .await
}

#[derive(Debug, Deserialize)]
pub struct ServiceForm {
  pub display_name: Option<String>,
  /// On creation only.
  pub kind: Option<String>,
  /// The kind's config form's fields.
  #[serde(default)]
  pub config: Map<String, Value>,
}

/// `MessagingServiceConfigNewForm`/`EditForm` (`display_name`, `kind`) and then the kind's config form.
async fn clean(
  webhooks: &WebhookSettings,
  form: &ServiceForm,
  kind: &str,
  creating: bool,
) -> Result<(String, Value), FieldErrors> {
  let mut errors = FieldErrors::new();
  let display_name = char_field(&mut errors, "display_name", form.display_name.as_deref(), 100);
  if creating && !KINDS.iter().any(|(k, _)| *k == kind) {
    // upstream fails on this (a 500)
    errors.insert(
      "kind",
      vec![format!(
        "Select a valid choice. {kind} is not one of the available choices."
      )],
    );
    return Err(errors);
  }
  // the page shows the config form's errors too, whether the rest is valid or not
  match clean_config(webhooks, kind, &form.config).await {
    Ok(config) if errors.is_empty() => Ok((display_name, config)),
    Ok(_) => Err(errors),
    Err(config_errors) => {
      errors.extend(config_errors);
      Err(errors)
    }
  }
}

pub async fn add_service(
  pool: &PgPool,
  webhooks: &WebhookSettings,
  project_id: i64,
  form: &ServiceForm,
) -> sqlx::Result<Result<i64, FieldErrors>> {
  // the form's default kind
  let kind = form
    .kind
    .clone()
    .filter(|k| !k.is_empty())
    .unwrap_or_else(|| "slack".into());
  let (display_name, config) = match clean(webhooks, form, &kind, true).await {
    Ok(cleaned) => cleaned,
    Err(errors) => return Ok(Err(errors)),
  };
  let id = sqlx::query_scalar(
    "INSERT INTO alerts_messagingserviceconfig (display_name, kind, config, project_id) VALUES ($1, $2, $3, $4)
         RETURNING id",
  )
  .bind(display_name)
  .bind(kind)
  .bind(json_dumps(&config))
  .bind(project_id)
  .fetch_one(pool)
  .await?;
  Ok(Ok(id))
}

/// The kind can't be changed. None: no such service in this project.
pub async fn edit_service(
  pool: &PgPool,
  webhooks: &WebhookSettings,
  project_id: i64,
  id: i64,
  form: &ServiceForm,
) -> sqlx::Result<Option<Result<(), FieldErrors>>> {
  let Some(kind): Option<String> =
    sqlx::query_scalar("SELECT kind FROM alerts_messagingserviceconfig WHERE id = $1 AND project_id = $2")
      .bind(id)
      .bind(project_id)
      .fetch_optional(pool)
      .await?
  else {
    return Ok(None);
  };
  let (display_name, config) = match clean(webhooks, form, &kind, false).await {
    Ok(cleaned) => cleaned,
    Err(errors) => return Ok(Some(Err(errors))),
  };
  sqlx::query("UPDATE alerts_messagingserviceconfig SET display_name = $2, config = $3 WHERE id = $1")
    .bind(id)
    .bind(display_name)
    .bind(json_dumps(&config))
    .execute(pool)
    .await?;
  Ok(Some(Ok(())))
}

pub async fn remove_service(pool: &PgPool, project_id: i64, id: i64) -> sqlx::Result<()> {
  sqlx::query("DELETE FROM alerts_messagingserviceconfig WHERE id = $1 AND project_id = $2")
    .bind(id)
    .bind(project_id)
    .execute(pool)
    .await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn url(s: &str) -> Result<String, Vec<String>> {
    clean_url(Some(&json!(s)), None)
  }

  #[test]
  fn urls_like_django() {
    assert_eq!(url(" hooks.slack.com/x "), Ok("https://hooks.slack.com/x".into()));
    assert_eq!(url("//example.org/x"), Ok("https://example.org/x".into()));
    assert_eq!(
      url("http://127.0.0.1:8025/slack"),
      Ok("http://127.0.0.1:8025/slack".into())
    );
    assert_eq!(url("http://[::1]:80/"), Ok("http://[::1]:80/".into()));
    assert_eq!(url("http://localhost/x"), Ok("http://localhost/x".into()));
    let invalid = [
      "http://",
      "http://-x.com",
      "http://x",
      "http://[::g]/",
      "gopher://x.com",
      "http://a b.com",
      "http://[::1",
    ];
    for bad in invalid {
      assert_eq!(url(bad), Err(vec!["Enter a valid URL.".to_string()]), "{bad}");
    }
    assert_eq!(url(""), Err(vec![REQUIRED.to_string()]));
  }
}
