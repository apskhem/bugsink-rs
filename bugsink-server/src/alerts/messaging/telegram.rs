//! Telegram's Bot API (`sendMessage`, HTML parse mode): `alerts/service_backends/telegram.py`.

use serde_json::{Value, json};

use super::AlertMessage;
use crate::alerts::webhook::{Failure, post_json};
use crate::grouping::py::{json_dumps, truncatechars};

const MASKED: &str = "[MASKED]";

/// `html.escape(text)` (with `quote=True`).
fn safe_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// `_chat_id_supports_message_thread_id`: not for channel usernames.
fn chat_id_supports_message_thread_id(chat_id: &Value) -> bool {
    !chat_id.as_str().is_some_and(|c| c.starts_with('@'))
}

/// `_build_alert_message`.
fn alert_text(m: &AlertMessage<'_>) -> String {
    let mut text = format!(
        "<b>{}</b>\n\n{} issue\n\n<b>Project:</b> {}",
        safe_html(&truncatechars(&m.issue.title, 200)),
        m.alert.reason(),
        safe_html(&m.issue.project_name),
    );
    if let Some(unmute_reason) = m.alert.unmute_reason().filter(|r| !r.is_empty()) {
        text += &format!("\n\n<b>Unmute Reason:</b> {}", safe_html(unmute_reason));
    }
    text += &format!("\n\n<a href=\"{}\">View on Bugsink</a>", safe_html(&m.issue.url(m.settings)));
    text
}

fn payload(m: &AlertMessage<'_>, chat_id: &Value) -> Value {
    let mut data = json!({"chat_id": chat_id, "text": alert_text(m), "parse_mode": "HTML"});
    if let Some(thread_id) = m.config.get("message_thread_id").filter(|t| !t.is_null())
        && chat_id_supports_message_thread_id(chat_id)
    {
        data["message_thread_id"] = thread_id.clone();
    }
    data
}

/// Telegram's `_store_failure_info` keeps the bot token (which is in the URL) out of what is stored.
fn mask_token(failure: Failure, bot_token: &str) -> Failure {
    if bot_token.is_empty() {
        return failure;
    }
    Failure {
        message: failure.message.replace(bot_token, MASKED),
        response: failure.response.map(|(status, text)| (status, text.replace(bot_token, MASKED))),
        ..failure
    }
}

/// `telegram_backend_send_alert`.
pub(super) async fn send_alert(m: &AlertMessage<'_>) -> Result<(), Failure> {
    let bot_token = m.config_str("bot_token")?;
    let chat_id = m.config.get("chat_id").ok_or_else(|| Failure {
        error_type: "KeyError",
        message: "'chat_id'".into(),
        response: None,
    })?;
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    post_json(&m.settings.webhooks, &url, json_dumps(&payload(m, chat_id)))
        .await
        .map_err(|failure| mask_token(failure, bot_token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::messaging::test_parts;

    #[test]
    fn payload_like_upstream() {
        let (issue, alert, settings, config) =
            test_parts(json!({"bot_token": "123456:ABC-DEF_ghi", "chat_id": "-1001234567890", "message_thread_id": 42}));
        let m = AlertMessage { issue: &issue, alert: &alert, settings: &settings, config: &config };
        // Expected: upstream 2.6.3's `telegram_backend_send_alert` for the same issue.
        assert_eq!(
            json_dumps(&payload(&m, &config["chat_id"])),
            r#"{"chat_id": "-1001234567890", "text": "<b>Value_Error&lt;T&gt;: bad *input* &amp; &quot;quoted&quot; &#x27;x&#x27; | [a]-#1 ~`\\ \u00e9 \u2713 \ud83d\udc1b</b>\n\nUNMUTED issue\n\n<b>Project:</b> My_Project &lt;web&gt; &amp; *api* [x]-#1 ~`\\\n\n<b>Unmute Reason:</b> Unmuted *because* &lt;x&gt; &amp; _y_ [z] #1 - &#x27;q&#x27; &quot;d&quot; ~`\\\n\n<a href=\"https://bugsink.example.com/issues/issue/8a3cc2c0-1111-4222-8333-444455556666/event/last/\">View on Bugsink</a>", "parse_mode": "HTML", "message_thread_id": 42}"#
        );

        assert!(payload(&m, &json!("@channel")).get("message_thread_id").is_none());
    }

    #[test]
    fn token_masked() {
        let failure = Failure {
            error_type: "HTTPError",
            message: "404 Client Error: Not Found for url: https://api.telegram.org/bot1:x/sendMessage".into(),
            response: Some((404, "no bot1:x here".into())),
        };
        let masked = mask_token(failure, "1:x");
        assert_eq!(masked.message, "404 Client Error: Not Found for url: https://api.telegram.org/bot[MASKED]/sendMessage");
        assert_eq!(masked.response, Some((404, "no bot[MASKED] here".into())));
    }
}
