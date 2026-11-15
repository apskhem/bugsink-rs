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
use serde_json::{
  Value,
  json,
};
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

  /// Upstream checks the installation's quota before even looking up the project.
  async fn check_installation_quota(&self, state: &IngestState) -> Result<(), IngestError> {
    if quota::installation_exceeded(&state.pool, &state.quotas, self.ingested_at).await? {
      return Err(IngestError::OverQuota);
    }
    Ok(())
  }

  /// Checks the key and then the project's quota. Upstream: a `dsn` in the envelope header takes precedence over the
  /// request's key.
  async fn authenticate(&self, state: &IngestState, dsn_key: Option<Uuid>) -> Result<(), IngestError> {
    let key = dsn_key.or(self.key_from_request).ok_or(IngestError::MissingKey)?;
    let project: Option<(Option<DateTime<Utc>>, String)> = sqlx::query_as(
      "SELECT quota_exceeded_until, quota_exceeded_reason FROM projects_project
             WHERE id = $1 AND sentry_key = $2 AND NOT is_deleted",
    )
    .bind(self.project_id)
    .bind(key)
    .fetch_optional(&state.pool)
    .await?;
    let (until, reason) = project.ok_or(IngestError::Forbidden)?;
    if quota::is_still_exceeded(
      &state.quotas,
      Scope::Project(self.project_id),
      until,
      &reason,
      self.ingested_at,
    ) {
      return Err(IngestError::OverQuota);
    }
    Ok(())
  }

  async fn enqueue(&self, pool: &PgPool, event_id: Uuid, event: Value) -> Result<(), IngestError> {
    let job = DigestJob {
      project_id: self.project_id,
      event_id,
      ingested_at: self.ingested_at,
      remote_addr: self.remote_addr.clone(),
      event,
    };
    crate::tasks::enqueue(pool, &job).await?;
    Ok(())
  }
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
  request.check_installation_quota(&state).await?;
  let dsn_key = parsed.headers.dsn.as_deref().and_then(auth::key_from_dsn);
  request.authenticate(&state, dsn_key).await?;

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
      let event: Value = serde_json::from_slice(item.payload)?;
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
  request.check_installation_quota(&state).await?;
  request.authenticate(&state, None).await?;
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
  request.enqueue(&state.pool, event_id, event).await?;
  Ok(Json(json!({ "id": raw_id })))
}
