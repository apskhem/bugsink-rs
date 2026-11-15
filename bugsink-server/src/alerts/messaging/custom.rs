//! Custom webhooks: the issue as the API serializes it, plus some convenience fields;
//! `alerts/service_backends/custom.py`.

use bugsink_db::PgPool;
use chrono::{
  DateTime,
  Utc,
};
use serde_json::{
  Value,
  json,
};
use sqlx::Row;
use uuid::Uuid;

use super::AlertMessage;
use crate::py::{
  drf_datetime,
  py_str,
};
use crate::releases::parse_lines;

/// The issue's columns that `IssueSerializer` reads.
pub(super) struct IssueData {
  id: Uuid,
  project_id: i64,
  project_slug: String,
  digest_order: i32,
  last_seen: DateTime<Utc>,
  first_seen: DateTime<Utc>,
  digested_event_count: i32,
  stored_event_count: i32,
  calculated_type: String,
  calculated_value: String,
  transaction: String,
  is_resolved: bool,
  is_resolved_unconditionally: bool,
  is_resolved_by_next_release: bool,
  fixed_at: String,
  events_at: String,
  is_muted: bool,
  unmute_after: Option<DateTime<Utc>>,
  unmute_on_volume_based_conditions: String,
}

impl IssueData {
  pub(super) async fn load(pool: &PgPool, issue_id: Uuid) -> sqlx::Result<Option<Self>> {
    let row = sqlx::query(
      "SELECT i.project_id, p.slug, i.digest_order, i.last_seen, i.first_seen, i.digested_event_count,
                 i.stored_event_count, i.calculated_type, i.calculated_value, i.transaction, i.is_resolved,
                 i.is_resolved_unconditionally, i.is_resolved_by_next_release, i.fixed_at, i.events_at, i.is_muted,
                 i.unmute_after, i.unmute_on_volume_based_conditions
             FROM issues_issue i JOIN projects_project p ON p.id = i.project_id WHERE i.id = $1",
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
      Self {
        id: issue_id,
        project_id: r.get("project_id"),
        project_slug: r.get("slug"),
        digest_order: r.get("digest_order"),
        last_seen: r.get("last_seen"),
        first_seen: r.get("first_seen"),
        digested_event_count: r.get("digested_event_count"),
        stored_event_count: r.get("stored_event_count"),
        calculated_type: r.get("calculated_type"),
        calculated_value: r.get("calculated_value"),
        transaction: r.get("transaction"),
        is_resolved: r.get("is_resolved"),
        is_resolved_unconditionally: r.get("is_resolved_unconditionally"),
        is_resolved_by_next_release: r.get("is_resolved_by_next_release"),
        fixed_at: r.get("fixed_at"),
        events_at: r.get("events_at"),
        is_muted: r.get("is_muted"),
        unmute_after: r.get("unmute_after"),
        unmute_on_volume_based_conditions: r.get("unmute_on_volume_based_conditions"),
      }
    }))
  }
}

/// `UnmuteVolumeConditionSerializer` (`period` a string, the others ints).
fn volume_conditions(conditions: &str) -> Value {
  let conditions: Vec<Value> = serde_json::from_str(conditions).unwrap_or_default();
  let int = |v: &Value| v.as_i64().map_or_else(|| v.clone(), Value::from);
  conditions
        .iter()
        .map(|c| json!({"period": py_str(&c["period"]), "nr_of_periods": int(&c["nr_of_periods"]), "volume": int(&c["volume"])}))
        .collect()
}

/// `IssueSerializer(issue).data` plus the convenience fields.
/// `custom_backend_send_alert`.
pub(super) fn payload(m: &AlertMessage<'_>, issue: &IssueData) -> Value {
  let mut data = json!({
      "id": issue.id.to_string(),
      "friendly_id": crate::issues::friendly_id(&issue.project_slug, issue.digest_order),
      "project": issue.project_id,
      "digest_order": issue.digest_order,
      "last_seen": drf_datetime(&issue.last_seen),
      "first_seen": drf_datetime(&issue.first_seen),
      "digested_event_count": issue.digested_event_count,
      "stored_event_count": issue.stored_event_count,
      "calculated_type": issue.calculated_type,
      "calculated_value": issue.calculated_value,
      "transaction": issue.transaction,
      "is_resolved": issue.is_resolved,
      "is_resolved_unconditionally": issue.is_resolved_unconditionally,
      "is_resolved_by_next_release": issue.is_resolved_by_next_release,
      "fixed_at": parse_lines(&issue.fixed_at),
      "events_at": parse_lines(&issue.events_at),
      "is_muted": issue.is_muted,
      "unmute_after": issue.unmute_after.as_ref().map(drf_datetime),
      "unmute_on_volume_based_conditions": volume_conditions(&issue.unmute_on_volume_based_conditions),
      "title": m.issue.title,
      "project_name": m.issue.project_name,
      "url": m.issue.url(m.settings),
      "alert_reason": m.alert.reason(),
  });
  if let Some(unmute_reason) = m.alert.unmute_reason() {
    data["unmute_reason"] = json!(unmute_reason);
  }
  data
}

