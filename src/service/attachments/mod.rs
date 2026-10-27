//! A stored message's attachments: what its import recorded, and the bytes of
//! one of them, read from the provider's own copy of the message because
//! Skrzynka keeps no attachment bytes.

use super::AppState;
use crate::{
    error::AppError,
    mail::{self, AttachmentPart, MessageAttachment},
    models::{Mailbox, Message, NewMessage},
};
use axum::http::StatusCode;
use uuid::Uuid;

mod find;
mod table;
pub use find::{FileFilter, StoredFile};
pub use table::file_table;

/// One attachment's description and its decoded bytes.
pub struct AttachmentContent {
    pub attachment: MessageAttachment,
    pub bytes: Vec<u8>,
}

impl AppState {
    /// The attachments of one stored message. A message imported before
    /// attachments were recorded is read from the provider once, and what it
    /// carries is recorded and answered.
    pub async fn list_attachments(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<MessageAttachment>, AppError> {
        let message = self.database.get_message(organization_id, message_id)?;
        if let Some(attachments) = message.attachments {
            return Ok(attachments);
        }
        let (provider, _) = self.read_provider_message(organization_id, &message).await?;
        self.database
            .record_message_details(organization_id, message_id, &provider)?
            .attachments
            .ok_or_else(|| {
                AppError::internal(format!(
                    "message {message_id} details were recorded, but the stored message reads as not recorded"
                ))
            })
    }

    /// The decoded bytes of attachment `index` of one stored message, read
    /// from the provider now. The provider's copy must still be the message
    /// that was stored, and the attachment the one its import recorded.
    pub async fn read_attachment(
        &self,
        organization_id: &str,
        message_id: Uuid,
        index: usize,
    ) -> Result<AttachmentContent, AppError> {
        let message = self.database.get_message(organization_id, message_id)?;
        if let Some(recorded) = &message.attachments {
            if recorded.get(index).is_none() {
                return Err(attachment_not_found(message_id, index, recorded));
            }
        }
        let (provider, parts) = self.read_provider_message(organization_id, &message).await?;
        let described: Vec<MessageAttachment> =
            parts.iter().map(|part| part.attachment.clone()).collect();
        if message.attachments.is_none() {
            self.database
                .record_message_details(organization_id, message_id, &provider)?;
        }
        let part = parts
            .into_iter()
            .nth(index)
            .ok_or_else(|| attachment_not_found(message_id, index, &described))?;
        if let Some(recorded) = message
            .attachments
            .as_ref()
            .and_then(|recorded| recorded.get(index))
        {
            if *recorded != part.attachment {
                return Err(AppError::conflict(
                    "MESSAGE_SOURCE_CHANGED",
                    format!(
                        "attachment {index} of message {message_id} was recorded as {} but the provider's copy now describes it as {}; nothing was read",
                        describe(recorded),
                        describe(&part.attachment)
                    ),
                ));
            }
        }
        let bytes = part.content.map_err(|reason| {
            AppError::dependency(
                "ATTACHMENT_UNDECODABLE",
                format!(
                    "attachment {index} of message {message_id}, {}, cannot be read: {reason}",
                    describe(&part.attachment)
                ),
                false,
            )
        })?;
        Ok(AttachmentContent {
            attachment: part.attachment,
            bytes,
        })
    }

    /// The provider's copy of `message`, read whole: what it states and every
    /// attachment with its bytes. The copy is read by the message's UID in its
    /// mailbox's INBOX, and must be the message Skrzynka stored under that UID.
    pub(in crate::service) async fn read_provider_message(
        &self,
        organization_id: &str,
        message: &Message,
    ) -> Result<(NewMessage, Vec<AttachmentPart>), AppError> {
        let (_, provider, parts) = self.read_provider_source(organization_id, message).await?;
        Ok((provider, parts))
    }

    /// The original message as the provider holds it (its whole MIME
    /// source, RFC 5322), read by UID with `BODY.PEEK[]` and only when it is
    /// still the message Skrzynka stored. Nothing is changed at the provider.
    pub async fn message_source(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<u8>, AppError> {
        let message = self.database.get_message(organization_id, message_id)?;
        let (source, _, _) = self.read_provider_source(organization_id, &message).await?;
        Ok(source)
    }

    /// The provider's copy of `message`: its source bytes and what they read
    /// as, refused when the UID no longer holds it.
    async fn read_provider_source(
        &self,
        organization_id: &str,
        message: &Message,
    ) -> Result<(Vec<u8>, NewMessage, Vec<AttachmentPart>), AppError> {
        let mailbox = self
            .database
            .get_mailbox(organization_id, message.mailbox_id)?;
        let credentials = self
            .resolver
            .resolve_credentials(&mailbox.skarbiec_item_id)
            .await?;
        let uid = message.external_uid;
        let reader = mailbox.clone();
        let source = tokio::task::spawn_blocking(move || {
            mail::fetch_message_source(&reader, &credentials, uid)
        })
        .await
        .map_err(|error| {
            AppError::internal(format!(
                "reading UID {uid} of mailbox {} stopped before the provider answered: {error}",
                mailbox.id
            ))
        })??;
        let Some(source) = source else {
            return Err(AppError::new(
                StatusCode::GONE,
                "MESSAGE_SOURCE_GONE",
                format!(
                    "INBOX of {} at {}:{} no longer holds UID {uid}, where message {} was imported from: the provider deleted, archived or moved it; its stored text remains, nothing more can be read from it",
                    mailbox.email, mailbox.imap_host, mailbox.imap_port, message.id
                ),
                false,
            ));
        };
        let (provider, parts) = mail::read_message(uid, &source)?;
        if !provider.is_stored_as(message) {
            return Err(source_changed(&mailbox, message, provider.message_id));
        }
        Ok((source, provider, parts))
    }
}

/// The refusal for an index the message's attachments do not reach, naming
/// the indexes it does have.
fn attachment_not_found(
    message_id: Uuid,
    index: usize,
    attachments: &[MessageAttachment],
) -> AppError {
    let held = match (attachments.first(), attachments.last()) {
        (Some(first), Some(last)) => format!(
            "it has {}, indexed {} to {}",
            attachments.len(),
            first.index,
            last.index
        ),
        _ => "it has no attachments".to_string(),
    };
    AppError::new(
        StatusCode::NOT_FOUND,
        "ATTACHMENT_NOT_FOUND",
        format!("message {message_id} has no attachment {index}: {held}"),
        false,
    )
}

fn source_changed(mailbox: &Mailbox, message: &Message, observed: Option<String>) -> AppError {
    AppError::conflict(
        "MESSAGE_SOURCE_CHANGED",
        format!(
            "UID {} in INBOX of {} at {}:{} now holds a different message than message {} (stored Message-ID {:?}, provider Message-ID {:?}); the provider renumbered its mail, so nothing was read by this UID",
            message.external_uid,
            mailbox.email,
            mailbox.imap_host,
            mailbox.imap_port,
            message.id,
            message.message_id,
            observed
        ),
    )
}

/// An attachment as a sentence names it: its file name or that it has none,
/// its type, and its size or that it is unknown.
fn describe(attachment: &MessageAttachment) -> String {
    let name = match &attachment.filename {
        Some(name) => format!("{name:?}"),
        None => "an unnamed part".to_string(),
    };
    match attachment.size_bytes {
        Some(size) => format!("{name} ({}, {size} bytes)", attachment.content_type),
        None => format!("{name} ({}, size unknown)", attachment.content_type),
    }
}
