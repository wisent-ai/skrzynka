//! The service behind the API: one `AppState` whose methods are grouped by topic in the
//! sub-modules (Gmail connection, mailboxes, mail reception, messages, outbound mail).

use crate::{
    db::Database,
    error::AppError,
    gmail::{GmailOAuthBroker, GmailProfile},
    models::{Mailbox, SkarbiecItemMetadata, StatusResponse},
    skarbiec::SkarbiecResolver,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

mod actions;
mod attachments;
pub use attachments::{file_table, FileFilter, StoredFile};
mod blocked;
pub use blocked::blocked_table;
mod delivery;
pub use delivery::ReplyFilter;
mod contacts;
mod gmail;
pub use contacts::{contact_table, vcards, Contact, ContactFilter};
mod drafts;
pub use drafts::{DraftFilter, DraftSendRequest};
mod follow_ups;
pub use follow_ups::{follow_up_table, FollowUpFilter, FollowUpState};
mod filters;
pub use filters::{
    CountedView, MailboxState, MessageFilters, OutboundFilters, SavedFilters, SavedView,
};
mod tables;
pub(crate) use filters::instant as filter_instant;
pub use tables::{bounced_table, draft_table, message_table, outbound_table, reply_table};
mod tags;
pub use tags::{tag_table, RetagRequest, TagFilter};
mod rules;
pub use rules::NewTagRule;
mod signatures;
mod snoozes;
mod suppressions;
pub use suppressions::{suppression_table, SuppressionFilter};
mod templates;
pub use signatures::SignatureRequest;
pub use templates::TemplateFilter;
mod auto_replies;
pub use auto_replies::AutoReplyRequest;
mod forward;
mod watch;
pub use forward::{ForwardRequest, ForwardViewRequest};
mod inbound;
pub use inbound::InboundSyncSummary;
mod limits;
pub use limits::WarmUpRequest;
mod mailboxes;
mod messages;
mod threads;
pub use threads::ReplyViewRequest;
mod outbound;
mod resend;
mod sync;
mod unsubscribe;
pub use unsubscribe::{subscription_table, SubscriptionFilter};

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    resolver: SkarbiecResolver,
    /// Present only in a process that listens for Google's callback: the
    /// service and `account authorize`, or `account connection`, which
    /// names the callback address it reports on.
    gmail_oauth: Option<GmailOAuthBroker>,
    operation_lock: Arc<Mutex<()>>,
    /// Why this service stopped receiving mailbox announcements, once it has:
    /// from then on a mailbox declared anywhere is not watched until the
    /// service starts again, and the health answer says so.
    watching_stopped: Arc<OnceLock<String>>,
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
/// `credential_source` names the vault the receiving item was looked for in,
/// so a refusal for an item that vault does not hold names both the item and
/// the vault, not only the item.
#[derive(Serialize)]
pub struct GmailConnectionReadiness {
    pub account: Option<String>,
    pub mailbox_id: Option<Uuid>,
    pub receiving_skarbiec_item_id: Option<String>,
    pub credential_source: crate::models::CredentialSource,
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
        Ok(Self {
            database,
            resolver,
            gmail_oauth,
            operation_lock: Arc::new(Mutex::new(())),
            watching_stopped: Arc::new(OnceLock::new()),
        })
    }

    pub async fn status(&self, organization_id: &str) -> Result<StatusResponse, AppError> {
        let (mailbox_count, enabled_mailbox_count, message_count) =
            self.database.counts(organization_id)?;
        let skarbiec_unavailable = match self.resolver.availability().await {
            Ok(()) => None,
            Err(error) => Some(error.to_string()),
        };
        Ok(StatusResponse {
            product: "skrzynka",
            version: env!("CARGO_PKG_VERSION"),
            database: self.database.name(),
            schema_version: crate::db::SCHEMA_VERSION,
            mailbox_count,
            enabled_mailbox_count,
            message_count,
            skarbiec_available: skarbiec_unavailable.is_none(),
            skarbiec_unavailable,
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

    /// Why mailbox announcements stopped reaching this service, if they have.
    pub fn watching_stopped(&self) -> Option<&str> {
        self.watching_stopped.get().map(String::as_str)
    }

    /// Reads every enabled mailbox as its provider reports new mail. A round
    /// reconciles Skarbiec's declarations and starts a watcher for every
    /// enabled mailbox that has none; a watcher imports what is waiting, then
    /// holds an IMAP IDLE until the provider announces new mail, and ends,
    /// recording its error on the mailbox, when the connection fails.
    ///
    /// A round runs once the service is subscribed to the fleet database's
    /// mailbox announcements and again on each announcement, which the
    /// database makes for every write that changes what is watched, by this
    /// service, the CLI or a process on another host (`db/sql/schema.sql`).
    /// A mailbox declared anywhere is watched at once; a watcher that stopped
    /// on a failure starts again once a sync of its mailbox succeeds
    /// (`skrzynka sync --mailbox <ID>`, `POST /v1/mailboxes/:id/sync`), or
    /// its declaration changes; no timer restarts it. A service that cannot
    /// subscribe refuses to start, naming the cause; one whose subscription
    /// ends and cannot be taken again keeps serving and answers /healthz
    /// with the cause (`watching_stopped`).
    pub fn start_watching(self) -> Result<(), AppError> {
        let subscription = self.database.subscribe_mailboxes()?;
        // Scheduled drafts are sent, and received mail answered automatically,
        // by the same running service.
        self.start_scheduling()?;
        self.start_answering()?;
        let rounds = Arc::new(Notify::new());
        rounds.notify_one();
        let database = self.database.clone();
        let heard = rounds.clone();
        let stopped = self.watching_stopped.clone();
        tokio::task::spawn_blocking(move || {
            let mut subscription = subscription;
            loop {
                subscription = match database.next_announcement(subscription) {
                    Ok(subscription) => subscription,
                    Err(ended) => {
                        tracing::warn!(code = ended.code, message = %ended.message, "mailbox announcements stopped; subscribing again");
                        match database.subscribe_mailboxes() {
                            Ok(subscription) => subscription,
                            Err(error) => {
                                let cause = format!(
                                    "{}: {}; mailboxes declared from now on are not watched until this service starts again",
                                    error.code, error.message
                                );
                                tracing::error!(code = error.code, message = %cause, "mailbox announcements cannot be received");
                                stopped.get_or_init(|| cause);
                                return;
                            }
                        }
                    }
                };
                heard.notify_one();
            }
        });
        tokio::spawn(async move {
            let mut watchers: HashMap<Uuid, tokio::task::JoinHandle<()>> = HashMap::new();
            loop {
                rounds.notified().await;
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
                        tracing::error!(code = error.code, message = %error.message, "mailboxes could not be listed")
                    }
                }
            }
        });
        Ok(())
    }
}
