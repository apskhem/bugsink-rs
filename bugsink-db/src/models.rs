//! Row types as read by the UI API, over upstream's tables. Writes are explicit SQL near their callers.
//!
//! Column types follow upstream's Postgres schema exactly (`integer` -> i32, `bigint` -> i64), because sqlx decodes
//! strictly by type.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::value::RawValue;
use sqlx::types::Json;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Project {
    pub id: i64,
    pub team_id: Option<Uuid>,
    pub name: String,
    pub slug: String,
    pub sentry_key: Uuid,
    pub digested_event_count: i32,
    pub stored_event_count: i32,
    pub issue_count: i32,
    pub has_releases: bool,
    pub retention_max_event_count: i32,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct IssueSummary {
    pub id: Uuid,
    pub project_id: i64,
    pub digest_order: i32,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub digested_event_count: i32,
    pub stored_event_count: i32,
    pub calculated_type: String,
    pub calculated_value: String,
    pub transaction: String,
    pub is_resolved: bool,
    pub is_muted: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct EventSummary {
    pub id: Uuid,
    pub event_id: Uuid,
    pub issue_id: Uuid,
    pub digest_order: i32,
    pub ingested_at: DateTime<Utc>,
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub release: String,
    pub environment: String,
    pub calculated_type: String,
    pub calculated_value: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Event {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub summary: EventSummary,
    pub platform: String,
    pub server_name: String,
    pub sdk_name: String,
    pub sdk_version: String,
    /// Passed through to the client verbatim. `None` when upstream stored the payload in file-based event storage
    /// (`storage_backend` set), which this build cannot read yet.
    pub data: Option<Json<Box<RawValue>>>,
}
