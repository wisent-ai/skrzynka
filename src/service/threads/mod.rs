//! One conversation: every stored message of a mailbox that the headers tie
//! to a given one. Two messages belong together when one names the other's
//! Message-ID in its In-Reply-To or References header, or both name a common
//! one; the tie is followed until no message adds a new Message-ID.

use super::AppState;
use crate::{error::AppError, models::Message};
use std::collections::HashSet;
use uuid::Uuid;

mod answers;
pub(super) mod mutes;
pub use answers::ReplyViewRequest;

impl AppState {
    /// The conversation message `id` belongs to, oldest first by when each
    /// was received, the message itself included. A message whose headers
    /// name no other message is a conversation of one.
    pub fn message_thread(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<Vec<Message>, AppError> {
        let message = self.database.get_message(organization_id, id)?;
        let stored =
            self.database
                .list_messages(organization_id, Some(message.mailbox_id), None, None)?;
        let mut known: HashSet<String> = identities(&message).collect();
        let mut members: HashSet<Uuid> = HashSet::from([message.id]);
        loop {
            let before = members.len();
            for candidate in &stored {
                if members.contains(&candidate.id) {
                    continue;
                }
                if identities(candidate).any(|identity| known.contains(&identity)) {
                    members.insert(candidate.id);
                    known.extend(identities(candidate));
                }
            }
            if members.len() == before {
                break;
            }
        }
        let mut thread: Vec<Message> = stored
            .into_iter()
            .filter(|candidate| members.contains(&candidate.id))
            .collect();
        thread.sort_by(|left, right| left.received_at.cmp(&right.received_at));
        Ok(thread)
    }
}

/// Every Message-ID a message carries or names: its own, its In-Reply-To
/// and each one its References header lists, each as written between angle
/// brackets.
fn identities(message: &Message) -> impl Iterator<Item = String> + '_ {
    [
        message.message_id.as_deref(),
        message.in_reply_to.as_deref(),
        message.references.as_deref(),
    ]
    .into_iter()
    .flatten()
    .flat_map(str::split_whitespace)
    .map(str::to_string)
}
