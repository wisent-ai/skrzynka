//! What a message listing keeps: a recipient address, a sender text, a text
//! in the subject or body, and the instants it was received between. Every
//! filter is the caller's; a blank one or an instant that cannot be read is
//! refused, never read as no filter.

use super::messages::{addressed_to, filter_value};
use crate::{error::AppError, models::Message};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};

mod mailboxes;
mod outbound;
mod views;
pub use mailboxes::MailboxState;
pub use outbound::OutboundFilters;
pub use views::{CountedView, SavedFilters, SavedView};

/// The filters a listing was given, as written.
#[derive(Debug, Default, Clone, Copy)]
pub struct MessageFilters<'a> {
    /// Keep messages whose To header names this address whole, case aside.
    pub recipient: Option<&'a str>,
    /// Keep messages whose Cc header names this address whole, case aside.
    pub cc: Option<&'a str>,
    /// Keep messages whose From header contains this, case aside.
    pub sender: Option<&'a str>,
    /// Keep messages whose subject or stored text contains this, case aside.
    pub text: Option<&'a str>,
    /// Keep messages received at or after this instant (RFC 3339) or the
    /// start of this UTC day (`YYYY-MM-DD`).
    pub since: Option<&'a str>,
    /// Keep messages received before this instant or the start of this UTC day.
    pub until: Option<&'a str>,
    /// Keep messages carrying this tag, case aside.
    pub tag: Option<&'a str>,
    /// Keep messages from the mailing list whose List-Id contains this, case aside.
    pub list: Option<&'a str>,
    /// Keep messages carrying an attachment whose file name contains this, case aside.
    pub attachment: Option<&'a str>,
    /// Keep only messages the import recorded at least one attachment of.
    pub with_attachments: bool,
    /// Keep only bounce reports (delivery status notifications).
    pub bounces: bool,
    /// Keep the mail of blocked senders too; without it, listings leave it out.
    pub include_blocked: bool,
    /// Keep snoozed messages too; without it, listings leave them out until they wake.
    pub include_snoozed: bool,
    /// Keep the mail of muted conversations too; without it, listings leave it out.
    pub include_muted: bool,
    /// Keep only messages no reply to has been sent for (a reply that ended `sent`).
    pub unanswered: bool,
    /// Keep only messages whose receiving provider recorded a verdict other than `pass`.
    pub failed_authentication: bool,
    /// List the messages kept oldest first instead of newest first.
    pub oldest_first: bool,
    /// Keep only messages snoozed until an instant still ahead.
    pub snoozed: bool,
    /// Keep only messages carrying your note.
    pub noted: bool,
    /// Keep only the mail of muted conversations.
    pub muted: bool,
    /// Keep only messages carrying no tag.
    pub untagged: bool,
    /// Keep only messages a reply to has been sent for (one ended `sent`).
    pub answered: bool,
    /// Keep only messages whose recorded files come to at least this many bytes.
    pub files_over: Option<u64>,
}

/// The filters read and validated.
pub(super) struct Matcher {
    recipient: Option<String>,
    cc: Option<String>,
    sender: Option<String>,
    text: Option<String>,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    tag: Option<String>,
    list: Option<String>,
    attachment: Option<String>,
    with_attachments: bool,
    bounces: bool,
    failed_authentication: bool,
    noted: bool,
    untagged: bool,
    files_over: Option<u64>,
}

impl MessageFilters<'_> {
    /// The filters to apply, or `None` when the caller gave none.
    pub(super) fn matcher(&self) -> Result<Option<Matcher>, AppError> {
        let matcher = Matcher {
            recipient: filter_value(self.recipient, "recipient")?,
            cc: filter_value(self.cc, "cc")?,
            sender: filter_value(self.sender, "sender")?,
            text: filter_value(self.text, "text")?,
            since: instant(self.since, "since")?,
            until: instant(self.until, "until")?,
            tag: filter_value(self.tag, "tag")?,
            list: filter_value(self.list, "list")?,
            attachment: filter_value(self.attachment, "attachment")?,
            with_attachments: self.with_attachments,
            bounces: self.bounces,
            failed_authentication: self.failed_authentication,
            noted: self.noted,
            untagged: self.untagged,
            files_over: self.files_over,
        };
        if let (Some(since), Some(until)) = (matcher.since, matcher.until) {
            if since >= until {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!(
                        "since {since} is not before until {until}; no message could be received between them"
                    ),
                ));
            }
        }
        if let (Some(tag), true) = (&matcher.tag, matcher.untagged) {
            return Err(AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                format!(
                    "the tag filter {tag:?} and untagged exclude each other; give one or the other"
                ),
            ));
        }
        if self.answered && self.unanswered {
            return Err(AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                "answered and unanswered exclude each other; give one or the other",
            ));
        }
        let given = matcher.recipient.is_some()
            || matcher.cc.is_some()
            || matcher.sender.is_some()
            || matcher.text.is_some()
            || matcher.since.is_some()
            || matcher.until.is_some()
            || matcher.tag.is_some()
            || matcher.list.is_some()
            || matcher.attachment.is_some()
            || matcher.with_attachments
            || matcher.bounces
            || matcher.failed_authentication
            || matcher.noted
            || matcher.untagged
            || matcher.files_over.is_some();
        Ok(given.then_some(matcher))
    }
}

