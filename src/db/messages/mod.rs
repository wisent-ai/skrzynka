//! Message rows: the import commit here, the message reads in `reads`.

use super::sql::{params, OptionalExtension};
use super::{is_unique_constraint, Database};
use crate::{
    error::AppError,
    models::{Mailbox, NewMessage},
};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

mod actions;
mod attachments;
mod blocked;
mod bounces;
mod drafts;
mod follow_ups;
mod limits;
mod mutes;
mod notes;
mod reads;
mod rules;
mod signatures;
mod snoozes;
mod statistics;
mod stream;
mod suppressions;
mod tags;
mod templates;
mod unreadable;
mod unsubscribes;
mod views;
pub(super) use reads::page_limit;

impl Database {
    /// Commit a newly adopted mailbox (when `create_mailbox` is true), the
    /// validated messages given, and the source cursor in one database
    /// transaction. A pass calls it once per provider message, so each message
    /// and the cursor past it land together. Existing UIDs are compared before
    /// mutation: identical rows are unchanged, while a different payload for
    /// the same provider UID refuses the commit as a conflict.
    pub fn commit_mailbox_import(
        &self,
        mailbox: &Mailbox,
        create_mailbox: bool,
        messages: &[NewMessage],
        last_uid: u32,
    ) -> Result<(Mailbox, usize, usize), AppError> {
        let connection = self.lock()?;
        let transaction = connection.transaction()?;
        if !create_mailbox {
            // The in-process operation lock cannot serialize another service
            // instance. Own the persisted cursor before comparing or writing
            // any message, so a stale pass cannot move it backwards.
            let stored_uid = transaction.query_row(
                "SELECT last_uid FROM mailboxes WHERE id=$1 FOR UPDATE",
                [mailbox.id.to_string()],
                |row| row.get::<_, i64>("last_uid"),
            )?;
            if stored_uid != i64::from(mailbox.last_uid) {
                return Err(AppError::conflict(
                    "MAILBOX_SYNC_CURSOR_CHANGED",
                    format!(
                        "mailbox {} cursor changed from expected UID {} to stored UID {}; no import data was changed; sync again from the stored cursor",
                        mailbox.id, mailbox.last_uid, stored_uid
                    ),
                ));
            }
        }
        // A UID given twice with the same data is one message; with different
        // data it is a conflict.
        let mut unique_messages: HashMap<u32, &NewMessage> = HashMap::new();
        for message in messages {
            match unique_messages.get(&message.external_uid) {
                Some(existing) if *existing == message => {}
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

        // Stored messages whose attachments were not recorded when they were
        // imported: they are compared on everything else, and their
        // attachments are recorded with this commit.
        let mut unrecorded: Vec<(String, &NewMessage)> = Vec::new();
        let mut existing_uids = HashSet::new();
        for message in unique_messages.values().copied() {
            let existing = transaction
                .query_row(
                    "SELECT id, external_uid, provider_message_id, in_reply_to,
                            references_header, sender, reply_to, recipients,
                            subject, sent_at, sent_at_unreadable, body_text, snippet,
                            details_recorded, list_id, list_unsubscribe, list_unsubscribe_post
                     FROM messages WHERE mailbox_id=$1 AND external_uid=$2",
                    params![mailbox.id.to_string(), i64::from(message.external_uid)],
                    |row| {
                        Ok((
                            row.get::<_, String>("id")?,
                            row.get::<_, bool>("details_recorded")?,
                            NewMessage {
                                external_uid: u32::try_from(row.get::<_, i64>("external_uid")?)?,
                                message_id: row.get("provider_message_id")?,
                                in_reply_to: row.get("in_reply_to")?,
                                references: row.get("references_header")?,
                                sender: row.get("sender")?,
                                reply_to: row.get("reply_to")?,
                                recipients: row.get("recipients")?,
                                cc: None,
                                subject: row.get("subject")?,
                                sent_at: row.get("sent_at")?,
                                sent_at_unreadable: unreadable::read(row)?,
                                body_text: row.get("body_text")?,
                                snippet: row.get("snippet")?,
                                attachments: Vec::new(),
                                mailing_list: attachments::mailing_list_from_row(row)?,
                                bounce: None,
                                authentication: Vec::new(),
                            },
                        ))
                    },
                )
                .optional()?;
            if let Some((stored_id, recorded, mut existing)) = existing {
                // The preview is derived presentation, not provider identity;
                // a bounce report and the provider's authentication verdicts
                // are read from what the row does not keep whole.
                existing.snippet = existing
                    .body_text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                existing.bounce = message.bounce.clone();
                existing.authentication = message.authentication.clone();
                existing.cc = message.cc.clone();
                if recorded {
                    existing.attachments = attachments::recorded_in(&transaction, &stored_id)?;
                } else {
                    existing.attachments = message.attachments.clone();
                    existing.mailing_list = message.mailing_list.clone();
                }
                if &existing != message {
                    return Err(AppError::conflict(
                        "MAILBOX_IMPORT_CONFLICT",
                        format!(
                            "provider UID {} conflicts with retained mailbox data; no import data was changed",
                            message.external_uid
                        ),
                    ));
                }
                if !recorded {
                    unrecorded.push((stored_id, message));
                }
                existing_uids.insert(message.external_uid);
            }
        }
        for (stored_id, message) in &unrecorded {
            attachments::record(&transaction, stored_id, message)?;
        }

        if create_mailbox {
            let inserted = transaction.execute(
                "INSERT INTO mailboxes (
                    id, organization_id, skarbiec_item_id, smtp_skarbiec_item_id,
                    display_name, email, imap_host, imap_port,
                    smtp_host, smtp_port, smtp_security,
                    enabled, last_uid, created_at, updated_at
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, TRUE, 0, $12, $12)",
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

        // The organization's tag rules tag each message as it is written.
        let rules = rules::for_mailbox(&transaction, &mailbox.id.to_string())?;
        let received_at = Utc::now().to_rfc3339();
        for message in unique_messages.values().copied() {
            if existing_uids.contains(&message.external_uid) {
                continue;
            }
            let id = Uuid::new_v4().to_string();
            let sent_at_unreadable = unreadable::stored(message.sent_at_unreadable.as_ref())?;
            transaction.execute(
                "INSERT INTO messages (
                    id, mailbox_id, external_uid, provider_message_id, in_reply_to,
                    references_header, sender, reply_to, recipients, subject,
                    sent_at, sent_at_unreadable, received_at, body_text, snippet
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
                params![
                    id,
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
                    sent_at_unreadable,
                    received_at,
                    message.body_text,
                    message.snippet,
                ],
            )?;
            attachments::record(&transaction, &id, message)?;
            rules::tag_imported(&transaction, mailbox.id, &id, message, &rules)?;
        }
        // Every message given was either already stored or written now; one
        // given twice with the same data counts once as written and once as
        // unchanged.
        let imported = unique_messages.len() - existing_uids.len();
        let unchanged = messages.len() - imported;
        let completed_at = Utc::now().to_rfc3339();
        transaction.execute(
            "UPDATE mailboxes SET last_uid=$2, last_sync_at=$3,
                    last_error_code=NULL, last_error_message=NULL, updated_at=$3,
                    display_name=$4, smtp_skarbiec_item_id=$5, smtp_host=$6,
                    smtp_port=$7, smtp_security=$8
             WHERE id=$1",
            params![
                mailbox.id.to_string(),
                i64::from(last_uid),
                completed_at,
                mailbox.display_name,
                mailbox.smtp_skarbiec_item_id,
                mailbox.smtp_host,
                i64::from(mailbox.smtp_port),
                mailbox.smtp_security.as_str(),
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        Ok((self.get_mailbox_internal(mailbox.id)?, imported, unchanged))
    }
}
