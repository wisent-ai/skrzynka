//! Stored messages and the replies sent to them.

use super::AppState;
use crate::{
    error::AppError,
    mail,
    models::{CreateReplyRequest, DeliveryStatus, Message, ReplyAttempt, MAX_BODY_BYTES, MAX_IDEMPOTENCY_KEY_LENGTH},
};
use uuid::Uuid;

impl AppState {
    pub fn list_messages(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Message>, AppError> {
        self.database
            .list_messages(organization_id, mailbox_id, limit.clamp(1, 500), offset)
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
    if key.is_empty()
        || key.len() > MAX_IDEMPOTENCY_KEY_LENGTH
        || key.chars().any(char::is_whitespace)
    {
        return Err(AppError::invalid(
            "IDEMPOTENCY_KEY_INVALID",
            "idempotency_key must contain 1 to 200 non-whitespace characters",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(AppError::invalid(
            "REPLY_BODY_INVALID",
            "reply body must not be empty",
        ));
    }
    if request.body.len() > MAX_BODY_BYTES {
        return Err(AppError::invalid(
            "REPLY_BODY_TOO_LARGE",
            "reply body exceeds the 256 KiB limit",
        ));
    }
    Ok(())
}
