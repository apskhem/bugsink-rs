//! Request body `Content-Encoding` handling, with a hard cap on the decompressed size (zip-bomb protection).

use std::io::Read;

use bytes::Bytes;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
  #[error("unsupported content-encoding: {0}")]
  Unsupported(String),
  #[error("decompressed body exceeds {0} bytes")]
  TooLarge(usize),
  #[error("invalid compressed body: {0}")]
  Invalid(#[from] std::io::Error),
}

/// Decodes `body` according to `content_encoding`, refusing to produce more than `max_len` bytes.
///
/// "deflate" means zlib-wrapped deflate, as sent by the Sentry SDKs. An uncompressed body is returned as is, not
/// copied.
pub fn decode(body: &Bytes, content_encoding: Option<&str>, max_len: usize) -> Result<Bytes, DecodeError> {
  let encoding = content_encoding.map(str::trim).unwrap_or("").to_ascii_lowercase();
  let compressed = &body[..];
  match encoding.as_str() {
    "" | "identity" if body.len() > max_len => Err(DecodeError::TooLarge(max_len)),
    "" | "identity" => Ok(body.clone()),
    "gzip" => limited(flate2::read::GzDecoder::new(compressed), max_len),
    "deflate" => limited(flate2::read::ZlibDecoder::new(compressed), max_len),
    "br" => limited(brotli::Decompressor::new(compressed, 8 * 1024), max_len),
    other => Err(DecodeError::Unsupported(other.to_string())),
  }
}

fn limited(reader: impl Read, max_len: usize) -> Result<Bytes, DecodeError> {
  let mut out = Vec::new();
  reader.take(max_len as u64 + 1).read_to_end(&mut out)?;
  if out.len() > max_len {
    return Err(DecodeError::TooLarge(max_len));
  }
  Ok(out.into())
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use super::*;

  #[test]
  fn gzip_roundtrip_and_limit() {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&[b'a'; 1000]).unwrap();
    let gz = enc.finish().unwrap();

    let gz = Bytes::from(gz);
    assert_eq!(decode(&gz, Some("gzip"), 1000).unwrap().len(), 1000);
    assert!(matches!(
      decode(&gz, Some("gzip"), 999),
      Err(DecodeError::TooLarge(999))
    ));
  }

  #[test]
  fn rejects_unknown_encoding() {
    assert!(matches!(
      decode(&Bytes::from_static(b"x"), Some("zstd"), 10),
      Err(DecodeError::Unsupported(_))
    ));
  }
}
