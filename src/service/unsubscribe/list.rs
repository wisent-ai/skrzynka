//! Leaving a mailing list by its List-Id: the newest stored message of that
//! list carries the way out `message unsubscribe` uses.

use super::{super::AppState, SubscriptionFilter};
use crate::{
    error::AppError,
    mail::{UnsubscribeAttempt, UnsubscribeRequest},
};
use uuid::Uuid;

impl AppState {
    /// Ask the list whose List-Id is `list_id` (case aside) to stop sending,
    /// through its newest stored message, as `unsubscribe` of that message
    /// does; with `mailbox_id`, the list as that mailbox receives it. A blank
    /// List-Id is `UNSUBSCRIBE_LIST_INVALID`; a List-Id no stored mail names
    /// is `UNSUBSCRIBE_LIST_NOT_FOUND`; one several mailboxes receive, without
    /// `mailbox_id`, is `UNSUBSCRIBE_LIST_AMBIGUOUS` naming them.
    pub async fn unsubscribe_list(
        &self,
        organization_id: &str,
        list_id: &str,
        mailbox_id: Option<Uuid>,
        request: UnsubscribeRequest,
    ) -> Result<UnsubscribeAttempt, AppError> {
        let wanted = list_id.trim().to_ascii_lowercase();
        if wanted.is_empty() {
            return Err(AppError::invalid(
                "UNSUBSCRIBE_LIST_INVALID",
                "the List-Id is blank; name it as message subscriptions prints it",
            ));
        }
        let filter = SubscriptionFilter { mailbox_id, ..SubscriptionFilter::default() };
        let named: Vec<_> = self
            .list_subscriptions(organization_id, &filter)?
            .lists
            .into_iter()
            .filter(|list| list.list_id.as_deref().is_some_and(|id| id.trim().to_ascii_lowercase() == wanted))
            .collect();
        let message_id = match named.as_slice() {
            [] => {
                return Err(AppError::new(
                    axum::http::StatusCode::NOT_FOUND,
                    "UNSUBSCRIBE_LIST_NOT_FOUND",
                    format!("no stored message names the List-Id {list_id:?}; message subscriptions lists the lists there are"),
                    false,
                ))
            }
            [only] => only.latest_message_id,
            several => {
                let mailboxes: Vec<String> = several.iter().map(|list| list.mailbox_id.to_string()).collect();
                return Err(AppError::new(
                    axum::http::StatusCode::CONFLICT,
                    "UNSUBSCRIBE_LIST_AMBIGUOUS",
                    format!(
                        "the List-Id {list_id:?} reaches mailboxes {}; name one with --mailbox",
                        mailboxes.join(", ")
                    ),
                    false,
                ))
            }
        };
        self.unsubscribe(organization_id, message_id, request).await
    }
}
