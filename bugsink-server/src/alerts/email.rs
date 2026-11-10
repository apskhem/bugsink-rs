//! Alert emails: who gets them (`_get_users_for_email_alert`), rendering (upstream's templates), the installation's
//! email quota (`Installation.check_and_inc_email_quota`) and delivery diagnostics (`record_email_attempt`), and
//! sending over SMTP (`send_rendered_email`, Django's SMTP backend settings).

use std::time::{Duration, Instant};

use bugsink_db::PgPool;
use chrono::{DateTime, Utc};
use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde_json::{Map, Value, json};
use sqlx::Row;

use super::template::{Context, render};
use super::{Alert, AlertSettings, IssueForAlert};
use crate::grouping::py::{float_repr, isoformat, json_dumps, truncatechars};

const HTML_TEMPLATE: &str = include_str!("templates/issue_alert.html");
const TEXT_TEMPLATE: &str = include_str!("templates/issue_alert.txt");
const EMAIL_ATTEMPTS_MAX: usize = 10;

/// Django's email settings, from the same environment variables as upstream's Docker image.
#[derive(clap::Args, Clone, Debug)]
pub struct EmailSettings {
    /// Only Django's SMTP backend sends; any other value (and no EMAIL_HOST) only logs.
    #[arg(long, env = "EMAIL_BACKEND")]
    pub email_backend: Option<String>,
    #[arg(long, env = "EMAIL_HOST")]
    pub email_host: Option<String>,
    #[arg(long, env = "EMAIL_HOST_USER")]
    pub email_host_user: Option<String>,
    #[arg(long, env = "EMAIL_HOST_PASSWORD", hide_env_values = true)]
    pub email_host_password: Option<String>,
    #[arg(long, env = "EMAIL_PORT", default_value_t = 587)]
    pub email_port: u16,
    /// Implicit TLS (SMTPS).
    #[arg(long, env = "EMAIL_USE_SSL", default_value = "false", value_parser = super::py_bool)]
    pub email_use_ssl: bool,
    /// STARTTLS; defaults to true unless EMAIL_USE_SSL.
    #[arg(long, env = "EMAIL_USE_TLS", value_parser = super::py_bool)]
    pub email_use_tls: Option<bool>,
    /// Seconds.
    #[arg(long, env = "EMAIL_TIMEOUT")]
    pub email_timeout: Option<u64>,
    #[arg(long, env = "DEFAULT_FROM_EMAIL", default_value = "Bugsink <bugsink@example.org>")]
    pub default_from_email: String,
    /// "None" for no limit.
    #[arg(long, env = "MAX_EMAILS_PER_HOUR", default_value = "60", value_parser = int_or_none)]
    pub max_emails_per_hour: Limit,
    /// Per calendar month; "None" (default) for no limit.
    #[arg(long, env = "MAX_EMAILS_PER_MONTH", default_value = "None", value_parser = int_or_none)]
    pub max_emails_per_month: Limit,
    /// Comma-separated; defaults to BASE_URL's host. Only used to refuse sending as bugsink.com from elsewhere.
    #[arg(long, env = "ALLOWED_HOSTS", value_delimiter = ',')]
    pub allowed_hosts: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct Limit(pub Option<i64>);

/// `int_or_none`: anything that isn't an integer means "no limit".
fn int_or_none(s: &str) -> Result<Limit, String> {
    Ok(Limit(s.trim().parse().ok()))
}

impl EmailSettings {
    fn delivers(&self) -> bool {
        match &self.email_backend {
            Some(backend) => backend == "django.core.mail.backends.smtp.EmailBackend",
            None => self.email_host.is_some(),
        }
    }

