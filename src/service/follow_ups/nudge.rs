//! Nudging: one draft per follow-up still owed an answer, the template
//! filled from the message sent and addressed to the same people from the
//! same mailbox. Every draft is filled and its recipients checked before any
//! is kept; nothing is sent, the drafts wait to be read and sent.

use super::{
    super::{drafts::drafted, outbound::stored_addresses},
    AppState, FollowUpFilter, FollowUpState,
};
use crate::{
    error::AppError,
    mail::{Draft, DraftChange},
    models::OutboundMessage,
};
use std::collections::BTreeMap;

impl AppState {
    /// One draft per follow-up `filter` keeps that no stored message answers
    /// yet, from the mailbox that sent it, to its To and Cc, holding the
    /// template `template` with the placeholders `subject`, `recipients`,
    /// `sent_at` and `due_at` it names filled from the sent message and the
    /// follow-up. Its subject is the template's when it has one, else the
    /// sent subject after `Re: `. `filter` keeping answered follow-ups, or
    /// none owed an answer, is `FOLLOW_UP_INVALID`; a placeholder nothing
    /// fills is the template's own refusal, naming the follow-up; a
    /// recipient a send would refuse is `FOLLOW_UP_INVALID` naming every
    /// such address. None of them keeps a draft.
    pub fn nudge_follow_ups(
        &self,
        organization_id: &str,
        filter: &FollowUpFilter,
        template: &str,
    ) -> Result<Vec<Draft>, AppError> {
        let invalid = |message: String| {
            AppError::invalid(
                "FOLLOW_UP_INVALID",
                format!("{message}; nothing was drafted"),
            )
        };
        if filter.state == Some(FollowUpState::Answered) {
            return Err(invalid(
                "an answered follow-up is owed nothing; nudge those waiting or due".to_string(),
            ));
        }
        let named = self.template(organization_id, template)?.placeholders;
        let owed: Vec<_> = self
            .follow_ups(organization_id, filter)?
            .into_iter()
            .filter(|follow_up| follow_up.state != FollowUpState::Answered)
            .collect();
        if owed.is_empty() {
            return Err(invalid(
                "no follow-up the filter keeps is owed an answer".to_string(),
            ));
        }
        let mut drafts = Vec::new();
        let mut addressed = Vec::new();
        for follow_up in owed {
            let sent = self.get_outbound(organization_id, follow_up.outbound_id)?;
            let about = |error: AppError| AppError {
                message: format!(
                    "the follow-up of {} ({:?}): {}",
                    sent.id, sent.subject, error.message
                ),
                ..error
            };
            let values = stated(&sent, &follow_up.due_at, &named);
            let filled = self
                .render_template(organization_id, template, &values, None)
                .map_err(about)?;
            let to = stored_addresses(&sent.recipients);
            let cc = sent.cc.as_deref().map(stored_addresses);
            addressed.extend(to.iter().cloned());
            addressed.extend(cc.iter().flatten().cloned());
            let subject = match filled.subject {
                Some(written) => written,
                None => replied(&sent.subject),
            };
            let change = DraftChange {
                to: Some(to),
                cc,
                bcc: None,
                subject: Some(subject),
                body: Some(filled.body),
                attachments: None,
            };
            drafts.push(drafted(sent.mailbox_id, change)?);
        }
        let refused = self.unsendable(organization_id, &addressed)?;
        if !refused.is_empty() {
            return Err(invalid(format!(
                "a send would refuse {}; narrow the follow-ups or clear them",
                refused.join(", ")
            )));
        }
        drafts
            .iter()
            .map(|draft| {
                self.database
                    .create_draft(organization_id, draft.mailbox_id, draft)
            })
            .collect()
    }
}

/// The values the sent message and its follow-up state for the
/// placeholders the template names: only those, since a value for a
/// placeholder the template lacks is refused.
fn stated(sent: &OutboundMessage, due_at: &str, named: &[String]) -> BTreeMap<String, String> {
    let known = [
        ("subject", Some(sent.subject.clone())),
        ("recipients", Some(sent.recipients.clone())),
        ("sent_at", sent.sent_at.clone()),
        ("due_at", Some(due_at.to_string())),
    ];
    known
        .into_iter()
        .filter(|(key, _)| named.iter().any(|placeholder| placeholder == key))
        .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
        .collect()
}

/// The subject of a message answering one sent as `subject`: the same when
/// it already begins `Re:`, else after `Re: `.
fn replied(subject: &str) -> String {
    match subject.to_ascii_lowercase().starts_with("re:") {
        true => subject.to_string(),
        false => format!("Re: {subject}"),
    }
}
