//! Python and Django semantics that upstream's behaviour depends on, shared by grouping, display and the APIs.
//!
//! Grouping keys are compared byte-for-byte with keys upstream already stored, so "close enough" is not: e.g.
//! `str.splitlines()` splits on 11 different line boundaries, `str(1e16)` is `1e+16`, and Django's `truncatechars`
//! NFC-normalizes its input even when it doesn't truncate.

use chrono::{
  DateTime,
  Utc,
};
use serde_json::{
  Number,
  Value,
};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

/// Python truthiness of a JSON value.
pub fn truthy(v: &Value) -> bool {
  match v {
    Value::Null => false,
    Value::Bool(b) => *b,
    Value::Number(n) => n.as_f64() != Some(0.0),
    Value::String(s) => !s.is_empty(),
    Value::Array(a) => !a.is_empty(),
    Value::Object(o) => !o.is_empty(),
  }
}

/// `str(v)` for a value that came from `json.loads`.
pub fn py_str(v: &Value) -> String {
  match v {
    Value::String(s) => s.clone(),
    other => py_repr(other),
  }
}

/// `repr(v)` for a value that came from `json.loads`.
pub fn py_repr(v: &Value) -> String {
  match v {
    Value::Null => "None".into(),
    Value::Bool(true) => "True".into(),
    Value::Bool(false) => "False".into(),
    Value::Number(n) => number_repr(n),
    Value::String(s) => str_repr(s),
    Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
    Value::Object(o) => {
      format!(
        "{{{}}}",
        o.iter()
          .map(|(k, v)| format!("{}: {}", str_repr(k), py_repr(v)))
          .collect::<Vec<_>>()
          .join(", ")
      )
    }
  }
}

fn number_repr(n: &Number) -> String {
  if n.is_i64() || n.is_u64() {
    n.to_string()
  } else {
    float_repr(n.as_f64().unwrap_or(f64::NAN))
  }
}

/// `repr(float)`: shortest round-trip digits; scientific notation iff the decimal exponent is < -4 or >= 16.
pub fn float_repr(f: f64) -> String {
  if f.is_nan() {
    return "nan".into();
  }
  if f.is_infinite() {
    return if f > 0.0 { "inf".into() } else { "-inf".into() };
  }
  if f == 0.0 {
    return if f.is_sign_negative() {
      "-0.0".into()
    } else {
      "0.0".into()
    };
  }
  let sci = format!("{f:e}"); // shortest round-trip, e.g. "-1.2345e17"
  let (mantissa, exp) = sci.split_once('e').expect("LowerExp always has an exponent");
  let exp: i32 = exp.parse().expect("exponent is an integer");
  let (sign, mantissa) = mantissa.strip_prefix('-').map_or(("", mantissa), |m| ("-", m));
  let digits: String = mantissa.chars().filter(|c| *c != '.').collect();

  if !(-4..16).contains(&exp) {
    let (first, rest) = digits.split_at(1);
    let frac = if rest.is_empty() {
      String::new()
    } else {
      format!(".{rest}")
    };
    let exp_sign = if exp < 0 { '-' } else { '+' };
    return format!("{sign}{first}{frac}e{exp_sign}{:02}", exp.abs());
  }
  if exp < 0 {
    return format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize));
  }
  let int_len = exp as usize + 1;
  if digits.len() <= int_len {
    format!("{sign}{digits}{}.0", "0".repeat(int_len - digits.len()))
  } else {
    format!("{sign}{}.{}", &digits[..int_len], &digits[int_len..])
  }
}

