//! Outbound message rows: begin, update, read and interrupted-send recovery.

use super::{is_unique_constraint, parse_enum, parse_uuid, Database};
use crate::{
    error::AppError,
    models::{DeliveryStatus, OutboundMessage},
};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

impl Database {
    /// Outbound mail is claimed the same way a reply is: the idempotency key is
    /// the unique row, so a repeated request returns the first attempt instead
    /// of handing the provider a second copy.
    pub fn begin_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        idempotency_key: &str,
        recipients: &str,
        cc: Option<&str>,
        subject: &str,
        body: &str,
    ) -> Result<(OutboundMessage, bool), AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        if let Some(existing) = self.get_outbound_by_key(organization_id, idempotency_key)? {
            if existing.mailbox_id != mailbox_id
                || existing.recipients != recipients
                || existing.cc.as_deref() != cc
                || existing.subject != subject
                || existing.body != body
            {
                return Err(AppError::conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key already belongs to a different outbound message",
                ));
            }
            return Ok((existing, false));
        }
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let result = self.lock()?.execute(
            "INSERT INTO outbound_messages (
                id, mailbox_id, idempotency_key, recipients, cc, subject, body,
                status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?8)",
            params![
                id.to_string(),
                mailbox_id.to_string(),
                idempotency_key,
                recipients,
                cc,
                subject,
                body,
                now,
            ],
        );
        match result {
            Ok(_) => Ok((self.get_outbound_internal(id)?, true)),
            Err(error) if is_unique_constraint(&error) => {
                let existing = self
                    .get_outbound_by_key(organization_id, idempotency_key)?
                    .ok_or_else(|| {
                        AppError::conflict(
                            "IDEMPOTENCY_CONFLICT",
                            "outbound message request already exists",
                        )
                    })?;
                Ok((existing, false))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn update_outbound(
        &self,
        id: Uuid,
        status: DeliveryStatus,
        provider_message_id: Option<&str>,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<OutboundMessage, AppError> {
        let now = Utc::now().to_rfc3339();
        let sent_at = (status == DeliveryStatus::Sent).then_some(now.as_str());
        self.lock()?.execute(
            "UPDATE outbound_messages SET status=?2, provider_message_id=?3,
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
        self.get_outbound_internal(id)
    }

    pub fn get_outbound(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<OutboundMessage, AppError> {
        self.lock()?
            .query_row(
                "SELECT outbound_messages.id, outbound_messages.mailbox_id,
                        outbound_messages.idempotency_key, outbound_messages.recipients,
                        outbound_messages.cc, outbound_messages.subject,
                        outbound_messages.body, outbound_messages.status,
                        outbound_messages.provider_message_id,
                        outbound_messages.error_code, outbound_messages.error_message,
                        outbound_messages.created_at, outbound_messages.updated_at,
                        outbound_messages.sent_at
                 FROM outbound_messages
                 JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
                 WHERE outbound_messages.id=?1 AND mailboxes.organization_id=?2",
                params![id.to_string(), organization_id],
                outbound_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("outbound message"))
    }

    pub fn list_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<OutboundMessage>, AppError> {
        let connection = self.lock()?;
        let sql = if mailbox_id.is_some() {
            "SELECT outbound_messages.id, outbound_messages.mailbox_id,
                    outbound_messages.idempotency_key, outbound_messages.recipients,
                    outbound_messages.cc, outbound_messages.subject,
                    outbound_messages.body, outbound_messages.status,
                    outbound_messages.provider_message_id,
                    outbound_messages.error_code, outbound_messages.error_message,
                    outbound_messages.created_at, outbound_messages.updated_at,
                    outbound_messages.sent_at
             FROM outbound_messages
             JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
             WHERE outbound_messages.mailbox_id=?1 AND mailboxes.organization_id=?2
             ORDER BY outbound_messages.created_at DESC LIMIT ?3 OFFSET ?4"
        } else {
            "SELECT outbound_messages.id, outbound_messages.mailbox_id,
                    outbound_messages.idempotency_key, outbound_messages.recipients,
                    outbound_messages.cc, outbound_messages.subject,
                    outbound_messages.body, outbound_messages.status,
                    outbound_messages.provider_message_id,
                    outbound_messages.error_code, outbound_messages.error_message,
                    outbound_messages.created_at, outbound_messages.updated_at,
                    outbound_messages.sent_at
             FROM outbound_messages
             JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
             WHERE mailboxes.organization_id=?1
             ORDER BY outbound_messages.created_at DESC LIMIT ?2 OFFSET ?3"
        };
        let mut statement = connection.prepare(sql)?;
        let rows = if let Some(mailbox_id) = mailbox_id {
            statement.query_map(
                params![
                    mailbox_id.to_string(),
                    organization_id,
                    i64::from(limit),
                    i64::from(offset)
                ],
                outbound_from_row,
            )?
        } else {
            statement.query_map(
                params![organization_id, i64::from(limit), i64::from(offset)],
                outbound_from_row,
            )?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub(super) fn get_outbound_internal(&self, id: Uuid) -> Result<OutboundMessage, AppError> {
        self.lock()?
            .query_row(
                "SELECT id, mailbox_id, idempotency_key, recipients, cc, subject,
                        body, status, provider_message_id, error_code,
                        error_message, created_at, updated_at, sent_at
                 FROM outbound_messages WHERE id=?1",
                [id.to_string()],
                outbound_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("outbound message"))
    }

    pub(super) fn get_outbound_by_key(
        &self,
        organization_id: &str,
        key: &str,
    ) -> Result<Option<OutboundMessage>, AppError> {
        self.lock()?
            .query_row(
                "SELECT outbound_messages.id, outbound_messages.mailbox_id,
                        outbound_messages.idempotency_key, outbound_messages.recipients,
                        outbound_messages.cc, outbound_messages.subject,
                        outbound_messages.body, outbound_messages.status,
                        outbound_messages.provider_message_id,
                        outbound_messages.error_code, outbound_messages.error_message,
                        outbound_messages.created_at, outbound_messages.updated_at,
                        outbound_messages.sent_at
                 FROM outbound_messages
                 JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
                 WHERE outbound_messages.idempotency_key=?1
                   AND mailboxes.organization_id=?2",
                params![key, organization_id],
                outbound_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// A process that dies mid-send leaves `sending` behind. That row lost its
    /// terminal SMTP evidence, so it becomes `uncertain` for a human to settle
    /// against the provider's Sent mail — never an automatic resend.
    pub(super) fn recover_interrupted_sends(&self) -> Result<(), AppError> {
        let now = Utc::now().to_rfc3339();
        let connection = self.lock()?;
        connection.execute(
            "UPDATE reply_attempts SET status='uncertain',
                    error_code='REPLY_UNCERTAIN',
                    error_message='send was interrupted before terminal SMTP evidence was recorded',
                    updated_at=?1 WHERE status='sending'",
            [&now],
        )?;
        connection.execute(
            "UPDATE outbound_messages SET status='uncertain',
                    error_code='OUTBOUND_UNCERTAIN',
                    error_message='send was interrupted before terminal SMTP evidence was recorded',
                    updated_at=?1 WHERE status='sending'",
            [&now],
        )?;
        Ok(())
    }
}

fn outbound_from_row(row: &Row<'_>) -> rusqlite::Result<OutboundMessage> {
    Ok(OutboundMessage {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        mailbox_id: parse_uuid(row.get::<_, String>(1)?)?,
        idempotency_key: row.get(2)?,
        recipients: row.get(3)?,
        cc: row.get(4)?,
        subject: row.get(5)?,
        body: row.get(6)?,
        status: parse_enum(row.get::<_, String>(7)?, 7)?,
        provider_message_id: row.get(8)?,
        error_code: row.get(9)?,
        error_message: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        sent_at: row.get(13)?,
    })
}
