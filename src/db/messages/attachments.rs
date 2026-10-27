//! What an import recorded of each message beyond its text: its files and
//! its mailing-list headers. Written in the import's own transaction, read
//! beside the messages, and recorded once, later, for a message imported
//! before they were recorded.

use super::super::sql::{self, params, Client, OptionalExtension, Row};
use super::super::Database;
use crate::{
    error::AppError,
    mail::{MailingList, MessageAttachment},
    models::{Message, NewMessage},
};
use stado_database::sync::Tx;
use std::collections::HashMap;
use uuid::Uuid;

const SELECT_FOR_MESSAGES: &str =
    "SELECT message_id, attachment_index, filename, content_type, size_bytes, unreadable
     FROM message_attachments WHERE message_id = ANY($1::text[])
     ORDER BY message_id, attachment_index";

impl Database {
    /// Record the attachments, list headers and bounce report of a message
    /// imported before they were recorded, as `provider` (the provider's copy
    /// read whole) states them, and answer the stored message. A message whose
    /// details another reader recorded first keeps that record.
    pub fn record_message_details(
        &self,
        organization_id: &str,
        message_id: Uuid,
        provider: &NewMessage,
    ) -> Result<Message, AppError> {
        let connection = self.lock()?;
        let transaction = connection.transaction()?;
        let recorded = transaction
            .query_row(
                "SELECT messages.details_recorded
                 FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
                 WHERE messages.id=$1 AND mailboxes.organization_id=$2
                 FOR UPDATE OF messages",
                params![message_id.to_string(), organization_id],
                |row| row.get::<_, bool>("details_recorded"),
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("message"))?;
        if !recorded {
            record(&transaction, &message_id.to_string(), provider)?;
        }
        transaction.commit()?;
        self.get_message(organization_id, message_id)
    }
}

/// Write the attachments, list headers, sender authentication verdicts and
/// bounce report `message` states for the stored message `message_id` and
/// mark its details recorded, inside the caller's transaction.
pub(super) fn record(
    transaction: &Tx<'_>,
    message_id: &str,
    message: &NewMessage,
) -> Result<(), AppError> {
    let mailing_list = &message.mailing_list;
    for attachment in &message.attachments {
        let size_bytes = match attachment.size_bytes {
            Some(size) => Some(bigint(size, "size_bytes")?),
            None => None,
        };
        transaction.execute(
            "INSERT INTO message_attachments (
                message_id, attachment_index, filename, content_type, size_bytes, unreadable
             ) VALUES ($1, $2, $3, $4, $5, $6)",
            params![
                message_id,
                bigint(attachment.index, "attachment_index")?,
                attachment.filename,
                attachment.content_type,
                size_bytes,
                attachment.unreadable,
            ],
        )?;
    }
    let authentication = serde_json::to_string(&message.authentication).map_err(|cause| {
        AppError::internal(format!(
            "message {message_id}'s authentication verdicts could not be recorded as JSON: {cause}"
        ))
    })?;
    transaction.execute(
        "UPDATE messages SET details_recorded=TRUE, list_id=$2, list_unsubscribe=$3,
                list_unsubscribe_post=$4, authentication_results=$5, cc_header=$6
         WHERE id=$1",
        params![
            message_id,
            mailing_list.list_id,
            mailing_list.unsubscribe,
            mailing_list.unsubscribe_post,
            authentication,
            message.cc,
        ],
    )?;
    if let Some(bounce) = &message.bounce {
        transaction.execute(
            "INSERT INTO message_bounces (message_id, original_message_id, unreadable)
             VALUES ($1, $2, $3)",
            params![message_id, bounce.original_message_id, bounce.unreadable],
        )?;
        for outcome in &bounce.recipients {
            transaction.execute(
                "INSERT INTO message_bounce_recipients (
                    message_id, recipient, action, status, diagnostic
                 ) VALUES ($1, $2, $3, $4, $5)",
                params![
                    message_id,
                    outcome.recipient,
                    outcome.action,
                    outcome.status,
                    outcome.diagnostic,
                ],
            )?;
        }
    }
    Ok(())
}

/// The mailing-list headers a stored row holds, read by column name.
pub(super) fn mailing_list_from_row(row: &Row<'_>) -> sql::Result<MailingList> {
    Ok(MailingList {
        list_id: row.get("list_id")?,
        unsubscribe: row.get("list_unsubscribe")?,
        unsubscribe_post: row.get("list_unsubscribe_post")?,
    })
}

/// The attachments recorded for one message, in index order, read inside the
/// caller's transaction.
pub(super) fn recorded_in(
    transaction: &Tx<'_>,
    message_id: &str,
) -> Result<Vec<MessageAttachment>, AppError> {
    let mut statement = transaction.prepare(SELECT_FOR_MESSAGES)?;
    let rows = statement.query_map(params![vec![message_id.to_string()]], attachment_row)?;
    rows.map(|row| row.map(|(_, attachment)| attachment))
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Fill in the recorded attachments of every message in `messages` whose
/// attachments were recorded; the others keep `attachments` absent.
pub(super) fn attach(client: &Client, messages: &mut [Message]) -> Result<(), AppError> {
    let ids: Vec<String> = messages
        .iter()
        .filter(|message| message.attachments.is_some())
        .map(|message| message.id.to_string())
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let mut statement = client.prepare(SELECT_FOR_MESSAGES)?;
    let mut by_message: HashMap<String, Vec<MessageAttachment>> = HashMap::new();
    for row in statement.query_map(params![ids], attachment_row)? {
        let (message_id, attachment) = row?;
        by_message.entry(message_id).or_default().push(attachment);
    }
    for message in messages.iter_mut() {
        if let Some(attachments) = message.attachments.as_mut() {
            if let Some(recorded) = by_message.remove(&message.id.to_string()) {
                *attachments = recorded;
            }
        }
    }
    Ok(())
}

fn attachment_row(row: &Row<'_>) -> sql::Result<(String, MessageAttachment)> {
    let index = row.get::<_, i64>("attachment_index")?;
    let size_bytes = row.get::<_, Option<i64>>("size_bytes")?;
    Ok((
        row.get("message_id")?,
        MessageAttachment {
            index: usize::try_from(index).map_err(|error| {
                sql::Error::Conversion(format!(
                    "attachment_index {index} is not an attachment index: {error}"
                ))
            })?,
            filename: row.get("filename")?,
            content_type: row.get("content_type")?,
            size_bytes: match size_bytes {
                Some(size) => Some(u64::try_from(size).map_err(|error| {
                    sql::Error::Conversion(format!("size_bytes {size} is not a size: {error}"))
                })?),
                None => None,
            },
            unreadable: row.get("unreadable")?,
        },
    ))
}

/// A count as the BIGINT column `column` holds it.
fn bigint<T>(value: T, column: &str) -> Result<i64, AppError>
where
    T: Copy + std::fmt::Display,
    i64: TryFrom<T>,
    <i64 as TryFrom<T>>::Error: std::fmt::Display,
{
    i64::try_from(value).map_err(|error| {
        AppError::internal(format!(
            "{column} {value} does not fit the database's BIGINT column: {error}"
        ))
    })
}
