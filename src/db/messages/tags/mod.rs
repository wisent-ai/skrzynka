//! The operator's own labels on stored messages, kept only in Skrzynka.

use super::super::sql::{params, Client};
use super::super::Database;
use crate::{error::AppError, models::Message};
use chrono::Utc;
use std::collections::HashMap;
use uuid::Uuid;

impl Database {
    /// Add `add` and remove `remove` on message `message_id` in one
    /// transaction, and answer the message with its tags. Adding a tag it
    /// carries or removing one it does not is no change.
    pub fn retag_message(
        &self,
        organization_id: &str,
        message_id: Uuid,
        add: &[String],
        remove: &[String],
    ) -> Result<Message, AppError> {
        self.get_message(organization_id, message_id)?;
        let id = message_id.to_string();
        let now = Utc::now().to_rfc3339();
        {
            let connection = self.lock()?;
            let transaction = connection.transaction()?;
            for tag in add {
                transaction.execute(
                    "INSERT INTO message_tags (message_id, tag, tagged_at) VALUES ($1, $2, $3)
                     ON CONFLICT (message_id, tag) DO NOTHING",
                    params![id, tag, now],
                )?;
            }
            for tag in remove {
                transaction.execute(
                    "DELETE FROM message_tags WHERE message_id=$1 AND tag=$2",
                    params![id, tag],
                )?;
            }
            transaction.commit()?;
        }
        self.get_message(organization_id, message_id)
    }

    /// Every tag the organization's messages carry, or one mailbox's, with
    /// how many carry it, most used first.
    pub fn list_tags(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
    ) -> Result<Vec<(String, i64)>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT message_tags.tag AS tag, COUNT(*) AS count
             FROM message_tags
             JOIN messages ON messages.id=message_tags.message_id
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=$1
               AND ($2::text IS NULL OR mailboxes.id=$2)
             GROUP BY message_tags.tag
             ORDER BY count DESC, tag",
        )?;
        let mailbox = mailbox_id.map(|id| id.to_string());
        let rows = statement.query_map(params![organization_id, mailbox], |row| {
            Ok((row.get::<_, String>("tag")?, row.get::<_, i64>("count")?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Give every message of the organization carrying `from` the tag `to`
    /// instead (a message carrying both keeps one), and the tag rules and
    /// saved views naming `from` the name `to`, in one transaction. Answers
    /// how many messages, rules and views changed.
    pub fn rename_tag(
        &self,
        organization_id: &str,
        from: &str,
        to: &str,
    ) -> Result<(u64, u64, u64), AppError> {
        let connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO message_tags (message_id, tag, tagged_at)
             SELECT message_tags.message_id, $3, message_tags.tagged_at FROM message_tags
             JOIN messages ON messages.id=message_tags.message_id
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=$1 AND message_tags.tag=$2
             ON CONFLICT (message_id, tag) DO NOTHING",
            params![organization_id, from, to],
        )?;
        let messages = transaction.execute(REMOVE_TAG, params![organization_id, from])?;
        let rules = transaction.execute(
            "UPDATE tag_rules SET tag=$3 WHERE organization_id=$1 AND tag=$2",
            params![organization_id, from, to],
        )?;
        let views = transaction.execute(
            "UPDATE saved_views SET filters=jsonb_set(filters::jsonb, '{tag}', to_jsonb($3::text))::text,
                    updated_at=$4
             WHERE organization_id=$1 AND filters::jsonb->>'tag'=$2",
            params![organization_id, from, to, Utc::now().to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok((messages, rules, views))
    }

    /// Take `tag` off every message of the organization; answers how many
    /// carried it. Rules and views naming it are left as they are.
    pub fn remove_tag(&self, organization_id: &str, tag: &str) -> Result<u64, AppError> {
        Ok(self.lock()?.execute(REMOVE_TAG, params![organization_id, tag])?)
    }
}

/// Delete tag `$2` from the messages of organization `$1`.
const REMOVE_TAG: &str = "DELETE FROM message_tags WHERE tag=$2 AND message_id IN (
     SELECT messages.id FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
     WHERE mailboxes.organization_id=$1)";

/// Fill in `tags` for every message in `messages`, in tag order.
pub(super) fn attach(client: &Client, messages: &mut [Message]) -> Result<(), AppError> {
    let ids: Vec<String> = messages.iter().map(|message| message.id.to_string()).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let mut statement = client.prepare(
        "SELECT message_id, tag FROM message_tags
         WHERE message_id = ANY($1::text[]) ORDER BY message_id, tag",
    )?;
    let mut by_message: HashMap<String, Vec<String>> = HashMap::new();
    for row in statement.query_map(params![ids], |row| {
        Ok((row.get::<_, String>("message_id")?, row.get::<_, String>("tag")?))
    })? {
        let (message_id, tag) = row?;
        by_message.entry(message_id).or_default().push(tag);
    }
    for message in messages.iter_mut() {
        if let Some(tags) = by_message.remove(&message.id.to_string()) {
            message.tags = tags;
        }
    }
    Ok(())
}
