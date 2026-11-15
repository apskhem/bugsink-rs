//! JSON as upstream's REST API reads and writes it: Python's `json` module behind Django REST Framework.
//!
//! Reading: `json.loads` (strict), so a malformed body gets Python's message ("Expecting value: line 1 column 1
//! (char 0)"). Writing: DRF's `JSONRenderer`: compact separators, non-ASCII as is, Python's float repr, and U+2028 /
//! U+2029 escaped.

use serde_json::{
  Map,
  Number,
  Value,
};

use crate::py::{
  JsonStyle,
  str_repr,
  write_json,
};

/// `json.dumps(v, ensure_ascii=False, allow_nan=False, separators=(",", ":"))` as DRF renders it.
pub fn dumps(v: &Value) -> String {
  let mut out = String::new();
  write_json(v, JsonStyle::Drf, &mut out);
  out
}

/// A request body as DRF's `JSONParser` reads it; Err: the message after "JSON parse error - ".
pub fn loads(body: &[u8]) -> Result<Value, String> {
  let s = std::str::from_utf8(body).map_err(|e| utf8_error(body, &e))?;
  let chars: Vec<char> = s.chars().collect();
  if chars.first() == Some(&'\u{feff}') {
    return Err(at(&chars, "Unexpected UTF-8 BOM (decode using utf-8-sig)", 0));
  }
  let mut p = Parser { s: &chars };
  let start = p.ws(0);
  let (value, end) = match p.value(start)? {
    Some(found) => found,
    None => return Err(at(&chars, "Expecting value", start)),
  };
  let end = p.ws(end);
  if end != chars.len() {
    return Err(at(&chars, "Extra data", end));
  }
  Ok(value)
}

/// Python's `UnicodeDecodeError` message for the utf-8 codec.
fn utf8_error(body: &[u8], e: &std::str::Utf8Error) -> String {
  let pos = e.valid_up_to();
  let first = body[pos];
  let reason = match e.error_len() {
    None => "unexpected end of data",
    Some(_) if matches!(first, 0x80..=0xc1 | 0xf5..=0xff) => "invalid start byte",
    Some(_) => "invalid continuation byte",
  };
  match e.error_len() {
    Some(len) if len > 1 => {
      format!(
        "'utf-8' codec can't decode bytes in position {pos}-{}: {reason}",
        pos + len - 1
      )
    }
    None if body.len() - pos > 1 => {
      format!(
        "'utf-8' codec can't decode bytes in position {pos}-{}: {reason}",
        body.len() - 1
      )
    }
    _ => format!("'utf-8' codec can't decode byte 0x{first:02x} in position {pos}: {reason}"),
  }
}

/// `JSONDecodeError`'s message: what, then where (line and column count characters, from 1).
fn at(s: &[char], msg: &str, pos: usize) -> String {
  let before = &s[..pos.min(s.len())];
  let line = before.iter().filter(|c| **c == '\n').count() + 1;
  let col = match before.iter().rposition(|c| *c == '\n') {
    Some(nl) => pos - nl,
    None => pos + 1,
  };
  format!("{msg}: line {line} column {col} (char {pos})")
}

struct Parser<'a> {
  s: &'a [char],
}

