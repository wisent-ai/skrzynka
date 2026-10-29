//! Tag rules carried between installations: the rules a listing keeps,
//! written as one JSON document naming each rule's mailbox by address, and
//! such a document read back, every rule checked before any is kept.

use super::{super::AppState, NewTagRule};
use crate::{error::AppError, mail::TagRule};
use serde::Serialize;
use uuid::Uuid;

/// What `message rule import` did: every rule kept, in file order, each
/// tagging imported mail.
#[derive(Debug, Serialize)]
pub struct ImportedRules {
    pub rules: Vec<TagRule>,
}

impl AppState {
    /// The rules `tag_rules` keeps for the same filters as the JSON document
    /// `import_tag_rules` reads: `[{tag, sender, list, contains, recipient,
    /// attachment, mailbox}]`, oldest first, `mailbox` the address of the
    /// mailbox a rule is kept to, which another installation resolves, or
    /// null for a rule of every mailbox. A rule's ID and paused state are not
    /// carried.
    pub fn tag_rule_document(
        &self,
        organization_id: &str,
        contains: Option<&str>,
        paused: Option<bool>,
        mailbox_id: Option<Uuid>,
    ) -> Result<String, AppError> {
        let mut rules = Vec::new();
        for rule in self.tag_rules(organization_id, contains, paused, mailbox_id)? {
            let mailbox = match rule.mailbox_id {
                Some(id) => Some(self.database.get_mailbox(organization_id, id)?.email),
                None => None,
            };
            rules.push(NewTagRule {
                tag: rule.tag,
                sender: rule.sender,
                list: rule.list,
                contains: rule.contains,
                recipient: rule.recipient,
                attachment: rule.attachment,
                mailbox,
            });
        }
        serde_json::to_string_pretty(&rules).map_err(|error| {
            AppError::internal(format!("the rules could not be written as JSON: {error}"))
        })
    }

    /// Keep every rule of a document `tag_rule_document` wrote, each as a
    /// new rule that tags imported mail. The whole document is read and every
    /// rule checked as `message rule add` checks one, its mailbox resolved by
    /// ID or address, before any is kept: a document that is not such a
    /// list or holds none is `TAG_RULE_INVALID`, and a rule `rule add`
    /// refuses refuses the document with its own code; nothing changes.
    pub fn import_tag_rules(
        &self,
        organization_id: &str,
        text: &str,
    ) -> Result<ImportedRules, AppError> {
        let asked: Vec<NewTagRule> = serde_json::from_str(text).map_err(|error| {
            AppError::invalid(
                "TAG_RULE_INVALID",
                format!("the import is not a list of {{tag, sender, list, contains, recipient, attachment, mailbox}} rules ({error}); nothing was imported"),
            )
        })?;
        if asked.is_empty() {
            return Err(AppError::invalid(
                "TAG_RULE_INVALID",
                "the import holds no rule; export some with message rule export",
            ));
        }
        let mut checked = Vec::new();
        for rule in asked {
            let tag = rule.tag.clone();
            match self.checked_rule(organization_id, rule) {
                Ok(stored) => checked.push(stored),
                Err(refusal) => {
                    return Err(AppError::new(
                        refusal.status,
                        refusal.code,
                        format!(
                            "the rule tagging {tag:?}: {}; nothing was imported",
                            refusal.message
                        ),
                        refusal.retryable,
                    ))
                }
            }
        }
        let mut rules = Vec::new();
        for stored in &checked {
            rules.push(self.database.add_tag_rule(organization_id, stored)?);
        }
        Ok(ImportedRules { rules })
    }
}
