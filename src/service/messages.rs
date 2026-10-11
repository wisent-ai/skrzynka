//! Stored messages and the replies sent to them.

use super::AppState;
use crate::{
    error::AppError,
    mail,
    models::{CreateReplyRequest, DeliveryStatus, Message, ReplyAttempt},
};
use uuid::Uuid;

impl AppState {
    /// Stored messages, newest first: every one from `offset`, or a page of
    /// `limit` when the caller names one. Zero is refused, never read as a
    /// different page size. `recipient` keeps the messages whose To header
    /// names that address whole (case aside): a longer address that merely
    /// ends in it does not match. `sender` keeps those whose From header
    /// contains it. An empty filter is refused rather than read as no filter.
    pub fn list_messages(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        recipient: Option<&str>,
        sender: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Message>, AppError> {
        page_size(limit)?;
        let recipient = filter_value(recipient, "recipient")?;
        let sender = filter_value(sender, "sender")?;
        if recipient.is_none() && sender.is_none() {
            return self
                .database
                .list_messages(organization_id, mailbox_id, limit, offset);
        }
        // The filters read the stored headers, so the page is cut after them:
        // every stored message is read, from the first one.
        let matching: Vec<Message> = self
            .database
            .list_messages(organization_id, mailbox_id, None, None)?
            .into_iter()
            .filter(|message| match &recipient {
                Some(address) => addressed_to(&message.recipients, address),
                None => true,
            })
            .filter(|message| match &sender {
                Some(part) => message.sender.to_lowercase().contains(part.as_str()),
                None => true,
            })
            .collect();
        let after_offset: Vec<Message> = match offset {
            Some(offset) => matching.into_iter().skip(offset as usize).collect(),
            None => matching,
        };
        Ok(match limit {
            Some(limit) => after_offset.into_iter().take(limit as usize).collect(),
            None => after_offset,
        })
    }

    pub fn get_message(&self, organization_id: &str, id: Uuid) -> Result<Message, AppError> {
        self.database.get_message(organization_id, id)
    }

    pub fn list_replies(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<ReplyAttempt>, AppError> {
        self.database.list_replies(organization_id, message_id)
    }

    pub async fn reply(
        &self,
        organization_id: &str,
        message_id: Uuid,
        request: CreateReplyRequest,
    ) -> Result<ReplyAttempt, AppError> {
        validate_reply_request(&request)?;
        let (attempt, created) = self.database.begin_reply(
            organization_id,
            message_id,
            request.idempotency_key.trim(),
            request.body.trim_end(),
        )?;
        if !created {
            return Ok(attempt);
        }
        let attempt =
            self.database
                .update_reply(attempt.id, DeliveryStatus::Sending, None, None, None)?;
        let message = self.database.get_message(organization_id, message_id)?;
        let mailbox = self
            .database
            .get_mailbox(organization_id, message.mailbox_id)?;
        let credentials = match self
            .resolver
            .resolve_credentials(mailbox.outbound_skarbiec_item_id())
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let recorded = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error.with_unrecorded(recorded, "sending"));
            }
        };
        let body = request.body.trim_end().to_string();
        let result = tokio::task::spawn_blocking(move || {
            mail::send_reply(&mailbox, &credentials, &message, &body)
        })
        .await;
        match result {
            Ok(Ok(provider_message_id)) => self.database.update_reply(
                attempt.id,
                DeliveryStatus::Sent,
                Some(&provider_message_id),
                None,
                None,
            ),
            Ok(Err(error)) if error.code == "SMTP_UNCERTAIN" => self.database.update_reply(
                attempt.id,
                DeliveryStatus::Uncertain,
                None,
                Some("REPLY_UNCERTAIN"),
                Some(&error.message),
            ),
            Ok(Err(error)) => {
                let recorded = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error.with_unrecorded(recorded, "sending"))
            }
            Err(stopped) => self.database.update_reply(
                attempt.id,
                DeliveryStatus::Uncertain,
                None,
                Some("REPLY_UNCERTAIN"),
                Some(&format!(
                    "send task stopped before terminal SMTP evidence was recorded: {stopped}"
                )),
            ),
        }
    }
}

fn validate_reply_request(request: &CreateReplyRequest) -> Result<(), AppError> {
    let key = request.idempotency_key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return Err(AppError::invalid(
            "IDEMPOTENCY_KEY_INVALID",
            "idempotency_key must contain non-whitespace characters only",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(AppError::invalid(
            "REPLY_BODY_INVALID",
            "reply body must not be empty",
        ));
    }
    Ok(())
}

/// A page size of zero names no page; every listing refuses it the same way.
pub(super) fn page_size(limit: Option<u32>) -> Result<(), AppError> {
    if limit == Some(0) {
        return Err(AppError::invalid(
            "LIMIT_INVALID",
            "limit must be at least one; omit it to list every message",
        ));
    }
    Ok(())
}

/// A listing filter as given, lowercased, or refused when it is blank.
pub(super) fn filter_value(value: Option<&str>, name: &str) -> Result<Option<String>, AppError> {
    match value.map(str::trim) {
        None => Ok(None),
        Some("") => Err(AppError::invalid(
            "MESSAGE_FILTER_INVALID",
            &format!("the {name} filter is empty; name an address or leave the filter out"),
        )),
        Some(value) => Ok(Some(value.to_lowercase())),
    }
}

/// Whether the To header names `address` whole: each comma-separated entry,
/// a display name with the address in angle brackets or a bare address,
/// compared without case.
pub(super) fn addressed_to(header: &str, address: &str) -> bool {
    header.split(',').any(|entry| {
        let entry = entry.trim();
        let bare = match (entry.rfind('<'), entry.rfind('>')) {
            (Some(open), Some(close)) if open < close => entry[open..close].trim_start_matches('<'),
            _ => entry,
        };
        bare.trim().eq_ignore_ascii_case(address)
    })
}
