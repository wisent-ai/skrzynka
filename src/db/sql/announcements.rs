//! Announcements that the declared mailboxes changed, carried by the fleet
//! database itself: the triggers `schema.sql` declares on `mailboxes`
//! announce every write that creates, removes, enables, disables or
//! re-points a mailbox, whichever process made it (this service, the CLI on
//! this host, a service on another). An announcement is a wakeup, not a
//! record; the rows stay the truth and a listener reads them after it is
//! subscribed.

use crate::db::Database;
use crate::error::AppError;
use axum::http::StatusCode;
use stado_database::notifications::{self, Subscription};

/// The channel `skrzynka_announce_mailboxes()` in `schema.sql` notifies.
const MAILBOX_CHANNEL: &str = "skrzynka_mailboxes";
/// The channel `skrzynka_announce_drafts()` in `schema.sql` notifies.
const DRAFT_CHANNEL: &str = "skrzynka_drafts";

impl Database {
    /// Subscribe to the announcements. The subscription lives on the fleet
    /// client's own runtime, so it is received through `next_announcement`.
    pub fn subscribe_mailboxes(&self) -> Result<Subscription, AppError> {
        self.lock()?
            .run(|connection| async move {
                notifications::subscribe(&connection, MAILBOX_CHANNEL.to_string()).await
            })?
            .map_err(unavailable)
    }

    /// Wait on `subscription` for the next announcement and hand the
    /// subscription back; an ended subscription answers the cause it ended on,
    /// and the caller subscribes again before reading rows.
    pub fn next_announcement(&self, subscription: Subscription) -> Result<Subscription, AppError> {
        let (subscription, heard) = self.lock()?.run(move |_connection| async move {
            let mut subscription = subscription;
            let heard = subscription.next().await;
            (subscription, heard)
        })?;
        heard.map(|_| subscription).map_err(unavailable)
    }

    /// Subscribe to announcements that a draft's schedule changed.
    pub fn subscribe_drafts(&self) -> Result<Subscription, AppError> {
        self.lock()?
            .run(|connection| async move {
                notifications::subscribe(&connection, DRAFT_CHANNEL.to_string()).await
            })?
            .map_err(draft_announcements_unavailable)
    }

    /// Wait on `subscription` for the next announcement, or for the moment
    /// `due` names, whichever comes first, and hand the subscription back.
    /// A scheduled draft is sent at its time: the wake is the schedule.
    pub fn next_draft_announcement(
        &self,
        subscription: Subscription,
        due: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Subscription, AppError> {
        let wake = match due.map(|due| (due - chrono::Utc::now()).to_std()) {
            // The time has already come: nothing to wait for.
            Some(Err(_passed)) => return Ok(subscription),
            Some(Ok(until_due)) => Some(tokio::time::Instant::now() + until_due),
            None => None,
        };
        let (subscription, heard) = self.lock()?.run(move |_connection| async move {
            let mut subscription = subscription;
            let heard = match wake {
                Some(wake) => tokio::select! {
                    heard = subscription.next() => heard.map(|_| ()),
                    () = tokio::time::sleep_until(wake) => Ok(()),
                },
                None => subscription.next().await.map(|_| ()),
            };
            (subscription, heard)
        })?;
        heard
            .map(|()| subscription)
            .map_err(draft_announcements_unavailable)
    }
}

fn unavailable(error: stado_database::Error) -> AppError {
    AppError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "MAILBOX_ANNOUNCEMENTS_UNAVAILABLE",
        format!("mailbox announcements on the fleet database channel {MAILBOX_CHANNEL}: {error}"),
        true,
    )
}

fn draft_announcements_unavailable(error: stado_database::Error) -> AppError {
    AppError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "DRAFT_ANNOUNCEMENTS_UNAVAILABLE",
        format!("draft schedule announcements on the fleet database channel {DRAFT_CHANNEL}: {error}"),
        true,
    )
}
