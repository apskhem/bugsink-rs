//! Everything behind the `bugsink` binary except database plumbing (`bugsink-db`). A library so that the parity
//! harnesses in `examples/` can call into grouping and digest directly.

pub mod accounts;
pub mod alerts;
pub mod api;
pub mod digest;
pub mod event_display;
mod event_counter;
pub mod forms;
pub mod grouping;
pub mod ingest;
pub mod installation;
pub mod issues;
pub mod mail;
pub mod memberships;
pub mod projects;
pub mod protocol;
pub mod proxy;
pub mod py;
pub mod rest;
pub mod quota;
mod releases;
pub mod search;
pub mod tasks;
pub mod user_agent;
pub mod vacuum;

use std::sync::Arc;

/// Upstream's settings that ingest and the workers need, from the same environment variables as upstream's image.
#[derive(clap::Args, Clone, Debug)]
pub struct Settings {
    #[command(flatten)]
    pub quotas: quota::QuotaSettings,
    #[command(flatten)]
    pub alerts: alerts::AlertSettings,
    #[command(flatten)]
    pub proxy: proxy::ProxySettings,
    #[command(flatten)]
    pub users: memberships::UserSettings,
}

/// Every background job kind and its handler.
pub fn job_registry(settings: &Settings) -> tasks::Registry {
    let quotas = settings.quotas;
    let alerts = Arc::new(settings.alerts.clone());
    tasks::Registry::default()
        .register_with(quotas, digest::handle_job)
        .register_with(quotas, digest::retention::handle_delete_by_age_job)
        .register(issues::deletion::handle_job)
        .register(projects::handle_delete_job)
        .register_with(alerts.clone(), accounts::self_service::handle_job)
        .register_with(alerts.clone(), memberships::handle_invite_job)
        .register_with(alerts.clone(), alerts::handle_job)
        .register_with(alerts.clone(), alerts::messaging::handle_test_job)
        .register_with(alerts, alerts::messaging::handle_job)
}
