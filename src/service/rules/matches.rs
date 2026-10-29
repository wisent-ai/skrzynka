//! What a tag rule matches: the subject a stored message offers a rule, and
//! the stored messages one rule matches now, read without tagging any.

use super::super::AppState;
use crate::{error::AppError, mail::RuleSubject, models::Message};
use uuid::Uuid;

/// The parts of a stored message a rule reads. A message imported before
/// files were recorded carries none.
pub(super) fn subject(message: &Message) -> RuleSubject<'_> {
    RuleSubject {
        mailbox_id: message.mailbox_id,
        sender: &message.sender,
        list_id: message
            .mailing_list
            .as_ref()
            .and_then(|list| list.list_id.as_deref()),
        to: message.recipients.as_deref(),
        cc: message.cc.as_deref(),
        subject: message.subject.as_deref(),
        body: &message.body_text,
        files: message
            .attachments
            .iter()
            .flatten()
            .filter_map(|file| file.filename.as_deref())
            .collect(),
    }
}

impl AppState {
    /// Every stored message of the organization the rule matches now, newest
    /// first, blocked senders' mail included, as `rule apply --rule` would tag
    /// it, paused or not; nothing is tagged. A rule the organization does not
    /// hold is `NOT_FOUND`.
    pub fn tag_rule_matches(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<Vec<Message>, AppError> {
        let rule = self.tag_rule(organization_id, id)?;
        Ok(self
            .database
            .list_messages(organization_id, None, None, None)?
            .into_iter()
            .filter(|message| rule.matches(&subject(message)))
            .collect())
    }
}
