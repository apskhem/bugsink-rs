//! Alerts on new issues, regressions and unmutes, by email to project members and through the project's messaging
//! services. A port of upstream Bugsink 2.6.3's `alerts/` (`tasks.py`, `service_backends/`) and the email plumbing it
//! uses (`bugsink/utils.py::send_rendered_email`, `phonehome/models.py::Installation`).
//!
//! Digest enqueues an `alert` job in its own transaction (upstream: on commit), if the project has that kind of alert
//! switched on. The alert job enqueues one `messaging_alert` job per messaging service and sends the emails itself.
//! As with upstream's tasks, nothing is retried once something may have been sent: failures are logged, and for
//! messaging services also stored on the service's row.
//!
//! SKIPPED(parity) (optional extras, see the README): `TIME_ZONE` (dates in unmute reasons are UTC).

pub(crate) mod email;
pub mod messaging;
pub mod setup;
pub(crate) mod template;
mod webhook;

use std::sync::Arc;

use bugsink_db::{PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::grouping::py::{py_str, splitlines};
use crate::tasks;

pub use email::EmailSettings;
pub use webhook::WebhookSettings;

pub const JOB_KIND: &str = "alert";

/// Settings for alerts, from the same environment variables as upstream's Docker image.
#[derive(clap::Args, Clone, Debug)]
pub struct AlertSettings {
    /// Where the UI is reachable; links in alerts (and DSNs) are built from it.
    #[arg(long, env = "BASE_URL", default_value = "http://localhost:8000")]
    pub base_url: String,
    #[arg(long, env = "SITE_TITLE", default_value = "Bugsink")]
    pub site_title: String,
    #[command(flatten)]
    pub email: EmailSettings,
    #[command(flatten)]
    pub webhooks: WebhookSettings,
}

impl AlertSettings {
    /// Without a trailing slash (upstream's `_sanitize`).
    pub fn base_url(&self) -> &str {
        self.base_url.strip_suffix('/').unwrap_or(&self.base_url)
    }
}

/// Upstream's boolean environment variables: only "true", "1" and "yes" (any case) are true.
pub(crate) fn py_bool(s: &str) -> Result<bool, String> {
    Ok(["true", "1", "yes"].contains(&s.to_lowercase().as_str()))
}

/// `send_new_issue_alert`, `send_regression_alert`, `send_unmute_alert`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "alert", rename_all = "snake_case")]
pub enum Alert {
    NewIssue,
    Regression,
    Unmute { unmute_reason: String },
}

impl Alert {
    fn state_description(&self) -> &'static str {
        match self {
            Self::NewIssue => "New issue",
            Self::Regression => "Regression",
            Self::Unmute { .. } => "Unmuted issue",
        }
    }

    fn article(&self) -> &'static str {
        match self {
            Self::Unmute { .. } => "an",
            _ => "a",
        }
    }

    fn reason(&self) -> &'static str {
        match self {
            Self::NewIssue => "NEW",
            Self::Regression => "REGRESSED",
            Self::Unmute { .. } => "UNMUTED",
        }
    }

    fn unmute_reason(&self) -> Option<&str> {
        match self {
            Self::Unmute { unmute_reason } => Some(unmute_reason),
            _ => None,
        }
    }
}

/// `format_unmute_reason` for a time-based unmute (`mute_for`); Django's `date` filter with `'j M G:i'`, in UTC.
pub(crate) fn unmute_reason_mute_for(unmute_after: DateTime<Utc>) -> String {
    format!(
        "An event was observed after the mute-deadline of {} and the issue was unmuted.",
        unmute_after.format("%-d %b %-H:%M")
    )
}

