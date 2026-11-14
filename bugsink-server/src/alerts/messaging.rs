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
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::webhook::Failure;
use super::{Alert, AlertSettings, IssueForAlert};

pub const JOB_KIND: &str = "messaging_alert";

/// The kinds upstream 2.6.3 knows (`get_alert_service_kind_choices`).
const KINDS: &[&str] = &["discord", "google_chat", "mattermost", "msteams", "slack", "telegram", "custom"];

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

impl MessagingJob {
    /// None (logged) for what upstream's `get_backend()` / `json.loads(config)` would fail on.
    pub(super) fn new(service_config_id: i64, kind: String, config: &str, issue_id: Uuid, alert: &Alert) -> Option<Self> {
        if !KINDS.contains(&kind.as_str()) {
            tracing::warn!(service_config_id, kind, "unknown messaging service kind; not sending");
            return None;
        }
        let Ok(config) = serde_json::from_str(config) else {
            tracing::warn!(service_config_id, "messaging service config is not valid JSON; not sending");
            return None;
        };
        Some(Self { service_config_id, kind, config, issue_id, alert: alert.clone() })
    }
}

/// What a backend needs to build its message.
pub(super) struct AlertMessage<'a> {
    pub issue: &'a IssueForAlert,
    pub alert: &'a Alert,
    pub settings: &'a AlertSettings,
    pub config: &'a Value,
}

impl AlertMessage<'_> {
    fn config_str(&self, key: &str) -> Result<&str, Failure> {
        self.config.get(key).and_then(Value::as_str).ok_or_else(|| Failure {
            error_type: "KeyError",
            message: format!("'{key}'"),
            response: None,
        })
    }
}

/// Job handler for `JOB_KIND`. Never retried: a failure is recorded on the service instead.
pub async fn handle_job(pool: PgPool, payload: Value, settings: Arc<AlertSettings>) -> anyhow::Result<()> {
    let job: MessagingJob = serde_json::from_value(payload)?;
    let Some(issue) = IssueForAlert::load(&pool, job.issue_id).await? else {
        return Ok(()); // deleted since
    };
    let message = AlertMessage { issue: &issue, alert: &job.alert, settings: &settings, config: &job.config };
    let result = match job.kind.as_str() {
        "discord" => discord::send_alert(&message).await,
        "google_chat" => google_chat::send_alert(&message).await,
        "mattermost" => mattermost::send_alert(&message).await,
        "msteams" => msteams::send_alert(&message).await,
        "slack" => slack::send_alert(&message).await,
        "telegram" => telegram::send_alert(&message).await,
        "custom" => match custom::IssueData::load(&pool, job.issue_id).await? {
            Some(data) => custom::send_alert(&message, &data).await,
            None => return Ok(()), // deleted since
        },
        other => {
            tracing::warn!(kind = other, "unknown messaging service kind; not sending");
            return Ok(());
        }
    };
    if let Err(failure) = &result {
        tracing::warn!(service_config_id = job.service_config_id, error = %failure.message, "messaging alert failed");
    }
    store_outcome(&pool, job.service_config_id, result.err()).await?;
    Ok(())
}

pub const TEST_JOB_KIND: &str = "messaging_test";

/// "Send test message" (`send_test_message`): what upstream passes its task, as of when the button was pressed.
#[derive(Debug, Serialize, Deserialize)]
pub struct TestJob {
    pub service_config_id: i64,
    pub kind: String,
    pub config: Value,
    pub project_name: String,
    pub display_name: String,
}

/// Job handler for `TEST_JOB_KIND`; like alerts, the outcome is stored on the service.
pub async fn handle_test_job(pool: PgPool, payload: Value, settings: Arc<AlertSettings>) -> anyhow::Result<()> {
    let job: TestJob = serde_json::from_value(payload)?;
    let (project, display) = (job.project_name.as_str(), job.display_name.as_str());
    let (url, data, bot_token) = match job.kind.as_str() {
        "telegram" => match telegram::test_request(&job.config, project, display) {
            Ok((url, data, bot_token)) => (url, data, Some(bot_token)),
            Err(failure) => return Ok(store_outcome(&pool, job.service_config_id, Some(failure)).await?),
        },
        kind => {
            let Some(url) = job.config.get("webhook_url").and_then(Value::as_str) else {
                let failure = Failure { error_type: "KeyError", message: "'webhook_url'".into(), response: None };
                return Ok(store_outcome(&pool, job.service_config_id, Some(failure)).await?);
            };
            let data = match kind {
                "slack" => slack::test_payload(project, display),
                "discord" => discord::test_payload(project, display),
                "google_chat" => google_chat::test_payload(project, display),
                "mattermost" => mattermost::test_payload(project, display, job.config.get("channel")),
                "msteams" => msteams::test_payload(project, display),
                "custom" => custom::test_payload(project),
                other => {
                    tracing::warn!(kind = other, "unknown messaging service kind; not sending");
                    return Ok(());
                }
            };
            (url.to_owned(), data, None)
        }
    };
    let result = super::webhook::post_json(&settings.webhooks, &url, crate::grouping::py::json_dumps(&data))
        .await
        .map_err(|f| match &bot_token {
            Some(token) => telegram::masked(f, token),
            None => f,
        });
    store_outcome(&pool, job.service_config_id, result.err()).await?;
    Ok(())
}

/// `_store_success_info` / `_store_failure_info`.
async fn store_outcome(pool: &PgPool, service_config_id: i64, failure: Option<Failure>) -> sqlx::Result<()> {
    let (status, text, is_json) = match failure.as_ref().and_then(|f| f.response.as_ref()) {
        Some((status, text)) => (
            Some(i32::from(*status)),
            Some(text.chars().take(2000).collect::<String>()),
            Some(serde_json::from_str::<Value>(text).is_ok()),
        ),
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
    let alert = Alert::Unmute { unmute_reason: "Unmuted *because* <x> & _y_ [z] #1 - 'q' \"d\" ~`\\".into() };
    (issue, alert, args.settings, config)
}
