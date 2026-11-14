//! Django REST Framework's serializer fields, as upstream's API uses them: what they accept from a JSON body and the
//! messages they refuse with. A serializer's errors are `{field: [messages]}` in the serializer's field order, or
//! `{"non_field_errors": [...]}` once every field is valid.

use std::sync::LazyLock;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde_json::{Map, Value, json};

use crate::forms::REQUIRED;
use crate::py::{PyInt, drf_datetime, float_repr, py_int, py_str, py_uuid, strip};

/// A JSON body as a serializer sees it: an object, or the error DRF gives for anything else.
pub fn body_object(data: &Value) -> Result<&Map<String, Value>, Value> {
    match data {
        Value::Object(o) => Ok(o),
        Value::Null => Err(json!({"non_field_errors": ["No data provided"]})),
        other => Err(json!({"non_field_errors": [format!(
            "Invalid data. Expected a dictionary, but got {}.",
            type_name(other)
        )]})),
    }
}

/// `type(data).__name__` of a value from `json.loads`.
pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `str(data)`, Python's, for a value from `json.loads`.
pub fn str_of(v: &Value) -> String {
    match v {
        Value::Number(n) if n.is_f64() => float_repr(n.as_f64().unwrap_or(0.0)),
        other => py_str(other),
    }
}

/// Collects a serializer's errors in field order.
#[derive(Default)]
pub struct Errors(Map<String, Value>);

impl Errors {
    pub fn add(&mut self, field: &str, messages: Vec<String>) {
        if !messages.is_empty() {
            self.0.insert(field.into(), json!(messages));
        }
    }

    /// The field's value, or None (and its error recorded).
    pub fn take<T>(&mut self, field: &str, result: Result<T, Vec<String>>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(messages) => {
                self.add(field, messages);
                None
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_value(self) -> Value {
        Value::Object(self.0)
    }
}

type Checked<T> = Result<T, Vec<String>>;

/// `validate_empty_values`: Ok(None) when the field is left out (and may be), Ok(Some) to validate.
pub fn present<'a>(data: &'a Map<String, Value>, field: &str, required: bool) -> Checked<Option<&'a Value>> {
    match data.get(field) {
        None if required => Err(vec![REQUIRED.into()]),
        None => Ok(None),
        Some(Value::Null) => Err(vec!["This field may not be null.".into()]),
        Some(v) => Ok(Some(v)),
    }
}

/// A field that must be given, validated with `check`.
pub fn required<T>(data: &Map<String, Value>, field: &str, check: impl FnOnce(&Value) -> Checked<T>) -> Checked<T> {
    check(present(data, field, true)?.expect("required"))
}

/// A field that may be left out (None), else validated with `check`; required when `must`.
pub fn optional<T>(
    data: &Map<String, Value>,
    field: &str,
    must: bool,
    check: impl FnOnce(&Value) -> Checked<T>,
) -> Checked<Option<T>> {
    present(data, field, must)?.map(check).transpose()
}

/// `CharField` (`trim_whitespace`), with its validators: max length and no NUL characters.
pub fn char_field(v: &Value, allow_blank: bool, max_length: Option<usize>) -> Result<String, Vec<String>> {
    let text = match v {
        Value::String(s) => s.clone(),
        Value::Number(_) => str_of(v),
        _ => return Err(vec!["Not a valid string.".into()]),
    };
    let text = strip(&text).to_owned();
    if text.is_empty() {
        return if allow_blank { Ok(text) } else { Err(vec!["This field may not be blank.".into()]) };
    }
    let mut messages = Vec::new();
    if let Some(max) = max_length
        && text.chars().count() > max
    {
        messages.push(format!("Ensure this field has no more than {max} characters."));
    }
    if text.contains('\0') {
        messages.push("Null characters are not allowed.".into());
    }
    if messages.is_empty() { Ok(text) } else { Err(messages) }
}

