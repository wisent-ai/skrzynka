//! Muted conversations: the Message-IDs a conversation carried or named when
//! it was muted, kept per mailbox, so later mail of it is recognized by its
//! headers alone.

use super::super::sql::params;
use super::super::{parse_uuid, Database};
use crate::error::AppError;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl Database {
    /// Keep each of `identities` as muted in `mailbox_id`; one kept already
    /// is no change. Answers how many were new.
    pub fn mute_identities(&self, mailbox_id: Uuid, identities: &[String]) -> Result<u64, AppError> {
        let connection = self.lock()?;
        let transaction = connection.transaction()?;
        let now = Utc::now().to_rfc3339();
        let mailbox = mailbox_id.to_string();
        let added = identities
            .iter()
            .map(|identity| {
                transaction.execute(
                    "INSERT INTO muted_conversations (mailbox_id, identity, muted_at) VALUES ($1, $2, $3)
                     ON CONFLICT (mailbox_id, identity) DO NOTHING",
                    params![mailbox, identity, now],
                )
            })
            .sum::<Result<u64, _>>()?;
        transaction.commit()?;
        Ok(added)
    }

    /// Stop keeping each of `identities` as muted in `mailbox_id`; answers
    /// how many were kept.
    pub fn unmute_identities(&self, mailbox_id: Uuid, identities: &[String]) -> Result<u64, AppError> {
        self.lock()?
            .execute(
                "DELETE FROM muted_conversations WHERE mailbox_id=$1 AND identity = ANY($2::text[])",
                params![mailbox_id.to_string(), identities.to_vec()],
            )
            .map_err(Into::into)
    }

    /// Every muted Message-ID of the organization's mailboxes, by mailbox.
    pub fn muted_identities(&self, organization_id: &str) -> Result<HashMap<Uuid, HashSet<String>>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT muted_conversations.mailbox_id AS mailbox_id, muted_conversations.identity AS identity
             FROM muted_conversations JOIN mailboxes ON mailboxes.id=muted_conversations.mailbox_id
             WHERE mailboxes.organization_id=$1",
        )?;
        let mut muted: HashMap<Uuid, HashSet<String>> = HashMap::new();
        for row in statement.query_map(params![organization_id], |row| {
            Ok((parse_uuid(row.get::<_, String>("mailbox_id")?)?, row.get::<_, String>("identity")?))
        })? {
            let (mailbox_id, identity) = row?;
            muted.entry(mailbox_id).or_default().insert(identity);
        }
        Ok(muted)
    }
}
