//! The service behind the API: one `AppState` whose methods are grouped by topic in the
//! sub-modules (Gmail connection, mailboxes, polling, messages, outbound mail).

use crate::{
    auth::AuthVerifier,
    db::Database,
    error::AppError,
    gmail::{GmailOAuthBroker, GmailProfile},
    models::{Mailbox, SkarbiecItemMetadata, StatusResponse, MAX_POLL_INTERVAL_SECONDS, MIN_POLL_INTERVAL_SECONDS},
    skarbiec::SkarbiecResolver,
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

mod gmail;
mod mailboxes;
mod messages;
mod outbound;
mod sync;

/// Due mailboxes are polled this often; each mailbox's own interval decides whether it is due.
const POLL_TICK: Duration = Duration::from_secs(15);


#[derive(Clone)]
pub struct AppState {
    pub auth_verifier: AuthVerifier,
    pub database: Database,
    resolver: SkarbiecResolver,
    gmail_oauth: GmailOAuthBroker,
    pub poll_interval_seconds: u64,
    operation_lock: Arc<Mutex<()>>,
}

#[derive(Serialize)]
pub struct GmailOAuthStatusResponse {
    pub flow_id: Uuid,
    pub status: &'static str,
    pub expires_at: String,
    pub mailbox: Option<Mailbox>,
    pub error: Option<GmailOAuthStatusError>,
}

#[derive(Serialize)]
pub struct GmailOAuthStatusError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

#[derive(Serialize)]
pub struct GmailDelegationStatus {
    pub configured: bool,
    pub service_account: Option<String>,
    pub client_id: Option<String>,
    pub scope: &'static str,
    pub admin_console_url: &'static str,
}

impl AppState {
    pub fn new(
        database: Database,
        resolver: SkarbiecResolver,
        poll_interval_seconds: u64,
        callback_base_url: &str,
    ) -> Result<Self, AppError> {
        if !(MIN_POLL_INTERVAL_SECONDS..=MAX_POLL_INTERVAL_SECONDS).contains(&poll_interval_seconds)
        {
            return Err(AppError::invalid(
                "POLL_INTERVAL_INVALID",
                "poll interval must be between 15 and 86400 seconds",
            ));
        }
        let gmail_oauth = GmailOAuthBroker::new(resolver.clone(), callback_base_url)?;
        let auth_verifier = AuthVerifier::from_environment()?;
        Ok(Self {
            auth_verifier,
            database,
            resolver,
            gmail_oauth,
            poll_interval_seconds,
            operation_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn status(&self, organization_id: &str) -> Result<StatusResponse, AppError> {
        let (mailbox_count, enabled_mailbox_count, message_count) =
            self.database.counts(organization_id)?;
        let skarbiec_available = self.resolver.is_available().await;
        Ok(StatusResponse {
            product: "skrzynka",
            version: env!("CARGO_PKG_VERSION"),
            database_path: self.database.path().display().to_string(),
            schema_version: crate::db::SCHEMA_VERSION,
            mailbox_count,
            enabled_mailbox_count,
            message_count,
            poll_interval_seconds: self.poll_interval_seconds,
            skarbiec_available,
        })
    }

    pub async fn list_skarbiec_items(&self) -> Result<Vec<SkarbiecItemMetadata>, AppError> {
        self.resolver.list_items().await
    }

    pub async fn list_gmail_profiles(&self) -> Result<Vec<GmailProfile>, AppError> {
        self.gmail_oauth.profiles().await
    }

    pub fn start_polling(self) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(POLL_TICK);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                match self.sync_due().await {
                    Ok(summary) => {
                        let failed = summary.mailboxes.iter().filter(|result| !result.ok).count();
                        tracing::info!(
                            mailboxes = summary.mailboxes.len(),
                            failed,
                            "mailbox poll completed"
                        );
                    }
                    Err(error) => tracing::error!(code = error.code, "mailbox poll failed"),
                }
            }
        });
    }
}

