//! Tag rules: stored per organization, read by every import to tag the mail
//! it writes, and by `message rule apply` to tag the mail already stored.

use super::super::sql::{self, params, Row};
use super::super::{parse_uuid, Database};
use crate::{
    error::AppError,
    mail::{RuleSubject, TagRule},
    models::NewMessage,
};
use chrono::Utc;
use stado_database::sync::Tx;
use uuid::Uuid;

const COLUMNS: &str =
    "id, tag, sender, list, contains, recipient, attachment, mailbox_id, created_at, paused_at";

impl Database {
    /// Every rule of the organization, oldest first.
    pub fn tag_rules(&self, organization_id: &str) -> Result<Vec<TagRule>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM tag_rules WHERE organization_id=$1 ORDER BY created_at, id"
        ))?;
        let rows = statement.query_map(params![organization_id], rule_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Store a rule and answer it.
    pub fn add_tag_rule(&self, organization_id: &str, rule: &TagRule) -> Result<TagRule, AppError> {
        let id = Uuid::new_v4();
        let created_at = Utc::now().to_rfc3339();
        self.lock()?.execute(
            "INSERT INTO tag_rules (id, organization_id, tag, sender, list, contains, recipient, attachment, mailbox_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            params![
                id.to_string(),
                organization_id,
                rule.tag,
                rule.sender,
                rule.list,
                rule.contains,
                rule.recipient,
                rule.attachment,
                rule.mailbox_id.map(|mailbox_id| mailbox_id.to_string()),
                created_at,
            ],
        )?;
        Ok(TagRule {
            id,
            created_at,
            paused_at: None,
            ..rule.clone()
        })
    }

    /// Replace the rule's tag, conditions and mailbox, keeping its ID and
    /// when it was added, and answer it; one the organization does not hold
    /// is not found. The tags it already gave stay.
    pub fn replace_tag_rule(
        &self,
        organization_id: &str,
        id: Uuid,
        rule: &TagRule,
    ) -> Result<TagRule, AppError> {
        let Some(held) = self
            .tag_rules(organization_id)?
            .into_iter()
            .find(|held| held.id == id)
        else {
            return Err(AppError::not_found("tag rule"));
        };
        self.lock()?.execute(
            "UPDATE tag_rules SET tag=$1, sender=$2, list=$3, contains=$4, recipient=$5, attachment=$6, mailbox_id=$7
             WHERE id=$8 AND organization_id=$9",
            params![
                rule.tag,
                rule.sender,
                rule.list,
                rule.contains,
                rule.recipient,
                rule.attachment,
                rule.mailbox_id.map(|mailbox_id| mailbox_id.to_string()),
                id.to_string(),
                organization_id,
            ],
        )?;
        Ok(TagRule {
            id,
            created_at: held.created_at,
            paused_at: held.paused_at,
            ..rule.clone()
        })
    }

    /// Pause the rule from now, or let it tag again with `false`, and answer
    /// it; one the organization does not hold is not found. Pausing a paused
    /// rule keeps when it was first paused.
    pub fn pause_tag_rule(
        &self,
        organization_id: &str,
        id: Uuid,
        paused: bool,
    ) -> Result<TagRule, AppError> {
        let Some(held) = self
            .tag_rules(organization_id)?
            .into_iter()
            .find(|held| held.id == id)
        else {
            return Err(AppError::not_found("tag rule"));
        };
        let paused_at = match (paused, held.paused_at.clone()) {
            (true, Some(since)) => Some(since),
            (true, None) => Some(Utc::now().to_rfc3339()),
            (false, _) => None,
        };
        self.lock()?.execute(
            "UPDATE tag_rules SET paused_at=$1 WHERE id=$2 AND organization_id=$3",
            params![paused_at, id.to_string(), organization_id],
        )?;
        Ok(TagRule { paused_at, ..held })
    }

    /// Remove the rule; one the organization does not hold is not found.
    pub fn remove_tag_rule(&self, organization_id: &str, id: Uuid) -> Result<(), AppError> {
        let held = self
            .tag_rules(organization_id)?
            .iter()
            .any(|rule| rule.id == id);
        if !held {
            return Err(AppError::not_found("tag rule"));
        }
        self.lock()?
            .execute("DELETE FROM tag_rules WHERE id=$1", params![id.to_string()])?;
        Ok(())
    }

    /// Give each stored message the tags listed for it; a tag it carries is
    /// no change.
    pub fn tag_messages(&self, tagged: &[(Uuid, String)]) -> Result<(), AppError> {
        let connection = self.lock()?;
        let transaction = connection.transaction()?;
        let now = Utc::now().to_rfc3339();
        for (message_id, tag) in tagged {
            insert_tag(&transaction, &message_id.to_string(), tag, &now)?;
        }
        transaction.commit()?;
        Ok(())
    }
}

