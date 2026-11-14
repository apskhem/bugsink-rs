//! Everything behind the `bugsink` binary except database plumbing (`bugsink-db`). A library so that the parity
//! harnesses in `examples/` can call into grouping and digest directly.

pub mod accounts;
pub mod alerts;
pub mod api;
pub mod digest;
mod event_counter;
pub mod grouping;
pub mod ingest;
pub mod issues;
pub mod protocol;
pub mod quota;
pub mod search;
pub mod tasks;

use std::sync::Arc;

/// Upstream's settings that ingest and the workers need, from the same environment variables as upstream's image.
#[derive(clap::Args, Clone, Debug)]
pub struct Settings {
    #[command(flatten)]
    pub quotas: quota::QuotaSettings,
    #[command(flatten)]
    pub alerts: alerts::AlertSettings,
}

/// Every background job kind and its handler.
pub fn job_registry(settings: &Settings) -> tasks::Registry {
    let quotas = settings.quotas;
    let alerts = Arc::new(settings.alerts.clone());
    let messaging = alerts.clone();
    tasks::Registry::default()
        .register(digest::JOB_KIND, move |pool, payload| digest::handle_job(pool, payload, quotas))
        .register(digest::retention::DELETE_BY_AGE_JOB_KIND, digest::retention::handle_delete_by_age_job)
        .register(issues::deletion::JOB_KIND, issues::deletion::handle_job)
        .register(alerts::JOB_KIND, move |pool, payload| alerts::handle_job(pool, payload, alerts.clone()))
        .register(alerts::messaging::JOB_KIND, move |pool, payload| {
            alerts::messaging::handle_job(pool, payload, messaging.clone())
        })
}
