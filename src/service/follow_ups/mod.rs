//! Follow-ups: an originated message the operator expects an answer to by
//! an instant. A stored message of the sending mailbox whose In-Reply-To or
//! References names the sent message's Message-ID answers it; a bounce
//! report does not. Nothing is sent by a follow-up: it says which messages
//! are owed an answer, and the operator decides what to do.

use super::{snoozes::instant, AppState};
use crate::{
    error::AppError,
    models::{DeliveryStatus, Message, OutboundMessage},
};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

mod many;
mod nudge;
mod send;
mod table;
pub use table::follow_up_table;

/// Where a follow-up stands; `followup list --state` keeps one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpState {
    /// Not answered, and the instant is still ahead.
    Waiting,
    /// Not answered, and the instant has passed.
    Due,
    /// A stored message answers it.
    Answered,
}

/// One follow-up as `message followup list` prints it.
#[derive(Debug, Clone, Serialize)]
pub struct FollowUp {
    pub outbound_id: Uuid,
    pub mailbox_id: Uuid,
    pub recipients: String,
    pub subject: String,
    pub sent_at: Option<String>,
    pub due_at: String,
    pub state: FollowUpState,
    /// The first stored message that answers it, and when it was received.
    pub answer: Option<Uuid>,
    pub answered_at: Option<String>,
}

/// Which follow-ups `message followup list` keeps; the default keeps every one.
#[derive(Debug, Default, Clone)]
pub struct FollowUpFilter {
    /// Only the ones standing so: waiting, due (past their instant and
    /// unanswered) or answered.
    pub state: Option<FollowUpState>,
    /// Only those of messages this mailbox sent.
    pub mailbox_id: Option<Uuid>,
    /// Only those whose recipients contain this, case aside.
    pub to: Option<String>,
    /// Only those whose sent message's subject or body contains this, case aside.
    pub contains: Option<String>,
    /// Only those expecting an answer before this: an RFC 3339 instant, a
    /// `YYYY-MM-DD` day, or a span from now such as `+3d` or `-1d`.
    pub due_by: Option<String>,
}

impl AppState {
    /// Expect an answer to a sent outbound message by `by`: an RFC 3339
    /// instant, or a span from now such as `+3d` (m, h, d or w), replacing an
    /// earlier instant.
    pub fn set_follow_up(
        &self,
        organization_id: &str,
        outbound_id: Uuid,
        by: &str,
    ) -> Result<FollowUp, AppError> {
        let due = due_instant(by)?;
        let outbound = self.database.get_outbound(organization_id, outbound_id)?;
        if outbound.status != DeliveryStatus::Sent || outbound.provider_message_id.is_none() {
            return Err(AppError::invalid(
                "FOLLOW_UP_INVALID",
                format!(
                    "outbound message {outbound_id} is {}, not sent: nothing reached anyone to answer it",
                    outbound.status.as_str()
                ),
            ));
        }
        let due_at = instant(due);
        self.database
            .set_follow_up(organization_id, outbound_id, &due_at)?;
        self.follow_up(organization_id, outbound, due_at)
    }

    /// Where one sent message's follow-up stands now. An outbound message
    /// the organization does not hold is `NOT_FOUND`; one with no follow-up
    /// is `FOLLOW_UP_NOT_FOUND`.
    pub fn get_follow_up(&self, organization_id: &str, outbound_id: Uuid) -> Result<FollowUp, AppError> {
        let standing = self
            .follow_ups(organization_id, &FollowUpFilter::default())?
            .into_iter()
            .find(|follow_up| follow_up.outbound_id == outbound_id);
        match standing {
            Some(standing) => Ok(standing),
            None => {
                self.database.get_outbound(organization_id, outbound_id)?;
                Err(AppError::new(
                    StatusCode::NOT_FOUND,
                    "FOLLOW_UP_NOT_FOUND",
                    format!("outbound message {outbound_id} has no follow-up"),
                    false,
                ))
            }
        }
    }

    /// Stop expecting an answer, and answer the follow-up as it stood.
    pub fn clear_follow_up(&self, organization_id: &str, outbound_id: Uuid) -> Result<FollowUp, AppError> {
        let standing = self.get_follow_up(organization_id, outbound_id)?;
        self.database.clear_follow_up(organization_id, outbound_id)?;
        Ok(standing)
    }

    /// Stop expecting an answer to every follow-up `filter` lists, such as
    /// every answered one, and answer them as they stood. A filter naming
    /// no standing is `FOLLOW_UP_INVALID`: clearing every follow-up at once
    /// is never what a narrowing left out meant.
    pub fn clear_follow_ups(&self, organization_id: &str, filter: &FollowUpFilter) -> Result<Vec<FollowUp>, AppError> {
        if filter.state.is_none() {
            return Err(AppError::invalid(
                "FOLLOW_UP_INVALID",
                "name the standing to clear (waiting, due or answered), or clear one follow-up by its outbound ID",
            ));
        }
        let cleared = self.follow_ups(organization_id, filter)?;
        for follow_up in &cleared {
            self.database.clear_follow_up(organization_id, follow_up.outbound_id)?;
        }
        Ok(cleared)
    }

