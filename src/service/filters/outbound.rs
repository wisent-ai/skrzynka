//! Finding sent mail: the outbound messages addressed to someone, that
//! mention a text, that carried a file by name, that ended in one delivery
//! status, that were claimed within a window, or that a bounce report returned.

use super::{
    super::messages::{filter_value, page_size},
    instant,
};
use crate::{
    error::AppError,
    models::{DeliveryStatus, OutboundMessage},
    service::AppState,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};
use uuid::Uuid;

/// What an outbound listing keeps, each as written; an absent one is no filter.
#[derive(Debug, Default, Clone)]
pub struct OutboundFilters {
    /// Keep messages whose To, Cc or Bcc contains this, case aside.
    pub to: Option<String>,
    /// Keep messages whose subject or body contains this, case aside.
    pub contains: Option<String>,
    /// Keep messages in this delivery status: `pending`, `sending`, `sent`,
    /// `failed` or `uncertain`.
    pub status: Option<String>,
    /// Keep messages that carried a file whose name contains this, case aside.
    pub attachment: Option<String>,
    /// Keep messages claimed at or after this RFC 3339 instant or the start
    /// of this UTC day.
    pub since: Option<String>,
    /// Keep messages claimed before this instant or the start of this UTC day.
    pub until: Option<String>,
    /// Keep only messages a stored bounce report (RFC 3464), in any mailbox
    /// of the organization, returned: its returned Message-ID is the one the
    /// message went out with.
    pub bounced: bool,
    /// Keep only messages a stored message of the sending mailbox answers
    /// (`Some(true)`), or only those nothing answers yet (`Some(false)`): its
    /// In-Reply-To or References names the Message-ID the message went out
    /// with, as a follow-up reads an answer. A bounce report is no answer.
    pub answered: Option<bool>,
    /// List the oldest claimed first; the page is cut in that order.
    pub oldest_first: bool,
}

impl AppState {
    /// Outbound messages newest first, or oldest first with `oldest_first`,
    /// narrowed by `filters`; the page is cut
    /// after them. With none, the database pages directly. A blank filter or
    /// an unknown status, an unreadable instant or a backwards window is
    /// `MESSAGE_FILTER_INVALID`.
    pub fn search_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        filters: &OutboundFilters,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<OutboundMessage>, AppError> {
        page_size(limit)?;
        let to = filter_value(filters.to.as_deref(), "to")?;
        let contains = filter_value(filters.contains.as_deref(), "contains")?;
        let status = filter_value(filters.status.as_deref(), "status")?
            .map(|written| {
                DeliveryStatus::from_str(&written).map_err(|error| {
                    AppError::invalid(
                        "MESSAGE_FILTER_INVALID",
                        format!(
                            "the status filter is not a delivery status ({error}); name pending, sending, sent, failed or uncertain"
                        ),
                    )
                })
            })
            .transpose()?;
        let attachment = filter_value(filters.attachment.as_deref(), "attachment")?;
        let since = instant(filters.since.as_deref(), "since")?;
        let until = instant(filters.until.as_deref(), "until")?;
        if let (Some(from), Some(to)) = (since, until) {
            if from >= to {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("since {from} is not before until {to}; nothing could be claimed between them"),
                ));
            }
        }
        let windowed = since.is_some() || until.is_some();
        let given = to.is_some()
            || contains.is_some()
            || status.is_some()
            || attachment.is_some()
            || windowed;
        if !given && !filters.bounced && filters.answered.is_none() && !filters.oldest_first {
            return self
                .database
                .list_outbound(organization_id, mailbox_id, limit, offset);
        }
        // The stored mail is read once for the listing when a filter needs it:
        // the Message-IDs the stored reports returned, and those each
        // mailbox's stored mail answers.
        let stored = match filters.bounced || filters.answered.is_some() {
            true => self
                .database
                .list_messages(organization_id, None, None, None)?,
            false => Vec::new(),
        };
        let returned: BTreeSet<&str> = stored
            .iter()
            .filter_map(|report| report.bounce.as_ref()?.original_message_id.as_deref())
            .collect();
        // Each mailbox's answered Message-IDs: what its stored mail names.
        let mut answers: BTreeMap<Uuid, BTreeSet<&str>> = BTreeMap::new();
        for message in stored.iter().filter(|message| message.bounce.is_none()) {
            let named = [
                message.in_reply_to.as_deref(),
                message.references.as_deref(),
            ]
            .into_iter()
            .flatten()
            .flat_map(str::split_whitespace);
            answers.entry(message.mailbox_id).or_default().extend(named);
        }
        let mut kept: Vec<OutboundMessage> = Vec::new();
        for message in self
            .database
            .list_outbound(organization_id, mailbox_id, None, None)?
        {
            let addressed = to.as_deref().is_none_or(|part| {
                [
                    Some(message.recipients.as_str()),
                    message.cc.as_deref(),
                    message.bcc.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|header| header.to_lowercase().contains(part))
            });
            let mentions = contains.as_deref().is_none_or(|part| {
                message.subject.to_lowercase().contains(part)
                    || message.body.to_lowercase().contains(part)
            });
            let ended = status.is_none_or(|status| message.status == status);
            let carried = attachment.as_deref().is_none_or(|part| {
                message
                    .attachments
                    .iter()
                    .any(|file| file.filename.to_lowercase().contains(part))
            });
            let inside = match windowed {
                true => {
                    let claimed = claimed_instant(&message)?;
                    since.is_none_or(|from| claimed >= from) && until.is_none_or(|to| claimed < to)
                }
                false => true,
            };
            let sent_as = message.provider_message_id.as_deref();
            let bounced = !filters.bounced || sent_as.is_some_and(|sent| returned.contains(sent));
            let answered = filters.answered.is_none_or(|wanted| {
                sent_as.is_some_and(|sent| {
                    answers
                        .get(&message.mailbox_id)
                        .is_some_and(|named| named.contains(sent))
                }) == wanted
            });
            if addressed && mentions && ended && carried && inside && bounced && answered {
                kept.push(message);
            }
        }
        if filters.oldest_first {
            kept.reverse();
        }
        let after_offset: Vec<OutboundMessage> = match offset {
            Some(offset) => kept.into_iter().skip(offset as usize).collect(),
            None => kept,
        };
        Ok(match limit {
            Some(limit) => after_offset.into_iter().take(limit as usize).collect(),
            None => after_offset,
        })
    }
}

/// When the row was claimed, as stored; a stored value that is not RFC 3339
/// is the database's fault and named as such.
fn claimed_instant(message: &OutboundMessage) -> Result<chrono::DateTime<chrono::Utc>, AppError> {
    chrono::DateTime::parse_from_rfc3339(&message.created_at)
        .map(|claimed| claimed.with_timezone(&chrono::Utc))
        .map_err(|error| {
            AppError::internal(format!(
                "outbound message {} stores claim instant {:?}, which is not RFC 3339: {error}",
                message.id, message.created_at
            ))
        })
}
