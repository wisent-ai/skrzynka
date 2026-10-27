//! Reply attempt rows: begin, update and read.

use super::sql::{self, params, OptionalExtension, Row};
use super::{is_unique_constraint, parse_uuid, Database};
use crate::{
    error::AppError,
    mail::{OutboundAttachment, ReplyCopies},
    models::{DeliveryStatus, ReplyAttempt},
};
use chrono::Utc;
use uuid::Uuid;

/// The columns every reply read selects, by the names `reply_from_row` reads.
const REPLY_COLUMNS: &str = "reply_attempts.id, reply_attempts.message_id,
        reply_attempts.idempotency_key, reply_attempts.body, reply_attempts.attachments,
        reply_attempts.status, reply_attempts.provider_message_id,
        reply_attempts.error_code, reply_attempts.error_message,
        reply_attempts.created_at, reply_attempts.updated_at,
        reply_attempts.sent_at, reply_attempts.signature, reply_attempts.reply_all,
        reply_attempts.cc, reply_attempts.bcc";

impl Database {
    /// A reply is claimed by its idempotency key before anything reaches the
    /// provider. The same key with a different message, body, files (by
    /// name, type, size or SHA-256), reply-all choice or copies is a
    /// different request.
    pub fn begin_reply(
        &self,
        organization_id: &str,
        message_id: Uuid,
        idempotency_key: &str,
        body: &str,
        attachments: &[OutboundAttachment],
        copies: &ReplyCopies,
    ) -> Result<(ReplyAttempt, bool), AppError> {
        let message = self.get_message(organization_id, message_id)?;
        // Copies are stored comma-separated, absent for none.
        let joined = |addresses: &[String]| (!addresses.is_empty()).then(|| addresses.join(", "));
        let (cc, bcc) = (joined(&copies.cc), joined(&copies.bcc));
        if let Some(existing) = self.get_reply_by_key(organization_id, idempotency_key)? {
            if existing.message_id != message_id
                || existing.body != body
                || existing.attachments != attachments
                || existing.reply_all != copies.all
                || existing.cc != cc
                || existing.bcc != bcc
            {
                return Err(AppError::conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key already belongs to a different reply request",
                ));
            }
            return Ok((existing, false));
        }
        let recorded = serde_json::to_string(attachments).map_err(|cause| {
            AppError::internal(format!(
                "the reply's attachments could not be recorded as JSON: {cause}"
            ))
        })?;
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let result = self.claim_send(&message.mailbox_id.to_string(), |transaction| {
            transaction.execute(
                "INSERT INTO reply_attempts (
                    id, message_id, idempotency_key, body, attachments, status, created_at,
                    updated_at, reply_all, cc, bcc, signature
                 ) VALUES ($1, $2, $3, $4, $5, 'pending', $6, $6, $7, $8, $9,
                    (SELECT mailbox_signatures.signature FROM mailbox_signatures
                     JOIN messages ON messages.mailbox_id=mailbox_signatures.mailbox_id
                     WHERE messages.id=$2))",
                params![
                    id.to_string(),
                    message_id.to_string(),
                    idempotency_key,
                    body,
                    recorded,
                    now,
                    copies.all,
                    cc,
                    bcc,
                ],
            )
        })?;
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
            "UPDATE reply_attempts SET status=$2, provider_message_id=$3,
                    error_code=$4, error_message=$5, updated_at=$6,
                    sent_at=COALESCE($7, sent_at) WHERE id=$1",
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
                &format!("SELECT {REPLY_COLUMNS} FROM reply_attempts WHERE id=$1"),
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
        let mut statement = connection.prepare(&format!(
            "SELECT {REPLY_COLUMNS}
             FROM reply_attempts
             JOIN messages ON messages.id=reply_attempts.message_id
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE reply_attempts.message_id=$1 AND mailboxes.organization_id=$2
             ORDER BY reply_attempts.created_at DESC"
        ))?;
        let rows = statement.query_map(
            params![message_id.to_string(), organization_id],
            reply_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Every reply attempt of the organization, or of one mailbox's messages,
    /// newest first.
    pub fn all_replies(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
    ) -> Result<Vec<ReplyAttempt>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {REPLY_COLUMNS}
             FROM reply_attempts
             JOIN messages ON messages.id=reply_attempts.message_id
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=$1 AND ($2::text IS NULL OR mailboxes.id=$2)
             ORDER BY reply_attempts.created_at DESC"
        ))?;
        let mailbox = mailbox_id.map(|id| id.to_string());
        let rows = statement.query_map(params![organization_id, mailbox], reply_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Every stored message of the organization a reply to was accepted by
    /// the provider (`sent`); a failed or uncertain reply answers nothing.
    pub fn answered_messages(
        &self,
        organization_id: &str,
    ) -> Result<std::collections::HashSet<Uuid>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT reply_attempts.message_id AS message_id
             FROM reply_attempts
             JOIN messages ON messages.id=reply_attempts.message_id
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=$1 AND reply_attempts.status='sent'",
        )?;
        let rows = statement.query_map(params![organization_id], |row| {
            parse_uuid(row.get::<_, String>("message_id")?)
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    pub(super) fn get_reply_by_key(
        &self,
        organization_id: &str,
        key: &str,
    ) -> Result<Option<ReplyAttempt>, AppError> {
        self.lock()?
            .query_row(
                &format!(
                    "SELECT {REPLY_COLUMNS}
                     FROM reply_attempts
                     JOIN messages ON messages.id=reply_attempts.message_id
                     JOIN mailboxes ON mailboxes.id=messages.mailbox_id
                     WHERE reply_attempts.idempotency_key=$1 AND mailboxes.organization_id=$2"
                ),
                params![key, organization_id],
                reply_from_row,
            )
            .optional()
            .map_err(Into::into)
    }
}

/// One reply attempt, read by the names of the columns every reply query selects.
fn reply_from_row(row: &Row<'_>) -> sql::Result<ReplyAttempt> {
    let attachments = row.get::<_, String>("attachments")?;
    Ok(ReplyAttempt {
        id: parse_uuid(row.get::<_, String>("id")?)?,
        message_id: parse_uuid(row.get::<_, String>("message_id")?)?,
        idempotency_key: row.get("idempotency_key")?,
        body: row.get("body")?,
        attachments: serde_json::from_str(&attachments).map_err(|error| {
            sql::Error::Conversion(format!(
                "column attachments holds {attachments:?}, which is no list of recorded attachments: {error}"
            ))
        })?,
        status: row
            .get::<_, String>("status")?
            .parse::<DeliveryStatus>()
            .map_err(|error| {
                sql::Error::Conversion(format!("column status holds no delivery status: {error}"))
            })?,
        provider_message_id: row.get("provider_message_id")?,
        error_code: row.get("error_code")?,
        error_message: row.get("error_message")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        sent_at: row.get("sent_at")?,
        signature: row.get("signature")?,
        reply_all: row.get("reply_all")?,
        cc: row.get("cc")?,
        bcc: row.get("bcc")?,
    })
}
