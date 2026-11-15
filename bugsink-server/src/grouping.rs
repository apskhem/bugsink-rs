//! Upstream-exact grouping: which issue an event belongs to, and the issue's display fields.
//!
//! Ported from upstream Bugsink 2.6.3 (`issues/utils.py`, `issues/grouping_mechanisms/`, and the vendored `sentry/`
//! helpers). Grouping keys must match upstream byte-for-byte: they are looked up against keys upstream already
//! stored, and a mismatch silently creates a duplicate issue. Hence the Python-semantics helpers in [`crate::py`].
//!
//! Where upstream would crash on malformed input (e.g. a non-string exception type combined with an empty value), we
//! produce a reasonable result instead; those paths are not part of the parity contract.

pub mod functions;
pub mod parameterization;

use serde_json::{
  Map,
  Value,
};
use sha2::{
  Digest,
  Sha256,
};

use crate::py::{
  py_str,
  splitlines,
  strip,
  take_chars,
  trim,
  truthy,
};

pub const LOG_MESSAGE_TYPE: &str = "Log Message";

pub const MECHANISM_INDEPENDENT: &str = "none";
pub const BUGSINK_V1: &str = "bugsink-v1";
pub const BUGSINK_V2: &str = "bugsink-v2";
pub const CURRENT_MECHANISM: &str = BUGSINK_V2;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GroupingError {
  /// Upstream raises (and drops the event) on these, too.
  #[error("expected {0} to be a list or a dict")]
  InvalidShape(&'static str),
  #[error("unknown grouping mechanism {0:?}")]
  UnknownMechanism(String),
}

type Result<T> = std::result::Result<T, GroupingError>;

// --- event shape helpers ----------------------------------------------------------------------------------------------

/// Sentry's `get_path` (object keys only): `None` if any step is missing, or if the final value is null.
pub(crate) fn get_path<'v>(data: &'v Value, keys: &[&str]) -> Option<&'v Value> {
  let mut cur = data;
  for key in keys {
    cur = cur.as_object()?.get(*key)?;
  }
  (!cur.is_null()).then_some(cur)
}

/// `get_values`: `exception`/`threads` as a list, whether given as a list, `{"values": [...]}`, or a bare dict.
fn get_values<'v>(obj: Option<&'v Value>, what: &'static str) -> Result<Option<Vec<&'v Value>>> {
  match obj {
    None => Ok(None),
    Some(Value::Array(items)) => Ok(Some(items.iter().collect())),
    Some(Value::Object(o)) => {
      match o.get("values") {
        None => Ok(Some(vec![obj.unwrap()])),
        Some(Value::Array(items)) => Ok(Some(items.iter().collect())),
        Some(Value::Null) => Ok(None),
        Some(_) => Err(GroupingError::InvalidShape(what)),
      }
    }
    Some(_) => Err(GroupingError::InvalidShape(what)),
  }
}

/// The last exception in the chain: the one you're most likely to care about.
pub fn main_exception(data: &Value) -> Result<Option<&Value>> {
  if data
    .get("exception")
    .and_then(Value::as_array)
    .is_some_and(Vec::is_empty)
  {
    return Ok(None);
  }
  let values = get_values(get_path(data, &["exception"]), "exception")?;
  Ok(values.and_then(|v| v.last().copied()).filter(|e| truthy(e)))
}

fn as_frames(v: Option<&Value>) -> Option<&Vec<Value>> {
  v.and_then(Value::as_array)
}

/// `get_crash_frame_from_event_data`: innermost in-app frame of the main exception (or top-level stacktrace, or the
/// only thread), falling back to the innermost frame.
type FrameFilter<'f> = &'f dyn Fn(&Map<String, Value>) -> bool;

fn crash_frame<'v>(data: &'v Value, filter: Option<FrameFilter>) -> Result<Option<&'v Map<String, Value>>> {
  let values = get_values(get_path(data, &["exception"]), "exception")?;
  let from_exception = values
    .as_ref()
    .and_then(|v| v.last())
    .and_then(|last| as_frames(get_path(last, &["stacktrace", "frames"])));
  let mut frames = from_exception
    .filter(|f| !f.is_empty())
    .or_else(|| as_frames(get_path(data, &["stacktrace", "frames"])).filter(|f| !f.is_empty()));

  if frames.is_none() {
    let threads = get_values(get_path(data, &["threads"]), "threads")?;
    if let Some(threads) = threads.filter(|t| t.len() == 1) {
      frames = as_frames(get_path(threads[0], &["stacktrace", "frames"]));
    }
  }

  let mut default = None;
  for frame in frames.into_iter().flatten().rev() {
    let Some(frame) = frame.as_object() else { continue };
    if filter.is_some_and(|f| !f(frame)) {
      continue;
    }
    if frame.get("in_app").is_some_and(truthy) {
      return Ok(Some(frame));
    }
    default.get_or_insert(frame);
  }
  Ok(default.filter(|f| !f.is_empty()))
}

