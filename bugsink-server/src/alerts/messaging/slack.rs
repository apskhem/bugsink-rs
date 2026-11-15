//! Slack (and Mattermost, which accepts the same payload) incoming webhooks: `alerts/service_backends/slack.py`.

use serde_json::{
  Value,
  json,
};

use super::AlertMessage;
use crate::py::truncatechars;

/// Slack (and Mattermost) give `& < > * _` special meaning.
pub(super) fn safe_markdown(text: &str) -> String {
  text
    .replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('*', "\\*")
    .replace('_', "\\_")
}

/// `slack_backend_send_alert`.
pub(super) fn payload(m: &AlertMessage<'_>) -> Value {
  let issue_url = m.issue.url(m.settings);
  let title = truncatechars(&m.issue.title.replace('|', ""), 150);

  let mut sections = vec![
    json!({"type": "header", "text": {"type": "plain_text", "text": title}}),
    json!({"type": "section", "text": {"type": "plain_text", "text": format!("{} issue", m.alert.reason())}}),
  ];
  if let Some(unmute_reason) = m.alert.unmute_reason() {
    sections.push(json!({"type": "section", "text": {"type": "mrkdwn", "text": unmute_reason}}));
  }
  let fields = [("project", m.issue.project_name.as_str())];
  sections.push(json!({
      "type": "section",
      "fields": fields
          .iter()
          .map(|(field, value)| json!({"type": "mrkdwn", "text": format!("*{field}*: {}", safe_markdown(value))}))
          .collect::<Vec<_>>(),
  }));
  sections
    .push(json!({"type": "section", "text": {"type": "mrkdwn", "text": format!("<{issue_url}|view on Bugsink>")}}));

  // Mattermost requires a top-level text: the first section's.
  json!({"text": sections[0]["text"]["text"].clone(), "blocks": Value::Array(sections)})
}

/// `slack_backend_send_test_message`.
pub(super) fn test_payload(project_name: &str, display_name: &str) -> Value {
  json!({"text": "Test message by Bugsink to test the webhook setup.",
  "blocks": [
      {"type": "header", "text": {"type": "plain_text", "text": "TEST issue"}},
      {"type": "section",
       "text": {"type": "mrkdwn", "text": "Test message by Bugsink to test the webhook setup."}},
      {"type": "section", "fields": [
          {"type": "mrkdwn", "text": format!("*project*: {}", safe_markdown(project_name))},
          {"type": "mrkdwn", "text": format!("*message backend*: {}", safe_markdown(display_name))},
      ]},
  ]})
}