impl Matcher {
    /// Whether `message` passes every filter. A stored receive instant that
    /// does not read as RFC 3339 is refused with the message it belongs to.
    pub(super) fn keeps(&self, message: &Message) -> Result<bool, AppError> {
        // A message that states no To is addressed to no one.
        let addressed = match (&self.recipient, message.recipients.as_deref()) {
            (Some(address), Some(header)) => addressed_to(header, address),
            (Some(_), None) => false,
            (None, _) => true,
        };
        // A message that states no Cc, or was imported before Cc was
        // recorded, copies no one.
        let copied = match (&self.cc, message.cc.as_deref()) {
            (Some(address), Some(header)) => addressed_to(header, address),
            (Some(_), None) => false,
            (None, _) => true,
        };
        let from = match &self.sender {
            Some(part) => message.sender.to_lowercase().contains(part.as_str()),
            None => true,
        };
        // The text is looked for in the subject, the stored text, and the
        // operator's own note, each case aside.
        let mentions = match &self.text {
            Some(part) => [
                Some(message.body_text.as_str()),
                message.subject.as_deref(),
                message.note.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|field| field.to_lowercase().contains(part.as_str())),
            None => true,
        };
        let tagged = match &self.tag {
            Some(tag) => message.tags.contains(tag),
            None => true,
        };
        let listed = match &self.list {
            Some(part) => message
                .mailing_list
                .as_ref()
                .and_then(|list| list.list_id.as_deref())
                .is_some_and(|list_id| list_id.to_lowercase().contains(part.as_str())),
            None => true,
        };
        let attached = !self.with_attachments || carries_files(message);
        // A file is found by its recorded name; one that states none is not.
        let named = match &self.attachment {
            Some(part) => message.attachments.as_ref().is_some_and(|attachments| {
                attachments.iter().any(|attachment| {
                    attachment
                        .filename
                        .as_deref()
                        .is_some_and(|name| name.to_lowercase().contains(part.as_str()))
                })
            }),
            None => true,
        };
        let bounce = !self.bounces || message.bounce.is_some();
        let doubted = !self.failed_authentication || doubts_sender(message);
        let annotated = !self.noted || message.note.is_some();
        let bare = !self.untagged || message.tags.is_empty();
        // Files whose size was not recorded weigh nothing here.
        let heavy = self.files_over.is_none_or(|least| {
            let sizes = message
                .attachments
                .iter()
                .flatten()
                .filter_map(|file| file.size_bytes);
            sizes.sum::<u64>() >= least
        });
        let headers = addressed && copied && from && mentions;
        if !(headers
            && tagged
            && listed
            && attached
            && named
            && bounce
            && doubted
            && annotated
            && bare
            && heavy)
        {
            return Ok(false);
        }
        if self.since.is_none() && self.until.is_none() {
            return Ok(true);
        }
        let received = received_instant(message)?;
        Ok(self.since.is_none_or(|since| received >= since)
            && self.until.is_none_or(|until| received < until))
    }
}

/// Whether the import recorded at least one file of `message`. A message
/// imported before attachments were recorded is not known to carry any.
pub(crate) fn carries_files(message: &Message) -> bool {
    message
        .attachments
        .as_ref()
        .is_some_and(|attachments| !attachments.is_empty())
}

/// Whether the receiving provider wrote a verdict on the sender as
/// `method=result` whose result is not `pass`. An unread clause, and a
/// message with no recorded verdicts, say nothing either way.
pub(crate) fn doubts_sender(message: &Message) -> bool {
    message.authentication.as_ref().is_some_and(|verdicts| {
        verdicts.iter().any(|verdict| {
            !verdict.result.is_empty() && !verdict.result.eq_ignore_ascii_case("pass")
        })
    })
}

/// The instant `message` was received, read from its stored RFC 3339 text.
pub(crate) fn received_instant(message: &Message) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(&message.received_at)
        .map_err(|error| {
            AppError::internal(format!(
                "message {} stores receive instant {:?}, which is not RFC 3339: {error}",
                message.id, message.received_at
            ))
        })
        .map(|instant| instant.with_timezone(&Utc))
}

/// An instant filter: RFC 3339, a `YYYY-MM-DD` day read as its start in
/// UTC, or a span back from now such as `-7d` (m, h, d or w), read when the
/// filter is, so a saved view holding one keeps rolling. A value that is
/// none of them is refused with every reason.
pub(crate) fn instant(value: Option<&str>, name: &str) -> Result<Option<DateTime<Utc>>, AppError> {
    let Some(value) = value.map(str::trim) else {
        return Ok(None);
    };
    if let Some(span) = value.strip_prefix('-') {
        let back = super::snoozes::span_from_now(span, '-', "MESSAGE_FILTER_INVALID")?;
        return Utc::now().checked_sub_signed(back).map(Some).ok_or_else(|| {
            AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                format!("the {name} filter {value:?} reaches before the first instant Skrzynka can name; go back less"),
            )
        });
    }
    match DateTime::parse_from_rfc3339(value) {
        Ok(instant) => Ok(Some(instant.with_timezone(&Utc))),
        Err(as_instant) => match NaiveDate::parse_from_str(value, "%Y-%m-%d") {
            Ok(day) => Ok(Some(day.and_time(NaiveTime::MIN).and_utc())),
            Err(as_day) => Err(AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                format!(
                    "the {name} filter {value:?} is neither an RFC 3339 instant ({as_instant}), a YYYY-MM-DD day ({as_day}) nor a span back from now such as -7d"
                ),
            )),
        },
    }
}