/// `get_crash_location`: (filename, function) of the crash frame, skipping frames without a meaningful function.
fn crash_location(data: &Value) -> Result<(Option<Value>, Option<Value>)> {
  let has_function = |f: &Map<String, Value>| {
    match f.get("function") {
      None | Some(Value::Null) => false,
      Some(Value::String(s)) => s != "<redacted>" && s != "<unknown>",
      Some(_) => true,
    }
  };
  let Some(frame) = crash_frame(data, Some(&has_function))? else {
    return Ok((None, None));
  };
  let function = functions::function_name_for_frame(frame, data.get("platform"));
  let filename = frame
    .get("filename")
    .filter(|f| truthy(f))
    .or_else(|| frame.get("abs_path"))
    .cloned();
  Ok((filename, function))
}

// --- type & value -----------------------------------------------------------------------------------------------------

/// Upstream has two copies of the type/value logic that differ only in the order in which message fields are tried:
/// the one feeding grouping keys (`building_blocks/v1.py`) and the one for display (`issues/utils.py`).
#[derive(Clone, Copy)]
enum Variant {
  Grouping,
  Display,
}

fn type_and_value_raw(data: &Value, variant: Variant) -> Result<(Value, Value)> {
  if data.get("exception").is_some_and(truthy) {
    return exception_type_and_value(data);
  }

  let paths: [[&str; 2]; 4] = match variant {
    Variant::Grouping => {
      [
        ["logentry", "message"],
        ["logentry", "formatted"],
        ["message", "message"],
        ["message", "formatted"],
      ]
    }
    Variant::Display => {
      [
        ["logentry", "formatted"],
        ["logentry", "message"],
        ["message", "formatted"],
        ["message", "message"],
      ]
    }
  };
  let first_truthy = paths
    .iter()
    .find_map(|[a, b]| get_path(data, &[a, b]).filter(|v| truthy(v)));
  let mut message = first_truthy.map(|v| strip(&py_str(v)).to_string()).unwrap_or_default();
  if let (true, Some(Value::String(s))) = (message.is_empty(), data.get("message")) {
    message = s.clone();
  }
  let value = if message.is_empty() {
    "<no log message>".to_string()
  } else {
    take_chars(splitlines(&message).first().copied().unwrap_or(""), 1024)
  };
  Ok((Value::String(LOG_MESSAGE_TYPE.into()), Value::String(value)))
}

fn exception_type_and_value(data: &Value) -> Result<(Value, Value)> {
  let unknown = || (Value::String("<unknown>".into()), Value::String(String::new()));
  let Some(exception) = main_exception(data)? else {
    return Ok(unknown());
  };
  let value = trim(
    get_path(exception, &["value"]).unwrap_or(&Value::String(String::new())),
    1024,
  );

  if get_path(exception, &["mechanism", "synthetic"]).is_some_and(truthy) {
    let (_, function) = crash_location(data)?;
    return Ok(match function.filter(truthy) {
      Some(f) => (f, Value::String(String::new())),
      None => unknown(),
    });
  }

  let ty = trim(
    get_path(exception, &["type"]).unwrap_or(&Value::String("Error".into())),
    128,
  );
  Ok((ty, value))
}

/// The issue's/event's `calculated_type` and `calculated_value` columns.
pub fn display_type_and_value(data: &Value) -> Result<(String, String)> {
  let (ty, value) = type_and_value_raw(data, Variant::Display)?;
  Ok((py_str(&ty), py_str(&value)))
}

// --- grouping keys ----------------------------------------------------------------------------------------------------

/// `get_title_for_exception_type_and_value` as used by the groupers (no special-casing of log messages).
fn grouper_title(ty: &Value, value: &Value) -> String {
  if !truthy(value) {
    return py_str(ty);
  }
  let value = py_str(value);
  format!("{}: {}", py_str(ty), splitlines(&value).first().copied().unwrap_or(""))
}

