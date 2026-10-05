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
    /// different page size.
    pub fn list_messages(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: Option<u32>,
        offset: u32,
    ) -> Result<Vec<Message>, AppError> {
        page_size(limit)?;
        self.database
            .list_messages(organization_id, mailbox_id, limit, offset)
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
                let _ = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error);
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
                let _ = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error)
            }
            Err(_) => self.database.update_reply(
                attempt.id,
                DeliveryStatus::Uncertain,
                None,
                Some("REPLY_UNCERTAIN"),
                Some("send task stopped before terminal SMTP evidence was recorded"),
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
