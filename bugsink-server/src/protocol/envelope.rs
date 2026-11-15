//! Sentry envelope parsing. See https://develop.sentry.dev/sdk/data-model/envelopes/
//!
//! ```text
//! {"event_id":"...","dsn":"..."}\n          <- envelope header
//! {"type":"event","length":41}\n            <- item header; `length` is optional
//! {"message":"hello","level":"error"}\n     <- payload: `length` bytes, or up to the next newline / EOF
//! ```
//!
//! Upstream Bugsink parses this as a stream; we parse an in-memory (already size-limited) body, which is simpler
//! and fine at the body sizes Sentry SDKs produce.

use serde::Deserialize;
use serde_json::{
  Map,
  Value,
};

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
  #[error("empty envelope")]
  Empty,
  #[error("invalid {what} header: {source}")]
  Header {
    what: &'static str,
    source: serde_json::Error,
  },
  #[error("item declares length {declared} but only {available} bytes remain")]
  Truncated { declared: usize, available: usize },
}

#[derive(Debug, Deserialize)]
pub struct EnvelopeHeaders {
  /// Kept as sent (in `extra["event_id"]`); validated by the caller, which knows whether it's required.
  pub dsn: Option<String>,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct ItemHeaders {
  #[serde(rename = "type")]
  pub ty: String,
  pub length: Option<usize>,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

#[derive(Debug)]
pub struct Item<'a> {
  pub headers: ItemHeaders,
  pub payload: &'a [u8],
}

#[derive(Debug)]
pub struct Envelope<'a> {
  pub headers: EnvelopeHeaders,
  pub items: Vec<Item<'a>>,
}

pub fn parse(input: &[u8]) -> Result<Envelope<'_>, EnvelopeError> {
  let (header_line, mut rest) = split_line(input);
  if header_line.iter().all(u8::is_ascii_whitespace) {
    return Err(EnvelopeError::Empty);
  }
  let headers = serde_json::from_slice(header_line).map_err(|source| {
    EnvelopeError::Header {
      what: "envelope",
      source,
    }
  })?;

  let mut items = Vec::new();
  while !rest.is_empty() {
    let (line, after) = split_line(rest);
    rest = after;
    if line.iter().all(u8::is_ascii_whitespace) {
      continue; // tolerate blank lines between items and a trailing newline
    }
    let item_headers: ItemHeaders =
      serde_json::from_slice(line).map_err(|source| EnvelopeError::Header { what: "item", source })?;

    let payload = match item_headers.length {
      Some(len) => {
        if len > rest.len() {
          return Err(EnvelopeError::Truncated {
            declared: len,
            available: rest.len(),
          });
        }
        let (payload, after) = rest.split_at(len);
        rest = after.strip_prefix(b"\n").unwrap_or(after);
        payload
      }
      None => {
        let (payload, after) = split_line(rest);
        rest = after;
        payload
      }
    };
    items.push(Item {
      headers: item_headers,
      payload,
    });
  }
  Ok(Envelope { headers, items })
}

/// Splits at the first `\n`, which is not included in either half.
fn split_line(input: &[u8]) -> (&[u8], &[u8]) {
  match input.iter().position(|&b| b == b'\n') {
    Some(i) => (&input[..i], &input[i + 1..]),
    None => (input, &[]),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_items_with_and_without_length() {
    let body = b"{\"event_id\":\"9ec79c33ec9942ab8353589fcb2e04dc\"}\n\
                     {\"type\":\"attachment\",\"length\":10}\n\
                     \x00\x01\n\x03\x04\x05\x06\x07\x08\x09\n\
                     {\"type\":\"event\"}\n\
                     {\"message\":\"hi\"}\n";
    let env = parse(body).unwrap();
    assert_eq!(env.headers.extra["event_id"], "9ec79c33ec9942ab8353589fcb2e04dc");
    assert_eq!(env.items.len(), 2);
    assert_eq!(env.items[0].headers.ty, "attachment");
    assert_eq!(env.items[0].payload.len(), 10);
    assert_eq!(env.items[1].headers.ty, "event");
    assert_eq!(env.items[1].payload, b"{\"message\":\"hi\"}");
  }

  #[test]
  fn payload_may_end_at_eof() {
    let env = parse(b"{}\n{\"type\":\"event\"}\n{}").unwrap();
    assert_eq!(env.items[0].payload, b"{}");
  }

  #[test]
  fn rejects_truncated_and_empty() {
    assert!(matches!(
      parse(b"{}\n{\"type\":\"event\",\"length\":50}\n{}"),
      Err(EnvelopeError::Truncated { .. })
    ));
    assert!(matches!(parse(b""), Err(EnvelopeError::Empty)));
  }
}
