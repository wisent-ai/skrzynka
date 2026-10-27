//! One request to leave a mailing list, as Skrzynka records it: claimed by
//! its idempotency key before the list is asked, and carried through the same
//! delivery states as a reply.

use crate::models::DeliveryStatus;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How the list was asked.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnsubscribeMethod {
    /// RFC 8058: a POST of `List-Unsubscribe=One-Click` over HTTPS.
    OneClick,
    /// RFC 2369 mailto: mail from the mailbox that received the message.
    Mailto,
}

impl UnsubscribeMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneClick => "one_click",
            Self::Mailto => "mailto",
        }
    }

    /// The stored spelling read back; another value is refused with it.
    pub fn parse(value: &str) -> Result<Self, String> {
        [Self::OneClick, Self::Mailto]
            .into_iter()
            .find(|method| method.as_str() == value)
            .ok_or_else(|| format!("{value:?} is not an unsubscribe method"))
    }
}

/// One request to leave the list a stored message came from. `target` is the
/// address asked: the HTTPS URL, or the mail address. `provider_message_id` is
/// the Message-ID of the unsubscribe mail; a one-click request has none.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeAttempt {
    pub id: Uuid,
    pub message_id: Uuid,
    pub idempotency_key: String,
    pub method: UnsubscribeMethod,
    pub target: String,
    pub status: DeliveryStatus,
    pub provider_message_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

/// What `message unsubscribe` and `POST /v1/messages/:id/unsubscribe` take.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnsubscribeRequest {
    pub idempotency_key: String,
}

/// One mailing list the organization's stored mail came from: its List-Id
/// (or, for a list that states none, its sender), how many stored messages it
/// sent, the newest of them, the ways out that message names, and the last
/// request to leave it.
#[derive(Debug, Clone, Serialize)]
pub struct Subscription {
    pub list_id: Option<String>,
    pub sender: String,
    pub mailbox_id: Uuid,
    pub messages: usize,
    pub latest_message_id: Uuid,
    pub latest_received_at: String,
    pub ways_out: Vec<super::UnsubscribeTarget>,
    pub ways_out_unreadable: Option<String>,
    pub last_attempt: Option<UnsubscribeAttempt>,
}

/// What `message subscriptions` answers: every list, most messages first, and
/// how many stored messages were imported before list headers were recorded
/// and so could not be placed under a list.
#[derive(Debug, Clone, Serialize)]
pub struct Subscriptions {
    pub lists: Vec<Subscription>,
    pub messages_not_read: usize,
}
