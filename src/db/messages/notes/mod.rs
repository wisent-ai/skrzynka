//! Notes: the operator's own text on a stored message.

use super::super::sql::{params, Client};
use super::super::Database;
use crate::{error::AppError, models::Message};
use chrono::Utc;
use std::collections::HashMap;
use uuid::Uuid;

impl Database {
    /// Keep `note` on the message, replacing its note, or remove the note
    /// when absent, and answer the message.
    pub fn set_message_note(
        &self,
        organization_id: &str,
        message_id: Uuid,
        note: Option<&str>,
    ) -> Result<Message, AppError> {
        self.get_message(organization_id, message_id)?;
        let id = message_id.to_string();
        match note {
            Some(note) => {
                self.lock()?.execute(
                    "INSERT INTO message_notes (message_id, body, updated_at) VALUES ($1, $2, $3)
                     ON CONFLICT (message_id)
                     DO UPDATE SET body=EXCLUDED.body, updated_at=EXCLUDED.updated_at",
                    params![id, note, Utc::now().to_rfc3339()],
                )?;
            }
            None => {
                self.lock()?
                    .execute("DELETE FROM message_notes WHERE message_id=$1", params![id])?;
            }
        }
        self.get_message(organization_id, message_id)
    }
}

/// Fill in `note` for every message in `messages` that carries one.
pub(super) fn attach(client: &Client, messages: &mut [Message]) -> Result<(), AppError> {
    let ids: Vec<String> = messages.iter().map(|message| message.id.to_string()).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let mut statement = client
        .prepare("SELECT message_id, body FROM message_notes WHERE message_id = ANY($1::text[])")?;
    let mut by_message: HashMap<String, String> = HashMap::new();
    for row in statement.query_map(params![ids], |row| {
        Ok((row.get::<_, String>("message_id")?, row.get::<_, String>("body")?))
    })? {
        let (message_id, body) = row?;
        by_message.insert(message_id, body);
    }
    for message in messages.iter_mut() {
        message.note = by_message.remove(&message.id.to_string());
    }
    Ok(())
}