    fn transport(&self) -> Result<AsyncSmtpTransport<Tokio1Executor>, lettre::transport::smtp::Error> {
        let host = self.email_host.as_deref().unwrap_or("localhost");
        let mut builder = if self.email_use_ssl {
            AsyncSmtpTransport::<Tokio1Executor>::relay(host)?
        } else if self.email_use_tls.unwrap_or(true) {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
        };
        builder = builder.port(self.email_port);
        if let Some(seconds) = self.email_timeout {
            builder = builder.timeout(Some(Duration::from_secs(seconds)));
        }
        // Django logs in only when both are set.
        if let (Some(user), Some(password)) = (&self.email_host_user, &self.email_host_password)
            && !user.is_empty()
            && !password.is_empty()
        {
            builder = builder.credentials(Credentials::new(user.clone(), password.clone()));
        }
        Ok(builder.build())
    }
}

/// Sends the alert to every member who wants it. Like upstream, stops at the first failed send (logged).
pub(super) async fn send_alert_emails(pool: &PgPool, settings: &AlertSettings, issue: &IssueForAlert, alert: &Alert) {
    let recipients = match recipients(pool, issue.project_id).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(issue_id = %issue.id, error = %e, "loading alert recipients failed");
            return;
        }
    };
    let subject =
        format!("\"{}\" in \"{}\" ({})", truncatechars(&issue.title, 80), issue.project_name, alert.state_description());
    let issue_url = issue.url(settings);
    let base_url = format!("{}/", settings.base_url());
    let settings_url = format!("{}/accounts/preferences/", settings.base_url());
    let ctx: Context = [
        ("site_title", settings.site_title.as_str()),
        ("base_url", base_url.as_str()),
        ("issue_title", issue.title.as_str()),
        ("project_name", issue.project_name.as_str()),
        ("issue_url", issue_url.as_str()),
        ("state_description", alert.state_description()),
        ("alert_article", alert.article()),
        ("alert_reason", alert.reason()),
        ("settings_url", settings_url.as_str()),
        ("unmute_reason", alert.unmute_reason().unwrap_or("")),
    ]
    .into_iter()
    .collect();

    for to in recipients {
        if let Err(e) = send_rendered_email(pool, settings, &subject, &to, &ctx).await {
            tracing::error!(issue_id = %issue.id, %to, error = %e, "sending alert email failed");
            break;
        }
    }
}

/// `_get_users_for_email_alert`: accepted, active project members; the project membership's preference wins, then the
/// team membership's, then the user's.
async fn recipients(pool: &PgPool, project_id: i64) -> sqlx::Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT u.email, pm.send_email_alerts AS project_pref, tm.id IS NOT NULL AS in_team,
                tm.send_email_alerts AS team_pref, u.send_email_alerts AS user_pref
         FROM projects_projectmembership pm
         JOIN auth_user u ON u.id = pm.user_id
         JOIN projects_project p ON p.id = pm.project_id
         LEFT JOIN teams_teammembership tm ON tm.team_id = p.team_id AND tm.user_id = pm.user_id AND tm.accepted
         WHERE pm.project_id = $1 AND pm.accepted AND u.is_active AND pm.send_email_alerts IS DISTINCT FROM false
         ORDER BY pm.id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|r| {
            let project_pref: Option<bool> = r.get("project_pref");
            let team_pref: Option<bool> = r.get("team_pref");
            let in_team: bool = r.get("in_team");
            let user_pref: bool = r.get("user_pref");
            match (project_pref, in_team, team_pref) {
                (Some(pref), _, _) => pref,
                (None, true, Some(pref)) => pref,
                _ => user_pref,
            }
        })
        .map(|r| r.get("email"))
        .collect())
}

fn email_uses_bugsink_domain(email: &str) -> bool {
    let email = email.trim().to_lowercase();
    email.ends_with("@bugsink.com") || email.ends_with("@bugsink.com>")
}

/// `send_rendered_email` for one recipient. Refusals and an exceeded quota are logged and count as done.
async fn send_rendered_email(
    pool: &PgPool,
    settings: &AlertSettings,
    subject: &str,
    to: &str,
    ctx: &Context<'_>,
) -> anyhow::Result<()> {
    let email = &settings.email;
    if to.is_empty() {
        return Ok(());
    }
    if email_uses_bugsink_domain(to) {
        tracing::error!(
            subject, to,
            "refusing to send email: you are not Bugsink, so configure the user with their own email address instead \
             of an @bugsink.com address"
        );
        return Ok(());
    }
    if email_uses_bugsink_domain(&email.default_from_email) && !hosted_on_bugsink(settings) {
        tracing::error!(
            subject, to, from = email.default_from_email,
            "refusing to send email: DEFAULT_FROM_EMAIL uses bugsink.com while ALLOWED_HOSTS does not"
        );
        return Ok(());
    }
    if !check_and_inc_email_quota(pool, email, Utc::now()).await? {
        tracing::warn!(subject, to, "email quota exceeded; not sending email");
        return Ok(());
    }
    tracing::info!(subject, to, "sending email");

    let html = render(HTML_TEMPLATE, ctx);
    let text = render(TEXT_TEMPLATE, ctx);

    let started = Instant::now();
    let result = deliver(email, subject, to, text, html).await;
    let duration = started.elapsed();
    record_email_attempt(pool, result.as_ref().err().map(|e| e.to_string()), duration).await?;
    result
}