/// The rules that apply to `mailbox_id`'s mail: its organization's rules for
/// every mailbox and those kept to it, paused ones left out, read inside an
/// import's transaction.
pub(super) fn for_mailbox(
    transaction: &Tx<'_>,
    mailbox_id: &str,
) -> Result<Vec<TagRule>, AppError> {
    let mut statement = transaction.prepare(
        "SELECT tag_rules.id, tag_rules.tag, tag_rules.sender, tag_rules.list, tag_rules.contains,
                tag_rules.recipient, tag_rules.attachment, tag_rules.mailbox_id, tag_rules.created_at,
                tag_rules.paused_at
         FROM tag_rules JOIN mailboxes ON mailboxes.organization_id=tag_rules.organization_id
         WHERE mailboxes.id=$1 AND (tag_rules.mailbox_id IS NULL OR tag_rules.mailbox_id=$1)
           AND tag_rules.paused_at IS NULL",
    )?;
    let rows = statement.query_map(params![mailbox_id], rule_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Tag a message the import just wrote with every rule it matches.
pub(super) fn tag_imported(
    transaction: &Tx<'_>,
    mailbox_id: Uuid,
    message_id: &str,
    message: &NewMessage,
    rules: &[TagRule],
) -> Result<(), AppError> {
    let subject = RuleSubject {
        mailbox_id,
        sender: &message.sender,
        list_id: message.mailing_list.list_id.as_deref(),
        to: message.recipients.as_deref(),
        cc: message.cc.as_deref(),
        subject: message.subject.as_deref(),
        body: &message.body_text,
        files: message
            .attachments
            .iter()
            .filter_map(|file| file.filename.as_deref())
            .collect(),
    };
    let now = Utc::now().to_rfc3339();
    for rule in rules.iter().filter(|rule| rule.matches(&subject)) {
        insert_tag(transaction, message_id, &rule.tag, &now)?;
    }
    Ok(())
}

fn insert_tag(
    transaction: &Tx<'_>,
    message_id: &str,
    tag: &str,
    now: &str,
) -> Result<(), AppError> {
    transaction.execute(
        "INSERT INTO message_tags (message_id, tag, tagged_at) VALUES ($1, $2, $3)
         ON CONFLICT (message_id, tag) DO NOTHING",
        params![message_id, tag, now],
    )?;
    Ok(())
}

fn rule_from_row(row: &Row<'_>) -> sql::Result<TagRule> {
    Ok(TagRule {
        id: parse_uuid(row.get::<_, String>("id")?)?,
        tag: row.get("tag")?,
        sender: row.get("sender")?,
        list: row.get("list")?,
        contains: row.get("contains")?,
        recipient: row.get("recipient")?,
        attachment: row.get("attachment")?,
        mailbox_id: row
            .get::<_, Option<String>>("mailbox_id")?
            .map(parse_uuid)
            .transpose()?,
        created_at: row.get("created_at")?,
        paused_at: row.get("paused_at")?,
    })
}
