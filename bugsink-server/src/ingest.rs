//! `POST /api/{project_id}/envelope/` and `POST /api/{project_id}/store/`, following upstream's `ingest/views.py`.
//!
//! Kept deliberately thin: everything that can wait happens in digest. The response only depends on auth and on the
//! body being parseable. Unlike upstream, an event payload that isn't valid JSON is rejected here (upstream accepts it
//! and fails in digest).
//!
//! While the installation's or the project's quota is exceeded (see `crate::quota`), requests get an empty 429.
//!
//! The client's address (for `remote_addr` and `{{auto}}` user IPs) is the peer's, or behind a proxy taken from a
//! header (`crate::proxy`).
//!
//! SKIPPED(parity) (optional extras, see the README): `/minidump/` and `/security/` (CSP reports) endpoints.

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::extract::{
  ConnectInfo,
  DefaultBodyLimit,
  Path,
  Query,
  State,
};
use axum::http::{
  HeaderMap,
  HeaderName,
  HeaderValue,
  StatusCode,
  header,
};
use axum::middleware::map_response;
use axum::response::{
  IntoResponse,
  Response,
};
use axum::routing::post;
use axum::{
  Json,
  Router,
};
use bugsink_db::PgPool;
use bytes::Bytes;
use chrono::{
  DateTime,
  Utc,
};
use serde::de::{
  Deserializer,
  MapAccess,
  SeqAccess,
  Visitor,
};
use serde::{
  Deserialize,
  Serialize,
};
use serde_json::value::RawValue;
use serde_json::{
  Value,
  json,
};
use sqlx::Row;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use crate::digest::DigestJob;
use crate::protocol::{
  auth,
  encoding,
  envelope,
};
use crate::proxy::{
  ClientAddr,
  SpoofedForwardedFor,
};
use crate::quota::{
  self,
  QuotaSettings,
  Scope,
};

#[derive(Clone)]
struct IngestState {
  pool: PgPool,
  max_body_bytes: usize,
  quotas: QuotaSettings,
  client_addr: ClientAddr,
}

const SENTRY_RATE_LIMITS: HeaderName = HeaderName::from_static("x-sentry-rate-limits");

/// Routes for the Sentry ingest API. `max_body_bytes` applies to both the compressed and the decompressed body.
pub fn router(pool: PgPool, max_body_bytes: usize, quotas: QuotaSettings, client_addr: ClientAddr) -> Router {
  Router::new()
    .route(
      "/api/{project_id}/envelope/",
      post(envelope_endpoint).layer(map_response(sentry_rate_limits)),
    )
    .route("/api/{project_id}/store/", post(store_endpoint))
    // Browser SDKs post cross-origin. Auth is the DSN key, not cookies, so a permissive policy is correct.
    .layer(CorsLayer::very_permissive().expose_headers([SENTRY_RATE_LIMITS]))
    .layer(DefaultBodyLimit::max(max_body_bytes))
    .with_state(IngestState {
      pool,
      max_body_bytes,
      quotas,
      client_addr,
    })
}

/// Like upstream, every envelope response tells SDKs not to send transactions and spans (unsupported); a 429 also
/// asks for a 60s backoff.
async fn sentry_rate_limits(mut response: Response) -> Response {
  let value = if response.status() == StatusCode::TOO_MANY_REQUESTS {
    "60::key, 86400:transaction;span:organization"
  } else {
    "86400:transaction;span:organization"
  };
  response
    .headers_mut()
    .insert(SENTRY_RATE_LIMITS, HeaderValue::from_static(value));
  response
}

