//! Putting a whole message into one of a mailbox's provider folders: how an
//! export made with `message export` goes back to the provider.

use super::super::AppState;
use crate::{
    error::AppError,
    mail::{self, AppendOutcome},
};
use uuid::Uuid;

impl AppState {
    /// Append `source` to `folder` of the mailbox's provider, unless that
    /// folder already holds a message with the same Message-ID. A blank
    /// folder or an empty message is refused before the provider is asked.
    pub async fn append_message(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        folder: &str,
        source: Vec<u8>,
    ) -> Result<AppendOutcome, AppError> {
        let folder = folder.trim().to_string();
        if folder.is_empty() {
            return Err(AppError::invalid(
                "MESSAGE_APPEND_INVALID",
                "the folder to append to is empty; name a folder as `mailbox folders` lists it",
            ));
        }
        if source.is_empty() {
            return Err(AppError::invalid(
                "MESSAGE_APPEND_INVALID",
                "the message to append is empty; give a whole message, such as an .eml file",
            ));
        }
        let mailbox = self.database.get_mailbox(organization_id, mailbox_id)?;
        let credentials = self
            .resolver
            .resolve_credentials(&mailbox.skarbiec_item_id)
            .await?;
        let email = mailbox.email.clone();
        tokio::task::spawn_blocking(move || {
            mail::append_message(&mailbox, &credentials, &folder, &source)
        })
        .await
        .map_err(|error| {
            AppError::internal(format!(
                "appending a message to {email} stopped before the provider answered: {error}"
            ))
        })?
    }
}
