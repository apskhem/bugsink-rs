//! Alert emails: who gets them (`_get_users_for_email_alert`) and what they say (upstream's `issue_alert` templates).

use bugsink_db::PgPool;
use sqlx::Row;

use super::{Alert, AlertSettings, IssueForAlert};
use crate::mail::send_rendered_email;
use crate::mail::template::Context;
use crate::py::truncatechars;

const HTML_TEMPLATE: &str = include_str!("../mail/templates/issue_alert.html");
const TEXT_TEMPLATE: &str = include_str!("../mail/templates/issue_alert.txt");

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
        if let Err(e) = send_rendered_email(pool, settings, &subject, &to, (HTML_TEMPLATE, TEXT_TEMPLATE), &ctx).await {
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
