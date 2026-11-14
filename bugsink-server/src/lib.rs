//! Everything behind the `bugsink` binary except database plumbing (`bugsink-db`). A library so that the parity
//! harnesses in `examples/` can call into grouping and digest directly.

pub mod accounts;
pub mod alerts;
pub mod api;
pub mod digest;
pub mod event_display;
mod event_counter;
pub mod grouping;
pub mod ingest;
pub mod issues;
pub mod memberships;
pub mod projects;
pub mod protocol;
pub mod proxy;
pub mod quota;
pub mod search;
pub mod tasks;
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
    let messaging = alerts.clone();
    tasks::Registry::default()
        .register(digest::JOB_KIND, move |pool, payload| digest::handle_job(pool, payload, quotas))
        .register(digest::retention::DELETE_BY_AGE_JOB_KIND, move |pool, payload| {
            digest::retention::handle_delete_by_age_job(pool, payload, quotas)
        })
        .register(issues::deletion::JOB_KIND, issues::deletion::handle_job)
        .register(projects::DELETE_JOB_KIND, projects::handle_delete_job)
        .register(accounts::self_service::JOB_KIND, {
            let alerts = alerts.clone();
            move |pool, payload| accounts::self_service::handle_job(pool, payload, alerts.clone())
        })
        .register(memberships::INVITE_JOB_KIND, {
            let alerts = alerts.clone();
            move |pool, payload| memberships::handle_invite_job(pool, payload, alerts.clone())
        })
        .register(alerts::JOB_KIND, move |pool, payload| alerts::handle_job(pool, payload, alerts.clone()))
        .register(alerts::messaging::TEST_JOB_KIND, {
            let alerts = messaging.clone();
            move |pool, payload| alerts::messaging::handle_test_job(pool, payload, alerts.clone())
        })
        .register(alerts::messaging::JOB_KIND, move |pool, payload| {
            alerts::messaging::handle_job(pool, payload, messaging.clone())
        })
}
