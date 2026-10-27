//! Tag rules: the operator's rules that tag mail as every import writes it,
//! and on demand the mail already stored.

use super::{tags::normalized_tag, AppState};
use crate::{
    error::AppError,
    mail::TagRule,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod matches;
mod transfer;

/// A rule as asked for: the tag and the conditions, at least one given,
/// and the one mailbox (its ID or address) whose mail it tags, if not every.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTagRule {
    pub tag: String,
    pub sender: Option<String>,
    pub list: Option<String>,
    pub contains: Option<String>,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub attachment: Option<String>,
    #[serde(default)]
    pub mailbox: Option<String>,
}

/// What `message rule apply` did: how many stored messages each rule matched.
#[derive(Debug, Serialize)]
pub struct RulesApplied {
    pub rules: Vec<RuleMatches>,
}

#[derive(Debug, Serialize)]
pub struct RuleMatches {
    pub rule_id: Uuid,
    pub tag: String,
    pub messages: usize,
}

impl AppState {
    /// Every rule, oldest first; with `contains` only those whose tag or a
    /// condition holds the text, case aside (a blank one is
    /// `MESSAGE_FILTER_INVALID`), with `paused` only the paused ones (`true`)
    /// or the ones tagging (`false`), and with `mailbox_id` only the rules
    /// that tag that mailbox's mail: those kept to it and those of every
    /// mailbox.
    pub fn tag_rules(
        &self,
        organization_id: &str,
        contains: Option<&str>,
        paused: Option<bool>,
        mailbox_id: Option<Uuid>,
    ) -> Result<Vec<TagRule>, AppError> {
        let contains = super::messages::filter_value(contains, "contains")?;
        Ok(self
            .database
            .tag_rules(organization_id)?
            .into_iter()
            .filter(|rule| paused.is_none_or(|wanted| rule.paused_at.is_some() == wanted))
            .filter(|rule| mailbox_id.is_none_or(|wanted| rule.mailbox_id.is_none_or(|kept| kept == wanted)))
            .filter(|rule| {
                contains.as_deref().is_none_or(|part| {
                    [Some(&rule.tag), rule.sender.as_ref(), rule.list.as_ref(), rule.contains.as_ref(), rule.recipient.as_ref(), rule.attachment.as_ref()]
                        .into_iter()
                        .flatten()
                        .any(|field| field.to_lowercase().contains(part))
                })
            })
            .collect())
    }

    /// Store a rule. Conditions are stored trimmed and in lower case; a
    /// blank one is refused, and so is a rule with none.
    pub fn add_tag_rule(
        &self,
        organization_id: &str,
        rule: NewTagRule,
    ) -> Result<TagRule, AppError> {
        let stored = self.checked_rule(organization_id, rule)?;
        self.database.add_tag_rule(organization_id, &stored)
    }

    /// Replace a rule's tag, conditions and mailbox with `rule`, checked as
    /// a new one is, keeping its ID; the tags it already gave stay, and
    /// later imports tag by the new conditions.
    pub fn replace_tag_rule(&self, organization_id: &str, id: Uuid, rule: NewTagRule) -> Result<TagRule, AppError> {
        let stored = self.checked_rule(organization_id, rule)?;
        self.database.replace_tag_rule(organization_id, id, &stored)
    }

    /// The rule as it is stored: tag normalized, conditions trimmed and in
    /// lower case, mailbox resolved; a blank condition or none is refused.
    fn checked_rule(&self, organization_id: &str, rule: NewTagRule) -> Result<TagRule, AppError> {
        let condition = |value: Option<String>, name: &str| match value.map(|text| text.trim().to_lowercase()) {
            Some(text) if text.is_empty() => Err(AppError::invalid(
                "TAG_RULE_INVALID",
                format!("the {name} condition is empty; name what it must contain or leave it out"),
            )),
            other => Ok(other),
        };
        let mailbox_id = match rule.mailbox.as_deref() {
            Some(selector) => Some(self.resolve_mailbox(organization_id, selector)?.id),
            None => None,
        };
        let stored = TagRule {
            id: Uuid::nil(),
            tag: normalized_tag(&rule.tag)?,
            sender: condition(rule.sender, "sender")?,
            list: condition(rule.list, "list")?,
            contains: condition(rule.contains, "contains")?,
            recipient: condition(rule.recipient, "recipient")?,
            attachment: condition(rule.attachment, "attachment")?,
            mailbox_id,
            created_at: String::new(),
            paused_at: None,
        };
        let conditions = [&stored.sender, &stored.list, &stored.contains, &stored.recipient, &stored.attachment];
        if conditions.iter().all(|condition| condition.is_none()) {
            return Err(AppError::invalid(
                "TAG_RULE_INVALID",
                "a rule names at least one condition (sender, list, contains, recipient or attachment); use `message tag` to tag a message by hand",
            ));
        }
        Ok(stored)
    }