impl Parser<'_> {
  fn ws(&self, mut i: usize) -> usize {
    while i < self.s.len() && matches!(self.s[i], ' ' | '\t' | '\n' | '\r') {
      i += 1;
    }
    i
  }

  fn starts_with(&self, i: usize, word: &str) -> bool {
    word.chars().enumerate().all(|(k, c)| self.s.get(i + k) == Some(&c))
  }

  /// `scan_once`: None where there is no value at all ("Expecting value" at `i`, said by the caller).
  fn value(&mut self, i: usize) -> Result<Option<(Value, usize)>, String> {
    let Some(&c) = self.s.get(i) else { return Ok(None) };
    Ok(Some(match c {
      '"' => {
        let (s, end) = self.string(i + 1)?;
        (Value::String(s), end)
      }
      '{' => self.object(i + 1)?,
      '[' => self.array(i + 1)?,
      'n' if self.starts_with(i, "null") => (Value::Null, i + 4),
      't' if self.starts_with(i, "true") => (Value::Bool(true), i + 4),
      'f' if self.starts_with(i, "false") => (Value::Bool(false), i + 5),
      'N' if self.starts_with(i, "NaN") => return Err(constant("NaN")),
      'I' if self.starts_with(i, "Infinity") => return Err(constant("Infinity")),
      '-' if self.starts_with(i, "-Infinity") => return Err(constant("-Infinity")),
      _ => {
        match self.number(i) {
          Some(found) => found,
          None => return Ok(None),
        }
      }
    }))
  }

  /// `-?(?:0|[1-9]\d*)(\.\d+)?([eE][-+]?\d+)?`
  fn number(&self, i: usize) -> Option<(Value, usize)> {
    let s = self.s;
    let digits = |mut j: usize| {
      let start = j;
      while j < s.len() && s[j].is_ascii_digit() {
        j += 1;
      }
      (j > start).then_some(j)
    };
    let mut j = i;
    if s.get(j) == Some(&'-') {
      j += 1;
    }
    match s.get(j) {
      Some('0') => j += 1,
      Some('1'..='9') => j = digits(j)?,
      _ => return None,
    }
    let int_end = j;
    if s.get(j) == Some(&'.')
      && let Some(end) = digits(j + 1)
    {
      j = end;
    }
    if matches!(s.get(j), Some('e' | 'E')) {
      let mut k = j + 1;
      if matches!(s.get(k), Some('+' | '-')) {
        k += 1;
      }
      if let Some(end) = digits(k) {
        j = end;
      }
    }
    let text: String = s[i..j].iter().collect();
    let value = if j == int_end {
      match text.parse::<i64>() {
        Ok(n) => Value::from(n),
        Err(_) => {
          match text.parse::<u64>() {
            Ok(n) => Value::from(n),
            // Python's ints don't overflow; ours become floats
            Err(_) => Number::from_f64(text.parse().unwrap_or(0.0)).map_or(Value::Null, Value::Number),
          }
        }
      }
    } else {
      Number::from_f64(text.parse().unwrap_or(0.0)).map_or(Value::Null, Value::Number)
    };
    Some((value, j))
  }

  /// `scanstring`: from just after the opening quote.
  fn string(&self, start: usize) -> Result<(String, usize), String> {
    let s = self.s;
    let begin = start - 1;
    let mut out = String::new();
    let mut i = start;
    loop {
      let Some(&c) = s.get(i) else {
        return Err(at(s, "Unterminated string starting at", begin));
      };
      match c {
        '"' => return Ok((out, i + 1)),
        '\\' => {
          let Some(&esc) = s.get(i + 1) else {
            return Err(at(s, "Unterminated string starting at", begin));
          };
          if esc != 'u' {
            out.push(match esc {
              '"' => '"',
              '\\' => '\\',
              '/' => '/',
              'b' => '\u{08}',
              'f' => '\u{0c}',
              'n' => '\n',
              'r' => '\r',
              't' => '\t',
              _ => return Err(at(s, "Invalid \\escape", i)),
            });
            i += 2;
            continue;
          }
          let mut code = self.hex4(i + 1)?;
          i += 6;
          if (0xd800..=0xdbff).contains(&code) && s.get(i) == Some(&'\\') && s.get(i + 1) == Some(&'u') {
            let low = self.hex4(i + 1)?;
            if (0xdc00..=0xdfff).contains(&low) {
              code = 0x10000 + (((code - 0xd800) << 10) | (low - 0xdc00));
              i += 6;
            }
          }
          // a lone surrogate: Python keeps it, Rust strings can't
          out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
        }
        c if (c as u32) < 0x20 => return Err(at(s, "Invalid control character at", i)),
        c => {
          out.push(c);
          i += 1;
        }
      }
    }
  }

  /// The four hex digits after the `u` at `pos` (CPython's C scanner: there must be room for more after them).
  fn hex4(&self, pos: usize) -> Result<u32, String> {
    let digits = self.s.get(pos + 1..pos + 5).filter(|_| pos + 5 < self.s.len());
    match digits {
      Some(d) if d.iter().all(char::is_ascii_hexdigit) => {
        Ok(d.iter().fold(0, |n, c| n * 16 + c.to_digit(16).unwrap_or(0)))
      }
      _ => Err(at(self.s, "Invalid \\uXXXX escape", pos)),
    }
  }

  /// `JSONObject`: from just after the `{`.
  fn object(&mut self, mut end: usize) -> Result<(Value, usize), String> {
    let s = self.s;
    let mut map = Map::new();
    end = self.ws(end);
    match s.get(end) {
      Some('}') => return Ok((Value::Object(map), end + 1)),
      Some('"') => {}
      _ => return Err(at(s, "Expecting property name enclosed in double quotes", end)),
    }
    end += 1;
    loop {
      let (key, after) = self.string(end)?;
      end = self.ws(after);
      if s.get(end) != Some(&':') {
        return Err(at(s, "Expecting ':' delimiter", end));
      }
      end = self.ws(end + 1);
      let Some((value, after)) = self.value(end)? else {
        return Err(at(s, "Expecting value", end));
      };
      map.insert(key, value);
      end = self.ws(after);
      let next = s.get(end).copied();
      end += 1;
      match next {
        Some('}') => break,
        Some(',') => {}
        _ => return Err(at(s, "Expecting ',' delimiter", end - 1)),
      }
      end = self.ws(end);
      let next = s.get(end).copied();
      end += 1;
      if next != Some('"') {
        return Err(at(s, "Expecting property name enclosed in double quotes", end - 1));
      }
    }
    Ok((Value::Object(map), end))
  }

  /// `JSONArray`: from just after the `[`.
  fn array(&mut self, mut end: usize) -> Result<(Value, usize), String> {
    let s = self.s;
    let mut items = Vec::new();
    end = self.ws(end);
    if s.get(end) == Some(&']') {
      return Ok((Value::Array(items), end + 1));
    }
    loop {
      let Some((value, after)) = self.value(end)? else {
        return Err(at(s, "Expecting value", end));
      };
      items.push(value);
      end = self.ws(after);
      let next = s.get(end).copied();
      end += 1;
      match next {
        Some(']') => break,
        Some(',') => {}
        _ => return Err(at(s, "Expecting ',' delimiter", end - 1)),
      }
      end = self.ws(end);
    }
    Ok((Value::Array(items), end))
  }
}

