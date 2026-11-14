//! Mattermost incoming webhooks (message attachments): `alerts/service_backends/mattermost.py`.

use serde_json::{Value, json};

use super::AlertMessage;
use crate::alerts::webhook::{Failure, post_json};
use crate::grouping::py::{json_dumps, truncatechars, truthy};

/// The same escaping as Slack's.
fn safe_markdown(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('*', "\\*").replace('_', "\\_")
}

fn payload(m: &AlertMessage<'_>) -> Value {
    let issue_url = m.issue.url(m.settings);
    let title = safe_markdown(&truncatechars(&m.issue.title.replace('|', ""), 200));
    let mut text = format!("<{issue_url}|{title}>");
    if let Some(unmute_reason) = m.alert.unmute_reason().filter(|r| !r.is_empty()) {
        text += &format!("\n\n{}", safe_markdown(unmute_reason));
    }
    let mut data = json!({
        "text": format!("### {title}"),
        "attachments": [{
            "title": format!("{} issue", m.alert.reason()),
            "text": text,
            "fields": [{"title": "Project", "value": safe_markdown(&m.issue.project_name)}],
        }],
    });
    // The optional channel override (`config.get("channel")`), sent only if truthy.
    if let Some(channel) = m.config.get("channel").filter(|c| truthy(c)) {
        data["channel"] = channel.clone();
    }
    data
}

/// `mattermost_backend_send_alert`.
pub(super) async fn send_alert(m: &AlertMessage<'_>) -> Result<(), Failure> {
    let webhook_url = m.config_str("webhook_url")?;
    post_json(&m.settings.webhooks, webhook_url, json_dumps(&payload(m))).await
}

/// `mattermost_backend_send_test_message`.
pub(super) fn test_payload(project_name: &str, display_name: &str, channel: Option<&Value>) -> Value {
    let mut data = json!({"text": "### Test message by Bugsink to test the webhook setup.",
        "attachments": [{
            "title": "TEST issue",
            "text": "Test message by Bugsink to test the webhook setup.",
            "fields": [
                {"title": "project", "value": safe_markdown(project_name)},
                {"title": "message backend", "value": safe_markdown(display_name)},
            ],
        }]});
    if let Some(channel) = channel.filter(|c| crate::grouping::py::truthy(c)) {
        data["channel"] = channel.clone();
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::messaging::test_parts;

    #[test]
    fn payload_like_upstream() {
        let (issue, alert, settings, config) =
            test_parts(json!({"webhook_url": "https://hooks.example.com/x", "channel": "town-square"}));
        let m = AlertMessage { issue: &issue, alert: &alert, settings: &settings, config: &config };
        // Expected: upstream 2.6.3's `mattermost_backend_send_alert` for the same issue.
        assert_eq!(
            json_dumps(&payload(&m)),
            r####"{"text": "### Value\\_Error&lt;T&gt;: bad \\*input\\* &amp; \"quoted\" 'x'  [a]-#1 ~`\\ \u00e9 \u2713 \ud83d\udc1b", "attachments": [{"title": "UNMUTED issue", "text": "<https://bugsink.example.com/issues/issue/8a3cc2c0-1111-4222-8333-444455556666/event/last/|Value\\_Error&lt;T&gt;: bad \\*input\\* &amp; \"quoted\" 'x'  [a]-#1 ~`\\ \u00e9 \u2713 \ud83d\udc1b>\n\nUnmuted \\*because\\* &lt;x&gt; &amp; \\_y\\_ [z] #1 - 'q' \"d\" ~`\\", "fields": [{"title": "Project", "value": "My\\_Project &lt;web&gt; &amp; \\*api\\* [x]-#1 ~`\\"}]}], "channel": "town-square"}"####
        );

        let config = json!({"webhook_url": "https://hooks.example.com/x", "channel": ""});
        let m = AlertMessage { config: &config, ..m };
        assert!(payload(&m).get("channel").is_none());
    }
}
