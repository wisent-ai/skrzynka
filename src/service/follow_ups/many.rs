//! Expecting answers to many sent messages at once: every message an
//! outbound listing keeps, such as a mail merge sent from scheduled drafts,
//! which carry no follow-up of their own.

use super::{super::AppState, due_instant, instant, standing, FollowUp};
use crate::{
    error::AppError,
    models::{DeliveryStatus, Message},
    service::OutboundFilters,
};
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

/// What `message outbound --follow-up` did: the follow-ups set, and the
/// messages listed that nothing reached, so none was set for them.
#[derive(Debug, Serialize)]
pub struct FollowUpsSet {
    pub follow_ups: Vec<FollowUp>,
    pub skipped: Vec<SkippedOutbound>,
}

/// One message listed that did not end `sent` with a Message-ID an answer
/// could name, and how it ended.
#[derive(Debug, Serialize)]
pub struct SkippedOutbound {
    pub outbound_id: Uuid,
    pub status: String,
}

impl AppState {
    /// Expect an answer by `by` (an RFC 3339 instant, or a span from now
    /// such as `+3d`) to every message the outbound listing `filters` keeps
    /// from `mailbox_id`, paged by `limit` and `offset`, replacing an earlier
    /// instant; a message that did not end `sent` is skipped and named. An
    /// unreadable `by` is `FOLLOW_UP_INVALID` before anything is read; a
    /// listing narrowed by no mailbox, addressee, text, file name, window or
    /// bounce is `FOLLOW_UP_INVALID`, so nothing expects an answer to all
    /// sent mail at once; one listing nothing is `NOT_FOUND`.
    pub fn expect_answers(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        filters: &OutboundFilters,
        limit: Option<u32>,
        offset: Option<u32>,
        by: &str,
    ) -> Result<FollowUpsSet, AppError> {
        let due_at = instant(due_instant(by)?);
        let narrowed = [
            &filters.to,
            &filters.contains,
            &filters.attachment,
            &filters.since,
            &filters.until,
        ]
        .iter()
        .any(|filter| filter.is_some());
        if mailbox_id.is_none() && !narrowed && !filters.bounced {
            return Err(AppError::invalid(
                "FOLLOW_UP_INVALID",
                "narrow the sent mail by mailbox, addressee, text, file name, window or bounce first; nothing expects an answer to every message sent at once",
            ));
        }
        let listed = self.search_outbound(organization_id, mailbox_id, filters, limit, offset)?;
        if listed.is_empty() {
            return Err(AppError::not_found("outbound message"));
        }
        let mut stored_by_mailbox: HashMap<Uuid, Vec<Message>> = HashMap::new();
        let mut set = FollowUpsSet {
            follow_ups: Vec::new(),
            skipped: Vec::new(),
        };
        for outbound in listed {
            if outbound.status != DeliveryStatus::Sent || outbound.provider_message_id.is_none() {
                set.skipped.push(SkippedOutbound {
                    outbound_id: outbound.id,
                    status: outbound.status.as_str().to_string(),
                });
                continue;
            }
            self.database
                .set_follow_up(organization_id, outbound.id, &due_at)?;
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
            set.follow_ups
                .push(standing(outbound, due_at.clone(), stored));
        }
        Ok(set)
    }
}