/// `repr(str)`.
pub fn str_repr(s: &str) -> String {
  let quote = if s.contains('\'') && !s.contains('"') {
    '"'
  } else {
    '\''
  };
  let mut out = String::with_capacity(s.len() + 2);
  out.push(quote);
  for c in s.chars() {
    match c {
      '\\' => out.push_str("\\\\"),
      '\t' => out.push_str("\\t"),
      '\n' => out.push_str("\\n"),
      '\r' => out.push_str("\\r"),
      c if c == quote => {
        out.push('\\');
        out.push(c);
      }
      c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
      c if (c as u32) < 0x7f || is_printable(c) => out.push(c),
      c if (c as u32) <= 0xff => out.push_str(&format!("\\x{:02x}", c as u32)),
      c if (c as u32) <= 0xffff => out.push_str(&format!("\\u{:04x}", c as u32)),
      c => out.push_str(&format!("\\U{:08x}", c as u32)),
    }
  }
  out.push(quote);
  out
}

/// Approximates `str.isprintable()` for non-ASCII: control, separator (other than space) and common format chars.
fn is_printable(c: char) -> bool {
  !(c.is_control()
    || (c.is_whitespace() && c != ' ')
    || matches!(c, '\u{ad}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{feff}'))
}

/// `str.isspace()`: Unicode whitespace plus the ASCII separators \x1c-\x1f.
pub fn is_space(c: char) -> bool {
  c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

pub fn strip(s: &str) -> &str {
  s.trim_matches(is_space)
}

pub fn rstrip(s: &str) -> &str {
  s.trim_end_matches(is_space)
}

/// `str.splitlines()`: no trailing empty element; `\r\n` is one boundary.
pub fn splitlines(s: &str) -> Vec<&str> {
  let mut lines = Vec::new();
  let mut start = 0;
  let mut iter = s.char_indices().peekable();
  while let Some((i, c)) = iter.next() {
    let is_boundary = matches!(
      c,
      '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    );
    if is_boundary {
      lines.push(&s[start..i]);
      let mut next = i + c.len_utf8();
      if c == '\r' && iter.peek().map(|(_, n)| *n) == Some('\n') {
        iter.next();
        next += 1;
      }
      start = next;
    }
  }
  if start < s.len() {
    lines.push(&s[start..]);
  }
  lines
}

/// `s[:n]` (code points).
pub fn take_chars(s: &str, n: usize) -> String {
  s.chars().take(n).collect()
}

/// Django's `truncatechars` (`Truncator.chars`): NFC-normalizes, doesn't count combining characters, and ends a
/// truncated string with "…" such that the result counts `length` characters.
pub fn truncatechars(value: &str, length: i64) -> String {
  if length <= 0 {
    return String::new();
  }
  let length = length as usize;
  let text: String = value.nfc().collect();
  let truncate_len = length - 1; // room for the "…"

  let mut counted = 0;
  let mut end_index = None;
  for (i, c) in text.chars().enumerate() {
    if canonical_combining_class(c) != 0 {
      continue;
    }
    counted += 1;
    if end_index.is_none() && counted > truncate_len {
      end_index = Some(i);
    }
    if counted > length {
      return format!("{}…", take_chars(&text, end_index.unwrap_or(0)));
    }
  }
  text
}

/// Sentry's `trim()` (vendored upstream as `sentry.utils.safe.trim`): size-limits a value; strings via `truncatechars`,
/// containers recursively with sorted keys.
pub fn trim(value: &Value, max_size: i64) -> Value {
  trim_inner(value, max_size, 6, 0, 0)
}

fn trim_inner(value: &Value, max_size: i64, max_depth: usize, depth: usize, mut size: i64) -> Value {
  if depth > max_depth {
    let s = match value {
      Value::String(s) => s.clone(),
      other => json_dumps(other),
    };
    return Value::String(truncatechars(&s, max_size - size));
  }
  match value {
    Value::Object(obj) => {
      let mut keys: Vec<&String> = obj.keys().collect();
      keys.sort();
      let mut result = serde_json::Map::new();
      size += 2;
      for k in keys {
        let trimmed = trim_inner(&obj[k.as_str()], max_size, max_depth, depth + 1, size);
        size += py_str(&trimmed).chars().count() as i64 + 1;
        result.insert(k.clone(), trimmed);
        if size >= max_size {
          break;
        }
      }
      Value::Object(result)
    }
    Value::Array(items) => {
      let mut result = Vec::new();
      size += 2;
      for v in items {
        let trimmed = trim_inner(v, max_size, max_depth, depth + 1, size);
        size += py_str(&trimmed).chars().count() as i64;
        result.push(trimmed);
        if size >= max_size {
          break;
        }
      }
      Value::Array(result)
    }
    Value::String(s) => Value::String(truncatechars(s, max_size - size)),
    other => other.clone(),
  }
}

/// `json.dumps(v)` with Python's defaults: `", "` / `": "` separators, `ensure_ascii=True`, Python float repr.
pub fn json_dumps(v: &Value) -> String {
  let mut out = String::new();
  write_json(v, JsonStyle::Python, &mut out);
  out
}

#[derive(Clone, Copy, PartialEq)]
pub enum JsonStyle {
  /// `json.dumps` defaults.
  Python,
  /// DRF's `JSONRenderer`: `separators=(",", ":")`, `ensure_ascii=False`, and U+2028 / U+2029 escaped.
  Drf,
}

pub fn write_json(v: &Value, style: JsonStyle, out: &mut String) {
  let (item_sep, key_sep) = match style {
    JsonStyle::Python => (", ", ": "),
    JsonStyle::Drf => (",", ":"),
  };
  match v {
    Value::Null => out.push_str("null"),
    Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
    Value::Number(n) => out.push_str(&number_repr(n)),
    Value::String(s) => write_json_str(s, style, out),
    Value::Array(a) => {
      out.push('[');
      for (i, item) in a.iter().enumerate() {
        if i > 0 {
          out.push_str(item_sep);
        }
        write_json(item, style, out);
      }
      out.push(']');
    }
    Value::Object(o) => {
      out.push('{');
      for (i, (k, item)) in o.iter().enumerate() {
        if i > 0 {
          out.push_str(item_sep);
        }
        write_json_str(k, style, out);
        out.push_str(key_sep);
        write_json(item, style, out);
      }
      out.push('}');
    }
  }
}

fn write_json_str(s: &str, style: JsonStyle, out: &mut String) {
  out.push('"');
  for c in s.chars() {
    match c {
      '"' => out.push_str("\\\""),
      '\\' => out.push_str("\\\\"),
      '\n' => out.push_str("\\n"),
      '\r' => out.push_str("\\r"),
      '\t' => out.push_str("\\t"),
      '\u{08}' => out.push_str("\\b"),
      '\u{0c}' => out.push_str("\\f"),
      ' '..='~' => out.push(c),
      c if style == JsonStyle::Drf && c > '~' && !matches!(c, '\u{2028}' | '\u{2029}') => out.push(c),
      c => {
        let mut buf = [0u16; 2];
        for unit in c.encode_utf16(&mut buf) {
          out.push_str(&format!("\\u{unit:04x}"));
        }
      }
    }
  }
  out.push('"');
}

/// Django's `escape` (and Python's `html.escape`): `& < > " '` as entities.
pub fn html_escape(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  for c in s.chars() {
    match c {
      '&' => out.push_str("&amp;"),
      '<' => out.push_str("&lt;"),
      '>' => out.push_str("&gt;"),
      '"' => out.push_str("&quot;"),
      '\'' => out.push_str("&#x27;"),
      c => out.push(c),
    }
  }
  out
}

/// `datetime.isoformat()` for a UTC datetime: microseconds only when non-zero, `+00:00` offset.
pub fn isoformat(dt: &DateTime<Utc>) -> String {
  format_utc(dt, 'T', "+00:00")
}

/// `str(datetime)` for a UTC datetime: `isoformat()` with a space between date and time.
pub fn str_datetime(dt: &DateTime<Utc>) -> String {
  format_utc(dt, ' ', "+00:00")
}

/// DRF's `DateTimeField` output in UTC: `isoformat()` with `Z` for the offset.
pub fn drf_datetime(dt: &DateTime<Utc>) -> String {
  format_utc(dt, 'T', "Z")
}

fn format_utc(dt: &DateTime<Utc>, sep: char, offset: &str) -> String {
  let micros = dt.timestamp_subsec_micros();
  let fraction = if micros == 0 {
    String::new()
  } else {
    format!(".{micros:06}")
  };
  format!(
    "{}{sep}{}{fraction}{offset}",
    dt.format("%Y-%m-%d"),
    dt.format("%H:%M:%S")
  )
}

pub enum PyInt {
  Small(i64),
  /// Too big for 64 bits; true if negative.
  Big(bool),
}

/// Python's `int(text)`: surrounding whitespace, a sign, and underscores between digits.
pub fn py_int(text: &str) -> Option<PyInt> {
  let t = strip(text);
  let (negative, digits) = match t.strip_prefix('-') {
    Some(rest) => (true, rest),
    None => (false, t.strip_prefix('+').unwrap_or(t)),
  };
  if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
    return None;
  }
  let digits: String = digits.chars().filter(|c| *c != '_').collect();
  if !digits.chars().all(|c| c.is_ascii_digit()) {
    return None;
  }
  let signed = if negative { format!("-{digits}") } else { digits };
  Some(match signed.parse::<i64>() {
    Ok(n) => PyInt::Small(n),
    Err(_) => PyInt::Big(negative),
  })
}

/// Python's `uuid.UUID(hex)`: optional `urn:uuid:` and braces, hyphens anywhere, 32 hex digits.
pub fn py_uuid(s: &str) -> Option<uuid::Uuid> {
  let hex = s.replace("urn:", "").replace("uuid:", "");
  let hex = hex.trim_matches(|c| c == '{' || c == '}').replace('-', "");
  if hex.chars().count() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
    return None;
  }
  uuid::Uuid::try_parse(&hex).ok()
}

