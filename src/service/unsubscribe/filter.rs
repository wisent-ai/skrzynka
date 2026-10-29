//! Which mailing lists `message subscriptions` keeps, checked once before
//! any stored mail is read and then applied to the lists it found.

use crate::{
    error::AppError,
    mail::{Subscription, UnsubscribeTarget},
    models::DeliveryStatus,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Which mailing lists `message subscriptions` keeps; the default keeps every one.
#[derive(Debug, Default, Clone)]
pub struct SubscriptionFilter {
    /// Only this mailbox's lists.
    pub mailbox_id: Option<Uuid>,
    /// Only the lists no request to leave has been sent to (none ended `sent`).
    pub subscribed: bool,
    /// Only the lists whose List-Id or sender contains this, case aside.
    pub contains: Option<String>,
    /// Only the lists whose newest stored message arrived at or after this
    /// instant, `YYYY-MM-DD` day or span back such as `-30d`: lists still writing.
    pub since: Option<String>,
    /// Only the lists whose newest stored message arrived before this: lists
    /// that have gone quiet.
    pub until: Option<String>,
    /// Only the lists `message unsubscribe` can leave itself: those naming a
    /// one-click or mailto way out, not only pages a person opens.
    pub leavable: bool,
}

/// A [`SubscriptionFilter`] whose text and window were read.
pub(super) struct Narrowing {
    subscribed: bool,
    leavable: bool,
    contains: Option<String>,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
}

impl SubscriptionFilter {
    /// Read the text and the window: a blank text, an unreadable instant or
    /// a window whose start is not before its end is `MESSAGE_FILTER_INVALID`.
    pub(super) fn checked(&self) -> Result<Narrowing, AppError> {
        let contains = super::super::messages::filter_value(self.contains.as_deref(), "contains")?;
        let since = super::super::filters::instant(self.since.as_deref(), "since")?;
        let until = super::super::filters::instant(self.until.as_deref(), "until")?;
        if let (Some(from), Some(to)) = (since, until) {
            if from >= to {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("since {from} is not before until {to}; no list's newest message could fall between them"),
                ));
            }
        }
        Ok(Narrowing {
            subscribed: self.subscribed,
            leavable: self.leavable,
            contains,
            since,
            until,
        })
    }
}

impl Narrowing {
    /// Keep the lists this narrowing admits, in their order.
    pub(super) fn narrow(&self, lists: Vec<Subscription>) -> Result<Vec<Subscription>, AppError> {
        let mut kept = Vec::new();
        for list in lists {
            if self.subscribed
                && list
                    .last_attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.status == DeliveryStatus::Sent)
            {
                continue;
            }
            let reachable = list
                .ways_out
                .iter()
                .any(|way| !matches!(way, UnsubscribeTarget::Page { .. }));
            if self.leavable && !reachable {
                continue;
            }
            if let Some(part) = self.contains.as_deref() {
                let named = list.sender.to_lowercase().contains(part)
                    || list
                        .list_id
                        .as_deref()
                        .is_some_and(|list_id| list_id.to_lowercase().contains(part));
                if !named {
                    continue;
                }
            }
            if self.since.is_some() || self.until.is_some() {
                let latest =
                    DateTime::parse_from_rfc3339(&list.latest_received_at).map_err(|error| {
                        AppError::internal(format!(
                        "message {} stores receive instant {:?}, which is not RFC 3339: {error}",
                        list.latest_message_id, list.latest_received_at
                    ))
                    })?;
                let inside = self.since.is_none_or(|from| latest >= from)
                    && self.until.is_none_or(|to| latest < to);
                if !inside {
                    continue;
                }
            }
            kept.push(list);
        }
        Ok(kept)
    }
}
