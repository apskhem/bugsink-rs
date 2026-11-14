use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use axum::Router;
use bugsink_db::PgPool;
use bugsink_server::{Settings, accounts, api, grouping, ingest, job_registry, projects, tasks, vacuum};
use clap::{Args, Parser, Subcommand};
use tokio::sync::watch;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

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

    /// Quotas, alerts, ...: same environment variables and defaults as upstream.
    #[command(flatten)]
    settings: Settings,

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
    /// Create a project (in no team, without members) and print its DSN.
    CreateProject { name: String },
    /// Email a user a link to set their password, with why they were added as its first line (upstream's
    /// `send_welcome_email`). Workers send it.
    SendWelcomeEmail {
        #[arg(long)]
        email: String,
        #[arg(long)]
        reason: String,
    },
    /// Clean up stale rows, as upstream's `vacuum` command; with no flags, all of the below. Nothing runs this
    /// automatically (upstream neither): schedule it, e.g. daily.
    Vacuum {
        /// Issue tags whose value none of the issue's (remaining) events has.
        #[arg(long)]
        eventless_issuetags: bool,
        /// Tag values and keys nothing uses.
        #[arg(long)]
        tags: bool,
        /// Finish deleting issues marked as deleted.
        #[arg(long)]
        deleted_issues: bool,
    },
    /// Create a superuser that logs in with this email address as username (upstream's `CREATE_SUPERUSER`). Reads
    /// the password from the first line of stdin.
    CreateSuperuser { email: String },
}

#[derive(Args)]
struct ServeArgs {
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
        Command::Serve(args) => serve(pool, args, cli.settings).await,
        Command::Worker { workers } => {
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            tokio::spawn(async move {
                shutdown_signal().await;
                let _ = shutdown_tx.send(true);
            });
            tasks::run_workers(pool, job_registry(&cli.settings), workers, shutdown_rx).await;
            Ok(())
        }
        Command::CreateProject { name } => create_project(&pool, &name, cli.settings.alerts.base_url()).await,
        Command::CreateSuperuser { email } => create_superuser(&pool, &email).await,
        Command::SendWelcomeEmail { email, reason } => {
            accounts::self_service::send_welcome_email(&pool, &email, &reason).await?;
            println!("Email sent successfully (delayed task)");
            Ok(())
        }
        Command::Vacuum { eventless_issuetags, tags, deleted_issues } => {
            let all = !(eventless_issuetags || tags || deleted_issues);
            let what = vacuum::What {
                eventless_issuetags: all || eventless_issuetags,
                tags: all || tags,
                deleted_issues: all || deleted_issues,
            };
            vacuum::run(&pool, what).await?;
            println!("Vacuum complete.");
            Ok(())
        }
    }
}

async fn serve(pool: PgPool, args: ServeArgs, settings: Settings) -> anyhow::Result<()> {
    let client_addr = settings.proxy.client_addr()?;
    let index = args.static_dir.join("index.html");
    if !index.exists() {
        tracing::warn!(path = %index.display(), "frontend not built; only the API will be useful");
    }
    let frontend = ServeDir::new(&args.static_dir).fallback(ServeFile::new(index));

    let app = Router::new()
        .merge(ingest::router(pool.clone(), args.max_body_bytes, settings.quotas, client_addr))
        .merge(api::router(api::ApiState::new(pool.clone(), settings.clone())?))
        .fallback_service(frontend)
        .layer(TraceLayer::new_for_http());

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let workers = (args.workers > 0)
        .then(|| tokio::spawn(tasks::run_workers(pool, job_registry(&settings), args.workers, shutdown_rx)));

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
    let mut tx = pool.begin().await?;
    let slug = projects::unique_slug(&mut tx, name).await?;
    let new = projects::NewProject {
        team_id: None,
        name,
        slug: &slug,
        alerts: [true; 3],
        visibility: bugsink_db::PROJECT_VISIBILITY_TEAM_MEMBERS,
        retention_max_event_count: 10_000,
        grouping_mechanism: grouping::CURRENT_MECHANISM,
    };
    let (id, key) = projects::insert_project(&mut tx, &new).await.context("creating project")?;
    tx.commit().await?;

    println!("{}", projects::dsn(base_url, id, key));
    Ok(())
}

async fn create_superuser(pool: &PgPool, email: &str) -> anyhow::Result<()> {
    // CREATE_SUPERUSER's check
    if accounts::validation::clean_email(email).is_err() {
        anyhow::bail!("{email:?} is not a valid email address");
    }
    let mut password = String::new();
    std::io::stdin().read_line(&mut password).context("reading the password from stdin")?;
    let password = password.strip_suffix('\n').unwrap_or(&password);
    let password = password.strip_suffix('\r').unwrap_or(password);
    anyhow::ensure!(!password.is_empty(), "no password on stdin");
    let id = accounts::create_superuser(pool, email, password).await.context("creating superuser")?;
    println!("Superuser created: {email} (id {id})");
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
