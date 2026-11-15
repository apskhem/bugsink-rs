//! Alerts through a project's messaging services (`alerts_messagingserviceconfig`): one job per service, so a slow or
//! failing service doesn't hold up the others. Each `kind` has a module building upstream's exact payload
//! (`alerts/service_backends/<kind>.py`, `<kind>_backend_send_alert`). The outcome of each send is stored on the
//! service's row (`last_failure_*`) for the UI.

mod custom;
mod discord;
mod google_chat;
mod mattermost;
mod msteams;
mod slack;
mod telegram;

use std::sync::Arc;

use bugsink_db::PgPool;
use chrono::Utc;
use serde::{
  Deserialize,
  Serialize,
};
use serde_json::Value;
use uuid::Uuid;

use super::webhook::{
  Failure,
  WebhookSettings,
  post_json,
};
use super::{
  Alert,
  AlertSettings,
  IssueForAlert,
};
use crate::py::{
  json_dumps,
  take_chars,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct MessagingJob {
  pub service_config_id: i64,
  pub kind: String,
  /// The service's `config` column (JSON), as it was when the alert fanned out (upstream passes it as task args).
  pub config: Value,
  pub issue_id: Uuid,
  #[serde(flatten)]
  pub alert: Alert,
}

impl crate::tasks::Job for MessagingJob {
  const KIND: &str = "messaging_alert";
}

impl MessagingJob {
  /// None (logged) for what upstream's `get_backend()` / `json.loads(config)` would fail on.
  pub(super) fn new(service_config_id: i64, kind: String, config: &str, issue_id: Uuid, alert: &Alert) -> Option<Self> {
    if !super::setup::KINDS.iter().any(|(k, _)| *k == kind) {
      tracing::warn!(service_config_id, kind, "unknown messaging service kind; not sending");
      return None;
    }
    let Ok(config) = serde_json::from_str(config) else {
      tracing::warn!(
        service_config_id,
        "messaging service config is not valid JSON; not sending"
      );
      return None;
    };
    Some(Self {
      service_config_id,
      kind,
      config,
      issue_id,
      alert: alert.clone(),
    })
  }
}

/// What a backend needs to build its message.
pub(super) struct AlertMessage<'a> {
  pub issue: &'a IssueForAlert,
  pub alert: &'a Alert,
  pub settings: &'a AlertSettings,
  pub config: &'a Value,
}

/// What a backend posts where: the service's `webhook_url`, or Telegram's API with the bot token in the URL (masked in
/// failures).
struct Request {
  url: String,
  data: Value,
  bot_token: Option<String>,
}

impl Request {
  fn webhook(config: &Value, data: Value) -> Result<Self, Failure> {
    Ok(Self {
      url: config_str(config, "webhook_url")?.to_owned(),
      data,
      bot_token: None,
    })
  }

  async fn send(self, webhooks: &WebhookSettings) -> Result<(), Failure> {
    let result = post_json(webhooks, &self.url, json_dumps(&self.data)).await;
    result.map_err(|failure| {
      match &self.bot_token {
        Some(bot_token) => telegram::mask_token(failure, bot_token),
        None => failure,
      }
    })
  }
}

/// `config[key]`, a string.
fn config_str<'c>(config: &'c Value, key: &str) -> Result<&'c str, Failure> {
  config
    .get(key)
    .and_then(Value::as_str)
    .ok_or_else(|| Failure::key_error(key))
}

/// Job handler. Never retried: a failure is recorded on the service instead.
pub async fn handle_job(pool: PgPool, job: MessagingJob, settings: Arc<AlertSettings>) -> anyhow::Result<()> {
  let Some(issue) = IssueForAlert::load(&pool, job.issue_id).await? else {
    return Ok(()); // deleted since
  };
  let m = AlertMessage {
    issue: &issue,
    alert: &job.alert,
    settings: &settings,
    config: &job.config,
  };
  let request = match job.kind.as_str() {
    "telegram" => telegram::alert_request(&m),
    "custom" => {
      match custom::IssueData::load(&pool, job.issue_id).await? {
        Some(data) => Request::webhook(&job.config, custom::payload(&m, &data)),
        None => return Ok(()), // deleted since
      }
    }
    kind => {
      let data = match kind {
        "discord" => discord::payload(&m),
        "google_chat" => google_chat::payload(&m),
        "mattermost" => mattermost::payload(&m),
        "msteams" => msteams::payload(&m),
        "slack" => slack::payload(&m),
        other => {
          tracing::warn!(kind = other, "unknown messaging service kind; not sending");
          return Ok(());
        }
      };
      Request::webhook(&job.config, data)
    }
  };
  let result = match request {
    Ok(request) => request.send(&settings.webhooks).await,
    Err(failure) => Err(failure),
  };
  if let Err(failure) = &result {
    tracing::warn!(service_config_id = job.service_config_id, error = %failure.message, "messaging alert failed");
  }
  store_outcome(&pool, job.service_config_id, result.err()).await?;
  Ok(())
}