/// `format_unmute_reason` for a volume-based unmute (`mute_until`, `{"period", "nr_of_periods", "volume"}`).
pub(crate) fn unmute_reason_mute_until(condition: &Value) -> String {
    let nr_of_periods = &condition["nr_of_periods"];
    let plural_s = if nr_of_periods.as_f64() == Some(1.0) { "" } else { "s" };
    format!(
        "More than {} events per {} {}{plural_s} occurred, unmuting the issue.",
        py_str(&condition["volume"]),
        py_str(nr_of_periods),
        py_str(&condition["period"]),
    )
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AlertJob {
    pub issue_id: Uuid,
    #[serde(flatten)]
    pub alert: Alert,
}

pub(crate) async fn enqueue(tx: &mut Transaction<'_, Postgres>, issue_id: Uuid, alert: Alert) -> anyhow::Result<()> {
    tasks::enqueue(&mut **tx, JOB_KIND, &AlertJob { issue_id, alert }).await
}

/// The issue as alerts show it, read when the alert is sent (like upstream).
pub(crate) struct IssueForAlert {
    id: Uuid,
    title: String,
    project_id: i64,
    project_name: String,
}

impl IssueForAlert {
    async fn load(pool: &PgPool, issue_id: Uuid) -> sqlx::Result<Option<Self>> {
        let row = sqlx::query(
            "SELECT i.calculated_type, i.calculated_value, p.id AS project_id, p.name AS project_name
             FROM issues_issue i JOIN projects_project p ON p.id = i.project_id WHERE i.id = $1",
        )
        .bind(issue_id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|r| Self {
            id: issue_id,
            title: issue_title(r.get("calculated_type"), r.get("calculated_value")),
            project_id: r.get("project_id"),
            project_name: r.get("project_name"),
        }))
    }

    /// `BASE_URL + issue.get_absolute_url()`.
    fn url(&self, settings: &AlertSettings) -> String {
        format!("{}/issues/issue/{}/event/last/", settings.base_url(), self.id)
    }
}

/// `Issue.title()` (`get_title_for_exception_type_and_value`).
fn issue_title(ty: &str, value: &str) -> String {
    if value.is_empty() {
        return ty.to_string();
    }
    let first_line = splitlines(value).first().copied().unwrap_or("");
    if ty == "Log Message" { first_line.to_string() } else { format!("{ty}: {first_line}") }
}

/// Job handler for `JOB_KIND` (`_send_alert`).
pub async fn handle_job(pool: PgPool, payload: Value, settings: Arc<AlertSettings>) -> anyhow::Result<()> {
    let job: AlertJob = serde_json::from_value(payload)?;
    let Some(issue) = IssueForAlert::load(&pool, job.issue_id).await? else {
        return Ok(()); // deleted since
    };

    let services = sqlx::query(
        "SELECT id, kind, config FROM alerts_messagingserviceconfig WHERE project_id = $1 ORDER BY id",
    )
    .bind(issue.project_id)
    .fetch_all(&pool)
    .await?;
    let mut tx = pool.begin().await?;
    for service in services {
        if let Some(messaging_job) =
            messaging::MessagingJob::new(service.get("id"), service.get("kind"), service.get("config"), issue.id, &job.alert)
        {
            tasks::enqueue(&mut *tx, messaging::JOB_KIND, &messaging_job).await?;
        }
    }
    tx.commit().await?;

    // From here on, failures are logged rather than retried: retrying could send messages twice.
    email::send_alert_emails(&pool, &settings, &issue, &job.alert).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn titles() {
        assert_eq!(issue_title("ValueError", ""), "ValueError");
        assert_eq!(issue_title("ValueError", "bad\nmore"), "ValueError: bad");
        assert_eq!(issue_title("Log Message", "disk full\nx"), "disk full");
    }

    #[test]
    fn unmute_reason() {
        let at = Utc.with_ymd_and_hms(2026, 11, 1, 5, 7, 0).unwrap();
        assert_eq!(
            unmute_reason_mute_for(at),
            "An event was observed after the mute-deadline of 1 Nov 5:07 and the issue was unmuted."
        );
    }

    #[test]
    fn unmute_reason_volume() {
        let condition = serde_json::json!({"period": "hour", "nr_of_periods": 24, "volume": 5});
        assert_eq!(unmute_reason_mute_until(&condition), "More than 5 events per 24 hours occurred, unmuting the issue.");
        let condition = serde_json::json!({"period": "hour", "nr_of_periods": 1, "volume": 5});
        assert_eq!(unmute_reason_mute_until(&condition), "More than 5 events per 1 hour occurred, unmuting the issue.");
    }

    #[test]
    fn job_payload() {
        let job = AlertJob { issue_id: Uuid::nil(), alert: Alert::Unmute { unmute_reason: "r".into() } };
        let v = serde_json::to_value(&job).unwrap();
        assert_eq!(v["alert"], "unmute");
        assert_eq!(serde_json::from_value::<AlertJob>(v).unwrap().alert, job.alert);
    }
}
