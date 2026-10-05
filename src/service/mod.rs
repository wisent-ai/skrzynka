//! The service behind the API: one `AppState` whose methods are grouped by topic in the
//! sub-modules (Gmail connection, mailboxes, mail reception, messages, outbound mail).

use crate::{
    auth::AuthVerifier,
    db::Database,
    error::AppError,
    gmail::{GmailOAuthBroker, GmailProfile},
    models::{Mailbox, SkarbiecItemMetadata, StatusResponse},
    skarbiec::SkarbiecResolver,
};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
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
    operation_lock: Arc<Mutex<()>>,
}

#[derive(Serialize)]
pub struct GmailOAuthStatusResponse {
    pub flow_id: Uuid,
    pub status: &'static str,
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
    pub fn new(
        database: Database,
        resolver: SkarbiecResolver,
        callback_base_url: Option<&str>,
    ) -> Result<Self, AppError> {
        let gmail_oauth = callback_base_url
            .map(|url| GmailOAuthBroker::new(resolver.clone(), url))
            .transpose()?;
        let auth_verifier = AuthVerifier::from_environment()?;
        Ok(Self {
            auth_verifier,
            database,
            resolver,
            gmail_oauth,
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

    /// Reads every enabled mailbox as its provider reports new mail. Each round
    /// reconciles Skarbiec's declarations and starts a watcher for every enabled
    /// mailbox that has none; a watcher imports what is waiting, then holds an
    /// IMAP IDLE until the provider announces new mail, and ends, recording its
    /// error on the mailbox, when the connection fails. Rounds are spaced by the
    /// IDLE refresh boundary RFC 2177 states for servers, so a failed watcher and
    /// a newly declared item are taken up within it; no cadence is chosen here.
    pub fn start_watching(self) {
        tokio::spawn(async move {
            let mut watchers: HashMap<Uuid, tokio::task::JoinHandle<()>> = HashMap::new();
            loop {
                if let Err(error) = self.reconcile_mailboxes(None).await {
                    tracing::warn!(code = error.code, message = %error.message, "Skarbiec mailbox declarations could not be read");
                }
                match self.database.list_all_mailboxes() {
                    Ok(mailboxes) => {
                        watchers.retain(|_, watcher| !watcher.is_finished());
                        for mailbox in mailboxes.into_iter().filter(|mailbox| mailbox.enabled) {
                            if !watchers.contains_key(&mailbox.id) {
                                let state = self.clone();
                                watchers.insert(
                                    mailbox.id,
                                    tokio::spawn(
                                        async move { state.watch_mailbox(mailbox.id).await },
                                    ),
                                );
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!(code = error.code, "mailboxes could not be listed")
                    }
                }
                tokio::time::sleep(IMAP_IDLE_REFRESH).await;
            }
        });
    }
}

/// RFC 2177: a server may drop an IDLE that has not been re-issued within 30
/// minutes, so clients refresh it every 29. The standard's number, not ours.
const IMAP_IDLE_REFRESH: Duration = Duration::from_secs(29 * 60);
