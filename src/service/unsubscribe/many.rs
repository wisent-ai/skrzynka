//! Leaving many mailing lists at once: every list a narrowed `message
//! subscriptions` listing keeps, such as those gone quiet for months, each
//! asked as `message unsubscribe` asks one, through its newest stored message.

use super::{super::{outbound::checked_key, AppState}, SubscriptionFilter};
use crate::{
    error::AppError,
    mail::{UnsubscribeAttempt, UnsubscribeRequest},
};
use serde::Serialize;
use uuid::Uuid;

/// What leaving the lists a listing keeps did.
#[derive(Debug, Serialize)]
pub struct LeftLists {
    /// The attempt of every list asked, in the listing's order.
    pub left: Vec<UnsubscribeAttempt>,
    /// Every list that could not be asked, and why.
    pub refused: Vec<RefusedList>,
}

/// One list not asked to stop sending: its List-Id (or sender), the mailbox
/// receiving it, and the refusal.
#[derive(Debug, Serialize)]
pub struct RefusedList {
    pub list_id: Option<String>,
    pub sender: String,
    pub mailbox_id: Uuid,
    pub code: &'static str,
    pub message: String,
}

impl AppState {
    /// Ask every list `filter` keeps to stop sending, each through its newest
    /// stored message as `unsubscribe` asks one, claimed by
    /// `<idempotency_key>-<that message's id>`, so the same request again asks
    /// no list twice while no newer message arrived. A key that is blank or
    /// holds whitespace, or a filter narrowed by no text or window, is
    /// refused before any list is asked; one keeping no list is
    /// `UNSUBSCRIBE_LIST_NOT_FOUND`. A list that cannot be left from here
    /// (only web pages, an unreachable way out) is named in `refused` and
    /// the others are still asked.
    pub async fn unsubscribe_listed(&self, organization_id: &str, filter: &SubscriptionFilter, idempotency_key: &str) -> Result<LeftLists, AppError> {
        let key = checked_key(idempotency_key)?.to_string();
        if filter.contains.is_none() && filter.since.is_none() && filter.until.is_none() {
            return Err(AppError::invalid(
                "UNSUBSCRIBE_LIST_INVALID",
                "name the lists to leave by a text they contain or a window of their newest message; nothing leaves every list at once",
            ));
        }
        let lists = self.list_subscriptions(organization_id, filter)?.lists;
        if lists.is_empty() {
            return Err(AppError::new(
                axum::http::StatusCode::NOT_FOUND,
                "UNSUBSCRIBE_LIST_NOT_FOUND",
                "no mailing list the stored mail came from matches; nothing was asked",
                false,
            ));
        }
        let mut outcome = LeftLists { left: Vec::new(), refused: Vec::new() };
        for list in lists {
            let request = UnsubscribeRequest { idempotency_key: format!("{key}-{}", list.latest_message_id) };
            match self.unsubscribe(organization_id, list.latest_message_id, request).await {
                Ok(attempt) => outcome.left.push(attempt),
                Err(error) => outcome.refused.push(RefusedList {
                    list_id: list.list_id,
                    sender: list.sender,
                    mailbox_id: list.mailbox_id,
                    code: error.code,
                    message: error.message,
                }),
            }
        }
        Ok(outcome)
    }
}