fn hosted_on_bugsink(settings: &AlertSettings) -> bool {
    let base_url_host = url::Url::parse(settings.base_url()).ok().and_then(|u| u.host_str().map(str::to_owned));
    let hosts: Vec<String> = match settings.email.allowed_hosts.as_slice() {
        [] => base_url_host.into_iter().collect(),
        hosts => hosts.to_vec(),
    };
    hosts.iter().any(|h| h.trim().to_lowercase().trim_start_matches('.').ends_with("bugsink.com"))
}

async fn deliver(email: &EmailSettings, subject: &str, to: &str, text: String, html: String) -> anyhow::Result<()> {
    if !email.delivers() {
        // Upstream's QuietConsoleEmailBackend.
        tracing::info!("Email is not set up, the following was not sent: {subject}");
        return Ok(());
    }
    let message = Message::builder()
        .from(email.default_from_email.parse::<Mailbox>()?)
        .to(to.parse::<Mailbox>()?)
        .subject(subject)
        .multipart(MultiPart::alternative_plain_html(text, html))?;
    email.transport()?.send(message).await?;
    Ok(())
}

/// `Installation.check_and_inc_email_quota`: counts sent emails per hour and per month in `email_quota_usage`, keeping
/// only the current hour and month.
async fn check_and_inc_email_quota(pool: &PgPool, email: &EmailSettings, now: DateTime<Utc>) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query("SELECT id, email_quota_usage FROM phonehome_installation LIMIT 1 FOR UPDATE")
        .fetch_optional(&mut *tx)
        .await?
    else {
        return Ok(true);
    };
    let id: uuid::Uuid = row.get("id");
    let mut usage = match serde_json::from_str::<Value>(row.get("email_quota_usage")) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let hour_key = now.format("%Y-%m-%dT%H").to_string();
    let month_key = now.format("%Y-%m").to_string();
    for (period, key) in [("per_hour", &hour_key), ("per_month", &month_key)] {
        if usage.get(period).and_then(|p| p.get(key)).is_none() {
            usage.insert(period.into(), json!({ key: 0 })); // only the current period is kept
        }
    }
    let per_hour = usage["per_hour"][&hour_key].as_i64().unwrap_or(0);
    let per_month = usage["per_month"][&month_key].as_i64().unwrap_or(0);
    let over = |limit: Limit, n: i64| limit.0.is_some_and(|max| n >= max);
    if over(email.max_emails_per_hour, per_hour) || over(email.max_emails_per_month, per_month) {
        return Ok(false);
    }
    usage["per_hour"][&hour_key] = json!(per_hour + 1);
    usage["per_month"][&month_key] = json!(per_month + 1);
    sqlx::query("UPDATE phonehome_installation SET email_quota_usage = $2 WHERE id = $1")
        .bind(id)
        .bind(json_dumps(&Value::Object(usage)))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// `Installation.record_email_attempt`: the last 10 attempts, shown on upstream's settings page.
async fn record_email_attempt(pool: &PgPool, error: Option<String>, duration: Duration) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query("SELECT id, email_sending_diagnostics FROM phonehome_installation LIMIT 1 FOR UPDATE")
        .fetch_optional(&mut *tx)
        .await?
    else {
        return Ok(());
    };
    let id: uuid::Uuid = row.get("id");
    let mut diagnostics = match serde_json::from_str::<Value>(row.get("email_sending_diagnostics")) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let mut attempt = json!({
        "at": isoformat(&Utc::now()),
        "ok": error.is_none(),
        "duration": serde_json::from_str::<Value>(&float_repr((duration.as_secs_f64() * 1000.0).round() / 1000.0))
            .unwrap_or(json!(0)),
    });
    if let Some(error) = error {
        attempt["error"] = json!(error.chars().take(100).collect::<String>());
    }
    let mut attempts = diagnostics.get("attempts").and_then(Value::as_array).cloned().unwrap_or_default();
    attempts.push(attempt);
    let keep_from = attempts.len().saturating_sub(EMAIL_ATTEMPTS_MAX);
    diagnostics.insert("attempts".into(), Value::Array(attempts.split_off(keep_from)));
    sqlx::query("UPDATE phonehome_installation SET email_sending_diagnostics = $2 WHERE id = $1")
        .bind(id)
        .bind(json_dumps(&Value::Object(diagnostics)))
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bugsink_domain() {
        assert!(email_uses_bugsink_domain(" Klaas <klaas@Bugsink.com>"));
        assert!(!email_uses_bugsink_domain("me@notbugsink.com.example"));
    }
}