/// `BooleanField`.
pub fn bool_field(v: &Value) -> Result<bool, Vec<String>> {
    let truthy = match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_f64() {
            Some(1.0) => Some(true),
            Some(0.0) => Some(false),
            _ => None,
        },
        Value::String(s) => match s.as_str() {
            "t" | "T" | "y" | "Y" | "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" | "1" => {
                Some(true)
            }
            "f" | "F" | "n" | "N" | "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off" | "OFF" | "0" => {
                Some(false)
            }
            _ => None,
        },
        _ => None,
    };
    truthy.ok_or_else(|| vec!["Must be a valid boolean.".into()])
}

/// `IntegerField(min_value, max_value)`: `int(str(data))` once a trailing `.0` is dropped.
pub fn int_field(v: &Value, min: Option<i64>, max: Option<i64>) -> Result<i64, Vec<String>> {
    let invalid = || vec!["A valid integer is required.".to_owned()];
    let n = match v {
        Value::Bool(_) | Value::Null | Value::Array(_) | Value::Object(_) => return Err(invalid()),
        Value::String(s) if s.chars().count() > 1000 => {
            return Err(vec!["String value too large.".into()]);
        }
        other => {
            let text = str_of(other);
            static TRAILING_ZEROS: LazyLock<regex::Regex> =
                LazyLock::new(|| regex::Regex::new(r"\.0*\s*$").expect("valid"));
            let text = TRAILING_ZEROS.replace(&text, "").into_owned();
            match py_int(&text) {
                Some(PyInt::Small(n)) => n,
                // beyond 64 bits: only the range checks can tell
                Some(PyInt::Big(negative)) => {
                    return Err(range_errors(if negative { i64::MIN } else { i64::MAX }, min, max));
                }
                None => return Err(invalid()),
            }
        }
    };
    let errors = range_errors(n, min, max);
    if errors.is_empty() { Ok(n) } else { Err(errors) }
}

fn range_errors(n: i64, min: Option<i64>, max: Option<i64>) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(min) = min
        && n < min
    {
        errors.push(format!("Ensure this value is greater than or equal to {min}."));
    }
    if let Some(max) = max
        && n > max
    {
        errors.push(format!("Ensure this value is less than or equal to {max}."));
    }
    errors
}

/// upstream's `make_enum_field`: a choice by name (lower case).
pub fn choice_field(v: &Value, choices: &[(&str, i32)]) -> Result<i32, Vec<String>> {
    let key = str_of(v);
    choices
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
        .ok_or_else(|| vec![format!("\"{key}\" is not a valid choice.")])
}

/// `PrimaryKeyRelatedField` for an integer primary key: the id to look up, or the error for its type.
pub fn int_pk(v: &Value) -> Result<Option<i64>, Vec<String>> {
    let incorrect = || vec![format!("Incorrect type. Expected pk value, received {}.", type_name(v))];
    match v {
        Value::Number(n) => match n.as_i64() {
            Some(i) => Ok(Some(i)),
            // `int(float)` truncates
            None => match n.as_f64() {
                Some(f) if f.is_finite() && f.abs() < 9.2e18 => Ok(Some(f.trunc() as i64)),
                _ => Ok(None),
            },
        },
        Value::String(s) => match py_int(s) {
            Some(PyInt::Small(i)) => Ok(Some(i)),
            Some(PyInt::Big(_)) => Ok(None),
            None => Err(incorrect()),
        },
        _ => Err(incorrect()),
    }
}

/// `PrimaryKeyRelatedField` for a UUID primary key: Django's UUIDField parses it first.
pub fn uuid_pk(v: &Value) -> Result<uuid::Uuid, Vec<String>> {
    let parsed = match v {
        Value::Bool(_) => return Err(vec!["Incorrect type. Expected pk value, received bool.".into()]),
        Value::String(s) => py_uuid(s),
        Value::Number(n) => n.as_u64().map(|i| uuid::Uuid::from_u128(i as u128)),
        _ => None,
    };
    parsed.ok_or_else(|| vec![format!("“{}” is not a valid UUID.", str_of(v))])
}