fn grouper(mechanism: &str, data: &Value, ty: &Value, value: &Value) -> String {
  match mechanism {
    BUGSINK_V1 => {
      let transaction = data.get("transaction").filter(|t| truthy(t)).map(py_str);
      format!(
        "{} ⋄ {}",
        grouper_title(ty, value),
        transaction.as_deref().unwrap_or("<no transaction>")
      )
    }
    _ => {
      let normalized = parameterization::normalize_message_for_grouping(&py_str(value));
      grouper_title(ty, &Value::String(normalized))
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyWithMechanism {
  pub key: String,
  pub mechanism: String,
}

impl KeyWithMechanism {
  /// `issues_grouping.grouping_key_hash`: hex sha256 of the key.
  pub fn hash(&self) -> String {
    Sha256::digest(self.key.as_bytes())
      .iter()
      .map(|b| format!("{b:02x}"))
      .collect()
  }
}

/// `get_key_with_mechanism_for_data`. An explicit `fingerprint` replaces the computed key; if it doesn't include
/// `{{ default }}`, the key is mechanism-independent.
pub fn key_with_mechanism(data: &Value, mechanism: &str) -> Result<KeyWithMechanism> {
  if mechanism != BUGSINK_V1 && mechanism != BUGSINK_V2 {
    return Err(GroupingError::UnknownMechanism(mechanism.into()));
  }
  let (ty, value) = type_and_value_raw(data, Variant::Grouping)?;

  let Some(fingerprint) = data.get("fingerprint").filter(|f| truthy(f)) else {
    return Ok(KeyWithMechanism {
      key: grouper(mechanism, data, &ty, &value),
      mechanism: mechanism.into(),
    });
  };

  // Python iterates whatever it was given: list items, a string's characters, a dict's keys.
  let parts: Vec<Value> = match fingerprint {
    Value::Array(items) => items.clone(),
    Value::String(s) => s.chars().map(|c| Value::String(c.into())).collect(),
    Value::Object(o) => o.keys().map(|k| Value::String(k.clone())).collect(),
    other => vec![other.clone()],
  };
  let mut used_default = false;
  let rendered: Vec<String> = parts
    .iter()
    .map(|part| {
      if part.as_str() == Some("{{ default }}") {
        used_default = true;
        grouper(mechanism, data, &ty, &value)
      } else {
        py_str(part)
      }
    })
    .collect();
  Ok(KeyWithMechanism {
    key: rendered.join(" ⋄ "),
    mechanism: if used_default {
      mechanism.into()
    } else {
      MECHANISM_INDEPENDENT.into()
    },
  })
}

// --- denormalized fields ----------------------------------------------------------------------------------------------

/// `get_denormalized_fields_for_data`: issue/event columns derived from the crash frame and transaction.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Denormalized {
  pub transaction: String,
  pub last_frame_filename: String,
  pub last_frame_module: String,
  pub last_frame_function: String,
}

pub fn denormalized_fields(data: &Value) -> Result<Denormalized> {
  let empty = Map::new();
  let frame = crash_frame(data, None)?.unwrap_or(&empty);
  let text = |v: Option<&Value>, max: usize| {
    v.filter(|v| truthy(v))
      .map(|v| take_chars(&py_str(v), max))
      .unwrap_or_default()
  };
  Ok(Denormalized {
    transaction: text(data.get("transaction"), 200),
    last_frame_filename: text(frame.get("filename"), 255),
    last_frame_module: text(frame.get("module"), 255),
    last_frame_function: text(
      functions::function_name_for_frame(frame, data.get("platform")).as_ref(),
      255,
    ),
  })
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  #[test]
  fn exception_event() {
    let event = json!({"transaction": "/api", "exception": {"values": [
        {"type": "KeyError", "value": "'a'"},
        {"type": "ValueError", "value": "user 17 bad\nsecond line",
         "stacktrace": {"frames": [{"function": "outer", "in_app": true}, {"function": "inner", "filename": "lib.py"}]}},
    ]}});
    assert_eq!(
      display_type_and_value(&event).unwrap(),
      ("ValueError".into(), "user 17 bad\nsecond line".into())
    );
    assert_eq!(
      key_with_mechanism(&event, BUGSINK_V2).unwrap().key,
      "ValueError: user <int> bad"
    );
    assert_eq!(
      key_with_mechanism(&event, BUGSINK_V1).unwrap().key,
      "ValueError: user 17 bad ⋄ /api"
    );
    let d = denormalized_fields(&event).unwrap();
    assert_eq!(
      (d.last_frame_function.as_str(), d.transaction.as_str()),
      ("outer", "/api")
    ); // in_app wins
  }

  #[test]
  fn log_messages_differ_by_variant() {
    let event = json!({"logentry": {"message": "raw %s", "formatted": "raw 5"}});
    assert_eq!(display_type_and_value(&event).unwrap().1, "raw 5");
    assert_eq!(
      key_with_mechanism(&event, BUGSINK_V2).unwrap().key,
      "Log Message: raw %s"
    );
    assert_eq!(display_type_and_value(&json!({})).unwrap().1, "<no log message>");
  }

  #[test]
  fn fingerprints() {
    let event = json!({"message": "x 1", "fingerprint": ["{{ default }}", "tenant", 7]});
    let k = key_with_mechanism(&event, BUGSINK_V2).unwrap();
    assert_eq!(
      (k.key.as_str(), k.mechanism.as_str()),
      ("Log Message: x <int> ⋄ tenant ⋄ 7", BUGSINK_V2)
    );
    let k = key_with_mechanism(&json!({"fingerprint": ["a"]}), BUGSINK_V2).unwrap();
    assert_eq!((k.key.as_str(), k.mechanism.as_str()), ("a", MECHANISM_INDEPENDENT));
  }

  #[test]
  fn invalid_shapes_error_like_upstream() {
    assert_eq!(
      main_exception(&json!({"exception": "boom"})),
      Err(GroupingError::InvalidShape("exception"))
    );
  }

  #[test]
  fn synthetic_uses_function() {
    let event = json!({"platform": "native", "exception": {"values": [{"type": "SIGSEGV", "mechanism": {"synthetic": true},
            "stacktrace": {"frames": [{"function": "std::vector<int>::at(int) const"}]}}]}});
    assert_eq!(
      display_type_and_value(&event).unwrap(),
      ("std::vector<T>::at".into(), "".into())
    );
  }
}
