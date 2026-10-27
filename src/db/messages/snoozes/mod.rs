//! Snoozes: a stored message put out of sight until an instant.

use super::super::sql::{params, Client};
use super::super::Database;
use crate::{error::AppError, models::Message};
use std::collections::HashMap;
use uuid::Uuid;

impl Database {
    /// Snooze the message until `until` (RFC 3339, UTC), or wake it when
    /// absent, and answer the message.
    pub fn snooze_message(
        &self,
        organization_id: &str,
        message_id: Uuid,
        until: Option<&str>,
    ) -> Result<Message, AppError> {
        self.get_message(organization_id, message_id)?;
        let id = message_id.to_string();
        match until {
            Some(until) => {
                self.lock()?.execute(
                    "INSERT INTO message_snoozes (message_id, until) VALUES ($1, $2)
                     ON CONFLICT (message_id) DO UPDATE SET until=EXCLUDED.until",
                    params![id, until],
                )?;
            }
            None => {
                self.lock()?
                    .execute("DELETE FROM message_snoozes WHERE message_id=$1", params![id])?;
            }
        }
        self.get_message(organization_id, message_id)
    }

    /// Whether any message of the organization is snoozed past `now`.
    pub fn any_snoozed(&self, organization_id: &str, now: &str) -> Result<bool, AppError> {
        self.lock()?
            .query_row(
                "SELECT EXISTS (
                    SELECT FROM message_snoozes
                    JOIN messages ON messages.id=message_snoozes.message_id
                    JOIN mailboxes ON mailboxes.id=messages.mailbox_id
                    WHERE mailboxes.organization_id=$1 AND message_snoozes.until > $2
                 ) AS snoozed",
                params![organization_id, now],
                |row| row.get::<_, bool>("snoozed"),
            )
            .map_err(Into::into)
    }
}

/// Fill in `snoozed_until` for every message in `messages` that is snoozed.
pub(super) fn attach(client: &Client, messages: &mut [Message]) -> Result<(), AppError> {
    let ids: Vec<String> = messages.iter().map(|message| message.id.to_string()).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let mut statement = client.prepare(
        "SELECT message_id, until FROM message_snoozes WHERE message_id = ANY($1::text[])",
    )?;
    let mut by_message: HashMap<String, String> = HashMap::new();
    for row in statement.query_map(params![ids], |row| {
        Ok((row.get::<_, String>("message_id")?, row.get::<_, String>("until")?))
    })? {
        let (message_id, until) = row?;
        by_message.insert(message_id, until);
    }
    for message in messages.iter_mut() {
        message.snoozed_until = by_message.remove(&message.id.to_string());
    }
    Ok(())
}
