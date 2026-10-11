//! Message reads: a page of an organization's messages, newest first, and
//! one message by id.

use super::super::sql::{self, params, OptionalExtension, Row};
use super::super::{checked_u32, parse_uuid, Database};
use crate::{error::AppError, models::Message};
use uuid::Uuid;

impl Database {
    /// Messages newest first; `offset` None reads from the first one, which
    /// `OFFSET NULL` means.
    pub fn list_messages(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Message>, AppError> {
        let connection = self.lock()?;
        let sql = if mailbox_id.is_some() {
            "SELECT messages.id, messages.mailbox_id, messages.external_uid,
                    messages.provider_message_id, messages.in_reply_to,
                    messages.references_header, messages.sender, messages.reply_to,
                    messages.recipients, messages.subject, messages.sent_at,
                    messages.received_at, messages.body_text, messages.snippet
             FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE messages.mailbox_id=$1 AND mailboxes.organization_id=$2
             ORDER BY messages.received_at DESC LIMIT $3 OFFSET $4"
        } else {
            "SELECT messages.id, messages.mailbox_id, messages.external_uid,
                    messages.provider_message_id, messages.in_reply_to,
                    messages.references_header, messages.sender, messages.reply_to,
                    messages.recipients, messages.subject, messages.sent_at,
                    messages.received_at, messages.body_text, messages.snippet
             FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=$1
             ORDER BY messages.received_at DESC LIMIT $2 OFFSET $3"
        };
        let mut statement = connection.prepare(sql)?;
        let rows = if let Some(mailbox_id) = mailbox_id {
            statement.query_map(
                params![
                    mailbox_id.to_string(),
                    organization_id,
                    page_limit(limit),
                    offset.map(i64::from)
                ],
                message_from_row,
            )?
        } else {
            statement.query_map(
                params![organization_id, page_limit(limit), offset.map(i64::from)],
                message_from_row,
            )?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_message(&self, organization_id: &str, id: Uuid) -> Result<Message, AppError> {
        self.lock()?
            .query_row(
                "SELECT messages.id, messages.mailbox_id, messages.external_uid,
                        messages.provider_message_id, messages.in_reply_to,
                        messages.references_header, messages.sender, messages.reply_to,
                        messages.recipients, messages.subject, messages.sent_at,
                        messages.received_at, messages.body_text, messages.snippet
                 FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
                 WHERE messages.id=$1 AND mailboxes.organization_id=$2",
                params![id.to_string(), organization_id],
                message_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("message"))
    }
}

/// The bound a listing passes to `LIMIT`: the caller's page size, or NULL,
/// which Postgres reads as `LIMIT ALL`.
pub(in crate::db) fn page_limit(limit: Option<u32>) -> Option<i64> {
    limit.map(i64::from)
}

fn message_from_row(row: &Row<'_>) -> sql::Result<Message> {
    let mut message = Message {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        mailbox_id: parse_uuid(row.get::<_, String>(1)?)?,
        external_uid: checked_u32(row.get::<_, i64>(2)?, 2)?,
        message_id: row.get(3)?,
        in_reply_to: row.get(4)?,
        references: row.get(5)?,
        sender: row.get(6)?,
        reply_to: row.get(7)?,
        recipients: row.get(8)?,
        subject: row.get(9)?,
        sent_at: row.get(10)?,
        received_at: row.get(11)?,
        body_text: row.get(12)?,
        snippet: String::new(),
    };
    message.snippet = message
        .body_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Ok(message)
}
