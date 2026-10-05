//! The service behind the API: one `AppState` whose methods are grouped by topic in the
//! sub-modules (Gmail connection, mailboxes, polling, messages, outbound mail).

use crate::{
    auth::AuthVerifier,
    db::Database,
    error::AppError,
    gmail::{GmailOAuthBroker, GmailProfile},
    models::{Mailbox, SkarbiecItemMetadata, StatusResponse},
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

#[derive(Clone)]
pub struct AppState {
    pub auth_verifier: AuthVerifier,
    pub database: Database,
    resolver: SkarbiecResolver,
    /// Present only in a process that listens for Google's callback: the
    /// service and `account authorize`, or `account connection`, which
    /// names the callback address it reports on.
    gmail_oauth: Option<GmailOAuthBroker>,
    pub poll_interval_seconds: Option<u64>,
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

/// What every Gmail connection path can do for one account right now.
///
/// The counted verdicts are the point: `usable_paths` of zero says reception
/// cannot be connected today, whatever the vault happens to contain.
#[derive(Serialize)]
pub struct GmailConnectionReadiness {
    pub account: Option<String>,
    pub mailbox_id: Option<Uuid>,
    pub receiving_skarbiec_item_id: Option<String>,
    pub usable_paths: usize,
    pub paths: Vec<GmailConnectionPath>,
}

/// One connection path, its verdict, and what was observed reaching it.
///
/// `observed` carries only non-secret facts an operator has to quote to
/// somebody else — a client ID, a redirect URI, an admin console URL, the
/// Skarbiec item that was authenticated — never a credential.
#[derive(Serialize)]
pub struct GmailConnectionPath {
    pub path: &'static str,
    pub verdict: &'static str,
    pub code: Option<String>,
    pub detail: String,
    pub action: String,
    pub observed: std::collections::BTreeMap<&'static str, String>,
}

impl AppState {
    /// `poll_interval_seconds` is the interval a mailbox whose Skarbiec item declares none is
    /// polled at: `serve --poll-seconds` states it; a one-shot CLI command polls nothing and
    /// passes `None`, so such an item is refused by name instead of given an invented interval.
    pub fn new(
        database: Database,
        resolver: SkarbiecResolver,
        poll_interval_seconds: Option<u64>,
        callback_base_url: Option<&str>,
    ) -> Result<Self, AppError> {
        if poll_interval_seconds == Some(0) {
            return Err(AppError::invalid(
                "POLL_INTERVAL_INVALID",
                "poll interval must be at least 1 second",
            ));
        }
        let gmail_oauth = callback_base_url
            .map(|url| GmailOAuthBroker::new(resolver.clone(), url))
            .transpose()?;
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
            database: self.database.name(),
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
        self.resolver.list_google_profiles().await
    }

    /// The OAuth broker, or a refusal naming the missing callback address
    /// when this process was started without one.
    pub(crate) fn gmail_oauth(&self) -> Result<&GmailOAuthBroker, AppError> {
        self.gmail_oauth.as_ref().ok_or_else(|| {
            AppError::invalid(
                "GMAIL_OAUTH_CALLBACK_NOT_BOUND",
                "Gmail OAuth needs the loopback callback address it will listen on; this \
                 command was started without one. Run `skrzynka account authorize \
                 --provider gmail --bind <SOCKET>` or `skrzynka serve --bind <SOCKET>`",
            )
        })
    }

    /// Polls every due mailbox, then sleeps until the next one is due: the earliest
    /// `last_sync_at + poll_interval_seconds` among enabled mailboxes, or this process's own
    /// interval when none is enabled yet. No tick is assumed between the two.
    pub fn start_polling(self) {
        tokio::spawn(async move {
            loop {
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
                let wait = match self.next_due_in() {
                    Ok(wait) => wait,
                    Err(error) => {
                        tracing::error!(
                            code = error.code,
                            "next mailbox poll could not be scheduled"
                        );
                        Duration::from_secs(self.poll_interval_seconds.unwrap_or_default())
                    }
                };
                tokio::time::sleep(wait).await;
            }
        });
    }
}
