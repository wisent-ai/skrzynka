//! Reply attempt rows: begin, update and read.

use super::{is_unique_constraint, parse_enum, parse_uuid, Database};
use crate::{
    error::AppError,
    models::{DeliveryStatus, ReplyAttempt},
};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

impl Database {
    pub fn begin_reply(
        &self,
        organization_id: &str,
        message_id: Uuid,
        idempotency_key: &str,
        body: &str,
    ) -> Result<(ReplyAttempt, bool), AppError> {
        self.get_message(organization_id, message_id)?;
        if let Some(existing) = self.get_reply_by_key(organization_id, idempotency_key)? {
            if existing.message_id != message_id || existing.body != body {
                return Err(AppError::conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key already belongs to a different reply request",
                ));
            }
            return Ok((existing, false));
        }
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let result = self.lock()?.execute(
            "INSERT INTO reply_attempts (
                id, message_id, idempotency_key, body, status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)",
            params![
                id.to_string(),
                message_id.to_string(),
                idempotency_key,
                body,
                now,
            ],
        );
        match result {
            Ok(_) => Ok((self.get_reply(id)?, true)),
            Err(error) if is_unique_constraint(&error) => {
                let existing = self
                    .get_reply_by_key(organization_id, idempotency_key)?
                    .ok_or_else(|| {
                        AppError::conflict("IDEMPOTENCY_CONFLICT", "reply request already exists")
                    })?;
                Ok((existing, false))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn update_reply(
        &self,
        id: Uuid,
        status: DeliveryStatus,
        provider_message_id: Option<&str>,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<ReplyAttempt, AppError> {
        let now = Utc::now().to_rfc3339();
        let sent_at = (status == DeliveryStatus::Sent).then_some(now.as_str());
        self.lock()?.execute(
            "UPDATE reply_attempts SET status=?2, provider_message_id=?3,
                    error_code=?4, error_message=?5, updated_at=?6,
                    sent_at=COALESCE(?7, sent_at) WHERE id=?1",
            params![
                id.to_string(),
                status.as_str(),
                provider_message_id,
                error_code,
                error_message,
                now,
                sent_at,
            ],
        )?;
        self.get_reply(id)
    }

    pub fn get_reply(&self, id: Uuid) -> Result<ReplyAttempt, AppError> {
        self.lock()?
            .query_row(
                "SELECT id, message_id, idempotency_key, body, status,
                        provider_message_id, error_code, error_message,
                        created_at, updated_at, sent_at
                 FROM reply_attempts WHERE id=?1",
                [id.to_string()],
                reply_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("reply attempt"))
    }

    pub fn list_replies(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<ReplyAttempt>, AppError> {
        self.get_message(organization_id, message_id)?;
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT reply_attempts.id, reply_attempts.message_id,
                    reply_attempts.idempotency_key, reply_attempts.body,
                    reply_attempts.status, reply_attempts.provider_message_id,
                    reply_attempts.error_code, reply_attempts.error_message,
                    reply_attempts.created_at, reply_attempts.updated_at,
                    reply_attempts.sent_at
             FROM reply_attempts
             JOIN messages ON messages.id=reply_attempts.message_id
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE reply_attempts.message_id=?1 AND mailboxes.organization_id=?2
             ORDER BY reply_attempts.created_at DESC",
        )?;
        let rows = statement.query_map(
            params![message_id.to_string(), organization_id],
            reply_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub(super) fn get_reply_by_key(
        &self,
        organization_id: &str,
        key: &str,
    ) -> Result<Option<ReplyAttempt>, AppError> {
        self.lock()?
            .query_row(
                "SELECT reply_attempts.id, reply_attempts.message_id,
                        reply_attempts.idempotency_key, reply_attempts.body,
                        reply_attempts.status, reply_attempts.provider_message_id,
                        reply_attempts.error_code, reply_attempts.error_message,
                        reply_attempts.created_at, reply_attempts.updated_at,
                        reply_attempts.sent_at
                 FROM reply_attempts
                 JOIN messages ON messages.id=reply_attempts.message_id
                 JOIN mailboxes ON mailboxes.id=messages.mailbox_id
                 WHERE reply_attempts.idempotency_key=?1 AND mailboxes.organization_id=?2",
                params![key, organization_id],
                reply_from_row,
            )
            .optional()
            .map_err(Into::into)
    }
}

fn reply_from_row(row: &Row<'_>) -> rusqlite::Result<ReplyAttempt> {
    Ok(ReplyAttempt {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        message_id: parse_uuid(row.get::<_, String>(1)?)?,
        idempotency_key: row.get(2)?,
        body: row.get(3)?,
        status: parse_enum(row.get::<_, String>(4)?, 4)?,
        provider_message_id: row.get(5)?,
        error_code: row.get(6)?,
        error_message: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        sent_at: row.get(10)?,
    })
}