/// The error for a primary key that names nothing.
pub fn does_not_exist(v: &Value) -> Vec<String> {
    vec![format!("Invalid pk \"{}\" - object does not exist.", str_of(v))]
}

const DATETIME_FORMAT: &str =
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

/// `DateTimeField(default_timezone=utc)`: Django's `parse_datetime` (Python's `fromisoformat`, then a looser
/// pattern); without an offset, UTC.
pub fn datetime_field(v: &Value) -> Result<DateTime<Utc>, Vec<String>> {
    let Value::String(s) = v else { return Err(vec![DATETIME_FORMAT.into()]) };
    parse_datetime(s).ok_or_else(|| vec![DATETIME_FORMAT.into()])
}

pub fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    fromisoformat(s).or_else(|| django_datetime_re(s))
}

/// The common shapes of Python 3.12's `datetime.fromisoformat`: a date (`YYYY-MM-DD` or `YYYYMMDD`), optionally
/// followed by a separator and a time (`HH[:MM[:SS[.f+]]]` or `HHMM[SS]`) with an offset (`Z`, `±HH[:MM]`).
fn fromisoformat(s: &str) -> Option<DateTime<Utc>> {
    if !s.is_ascii() || s.len() < 8 {
        return None;
    }
    let (date, rest) = if s.as_bytes().get(4) == Some(&b'-') {
        (NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()?, s.get(10..)?)
    } else {
        (NaiveDate::parse_from_str(s.get(..8)?, "%Y%m%d").ok()?, s.get(8..)?)
    };
    if !s.get(..4)?.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if rest.is_empty() {
        return Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?));
    }
    let time = &rest[1..]; // any one separator
    let (clock, offset) = match time.find(['Z', '+', '-']) {
        Some(i) => (&time[..i], Some(&time[i..])),
        None => (time, None),
    };
    let naive_time = iso_time(clock)?;
    let local = NaiveDateTime::new(date, naive_time);
    let offset = match offset {
        None => FixedOffset::east_opt(0)?,
        Some("Z") => FixedOffset::east_opt(0)?,
        Some(o) => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let digits = o[1..].replace(':', "");
            if digits.len() != 2 && digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let hours: i32 = digits[..2].parse().ok()?;
            let minutes: i32 = digits.get(2..).filter(|m| !m.is_empty()).map_or(Some(0), |m| m.parse().ok())?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))?
        }
    };
    Some(offset.from_local_datetime(&local).single()?.with_timezone(&Utc))
}

fn iso_time(clock: &str) -> Option<NaiveTime> {
    let (hms, fraction) = match clock.find(['.', ',']) {
        Some(i) => (&clock[..i], Some(&clock[i + 1..])),
        None => (clock, None),
    };
    let parts: Vec<&str> = if hms.contains(':') {
        hms.split(':').collect()
    } else {
        (0..hms.len()).step_by(2).map(|i| hms.get(i..i + 2).unwrap_or("")).collect()
    };
    let two_digits = |p: &&str| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit());
    if parts.is_empty() || parts.len() > 3 || !parts.iter().all(two_digits) {
        return None;
    }
    let num = |i: usize| parts.get(i).map_or(Some(0), |p| p.parse::<u32>().ok());
    let micros = match fraction {
        None => 0,
        Some(f) if parts.len() == 3 && !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()) => {
            let digits: String = f.chars().take(6).collect();
            format!("{digits:0<6}").parse().ok()?
        }
        Some(_) => return None,
    };
    NaiveTime::from_hms_micro_opt(num(0)?, num(1)?, num(2)?, micros)
}