/// Upstream's boolean environment variables: only "true", "1" and "yes" (any case) are true.
pub fn py_bool(s: &str) -> Result<bool, String> {
  Ok(["true", "1", "yes"].contains(&s.to_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  #[test]
  fn floats_like_python() {
    for (f, expected) in [
      (1.0, "1.0"),
      (0.1, "0.1"),
      (1e16, "1e+16"),
      (1e15, "1000000000000000.0"),
      (1.5e-5, "1.5e-05"),
      (0.0001, "0.0001"),
      (-2.5, "-2.5"),
      (123456789012345678.0, "1.2345678901234568e+17"),
    ] {
      assert_eq!(float_repr(f), expected, "{f}");
    }
  }

  #[test]
  fn repr_like_python() {
    assert_eq!(
      py_str(&json!({"a": [1, true, null, "it's"]})),
      r#"{'a': [1, True, None, "it's"]}"#
    );
    assert_eq!(py_str(&json!("x")), "x");
    assert_eq!(str_repr("a\nb\u{1}"), r"'a\nb\x01'");
  }

  #[test]
  fn splitlines_like_python() {
    assert_eq!(splitlines("a\r\nb\rc\u{2028}d\n"), ["a", "b", "c", "d"]);
    assert_eq!(splitlines("\n"), [""]);
    assert!(splitlines("").is_empty());
  }

  #[test]
  fn truncatechars_like_django() {
    assert_eq!(truncatechars("abcdef", 4), "abc…");
    assert_eq!(truncatechars("abcd", 4), "abcd");
    assert_eq!(truncatechars("e\u{301}", 10), "\u{e9}"); // NFC even without truncation
  }

  #[test]
  fn json_dumps_like_python() {
    let expected = "{\"a\": [1, 2.0], \"\\u00e9\": \"\\ud83d\\ude00\"}";
    assert_eq!(json_dumps(&json!({"a": [1, 2.0], "\u{e9}": "\u{1f600}"})), expected);
  }
}
