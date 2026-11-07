//! Postgres access against upstream Bugsink's own (Django-managed) schema, so that bugsink-rs is a drop-in
//! replacement for a Postgres-backed Bugsink install, and vice versa.
//!
//! Schema ownership:
//! - Upstream tables (`projects_project`, `issues_issue`, `events_event`, ...) belong to Django's migrations. We never
//!   alter them. We support exactly one upstream schema version, [`UPSTREAM_VERSION`], and refuse to run against a
//!   database that is behind or ahead of it.
//! - `bsrs_*` tables are ours, managed by sqlx migrations in `migrations/`.
//!
//! An empty database is bootstrapped from a snapshot of what `bugsink-manage migrate` produces for
//! [`UPSTREAM_VERSION`] (`upstream/`), including the `django_migrations` rows, so upstream can take over that database
//! later as if it had created it.

use std::collections::BTreeSet;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, Row};

pub use sqlx::{PgPool, Postgres, Transaction};

pub mod models;

pub const UPSTREAM_VERSION: &str = "2.6.3";

const UPSTREAM_SCHEMA: &str = include_str!("../upstream/2.6.3-schema.sql");
const UPSTREAM_SEED: &str = include_str!("../upstream/2.6.3-seed.sql");
const UPSTREAM_MIGRATIONS: &str = include_str!("../upstream/2.6.3-migrations.txt");

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error(
        "database is behind Bugsink {UPSTREAM_VERSION}: missing upstream migrations {0:?}. Upgrade it with upstream \
         Bugsink {UPSTREAM_VERSION} (`bugsink-manage migrate`) first."
    )]
    Behind(Vec<String>),
    #[error(
        "database has upstream migrations this build does not know {0:?}: it was migrated by a newer Bugsink than \
         {UPSTREAM_VERSION}, and its schema may not match what this build reads and writes."
    )]
    Ahead(Vec<String>),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

/// Bootstraps an empty database, or verifies an existing one is at [`UPSTREAM_VERSION`]; then applies our own
/// (`bsrs_*`) migrations.
pub async fn prepare(pool: &PgPool, allow_newer_upstream: bool) -> Result<(), SchemaError> {
    let has_django: bool = sqlx::query_scalar("SELECT to_regclass('public.django_migrations') IS NOT NULL")
        .fetch_one(pool)
        .await?;

    if has_django {
        verify_upstream_version(pool, allow_newer_upstream).await?;
    } else {
        tracing::info!("empty database: creating Bugsink {UPSTREAM_VERSION} schema");
        let mut tx = pool.begin().await?;
        tx.execute(UPSTREAM_SCHEMA).await?;
        tx.execute(UPSTREAM_SEED).await?;
        // Upstream creates the singleton installation row in a data migration; its id identifies the install.
        tx.execute(
            "INSERT INTO phonehome_installation (id, created_at, silence_email_system_warning, email_quota_usage,
                 next_quota_check, quota_exceeded_reason, email_sending_diagnostics)
             VALUES (gen_random_uuid(), now(), false, '{\"per_month\": {}}', 1, 'null', '{\"attempts\": []}')",
        )
        .await?;
        tx.commit().await?;
    }

    MIGRATOR.run(pool).await?;
    Ok(())
}

async fn verify_upstream_version(pool: &PgPool, allow_newer_upstream: bool) -> Result<(), SchemaError> {
    let expected: BTreeSet<&str> = UPSTREAM_MIGRATIONS.lines().filter(|l| !l.is_empty()).collect();
    let applied: BTreeSet<String> = sqlx::query("SELECT app || ' ' || name AS m FROM django_migrations")
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| r.get("m"))
        .collect();

    let missing: Vec<String> = expected.iter().filter(|m| !applied.contains(**m)).map(|m| m.to_string()).collect();
    if !missing.is_empty() {
        return Err(SchemaError::Behind(missing));
    }
    let unknown: Vec<String> = applied.into_iter().filter(|m| !expected.contains(m.as_str())).collect();
    if !unknown.is_empty() {
        if !allow_newer_upstream {
            return Err(SchemaError::Ahead(unknown));
        }
        tracing::warn!(?unknown, "running against a database migrated by a newer upstream Bugsink");
    }
    Ok(())
}

/// `issues_turningpoint.kind`; values from upstream `issues.models.TurningPointKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TurningPointKind {
    FirstSeen = 1,
    Resolved = 2,
    Muted = 3,
    Regressed = 4,
    Unmuted = 5,
    Reopened = 6,
    NextMaterialized = 10,
    ManualAnnotation = 100,
}

/// `projects_project.visibility` default (upstream `ProjectVisibility.TEAM_MEMBERS`).
pub const PROJECT_VISIBILITY_TEAM_MEMBERS: i32 = 99;