/// Django's `datetime_re` fallback.
fn django_datetime_re(s: &str) -> Option<DateTime<Utc>> {
    static DATETIME_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(concat!(
            r"^(\d{4})-(\d{1,2})-(\d{1,2})[T ](\d{1,2}):(\d{1,2})(?::(\d{1,2})(?:[.,](\d{1,6})\d{0,6})?)?",
            r"\s*(Z|[+-]\d{2}(?::?\d{2})?)?$",
        ))
        .expect("valid")
    });
    let c = DATETIME_RE.captures(s)?;
    let n = |i: usize| c.get(i).map_or(Some(0), |m| m.as_str().parse::<u32>().ok());
    let micros = c.get(7).map_or(Some(0), |m| format!("{:0<6}", m.as_str()).parse().ok())?;
    let date = NaiveDate::from_ymd_opt(c[1].parse().ok()?, n(2)?, n(3)?)?;
    let time = NaiveTime::from_hms_micro_opt(n(4)?, n(5)?, n(6)?, micros)?;
    let offset = match c.get(8).map(|m| m.as_str()) {
        None | Some("Z") => 0,
        Some(o) => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let digits = o[1..].replace(':', "");
            let minutes: i32 = digits.get(2..).filter(|m| !m.is_empty()).map_or(Some(0), |m| m.parse().ok())?;
            sign * (digits[..2].parse::<i32>().ok()? * 60 + minutes) * 60
        }
    };
    let offset = FixedOffset::east_opt(offset)?;
    Some(offset.from_local_datetime(&NaiveDateTime::new(date, time)).single()?.with_timezone(&Utc))
}

pub fn opt_datetime(t: &Option<DateTime<Utc>>) -> Value {
    t.as_ref().map_or(Value::Null, |t| Value::String(drf_datetime(t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datetimes_like_django() {
        let utc = |s: &str| parse_datetime(s).map(|t| drf_datetime(&t));
        assert_eq!(utc("2026-11-02T03:00").as_deref(), Some("2026-11-02T03:00:00Z"));
        assert_eq!(utc("2026-11-02").as_deref(), Some("2026-11-02T00:00:00Z"));
        assert_eq!(utc("2026-11-02 01:30:00.5+01:00").as_deref(), Some("2026-11-02T00:30:00.500000Z"));
        assert_eq!(utc("20261102T0300Z").as_deref(), Some("2026-11-02T03:00:00Z"));
        assert_eq!(utc("2026-1-2 3:04").as_deref(), Some("2026-01-02T03:04:00Z"));
        assert_eq!(utc("garbage"), None);
        assert_eq!(utc("2026-11-02T25:00"), None);
    }

    #[test]
    fn fields_like_drf() {
        assert_eq!(int_field(&json!("12"), None, None), Ok(12));
        assert_eq!(int_field(&json!(2.0), None, None), Ok(2));
        assert_eq!(int_field(&json!(" 1_0 "), None, None), Ok(10));
        assert!(int_field(&json!(1.5), None, None).is_err());
        assert!(int_field(&json!(true), None, None).is_err());
        let too_small = vec!["Ensure this value is greater than or equal to 1.".to_owned()];
        assert_eq!(int_field(&json!(0), Some(1), None), Err(too_small));
        assert_eq!(char_field(&json!(123), false, None), Ok("123".into()));
        assert_eq!(char_field(&json!(true), false, None), Err(vec!["Not a valid string.".into()]));
        assert_eq!(bool_field(&json!("yes")), Ok(true));
        assert_eq!(bool_field(&json!(0.0)), Ok(false));
        assert_eq!(uuid_pk(&json!(5)).map(|u| u.to_string()), Ok("00000000-0000-0000-0000-000000000005".into()));
        assert_eq!(uuid_pk(&json!("abc")), Err(vec!["“abc” is not a valid UUID.".into()]));
        assert_eq!(int_pk(&json!("abc")), Err(vec!["Incorrect type. Expected pk value, received str.".into()]));
    }
}
