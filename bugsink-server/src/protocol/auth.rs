//! Extracting the project's public key ("sentry_key") from a request.
//!
//! SDKs send it in one of three places: the `X-Sentry-Auth` header (or `Authorization`), the `sentry_key` query
//! parameter (browsers, CSP reports), or the `dsn` field of the envelope header (tunneled browser requests).

use uuid::Uuid;

/// Parses `Sentry sentry_key=abc, sentry_version=7, ...` and returns the key.
pub fn key_from_auth_header(value: &str) -> Option<Uuid> {
  let rest = value.trim().strip_prefix("Sentry ")?;
  rest
    .split(',')
    .filter_map(|part| part.trim().split_once('='))
    .find(|(k, _)| *k == "sentry_key")
    .and_then(|(_, v)| Uuid::try_parse(v.trim()).ok())
}

/// Extracts the key from a DSN such as `https://<key>@host/<project_id>`.
pub fn key_from_dsn(dsn: &str) -> Option<Uuid> {
  let after_scheme = dsn.split_once("://")?.1;
  let userinfo = after_scheme.split_once('@')?.0;
  let public_key = userinfo.split(':').next()?;
  Uuid::try_parse(public_key).ok()
}

#[cfg(test)]
mod tests {
  use super::*;

  const KEY: &str = "0123456789abcdef0123456789abcdef";

  #[test]
  fn parses_auth_header() {
    let header = format!("Sentry sentry_version=7, sentry_client=sentry.python/2.0, sentry_key={KEY}");
    assert_eq!(key_from_auth_header(&header), Uuid::try_parse(KEY).ok());
    assert_eq!(key_from_auth_header("Bearer abc"), None);
  }

  #[test]
  fn parses_dsn() {
    let dsn = format!("https://{KEY}@bugsink.example.com/12");
    assert_eq!(key_from_dsn(&dsn), Uuid::try_parse(KEY).ok());
    assert_eq!(key_from_dsn("https://bugsink.example.com/12"), None);
  }
}
