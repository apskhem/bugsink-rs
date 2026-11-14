//! Discord webhooks (embeds): `alerts/service_backends/discord.py`.

use serde_json::{Value, json};

use super::AlertMessage;
use crate::alerts::webhook::{Failure, post_json};
use crate::grouping::py::{json_dumps, truncatechars};

/// `url_valid_according_to_discord`: Discord rejects embeds whose url has no dot in the hostname (e.g. localhost).
fn url_valid_according_to_discord(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else { return false };
    let netloc = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = netloc.rsplit_once('@').map_or(netloc, |(_, host)| host);
    let hostname = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    };
    ["http", "https"].contains(&scheme.to_lowercase().as_str()) && hostname.contains('.')
}

/// Upstream's colors by alert reason; its map has `RECURRING` rather than `REGRESSED`, so regressions are gray.
fn color(alert_reason: &str) -> u32 {
    match alert_reason {
        "NEW" => 0xE74C3C,
        "RECURRING" => 0xE67E22,
        "UNMUTED" => 0x3498DB,
        _ => 0x95A5A6,
    }
}

fn payload(m: &AlertMessage<'_>) -> Value {
    let issue_url = m.issue.url(m.settings);
    let mut fields = vec![json!({"name": "Project", "value": m.issue.project_name, "inline": true})];
    if let Some(unmute_reason) = m.alert.unmute_reason().filter(|r| !r.is_empty()) {
        fields.push(json!({"name": "Unmute Reason", "value": unmute_reason, "inline": false}));
    }
    let mut embed = json!({
        "title": truncatechars(&m.issue.title, 256),
        "description": format!("{} issue", m.alert.reason()),
        "color": color(m.alert.reason()),
        "fields": fields,
    });
    if url_valid_according_to_discord(&issue_url) {
        embed["url"] = json!(issue_url);
    }
    json!({"embeds": [embed]})
}

/// `discord_backend_send_alert`.
pub(super) async fn send_alert(m: &AlertMessage<'_>) -> Result<(), Failure> {
    let webhook_url = m.config_str("webhook_url")?;
    post_json(&m.settings.webhooks, webhook_url, json_dumps(&payload(m))).await
}

/// `discord_backend_send_test_message`.
pub(super) fn test_payload(project_name: &str, display_name: &str) -> Value {
    json!({"embeds": [{
        "title": "TEST issue",
        "description": "Test message by Bugsink to test the webhook setup.",
        "color": 0x7289DA,
        "fields": [
            {"name": "Project", "value": project_name, "inline": true},
            {"name": "Message Backend", "value": display_name, "inline": true},
        ],
    }]})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::messaging::test_parts;

    #[test]
    fn payload_like_upstream() {
        let (issue, alert, settings, config) = test_parts(json!({"webhook_url": "https://hooks.example.com/x"}));
        let m = AlertMessage { issue: &issue, alert: &alert, settings: &settings, config: &config };
        // Expected: upstream 2.6.3's `discord_backend_send_alert` for the same issue.
        assert_eq!(
            json_dumps(&payload(&m)),
            r#"{"embeds": [{"title": "Value_Error<T>: bad *input* & \"quoted\" 'x' | [a]-#1 ~`\\ \u00e9 \u2713 \ud83d\udc1b", "description": "UNMUTED issue", "color": 3447003, "fields": [{"name": "Project", "value": "My_Project <web> & *api* [x]-#1 ~`\\", "inline": true}, {"name": "Unmute Reason", "value": "Unmuted *because* <x> & _y_ [z] #1 - 'q' \"d\" ~`\\", "inline": false}], "url": "https://bugsink.example.com/issues/issue/8a3cc2c0-1111-4222-8333-444455556666/event/last/"}]}"#
        );
    }

    #[test]
    fn discord_urls() {
        assert!(url_valid_according_to_discord("https://bugsink.example.com/issues/"));
        assert!(url_valid_according_to_discord("HTTP://user@10.0.0.1:8000/x"));
        assert!(!url_valid_according_to_discord("http://localhost:8000/issues/"));
        assert!(!url_valid_according_to_discord("http://[::1]:8000/issues/"));
        assert!(!url_valid_according_to_discord("ftp://example.com/"));
        assert_eq!(color("REGRESSED"), 0x95A5A6);
    }
}
