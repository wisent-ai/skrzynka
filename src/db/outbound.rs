//! Outbound message rows: begin, update, read and interrupted-send recovery.

use super::sql::{self, params, OptionalExtension, Row};
use super::{is_unique_constraint, parse_uuid, Database};
use crate::{
    error::AppError,
    mail::OutboundAttachment,
    models::{DeliveryStatus, OutboundMessage},
};
use chrono::Utc;
use uuid::Uuid;

/// The columns every outbound read selects, by the names `outbound_from_row` reads.
const OUTBOUND_COLUMNS: &str = "outbound_messages.id, outbound_messages.mailbox_id,
        outbound_messages.idempotency_key, outbound_messages.recipients,
        outbound_messages.cc, outbound_messages.bcc, outbound_messages.subject,
        outbound_messages.body, outbound_messages.attachments, outbound_messages.status,
        outbound_messages.provider_message_id,
        outbound_messages.error_code, outbound_messages.error_message,
        outbound_messages.created_at, outbound_messages.updated_at,
        outbound_messages.sent_at, outbound_messages.signature";

impl Database {
    /// Outbound mail is claimed the same way a reply is: the idempotency key is
    /// the unique row, so a repeated request returns the first attempt instead
    /// of handing the provider a second copy. The same key with different
    /// files, by name, type, size or SHA-256, is a different request.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        idempotency_key: &str,
        recipients: &str,
        cc: Option<&str>,
        bcc: Option<&str>,
        subject: &str,
        body: &str,
        attachments: &[OutboundAttachment],
    ) -> Result<(OutboundMessage, bool), AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        if let Some(existing) = self.get_outbound_by_key(organization_id, idempotency_key)? {
            if existing.mailbox_id != mailbox_id
                || existing.recipients != recipients
                || existing.cc.as_deref() != cc
                || existing.bcc.as_deref() != bcc
                || existing.subject != subject
                || existing.body != body
                || existing.attachments != attachments
            {
                return Err(AppError::conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key already belongs to a different outbound message",
                ));
            }
            return Ok((existing, false));
        }
        let recorded = serde_json::to_string(attachments).map_err(|cause| {
            AppError::internal(format!(
                "the outbound message's attachments could not be recorded as JSON: {cause}"
            ))
        })?;
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let result = self.claim_send(&mailbox_id.to_string(), |transaction| {
            transaction.execute(
                "INSERT INTO outbound_messages (
                    id, mailbox_id, idempotency_key, recipients, cc, bcc, subject, body,
                    attachments, status, created_at, updated_at, signature
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending', $10, $10,
                    (SELECT signature FROM mailbox_signatures WHERE mailbox_id=$2))",
                params![
                    id.to_string(),
                    mailbox_id.to_string(),
                    idempotency_key,
                    recipients,
                    cc,
                    bcc,
                    subject,
                    body,
                    recorded,
                    now,
                ],
            )
        })?;
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
            "UPDATE outbound_messages SET status=$2, provider_message_id=$3,
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
        self.get_outbound_internal(id)
    }

    pub fn get_outbound(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<OutboundMessage, AppError> {
        self.lock()?
            .query_row(
                &format!(
                    "SELECT {OUTBOUND_COLUMNS}
                     FROM outbound_messages
                     JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
                     WHERE outbound_messages.id=$1 AND mailboxes.organization_id=$2"
                ),
                params![id.to_string(), organization_id],
                outbound_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("outbound message"))
    }

    /// Outbound messages newest first; `offset` None reads from the first
    /// one, which `OFFSET NULL` means, as for stored messages.
    pub fn list_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<OutboundMessage>, AppError> {
        let connection = self.lock()?;
        let sql = if mailbox_id.is_some() {
            format!(
                "SELECT {OUTBOUND_COLUMNS}
                 FROM outbound_messages
                 JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
                 WHERE outbound_messages.mailbox_id=$1 AND mailboxes.organization_id=$2
                 ORDER BY outbound_messages.created_at DESC LIMIT $3 OFFSET $4"
            )
        } else {
            format!(
                "SELECT {OUTBOUND_COLUMNS}
                 FROM outbound_messages
                 JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
                 WHERE mailboxes.organization_id=$1
                 ORDER BY outbound_messages.created_at DESC LIMIT $2 OFFSET $3"
            )
        };
        let mut statement = connection.prepare(&sql)?;
        let rows = if let Some(mailbox_id) = mailbox_id {
            statement.query_map(
                params![
                    mailbox_id.to_string(),
                    organization_id,
                    super::messages::page_limit(limit),
                    offset.map(i64::from)
                ],
                outbound_from_row,
            )?
        } else {
            statement.query_map(
                params![
                    organization_id,
                    super::messages::page_limit(limit),
                    offset.map(i64::from)
                ],
                outbound_from_row,
            )?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub(super) fn get_outbound_internal(&self, id: Uuid) -> Result<OutboundMessage, AppError> {
        self.lock()?
            .query_row(
                &format!("SELECT {OUTBOUND_COLUMNS} FROM outbound_messages WHERE id=$1"),
                [id.to_string()],
                outbound_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("outbound message"))
    }

    /// The organization's outbound row claimed under `key`, if one is.
    pub fn get_outbound_by_key(
        &self,
        organization_id: &str,
        key: &str,
    ) -> Result<Option<OutboundMessage>, AppError> {
        self.lock()?
            .query_row(
                &format!(
                    "SELECT {OUTBOUND_COLUMNS}
                     FROM outbound_messages
                     JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
                     WHERE outbound_messages.idempotency_key=$1
                       AND mailboxes.organization_id=$2"
                ),
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
                    updated_at=$1 WHERE status='sending'",
            [&now],
        )?;
        connection.execute(
            "UPDATE outbound_messages SET status='uncertain',
                    error_code='OUTBOUND_UNCERTAIN',
                    error_message='send was interrupted before terminal SMTP evidence was recorded',
                    updated_at=$1 WHERE status='sending'",
            [&now],
        )?;
        connection.execute(
            "UPDATE unsubscribe_attempts SET status='uncertain',
                    error_code='UNSUBSCRIBE_UNCERTAIN',
                    error_message='the unsubscribe request was interrupted before the list answered',
                    updated_at=$1 WHERE status='sending'",
            [&now],
        )?;
        Ok(())
    }
}

/// One outbound message, read by the names of the columns every outbound
/// query selects.
fn outbound_from_row(row: &Row<'_>) -> sql::Result<OutboundMessage> {
    let attachments = row.get::<_, String>("attachments")?;
    Ok(OutboundMessage {
        id: parse_uuid(row.get::<_, String>("id")?)?,
        mailbox_id: parse_uuid(row.get::<_, String>("mailbox_id")?)?,
        idempotency_key: row.get("idempotency_key")?,
        recipients: row.get("recipients")?,
        cc: row.get("cc")?,
        bcc: row.get("bcc")?,
        subject: row.get("subject")?,
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
    })
}
