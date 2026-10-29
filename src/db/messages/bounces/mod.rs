//! The delivery status notification a stored message is, read back with the
//! message: what its import recorded in `message_bounces` and
//! `message_bounce_recipients`.

use super::super::sql::{params, Client};
use crate::{
    error::AppError,
    mail::{BounceReport, RecipientOutcome},
    models::Message,
};
use std::collections::HashMap;

mod clearances;

/// Every report of the given messages, with each recipient row; a report
/// that names no recipient comes back as one row with every recipient column
/// absent and is read as no recipient. Recipients come in address order.
const SELECT_FOR_MESSAGES: &str = "SELECT message_bounces.message_id AS message_id,
            message_bounces.original_message_id AS original_message_id,
            message_bounces.unreadable AS unreadable,
            outcomes.message_id IS NOT NULL AS has_recipient,
            outcomes.recipient AS recipient, outcomes.action AS action,
            outcomes.status AS status, outcomes.diagnostic AS diagnostic
     FROM message_bounces
     LEFT JOIN message_bounce_recipients AS outcomes
         ON outcomes.message_id=message_bounces.message_id
     WHERE message_bounces.message_id = ANY($1::text[])
     ORDER BY message_bounces.message_id, outcomes.recipient";

/// Fill in `bounce` for every message in `messages` that is a recorded
/// delivery status notification; the others keep it absent.
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
    let mut by_message: HashMap<String, BounceReport> = HashMap::new();
    let rows = statement.query_map(params![ids], |row| {
        let message_id: String = row.get("message_id")?;
        let original_message_id: Option<String> = row.get("original_message_id")?;
        let unreadable: Option<String> = row.get("unreadable")?;
        let outcome = match row.get::<_, bool>("has_recipient")? {
            true => Some(RecipientOutcome {
                recipient: row.get("recipient")?,
                action: row.get("action")?,
                status: row.get("status")?,
                diagnostic: row.get("diagnostic")?,
            }),
            false => None,
        };
        Ok((message_id, original_message_id, unreadable, outcome))
    })?;
    for row in rows {
        let (message_id, original_message_id, unreadable, outcome) = row?;
        let report = by_message
            .entry(message_id)
            .or_insert_with(|| BounceReport {
                original_message_id,
                recipients: Vec::new(),
                unreadable,
            });
        report.recipients.extend(outcome);
    }
    for message in messages.iter_mut() {
        message.bounce = by_message.remove(&message.id.to_string());
    }
    Ok(())
}