    /// One rule the organization holds, as `rule list` prints it; an ID it
    /// does not hold is `NOT_FOUND`.
    pub fn tag_rule(&self, organization_id: &str, id: Uuid) -> Result<TagRule, AppError> {
        match self.database.tag_rules(organization_id)?.into_iter().find(|rule| rule.id == id) {
            Some(rule) => Ok(rule),
            None => Err(AppError::not_found("tag rule")),
        }
    }

    /// A new rule with rule `id`'s conditions and `tag`, kept to the mailbox
    /// `mailbox` names (its ID or address) or to the rule's own mailbox, or to
    /// every mailbox as the rule is; checked as `rule add` checks a new rule.
    /// The rule copied is left as it was; an ID the organization does not
    /// hold is `NOT_FOUND`.
    pub fn copy_tag_rule(&self, organization_id: &str, id: Uuid, tag: &str, mailbox: Option<&str>) -> Result<TagRule, AppError> {
        let rule = self.tag_rule(organization_id, id)?;
        let mailbox = match mailbox {
            Some(selector) => Some(selector.to_string()),
            None => rule.mailbox_id.map(|kept| kept.to_string()),
        };
        self.add_tag_rule(
            organization_id,
            NewTagRule {
                tag: tag.to_string(),
                sender: rule.sender,
                list: rule.list,
                contains: rule.contains,
                recipient: rule.recipient,
                attachment: rule.attachment,
                mailbox,
            },
        )
    }

    pub fn remove_tag_rule(&self, organization_id: &str, id: Uuid) -> Result<(), AppError> {
        self.database.remove_tag_rule(organization_id, id)
    }

    /// Stop the rule tagging imported mail without losing it, or with
    /// `paused: false` let it tag again; the tags it already gave stay. Mail
    /// imported while it was paused is tagged by `rule apply --rule <ID>`.
    pub fn pause_tag_rule(&self, organization_id: &str, id: Uuid, paused: bool) -> Result<TagRule, AppError> {
        self.database.pause_tag_rule(organization_id, id, paused)
    }

    /// Tag every stored message of the organization by every rule it
    /// matches (a rule kept to one mailbox only that mailbox's mail),
    /// blocked senders' mail included, and answer the matches; paused rules
    /// are skipped. With `only`, by that one rule alone, paused or not, which
    /// the organization must hold.
    pub fn apply_tag_rules(&self, organization_id: &str, only: Option<Uuid>) -> Result<RulesApplied, AppError> {
        let rules: Vec<TagRule> = self
            .database
            .tag_rules(organization_id)?
            .into_iter()
            .filter(|rule| match only {
                Some(id) => rule.id == id,
                None => rule.paused_at.is_none(),
            })
            .collect();
        if only.is_some() && rules.is_empty() {
            return Err(AppError::not_found("tag rule"));
        }
        let messages = self
            .database
            .list_messages(organization_id, None, None, None)?;
        let mut tagged = Vec::new();
        let mut applied = Vec::new();
        for rule in rules {
            let matched: Vec<Uuid> = messages
                .iter()
                .filter(|message| rule.matches(&matches::subject(message)))
                .map(|message| message.id)
                .collect();
            tagged.extend(matched.iter().map(|id| (*id, rule.tag.clone())));
            applied.push(RuleMatches {
                rule_id: rule.id,
                tag: rule.tag,
                messages: matched.len(),
            });
        }
        self.database.tag_messages(&tagged)?;
        Ok(RulesApplied { rules: applied })
    }
}
