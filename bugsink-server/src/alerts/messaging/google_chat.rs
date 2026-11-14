//! Google Chat incoming webhooks (a text message): `alerts/service_backends/google_chat.py`.

use serde_json::{Value, json};

use super::AlertMessage;
use crate::alerts::webhook::{Failure, post_json};
use crate::grouping::py::{json_dumps, truncatechars};

/// Backslash-escapes Google Chat's formatting characters.
fn safe_markdown(text: &str) -> String {
    "\\*_~`<>".chars().fold(text.to_string(), |text, c| text.replace(c, &format!("\\{c}")))
}

fn payload(m: &AlertMessage<'_>) -> Value {
    let mut text = format!(
        "*{}*\n{} issue\n• *Project*: {}\n<{}|view on Bugsink>",
        safe_markdown(&truncatechars(&m.issue.title, 256)),
        m.alert.reason(),
        safe_markdown(&m.issue.project_name),
        m.issue.url(m.settings),
    );
    if let Some(unmute_reason) = m.alert.unmute_reason().filter(|r| !r.is_empty()) {
        text += &format!("\n*Unmute Reason*: {}", safe_markdown(unmute_reason));
    }
    json!({"text": text})
}

/// `google_chat_backend_send_alert` (`_post_text`).
pub(super) async fn send_alert(m: &AlertMessage<'_>) -> Result<(), Failure> {
    let webhook_url = m.config_str("webhook_url")?;
    post_json(&m.settings.webhooks, webhook_url, json_dumps(&payload(m))).await
}

/// `google_chat_backend_send_test_message`.
pub(super) fn test_payload(project_name: &str, display_name: &str) -> Value {
    json!({"text": format!(
        concat!(
            "*TEST issue*\nTest message by Bugsink to test the webhook setup.\n",
            "• *Project*: {}\n• *Message Backend*: {}",
        ),
        safe_markdown(project_name),
        safe_markdown(display_name),
    )})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::messaging::test_parts;

    #[test]
    fn payload_like_upstream() {
        let (issue, alert, settings, config) = test_parts(json!({"webhook_url": "https://hooks.example.com/x"}));
        let m = AlertMessage { issue: &issue, alert: &alert, settings: &settings, config: &config };
        // Expected: upstream 2.6.3's `google_chat_backend_send_alert` for the same issue.
        assert_eq!(
            json_dumps(&payload(&m)),
            r#"{"text": "*Value\\_Error\\<T\\>: bad \\*input\\* & \"quoted\" 'x' | [a]-#1 \\~\\`\\\\ \u00e9 \u2713 \ud83d\udc1b*\nUNMUTED issue\n\u2022 *Project*: My\\_Project \\<web\\> & \\*api\\* [x]-#1 \\~\\`\\\\\n<https://bugsink.example.com/issues/issue/8a3cc2c0-1111-4222-8333-444455556666/event/last/|view on Bugsink>\n*Unmute Reason*: Unmuted \\*because\\* \\<x\\> & \\_y\\_ [z] #1 - 'q' \"d\" \\~\\`\\\\"}"#
        );
    }
}
