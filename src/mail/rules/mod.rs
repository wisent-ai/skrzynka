//! A tag rule: the operator's own rule that gives mail a tag as it arrives.
//! Each condition is a text the field must contain, case aside; a condition
//! not given matches every message, and a rule names at least one.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagRule {
    pub id: Uuid,
    /// The tag a matching message gets, as `message tag` stores it.
    pub tag: String,
    /// Text the From header must contain.
    pub sender: Option<String>,
    /// Text the List-Id header must contain.
    pub list: Option<String>,
    /// Text the subject or stored text must contain.
    pub contains: Option<String>,
    /// Text the To or Cc header must contain: mail sent to one address or alias.
    #[serde(default)]
    pub recipient: Option<String>,
    /// Text the recorded name of one of the message's files must contain:
    /// mail carrying an invoice, a contract, a `.pdf`.
    #[serde(default)]
    pub attachment: Option<String>,
    /// The one mailbox whose mail the rule tags; absent for every mailbox.
    #[serde(default)]
    pub mailbox_id: Option<Uuid>,
    pub created_at: String,
    /// When the rule was paused, as `message rule pause` keeps it: imports
    /// and `rule apply` without `--rule` skip it. Absent while it tags.
    #[serde(default)]
    pub paused_at: Option<String>,
}

/// The fields of one message a rule reads.
pub struct RuleSubject<'a> {
    pub mailbox_id: Uuid,
    pub sender: &'a str,
    pub list_id: Option<&'a str>,
    pub to: Option<&'a str>,
    pub cc: Option<&'a str>,
    pub subject: Option<&'a str>,
    pub body: &'a str,
    /// The recorded names of the message's files; a file that states none is left out.
    pub files: Vec<&'a str>,
}

impl TagRule {
    /// Whether the message is of the rule's mailbox, when it names one, and
    /// passes every condition the rule names.
    pub fn matches(&self, message: &RuleSubject<'_>) -> bool {
        let holds = |text: &str, part: &str| text.to_lowercase().contains(part);
        let from = self
            .sender
            .as_deref()
            .is_none_or(|part| holds(message.sender, part));
        let listed = self
            .list
            .as_deref()
            .is_none_or(|part| message.list_id.is_some_and(|list_id| holds(list_id, part)));
        let mentions = self.contains.as_deref().is_none_or(|part| {
            holds(message.body, part) || message.subject.is_some_and(|subject| holds(subject, part))
        });
        let addressed = self.recipient.as_deref().is_none_or(|part| {
            message.to.is_some_and(|to| holds(to, part)) || message.cc.is_some_and(|cc| holds(cc, part))
        });
        let carried = self
            .attachment
            .as_deref()
            .is_none_or(|part| message.files.iter().any(|name| holds(name, part)));
        let kept = self.mailbox_id.is_none_or(|mailbox_id| mailbox_id == message.mailbox_id);
        kept && from && listed && mentions && addressed && carried
    }
}
