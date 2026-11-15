//! Microsoft Teams workflow webhooks (Adaptive Cards): `alerts/service_backends/msteams.py`.

use serde_json::{
  Value,
  json,
};

use super::AlertMessage;
use crate::py::{
  truncatechars,
  truthy,
};

/// Adaptive Card TextBlocks render a subset of markdown.
fn safe_markdown(text: &str) -> String {
  text
    .replace('\\', "\\\\")
    .replace('*', "\\*")
    .replace('_', "\\_")
    .replace('[', "\\[")
    .replace(']', "\\]")
    .replace('#', "\\#")
    .replace('-', "\\-")
}

/// `_as_message`: the card (with `actions`, if any), wrapped in a message attachment.
fn as_message(body: Vec<Value>, actions: Value) -> Value {
  let mut content = json!({
      "type": "AdaptiveCard",
      "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
      "version": "1.4",
      "body": body,
  });
  if truthy(&actions) {
    content["actions"] = actions;
  }
  json!({
      "type": "message",
      "attachments": [{"contentType": "application/vnd.microsoft.card.adaptive", "content": content}],
  })
}

/// `msteams_backend_send_alert`.
pub(super) fn payload(m: &AlertMessage<'_>) -> Value {
  let mut body = vec![
    json!({
        "type": "TextBlock",
        "text": safe_markdown(&truncatechars(&m.issue.title, 150)),
        "size": "Large",
        "weight": "Bolder",
        "wrap": true,
    }),
    json!({"type": "TextBlock", "text": format!("{} issue", m.alert.reason()), "wrap": true}),
  ];
  if let Some(unmute_reason) = m.alert.unmute_reason() {
    body.push(json!({"type": "TextBlock", "text": safe_markdown(unmute_reason), "wrap": true}));
  }
  body.push(json!({"type": "FactSet", "facts": [{"title": "project", "value": safe_markdown(&m.issue.project_name)}]}));
  as_message(
    body,
    json!([{"type": "Action.OpenUrl", "title": "view on Bugsink", "url": m.issue.url(m.settings)}]),
  )
}

/// `msteams_backend_send_test_message`.
pub(super) fn test_payload(project_name: &str, display_name: &str) -> Value {
  as_message(
    vec![
      json!({"type": "TextBlock", "text": "TEST issue", "size": "Large", "weight": "Bolder", "wrap": true}),
      json!({"type": "TextBlock", "text": "Test message by Bugsink to test the webhook setup.", "wrap": true}),
      json!({"type": "FactSet", "facts": [
          {"title": "project", "value": safe_markdown(project_name)},
          {"title": "message backend", "value": safe_markdown(display_name)},
      ]}),
    ],
    Value::Null,
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::alerts::messaging::test_parts;
  use crate::py::json_dumps;

  #[test]
  fn payload_like_upstream() {
    let (issue, alert, settings, config) = test_parts(json!({"webhook_url": "https://hooks.example.com/x"}));
    let m = AlertMessage {
      issue: &issue,
      alert: &alert,
      settings: &settings,
      config: &config,
    };
    // Expected: upstream 2.6.3's `msteams_backend_send_alert` for the same issue.
    assert_eq!(
      json_dumps(&payload(&m)),
      r#"{"type": "message", "attachments": [{"contentType": "application/vnd.microsoft.card.adaptive", "content": {"type": "AdaptiveCard", "$schema": "http://adaptivecards.io/schemas/adaptive-card.json", "version": "1.4", "body": [{"type": "TextBlock", "text": "Value\\_Error<T>: bad \\*input\\* & \"quoted\" 'x' | \\[a\\]\\-\\#1 ~`\\\\ \u00e9 \u2713 \ud83d\udc1b", "size": "Large", "weight": "Bolder", "wrap": true}, {"type": "TextBlock", "text": "UNMUTED issue", "wrap": true}, {"type": "TextBlock", "text": "Unmuted \\*because\\* <x> & \\_y\\_ \\[z\\] \\#1 \\- 'q' \"d\" ~`\\\\", "wrap": true}, {"type": "FactSet", "facts": [{"title": "project", "value": "My\\_Project <web> & \\*api\\* \\[x\\]\\-\\#1 ~`\\\\"}]}], "actions": [{"type": "Action.OpenUrl", "title": "view on Bugsink", "url": "https://bugsink.example.com/issues/issue/8a3cc2c0-1111-4222-8333-444455556666/event/last/"}]}}]}"#
    );
  }
}
