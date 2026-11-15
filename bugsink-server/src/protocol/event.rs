//! Event payload helpers that don't belong to grouping.

use chrono::{
  DateTime,
  NaiveDateTime,
  TimeZone,
  Utc,
};
use serde_json::Value;

/// Upstream's `compat.timestamp.parse_timestamp`: seconds since the epoch (int or float), or an ISO 8601 / RFC 3339
/// string (UTC when no offset is given). `None` for anything else.
pub fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
  match value {
    Value::Number(n) => {
      let secs = n.as_f64()?;
      // Python's fromtimestamp rounds to microseconds.
      let micros = (secs * 1e6).round() as i64;
      Utc.timestamp_micros(micros).single()
    }
    Value::String(s) => {
      DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
          [
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%d %H:%M:%S%.f",
            "%Y-%m-%dT%H:%M",
            "%Y-%m-%d %H:%M",
          ]
          .iter()
          .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
          .map(|naive| naive.and_utc())
        })
    }
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  #[test]
  fn timestamps() {
    let expected = Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
    assert_eq!(parse_timestamp(&json!(1704164645)), Some(expected));
    assert_eq!(parse_timestamp(&json!(1704164645.0)), Some(expected));
    assert_eq!(parse_timestamp(&json!("2024-01-02T03:04:05Z")), Some(expected));
    assert_eq!(parse_timestamp(&json!("2024-01-02T05:04:05+02:00")), Some(expected));
    assert_eq!(parse_timestamp(&json!("2024-01-02T03:04:05")), Some(expected));
    assert_eq!(parse_timestamp(&json!("2024-01-02 03:04:05")), Some(expected));
    assert_eq!(parse_timestamp(&json!("yesterday")), None);
  }
}