    /// Every follow-up, earliest due first, narrowed as `filter` says. A
    /// blank `to` or `contains`, or an unreadable `due_by`, is
    /// `MESSAGE_FILTER_INVALID`.
    pub fn follow_ups(&self, organization_id: &str, filter: &FollowUpFilter) -> Result<Vec<FollowUp>, AppError> {
        let to = super::messages::filter_value(filter.to.as_deref(), "recipient")?;
        let contains = super::messages::filter_value(filter.contains.as_deref(), "contains")?;
        let due_by = due_bound(filter.due_by.as_deref(), "due-by")?;
        let mut stored_by_mailbox: HashMap<Uuid, Vec<Message>> = HashMap::new();
        let mut follow_ups = Vec::new();
        for (outbound_id, due_at) in self.database.follow_ups(organization_id)? {
            if let Some(bound) = due_by {
                let due = DateTime::parse_from_rfc3339(&due_at).map_err(|error| {
                    AppError::internal(format!(
                        "follow-up of {outbound_id} stores the instant {due_at:?}, which is not RFC 3339: {error}"
                    ))
                })?;
                if due >= bound {
                    continue;
                }
            }
            let outbound = self.database.get_outbound(organization_id, outbound_id)?;
            if filter.mailbox_id.is_some_and(|mailbox_id| mailbox_id != outbound.mailbox_id) {
                continue;
            }
            if to.as_deref().is_some_and(|text| !outbound.recipients.to_lowercase().contains(text)) {
                continue;
            }
            let mentions = |text: &str| {
                outbound.subject.to_lowercase().contains(text) || outbound.body.to_lowercase().contains(text)
            };
            if contains.as_deref().is_some_and(|text| !mentions(text)) {
                continue;
            }
            if !stored_by_mailbox.contains_key(&outbound.mailbox_id) {
                let stored = self.database.list_messages(
                    organization_id,
                    Some(outbound.mailbox_id),
                    None,
                    None,
                )?;
                stored_by_mailbox.insert(outbound.mailbox_id, stored);
            }
            let stored = &stored_by_mailbox[&outbound.mailbox_id];
            let follow_up = standing(outbound, due_at, stored);
            if filter.state.is_none_or(|kept| follow_up.state == kept) {
                follow_ups.push(follow_up);
            }
        }
        Ok(follow_ups)
    }

    fn follow_up(
        &self,
        organization_id: &str,
        outbound: OutboundMessage,
        due_at: String,
    ) -> Result<FollowUp, AppError> {
        let stored =
            self.database
                .list_messages(organization_id, Some(outbound.mailbox_id), None, None)?;
        Ok(standing(outbound, due_at, &stored))
    }
}

/// When `by` expects an answer: an RFC 3339 instant, or a span from now such
/// as `+3d` (m, h, d or w). Anything else is `FOLLOW_UP_INVALID`.
pub(super) fn due_instant(by: &str) -> Result<DateTime<Utc>, AppError> {
    let written = by.trim();
    match written.strip_prefix('+') {
        Some(span) => {
            let delta = super::snoozes::span_from_now(span, '+', "FOLLOW_UP_INVALID")?;
            Utc::now().checked_add_signed(delta).ok_or_else(|| {
                AppError::invalid(
                    "FOLLOW_UP_INVALID",
                    format!("+{span} reaches past the last instant Skrzynka can store; expect an answer sooner"),
                )
            })
        }
        None => DateTime::parse_from_rfc3339(written)
            .map(|due| due.with_timezone(&Utc))
            .map_err(|error| {
                AppError::invalid(
                    "FOLLOW_UP_INVALID",
                    format!("{by:?} is neither an RFC 3339 instant ({error}) nor a span from now such as +3d"),
                )
            }),
    }
}

/// The bound a `--due-by` or `--send-by` filter (named `name`) keeps what is
/// due before: a span from now such as `+3d`, or anything a listing's
/// `--until` takes (an RFC 3339 instant, a `YYYY-MM-DD` day, a span back
/// such as `-1d`). Anything else is `MESSAGE_FILTER_INVALID`.
pub(in crate::service) fn due_bound(written: Option<&str>, name: &str) -> Result<Option<DateTime<Utc>>, AppError> {
    let Some(span) = written.map(str::trim).and_then(|value| value.strip_prefix('+')) else {
        return super::filters::instant(written, name);
    };
    let ahead = super::snoozes::span_from_now(span, '+', "MESSAGE_FILTER_INVALID")?;
    Utc::now().checked_add_signed(ahead).map(Some).ok_or_else(|| {
        AppError::invalid(
            "MESSAGE_FILTER_INVALID",
            format!("the {name} filter +{span} reaches past the last instant Skrzynka can name; look less far ahead"),
        )
    })
}

/// The follow-up's state against the mailbox's stored mail now.
fn standing(outbound: OutboundMessage, due_at: String, stored: &[Message]) -> FollowUp {
    let answer = outbound.provider_message_id.as_deref().and_then(|sent| {
        stored
            .iter()
            .filter(|message| message.bounce.is_none() && names(message, sent))
            .min_by(|left, right| left.received_at.cmp(&right.received_at))
    });
    let state = match answer {
        Some(_) => FollowUpState::Answered,
        None if due_at <= instant(Utc::now()) => FollowUpState::Due,
        None => FollowUpState::Waiting,
    };
    FollowUp {
        outbound_id: outbound.id,
        mailbox_id: outbound.mailbox_id,
        recipients: outbound.recipients,
        subject: outbound.subject,
        sent_at: outbound.sent_at,
        due_at,
        state,
        answer: answer.map(|message| message.id),
        answered_at: answer.map(|message| message.received_at.clone()),
    }
}

/// Whether the message's In-Reply-To or References names `sent`.
fn names(message: &Message, sent: &str) -> bool {
    [message.in_reply_to.as_deref(), message.references.as_deref()]
        .into_iter()
        .flatten()
        .flat_map(str::split_whitespace)
        .any(|named| named == sent)
}
