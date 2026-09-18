//! Message rows: the import commit and message reads.

use super::{checked_u32, is_unique_constraint, parse_uuid, Database};
use crate::{error::AppError, models::{Mailbox, Message, NewMessage}};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl Database {
    /// Commit a newly adopted mailbox (when `create_mailbox` is true), every
    /// validated message, and the source cursor in one SQLite transaction.
    /// Existing UIDs are compared before mutation: identical rows are
    /// unchanged, while a different payload for the same provider UID refuses
    /// the entire page as a conflict.
    pub fn commit_mailbox_import(
        &self,
        mailbox: &Mailbox,
        create_mailbox: bool,
        messages: &[NewMessage],
        last_uid: u32,
    ) -> Result<(Mailbox, usize, usize), AppError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let mut unique_messages: HashMap<u32, &NewMessage> = HashMap::new();
        let mut unchanged = 0usize;
        for message in messages {
            match unique_messages.get(&message.external_uid) {
                Some(existing) if *existing == message => {
                    unchanged += 1;
                }
                Some(_) => {
                    return Err(AppError::conflict(
                        "MAILBOX_IMPORT_CONFLICT",
                        format!(
                            "provider UID {} occurs with different message data; no import data was changed",
                            message.external_uid
                        ),
                    ));
                }
                None => {
                    unique_messages.insert(message.external_uid, message);
                }
            }
        }

        let mut existing_uids = HashSet::new();
        for message in unique_messages.values().copied() {
            let existing = transaction
                .query_row(
                    "SELECT external_uid, provider_message_id, in_reply_to,
                            references_header, sender, reply_to, recipients,
                            subject, sent_at, body_text, snippet
                     FROM messages WHERE mailbox_id=?1 AND external_uid=?2",
                    params![mailbox.id.to_string(), i64::from(message.external_uid)],
                    |row| {
                        Ok(NewMessage {
                            external_uid: checked_u32(row.get::<_, i64>(0)?, 0)?,
                            message_id: row.get(1)?,
                            in_reply_to: row.get(2)?,
                            references: row.get(3)?,
                            sender: row.get(4)?,
                            reply_to: row.get(5)?,
                            recipients: row.get(6)?,
                            subject: row.get(7)?,
                            sent_at: row.get(8)?,
                            body_text: row.get(9)?,
                            snippet: row.get(10)?,
                        })
                    },
                )
                .optional()?;
            if let Some(existing) = existing {
                if &existing != message {
                    return Err(AppError::conflict(
                        "MAILBOX_IMPORT_CONFLICT",
                        format!(
                            "provider UID {} conflicts with retained mailbox data; no import data was changed",
                            message.external_uid
                        ),
                    ));
                }
                existing_uids.insert(message.external_uid);
                unchanged += 1;
            }
        }

        if create_mailbox {
            let inserted = transaction.execute(
                "INSERT INTO mailboxes (
                    id, organization_id, skarbiec_item_id, smtp_skarbiec_item_id,
                    display_name, email, imap_host, imap_port,
                    smtp_host, smtp_port, smtp_security, poll_interval_seconds,
                    enabled, last_uid, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, 0, ?13, ?13)",
                params![
                    mailbox.id.to_string(),
                    mailbox.organization_id,
                    mailbox.skarbiec_item_id,
                    mailbox.smtp_skarbiec_item_id,
                    mailbox.display_name,
                    mailbox.email,
                    mailbox.imap_host,
                    i64::from(mailbox.imap_port),
                    mailbox.smtp_host,
                    i64::from(mailbox.smtp_port),
                    mailbox.smtp_security.as_str(),
                    mailbox.poll_interval_seconds as i64,
                    mailbox.created_at,
                ],
            );
            match inserted {
                Ok(_) => {}
                Err(error) if is_unique_constraint(&error) => {
                    return Err(AppError::conflict(
                        "MAILBOX_ALREADY_EXISTS",
                        "a mailbox already uses this Skarbiec item; no import data was changed",
                    ));
                }
                Err(error) => return Err(error.into()),
            }
        }

        let received_at = Utc::now().to_rfc3339();
        let mut imported = 0usize;
        for message in unique_messages.values().copied() {
            if existing_uids.contains(&message.external_uid) {
                continue;
            }
            transaction.execute(
                "INSERT INTO messages (
                    id, mailbox_id, external_uid, provider_message_id, in_reply_to,
                    references_header, sender, reply_to, recipients, subject,
                    sent_at, received_at, body_text, snippet
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    Uuid::new_v4().to_string(),
                    mailbox.id.to_string(),
                    i64::from(message.external_uid),
                    message.message_id,
                    message.in_reply_to,
                    message.references,
                    message.sender,
                    message.reply_to,
                    message.recipients,
                    message.subject,
                    message.sent_at,
                    received_at,
                    message.body_text,
                    message.snippet,
                ],
            )?;
            imported += 1;
        }
        let completed_at = Utc::now().to_rfc3339();
        transaction.execute(
            "UPDATE mailboxes SET last_uid=?2, last_sync_at=?3,
                    last_error_code=NULL, last_error_message=NULL, updated_at=?3
             WHERE id=?1",
            params![mailbox.id.to_string(), i64::from(last_uid), completed_at],
        )?;
        transaction.commit()?;
        drop(connection);
        Ok((self.get_mailbox_internal(mailbox.id)?, imported, unchanged))
    }

    pub fn list_messages(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Message>, AppError> {
        let connection = self.lock()?;
        let sql = if mailbox_id.is_some() {
            "SELECT messages.id, messages.mailbox_id, messages.external_uid,
                    messages.provider_message_id, messages.in_reply_to,
                    messages.references_header, messages.sender, messages.reply_to,
                    messages.recipients, messages.subject, messages.sent_at,
                    messages.received_at, messages.body_text, messages.snippet
             FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE messages.mailbox_id=?1 AND mailboxes.organization_id=?2
             ORDER BY messages.received_at DESC LIMIT ?3 OFFSET ?4"
        } else {
            "SELECT messages.id, messages.mailbox_id, messages.external_uid,
                    messages.provider_message_id, messages.in_reply_to,
                    messages.references_header, messages.sender, messages.reply_to,
                    messages.recipients, messages.subject, messages.sent_at,
                    messages.received_at, messages.body_text, messages.snippet
             FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=?1
             ORDER BY messages.received_at DESC LIMIT ?2 OFFSET ?3"
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
                message_from_row,
            )?
        } else {
            statement.query_map(
                params![organization_id, i64::from(limit), i64::from(offset)],
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
                 WHERE messages.id=?1 AND mailboxes.organization_id=?2",
                params![id.to_string(), organization_id],
                message_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("message"))
    }
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
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
        snippet: row.get(13)?,
    })
}
