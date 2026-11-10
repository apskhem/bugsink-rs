use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use axum::Router;
use bugsink_db::PgPool;
use bugsink_server::{api, digest, grouping, ingest, tasks};
use clap::{Args, Parser, Subcommand};
use tokio::sync::watch;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "bugsink", about = "Self-hosted error tracking (Rust rewrite of Bugsink)")]
struct Cli {
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    #[arg(long, env = "BUGSINK_DB_MAX_CONNECTIONS", default_value_t = 20)]
    db_max_connections: u32,

    /// Run against a database migrated by a newer upstream Bugsink than this build supports (at your own risk).
    #[arg(long, env = "BUGSINK_ALLOW_NEWER_UPSTREAM_SCHEMA", default_value_t = false)]
    allow_newer_upstream_schema: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server (and, unless --workers 0, background workers in the same process).
    Serve(ServeArgs),
    /// Run background workers only (to scale digestion separately from HTTP).
    Worker {
        #[arg(long, env = "BUGSINK_WORKERS", default_value_t = 4)]
        workers: usize,
    },
    /// Create the upstream Bugsink schema in an empty database, or verify an existing one; then apply our own
    /// (bsrs_*) migrations.
    Migrate,
    /// Create a project and print its DSN. Stopgap until the UI can do it.
    CreateProject {
        name: String,
        #[arg(long, env = "BUGSINK_BASE_URL", default_value = "http://localhost:8000")]
        base_url: String,
    },
}

#[derive(Args)]
struct ServeArgs {
    /// Defaults to localhost: the UI API has no auth yet (see api.rs).
    #[arg(long, env = "BUGSINK_BIND", default_value = "127.0.0.1:8000")]
    bind: SocketAddr,

    /// Directory with the built frontend (index.html + assets). Unknown non-API paths serve index.html.
    #[arg(long, env = "BUGSINK_STATIC_DIR", default_value = "frontend/dist")]
    static_dir: PathBuf,

    #[arg(long, env = "BUGSINK_WORKERS", default_value_t = 4)]
    workers: usize,

    /// Applies to both compressed and decompressed ingest bodies.
    #[arg(long, env = "BUGSINK_MAX_BODY_BYTES", default_value_t = 20 * 1024 * 1024)]
    max_body_bytes: usize,

}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    let pool = bugsink_db::connect(&cli.database_url, cli.db_max_connections)
        .await
        .context("connecting to Postgres")?;

    // Always: refuse to touch a database whose upstream schema version we don't match.
    bugsink_db::prepare(&pool, cli.allow_newer_upstream_schema).await?;
    tracing::info!(upstream = bugsink_db::UPSTREAM_VERSION, "database schema ok");

    match cli.command {
        Command::Migrate => Ok(()),
        Command::Serve(args) => serve(pool, args).await,
        Command::Worker { workers } => {
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            tokio::spawn(async move {
                shutdown_signal().await;
                let _ = shutdown_tx.send(true);
            });
            tasks::run_workers(pool, registry(), workers, shutdown_rx).await;
            Ok(())
        }
        Command::CreateProject { name, base_url } => create_project(&pool, &name, &base_url).await,
    }
}

fn registry() -> tasks::Registry {
    tasks::Registry::default()
        .register(digest::JOB_KIND, digest::handle_job)
        .register(digest::retention::DELETE_BY_AGE_JOB_KIND, digest::retention::handle_delete_by_age_job)
}

async fn serve(pool: PgPool, args: ServeArgs) -> anyhow::Result<()> {
    let index = args.static_dir.join("index.html");
    if !index.exists() {
        tracing::warn!(path = %index.display(), "frontend not built; only the API will be useful");
    }
    let frontend = ServeDir::new(&args.static_dir).fallback(ServeFile::new(index));

    let app = Router::new()
        .merge(ingest::router(pool.clone(), args.max_body_bytes))
        .merge(api::router(pool.clone()))
        .fallback_service(frontend)
        .layer(TraceLayer::new_for_http());

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let workers = (args.workers > 0)
        .then(|| tokio::spawn(tasks::run_workers(pool, registry(), args.workers, shutdown_rx)));

    let listener = tokio::net::TcpListener::bind(args.bind).await.with_context(|| format!("binding {}", args.bind))?;
    tracing::info!(addr = %args.bind, workers = args.workers, "listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    let _ = shutdown_tx.send(true);
    if let Some(workers) = workers {
        let _ = workers.await;
    }
    Ok(())
}

async fn create_project(pool: &PgPool, name: &str, base_url: &str) -> anyhow::Result<()> {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    // Django model defaults are not database defaults: every NOT NULL column is spelled out, with upstream's values.
    let (id, key): (i64, Uuid) = sqlx::query_as(
        "INSERT INTO projects_project (
             name, slug, sentry_key, is_deleted, has_releases, issue_count, digested_event_count, stored_event_count,
             alert_on_new_issue, alert_on_regression, alert_on_unmute, visibility, quota_exceeded_reason,
             next_quota_check, retention_max_event_count, grouping_mechanism)
         VALUES ($1, $2, gen_random_uuid(), false, false, 0, 0, 0, true, true, true, $3, 'null', 0, 10000, $4)
         RETURNING id, sentry_key",
    )
    .bind(name)
    .bind(&slug)
    .bind(bugsink_db::PROJECT_VISIBILITY_TEAM_MEMBERS)
    .bind(grouping::CURRENT_MECHANISM)
    .fetch_one(pool)
    .await
    .context("creating project")?;

    let (scheme, host) = base_url.trim_end_matches('/').split_once("://").context("base URL needs a scheme")?;
    println!("{scheme}://{}@{host}/{id}", key.simple());
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
