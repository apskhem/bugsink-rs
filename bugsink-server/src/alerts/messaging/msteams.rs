//! Microsoft Teams workflow webhooks (Adaptive Cards): `alerts/service_backends/msteams.py`.

use serde_json::{Value, json};

use super::AlertMessage;
use crate::alerts::webhook::{Failure, post_json};
use crate::grouping::py::{json_dumps, truncatechars};

/// Adaptive Card TextBlocks render a subset of markdown.
fn safe_markdown(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('#', "\\#")
        .replace('-', "\\-")
}

/// `_as_message`: the card, wrapped in a message attachment.
fn as_message(body: Vec<Value>, actions: Value) -> Value {
    json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "type": "AdaptiveCard",
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "version": "1.4",
                "body": body,
                "actions": actions,
            },
        }],
    })
}

fn payload(m: &AlertMessage<'_>) -> Value {
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
    if let Some(unmute_reason) = m.alert.unmute_reason().filter(|r| !r.is_empty()) {
        body.push(json!({"type": "TextBlock", "text": safe_markdown(unmute_reason), "wrap": true}));
    }
    body.push(json!({"type": "FactSet", "facts": [{"title": "project", "value": safe_markdown(&m.issue.project_name)}]}));
    as_message(body, json!([{"type": "Action.OpenUrl", "title": "view on Bugsink", "url": m.issue.url(m.settings)}]))
}

/// `msteams_backend_send_alert`.
pub(super) async fn send_alert(m: &AlertMessage<'_>) -> Result<(), Failure> {
    let webhook_url = m.config_str("webhook_url")?;
    post_json(&m.settings.webhooks, webhook_url, json_dumps(&payload(m))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::messaging::test_parts;

    #[test]
    fn payload_like_upstream() {
        let (issue, alert, settings, config) = test_parts(json!({"webhook_url": "https://hooks.example.com/x"}));
        let m = AlertMessage { issue: &issue, alert: &alert, settings: &settings, config: &config };
        // Expected: upstream 2.6.3's `msteams_backend_send_alert` for the same issue.
        assert_eq!(
            json_dumps(&payload(&m)),
            r#"{"type": "message", "attachments": [{"contentType": "application/vnd.microsoft.card.adaptive", "content": {"type": "AdaptiveCard", "$schema": "http://adaptivecards.io/schemas/adaptive-card.json", "version": "1.4", "body": [{"type": "TextBlock", "text": "Value\\_Error<T>: bad \\*input\\* & \"quoted\" 'x' | \\[a\\]\\-\\#1 ~`\\\\ \u00e9 \u2713 \ud83d\udc1b", "size": "Large", "weight": "Bolder", "wrap": true}, {"type": "TextBlock", "text": "UNMUTED issue", "wrap": true}, {"type": "TextBlock", "text": "Unmuted \\*because\\* <x> & \\_y\\_ \\[z\\] \\#1 \\- 'q' \"d\" ~`\\\\", "wrap": true}, {"type": "FactSet", "facts": [{"title": "project", "value": "My\\_Project <web> & \\*api\\* \\[x\\]\\-\\#1 ~`\\\\"}]}], "actions": [{"type": "Action.OpenUrl", "title": "view on Bugsink", "url": "https://bugsink.example.com/issues/issue/8a3cc2c0-1111-4222-8333-444455556666/event/last/"}]}}]}"#
        );
    }
}