/// `json.strict_constant`, which DRF's parser uses for NaN and the infinities.
fn constant(name: &str) -> String {
  format!("Out of range float values are not JSON compliant: {}", str_repr(name))
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  #[test]
  fn errors_like_python() {
    for (body, expected) in [
      ("", "Expecting value: line 1 column 1 (char 0)"),
      ("not json", "Expecting value: line 1 column 1 (char 0)"),
      (
        "{bad",
        "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)",
      ),
      ("[1,]", "Expecting value: line 1 column 4 (char 3)"),
      (
        r#"{"a":1,}"#,
        "Expecting property name enclosed in double quotes: line 1 column 8 (char 7)",
      ),
      (
        r#"{"name":"  "#,
        "Unterminated string starting at: line 1 column 9 (char 8)",
      ),
      ("{\"a\" 1}", "Expecting ':' delimiter: line 1 column 6 (char 5)"),
      ("[1 2]", "Expecting ',' delimiter: line 1 column 4 (char 3)"),
      ("{}\n x", "Extra data: line 2 column 2 (char 4)"),
      ("\"a\nb\"", "Invalid control character at: line 1 column 3 (char 2)"),
      (r#""\q""#, "Invalid \\escape: line 1 column 2 (char 1)"),
      (r#""\u12""#, "Invalid \\uXXXX escape: line 1 column 3 (char 2)"),
      ("{\"a\":}", "Expecting value: line 1 column 6 (char 5)"),
      ("\n\n  x", "Expecting value: line 3 column 3 (char 4)"),
      ("1.", "Extra data: line 1 column 2 (char 1)"),
      ("NaN", "Out of range float values are not JSON compliant: 'NaN'"),
      (
        "\u{feff}{}",
        "Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)",
      ),
    ] {
      assert_eq!(loads(body.as_bytes()).unwrap_err(), expected, "{body:?}");
    }
    assert_eq!(
      loads(b"\xff").unwrap_err(),
      "'utf-8' codec can't decode byte 0xff in position 0: invalid start byte"
    );
  }

  #[test]
  fn parses_like_python() {
    assert_eq!(
      loads(r#" {"a": [1, 2.5, -0, "é😀"], "a": null} "#.as_bytes()).unwrap(),
      json!({"a": null})
    );
    assert_eq!(loads(b"1e2").unwrap(), json!(100.0));
  }

  #[test]
  fn dumps_like_drf() {
    let v = json!({"a": [1, 1.5, 1e16, null], "é": "\u{2028}\n\u{1}\u{7f}\u{1f600}"});
    assert_eq!(
      dumps(&v),
      "{\"a\":[1,1.5,1e+16,null],\"é\":\"\\u2028\\n\\u0001\u{7f}\u{1f600}\"}"
    );
  }
}
