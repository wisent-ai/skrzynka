//! Watching stored mail (docs/messages/watch.md): every message an import
//! stores from now on, or after a stored-order number the caller already
//! holds, handed on once, in the order it was stored. A watch wakes on the
//! fleet database's announcement of each import and never polls; a watcher
//! that resumes from the last number it printed misses nothing and repeats
//! nothing, whichever process stored the mail.

use super::AppState;
use crate::{error::AppError, models::Message};
use axum::http::StatusCode;
use serde::Serialize;
use stado_database::notifications::Subscription;
use uuid::Uuid;

/// One stored message and its number in the store order, which a watcher
/// passes back as `after` to resume.
#[derive(Debug, Serialize)]
pub struct WatchedMessage {
    pub stored_order: i64,
    pub message: Message,
}

/// One watcher's place: the announcement it listens on, the mailbox it keeps
/// to and the last number it handed on.
pub struct MessageWatch {
    state: AppState,
    organization_id: String,
    mailbox_id: Option<Uuid>,
    after: Option<i64>,
    subscription: Option<Subscription>,
}

impl AppState {
    /// Start watching the organization's mail, or one mailbox's (by id or
    /// address): after `after` when given, else from what is stored now. The
    /// announcement is listened to before the starting point is read, so no
    /// message stored after it is missed.
    pub fn watch_messages(
        &self,
        organization_id: &str,
        mailbox: Option<&str>,
        after: Option<i64>,
    ) -> Result<MessageWatch, AppError> {
        let mailbox_id = match mailbox {
            Some(selector) => Some(self.resolve_mailbox(organization_id, selector)?.id),
            None => None,
        };
        let subscription = self.database.subscribe_received()?;
        let after = match after {
            Some(after) => Some(after),
            None => self.database.last_stored(organization_id)?,
        };
        Ok(MessageWatch {
            state: self.clone(),
            organization_id: organization_id.to_owned(),
            mailbox_id,
            after,
            subscription: Some(subscription),
        })
    }
}

impl MessageWatch {
    /// The messages stored since the last call, listening for the next
    /// announcement while there are none. A message deleted between being
    /// stored and being read is passed over, as it is no longer stored.
    pub async fn next(&mut self) -> Result<Vec<WatchedMessage>, AppError> {
        loop {
            let stored = self.state.database.stored_after(
                &self.organization_id,
                self.mailbox_id,
                self.after,
            )?;
            if let Some(last) = stored.last() {
                self.after = Some(last.stored_order);
                let mut watched = Vec::with_capacity(stored.len());
                for each in stored {
                    match self
                        .state
                        .database
                        .get_message(&self.organization_id, each.message_id)
                    {
                        Ok(message) => watched.push(WatchedMessage {
                            stored_order: each.stored_order,
                            message,
                        }),
                        Err(error) if error.status == StatusCode::NOT_FOUND => {}
                        Err(error) => return Err(error),
                    }
                }
                if !watched.is_empty() {
                    return Ok(watched);
                }
                continue;
            }
            self.hear_announcement().await?;
        }
    }

    /// The fleet database's next announcement of stored mail, heard through
    /// `next_received`, which reports what it listens for while it does.
    async fn hear_announcement(&mut self) -> Result<(), AppError> {
        let subscription = self.subscription.take().ok_or_else(|| {
            AppError::internal(
                "the watch lost its announcement subscription after an earlier failure",
            )
        })?;
        let database = self.state.database.clone();
        let heard = tokio::task::spawn_blocking(move || database.next_received(subscription))
            .await
            .map_err(|stopped| {
                AppError::internal(format!(
                    "the watch's listening for stored mail stopped: {stopped}"
                ))
            })??;
        self.subscription = Some(heard);
        Ok(())
    }
}