#[derive(Debug, thiserror::Error)]
enum IngestError {
  #[error("missing or invalid sentry_key")]
  MissingKey,
  #[error("{0}")]
  Parse(&'static str),
  #[error("unknown project or wrong sentry_key")]
  Forbidden,
  #[error("quota exceeded")]
  OverQuota,
  #[error(transparent)]
  Decode(#[from] encoding::DecodeError),
  #[error(transparent)]
  Envelope(#[from] envelope::EnvelopeError),
  #[error("invalid event JSON: {0}")]
  EventJson(#[from] serde_json::Error),
  #[error(transparent)]
  SpoofedForwardedFor(#[from] SpoofedForwardedFor),
  #[error("internal error")]
  Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for IngestError {
  fn from(e: sqlx::Error) -> Self {
    Self::Internal(e.into())
  }
}

impl IntoResponse for IngestError {
  fn into_response(self) -> Response {
    let status = match &self {
      Self::MissingKey | Self::Forbidden => StatusCode::FORBIDDEN,
      Self::Decode(encoding::DecodeError::TooLarge(_)) => StatusCode::PAYLOAD_TOO_LARGE,
      Self::Decode(_) | Self::Envelope(_) | Self::EventJson(_) | Self::Parse(_) => StatusCode::BAD_REQUEST,
      Self::SpoofedForwardedFor(_) => StatusCode::BAD_REQUEST,
      Self::OverQuota => return StatusCode::TOO_MANY_REQUESTS.into_response(), // empty body, as upstream
      Self::Internal(e) => {
        tracing::error!(error = %e, "ingest failed");
        StatusCode::INTERNAL_SERVER_ERROR
      }
    };
    (status, Json(json!({ "message": self.to_string() }))).into_response()
  }
}

struct Request {
  ingested_at: DateTime<Utc>,
  project_id: i64,
  key_from_request: Option<Uuid>,
  remote_addr: Option<String>,
  body: Vec<u8>,
}

impl Request {
  fn new(
    state: &IngestState,
    project_id: i64,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    peer: SocketAddr,
    body: &Bytes,
  ) -> Result<Self, IngestError> {
    let key_from_request = query
      .get("sentry_key")
      .and_then(|k| Uuid::try_parse(k).ok())
      .or_else(|| {
        ["x-sentry-auth", "authorization"]
          .iter()
          .filter_map(|h| headers.get(*h)?.to_str().ok())
          .find_map(auth::key_from_auth_header)
      });
    let content_encoding = headers.get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok());
    Ok(Self {
      ingested_at: Utc::now(),
      project_id,
      key_from_request,
      remote_addr: state.client_addr.of(headers, peer)?,
      body: encoding::decode(body, content_encoding, state.max_body_bytes)?,
    })
  }

  /// Upstream's order: the installation's quota (before even looking up the project), the key, the project, and its
  /// quota. A `dsn` in the envelope header takes precedence over the request's key. One query reads both quotas.
  async fn admit(&self, state: &IngestState, dsn_key: Option<Uuid>) -> Result<(), IngestError> {
    let key = dsn_key.or(self.key_from_request);
    let row = sqlx::query(
      "SELECT i.quota_exceeded_until AS installation_until, i.quota_exceeded_reason AS installation_reason,
                  p.id IS NOT NULL AS project_found, p.quota_exceeded_until, p.quota_exceeded_reason
             FROM (VALUES (1)) AS one (x)
             LEFT JOIN LATERAL (SELECT quota_exceeded_until, quota_exceeded_reason FROM phonehome_installation LIMIT 1) i
                 ON true
             LEFT JOIN projects_project p ON p.id = $1 AND p.sentry_key = $2 AND NOT p.is_deleted",
    )
    .bind(self.project_id)
    .bind(key)
    .fetch_one(&state.pool)
    .await?;
    // No installation row: not exceeded (its columns are then null).
    if quota::is_still_exceeded(
      &state.quotas,
      Scope::Installation,
      row.get("installation_until"),
      row.get::<Option<&str>, _>("installation_reason").unwrap_or_default(),
      self.ingested_at,
    ) {
      return Err(IngestError::OverQuota);
    }
    if key.is_none() {
      return Err(IngestError::MissingKey);
    }
    if !row.get::<bool, _>("project_found") {
      return Err(IngestError::Forbidden);
    }
    if quota::is_still_exceeded(
      &state.quotas,
      Scope::Project(self.project_id),
      row.get("quota_exceeded_until"),
      row.get("quota_exceeded_reason"),
      self.ingested_at,
    ) {
      return Err(IngestError::OverQuota);
    }
    Ok(())
  }

  /// `event` as `DigestJob::event`: a `Value`, or the payload's own JSON text.
  async fn enqueue<E: Serialize + ?Sized>(&self, pool: &PgPool, event_id: Uuid, event: &E) -> Result<(), IngestError> {
    #[derive(Serialize)]
    struct Job<'a, E: ?Sized> {
      project_id: i64,
      event_id: Uuid,
      ingested_at: DateTime<Utc>,
      remote_addr: &'a Option<String>,
      event: &'a E,
    }
    let job = Job {
      project_id: self.project_id,
      event_id,
      ingested_at: self.ingested_at,
      remote_addr: &self.remote_addr,
      event,
    };
    crate::tasks::enqueue_as::<DigestJob>(pool, &job).await?;
    Ok(())
  }
}

/// Fails exactly where parsing into a `Value` would (the same parser, through `deserialize_any`), without building
/// one: an event is only parsed into one in digest.
struct ValidJson;

impl<'de> Deserialize<'de> for ValidJson {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    deserializer.deserialize_any(ValidJson)
  }
}

impl<'de> Visitor<'de> for ValidJson {
  type Value = ValidJson;

  fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    f.write_str("any JSON value")
  }

  fn visit_bool<E>(self, _: bool) -> Result<Self, E> {
    Ok(self)
  }

  fn visit_i64<E>(self, _: i64) -> Result<Self, E> {
    Ok(self)
  }

  fn visit_u64<E>(self, _: u64) -> Result<Self, E> {
    Ok(self)
  }

  fn visit_f64<E>(self, _: f64) -> Result<Self, E> {
    Ok(self)
  }

  fn visit_str<E>(self, _: &str) -> Result<Self, E> {
    Ok(self)
  }

  fn visit_unit<E>(self) -> Result<Self, E> {
    Ok(self)
  }

  fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self, A::Error> {
    while seq.next_element::<ValidJson>()?.is_some() {}
    Ok(self)
  }

  fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self, A::Error> {
    while map.next_entry::<ValidJson, ValidJson>()?.is_some() {}
    Ok(self)
  }
}

/// The event payload's JSON text, validated as a `Value` would be.
fn event_json(payload: &[u8]) -> Result<&RawValue, IngestError> {
  serde_json::from_slice::<ValidJson>(payload)?;
  Ok(serde_json::from_slice(payload)?)
}

async fn envelope_endpoint(
  State(state): State<IngestState>,
  Path(project_id): Path<i64>,
  Query(query): Query<HashMap<String, String>>,
  ConnectInfo(peer): ConnectInfo<SocketAddr>,
  headers: HeaderMap,
  body: Bytes,
) -> Result<Json<Value>, IngestError> {
  let request = Request::new(&state, project_id, &query, &headers, peer, &body)?;
  let parsed = envelope::parse(&request.body)?;
  let dsn_key = parsed.headers.dsn.as_deref().and_then(auth::key_from_dsn);
  request.admit(&state, dsn_key).await?;

  // Upstream digests at most one event per envelope, identified by the envelope header's event_id.
  // SKIPPED(parity): minidump attachments (`attachment_type: event.minidump`).
  let events: Vec<_> = parsed.items.iter().filter(|item| item.headers.ty == "event").collect();
  let header_event_id = parsed
    .headers
    .extra
    .get("event_id")
    .and_then(Value::as_str)
    .map(str::to_owned);
  let response = Json(json!({ "id": header_event_id }));
  match events.as_slice() {
    [] => Ok(response),
    [item] => {
      let raw_id = header_event_id
        .as_deref()
        .ok_or(IngestError::Parse("event_id not found in envelope headers"))?;
      let event_id =
        Uuid::try_parse(raw_id).map_err(|_| IngestError::Parse("event_id in envelope headers is not a valid UUID"))?;
      let event = event_json(item.payload)?;
      request.enqueue(&state.pool, event_id, event).await?;
      Ok(response)
    }
    _ => {
      tracing::info!(
        "can only deal with one event per envelope but found {}, ignoring it",
        events.len()
      );
      Ok(response)
    }
  }
}

async fn store_endpoint(
  State(state): State<IngestState>,
  Path(project_id): Path<i64>,
  Query(query): Query<HashMap<String, String>>,
  ConnectInfo(peer): ConnectInfo<SocketAddr>,
  headers: HeaderMap,
  body: Bytes,
) -> Result<Json<Value>, IngestError> {
  let request = Request::new(&state, project_id, &query, &headers, peer, &body)?;
  request.admit(&state, None).await?;
  let mut event: Value = serde_json::from_slice(&request.body)?;
  let obj = event
    .as_object_mut()
    .ok_or(IngestError::Parse("event is not a JSON object"))?;
  // Upstream adds a missing event_id to the stored payload itself.
  let raw_id = obj
    .entry("event_id")
    .or_insert_with(|| Value::String(Uuid::new_v4().simple().to_string()))
    .clone();
  let event_id = raw_id
    .as_str()
    .and_then(|s| Uuid::try_parse(s).ok())
    .ok_or(IngestError::Parse("invalid event_id"))?;
  request.enqueue(&state.pool, event_id, &event).await?;
  Ok(Json(json!({ "id": raw_id })))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn event_json_accepts_and_rejects_as_value() {
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    let inputs: Vec<Vec<u8>> = [
      r#"{"a": 1, "b": [true, null, -2.5e-3, "xé\n"], "c": {}}"#,
      r#"{"a": 1, "a": 2}"#,
      r#""\u0000""#,
      r#""😀""#,
      r#""\ud800""#,
      r#""\udc00x""#,
      "1e400",
      "-1e400",
      "18446744073709551616",
      "  {} \n",
      "{} x",
      "{}{}",
      "[1,]",
      r#"{"a" 1}"#,
      "",
      "nul",
      r#""\x""#,
      "\"tab\there\"",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .chain([deep(127).into_bytes(), deep(128).into_bytes(), deep(129).into_bytes()])
    .chain([b"\"\xff\"".to_vec(), b"{\"\xc3\x28\": 1}".to_vec()])
    .collect();
    for input in inputs {
      let as_value = serde_json::from_slice::<Value>(&input).map_err(|e| e.to_string());
      let ours = event_json(&input).map_err(|e| e.to_string());
      assert_eq!(
        ours
          .as_ref()
          .map(|_| ())
          .map_err(|e| e.trim_start_matches("invalid event JSON: ")),
        as_value.as_ref().map(|_| ()).map_err(String::as_str),
        "{:?}",
        String::from_utf8_lossy(&input)
      );
      // What digest gets is what parsing the payload gives.
      if let (Ok(raw), Ok(value)) = (ours, as_value) {
        assert_eq!(serde_json::from_str::<Value>(raw.get()).unwrap(), value);
      }
    }
  }
}