/// "Send test message" (`send_test_message`): what upstream passes its task, as of when the button was pressed.
#[derive(Debug, Serialize, Deserialize)]
pub struct TestJob {
  pub service_config_id: i64,
  pub kind: String,
  pub config: Value,
  pub project_name: String,
  pub display_name: String,
}

impl crate::tasks::Job for TestJob {
  const KIND: &str = "messaging_test";
}

/// Job handler; like alerts, the outcome is stored on the service.
pub async fn handle_test_job(pool: PgPool, job: TestJob, settings: Arc<AlertSettings>) -> anyhow::Result<()> {
  let (project, display) = (job.project_name.as_str(), job.display_name.as_str());
  let data = match job.kind.as_str() {
    "slack" => Some(slack::test_payload(project, display)),
    "discord" => Some(discord::test_payload(project, display)),
    "google_chat" => Some(google_chat::test_payload(project, display)),
    "mattermost" => Some(mattermost::test_payload(project, display, job.config.get("channel"))),
    "msteams" => Some(msteams::test_payload(project, display)),
    "custom" => Some(custom::test_payload(project)),
    _ => None,
  };
  let request = match (job.kind.as_str(), data) {
    ("telegram", _) => telegram::test_request(&job.config, project, display),
    (_, Some(data)) => Request::webhook(&job.config, data),
    // an unknown kind: a missing URL is still the failure stored
    (kind, None) => {
      match config_str(&job.config, "webhook_url") {
        Ok(_) => {
          tracing::warn!(kind, "unknown messaging service kind; not sending");
          return Ok(());
        }
        Err(failure) => Err(failure),
      }
    }
  };
  let result = match request {
    Ok(request) => request.send(&settings.webhooks).await,
    Err(failure) => Err(failure),
  };
  store_outcome(&pool, job.service_config_id, result.err()).await?;
  Ok(())
}

/// `_store_success_info` / `_store_failure_info`.
async fn store_outcome(pool: &PgPool, service_config_id: i64, failure: Option<Failure>) -> sqlx::Result<()> {
  let (status, text, is_json) = match failure.as_ref().and_then(|f| f.response.as_ref()) {
    Some((status, text)) => {
      (
        Some(i32::from(*status)),
        Some(take_chars(text, 2000)),
        Some(serde_json::from_str::<Value>(text).is_ok()),
      )
    }
    None => (None, None, None),
  };
  sqlx::query(
    "UPDATE alerts_messagingserviceconfig SET last_failure_timestamp = $2, last_failure_error_type = $3,
             last_failure_error_message = $4, last_failure_status_code = $5, last_failure_response_text = $6,
             last_failure_is_json = $7
         WHERE id = $1",
  )
  .bind(service_config_id)
  .bind(failure.as_ref().map(|_| Utc::now()))
  .bind(failure.as_ref().map(|f| f.error_type))
  .bind(failure.as_ref().map(|f| f.message.as_str()))
  .bind(status)
  .bind(text)
  .bind(is_json)
  .execute(pool)
  .await?;
  Ok(())
}

/// The parts of an `AlertMessage` for backend tests: an unmute alert for an issue whose title, project name and
/// unmute reason contain the characters the backends escape.
#[cfg(test)]
fn test_parts(config: Value) -> (IssueForAlert, Alert, AlertSettings, Value) {
  #[derive(clap::Parser)]
  struct Args {
    #[command(flatten)]
    settings: AlertSettings,
  }
  let args = <Args as clap::Parser>::try_parse_from(["bugsink", "--base-url", "https://bugsink.example.com/"]).unwrap();
  let issue = IssueForAlert {
    id: "8a3cc2c0-1111-4222-8333-444455556666".parse().unwrap(),
    title: "Value_Error<T>: bad *input* & \"quoted\" 'x' | [a]-#1 ~`\\ é ✓ 🐛".into(),
    project_id: 7,
    project_name: "My_Project <web> & *api* [x]-#1 ~`\\".into(),
  };
  let alert = Alert::Unmute {
    unmute_reason: "Unmuted *because* <x> & _y_ [z] #1 - 'q' \"d\" ~`\\".into(),
  };
  (issue, alert, args.settings, config)
}