/// `custom_backend_send_test_message`: a made-up issue.
pub(super) fn test_payload(project_name: &str) -> Value {
  json!({
      "id": "497f6eca-6276-4993-bfeb-53cbbbba6f08",
      "friendly_id": "TEST-1",
      "project": 1,
      "digest_order": 1,
      "first_seen": "2024-01-10T08:15:00Z",
      "last_seen": "2024-01-15T10:30:00Z",
      "digested_event_count": 15,
      "stored_event_count": 15,
      "calculated_type": "ValueError",
      "calculated_value": "invalid literal for int()",
      "transaction": "/api/users/login",
      "is_resolved": false,
      "is_resolved_unconditionally": false,
      "is_resolved_by_next_release": false,
      "fixed_at": [],
      "events_at": [],
      "is_muted": false,
      "unmute_after": null,
      "unmute_on_volume_based_conditions": [],
      "title": "ValueError: invalid literal for int()",
      "project_name": project_name,
      "url": "https://bugsink.example.com/issues/497f6eca-6276-4993-bfeb-53cbbbba6f08/",
      "alert_reason": "TEST",
  })
}

#[cfg(test)]
mod tests {
  use chrono::TimeZone;

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
    let data = IssueData {
      id: issue.id,
      project_id: 7,
      project_slug: "web-api".into(),
      digest_order: 42,
      last_seen: Utc.with_ymd_and_hms(2026, 11, 2, 6, 8, 9).unwrap(),
      first_seen: Utc.with_ymd_and_hms(2026, 11, 1, 5, 7, 0).unwrap() + chrono::Duration::microseconds(123456),
      digested_event_count: 15,
      stored_event_count: 12,
      calculated_type: "Value_Error<T>".into(),
      calculated_value: "bad *input* & \"quoted\" 'x' | [a]-#1 ~`\\ é ✓ 🐛\nsecond line".into(),
      transaction: "/api/users/<id>".into(),
      is_resolved: false,
      is_resolved_unconditionally: false,
      is_resolved_by_next_release: true,
      fixed_at: "1.0\n2.0\n".into(),
      events_at: "\n1.0\n".into(),
      is_muted: false,
      unmute_after: Some(Utc.with_ymd_and_hms(2026, 11, 3, 0, 0, 0).unwrap()),
      unmute_on_volume_based_conditions: r#"[{"period": "day", "nr_of_periods": 1, "volume": 100}]"#.into(),
    };
    // Expected: upstream 2.6.3's `custom_backend_send_alert` for the same issue.
    assert_eq!(
      json_dumps(&payload(&m, &data)),
      r#"{"id": "8a3cc2c0-1111-4222-8333-444455556666", "friendly_id": "WEB-API-42", "project": 7, "digest_order": 42, "last_seen": "2026-11-02T06:08:09Z", "first_seen": "2026-11-01T05:07:00.123456Z", "digested_event_count": 15, "stored_event_count": 12, "calculated_type": "Value_Error<T>", "calculated_value": "bad *input* & \"quoted\" 'x' | [a]-#1 ~`\\ \u00e9 \u2713 \ud83d\udc1b\nsecond line", "transaction": "/api/users/<id>", "is_resolved": false, "is_resolved_unconditionally": false, "is_resolved_by_next_release": true, "fixed_at": ["1.0", "2.0"], "events_at": ["", "1.0"], "is_muted": false, "unmute_after": "2026-11-03T00:00:00Z", "unmute_on_volume_based_conditions": [{"period": "day", "nr_of_periods": 1, "volume": 100}], "title": "Value_Error<T>: bad *input* & \"quoted\" 'x' | [a]-#1 ~`\\ \u00e9 \u2713 \ud83d\udc1b", "project_name": "My_Project <web> & *api* [x]-#1 ~`\\", "url": "https://bugsink.example.com/issues/issue/8a3cc2c0-1111-4222-8333-444455556666/event/last/", "alert_reason": "UNMUTED", "unmute_reason": "Unmuted *because* <x> & _y_ [z] #1 - 'q' \"d\" ~`\\"}"#
    );
  }
}
