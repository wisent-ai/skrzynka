//! Muting a conversation: every Message-ID its stored messages carry or name
//! is kept as muted for the mailbox, and listings leave out each message that
//! carries or names one of them, so a later reply to the conversation stays
//! out of sight as it arrives. Nothing changes at the provider.

use super::{identities, AppState};
use crate::service::actions::RefusedAction;
use crate::{error::AppError, models::Message};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// What a mute or unmute changed.
#[derive(Debug, Serialize)]
pub struct MuteChange {
    pub message_id: Uuid,
    pub mailbox_id: Uuid,
    /// The stored messages of the conversation.
    pub messages: usize,
    /// The Message-IDs the conversation carries or names.
    pub identities: usize,
    /// How many of them this kept as muted, or stopped keeping.
    pub changed: u64,
}

/// What `message mute --view` or `message unmute --view` did: the view,
/// every conversation changed, and every one refused and why.
#[derive(Debug, Serialize)]
pub struct MutedView {
    pub view: String,
    pub conversations: Vec<MuteChange>,
    pub refused: Vec<RefusedAction>,
}

impl AppState {
    /// Mute the conversation message `id` belongs to. A conversation whose
    /// messages carry no Message-ID header could not be recognized again and
    /// is refused with `CONVERSATION_UNMUTABLE`.
    pub fn mute_conversation(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<MuteChange, AppError> {
        let (mailbox_id, messages, identities) =
            self.conversation_identities(organization_id, id)?;
        if identities.is_empty() {
            return Err(AppError::invalid(
                "CONVERSATION_UNMUTABLE",
                format!("message {id} and its conversation carry no Message-ID, In-Reply-To or References header, so later mail of it could not be recognized"),
            ));
        }
        let changed = self.database.mute_identities(mailbox_id, &identities)?;
        Ok(MuteChange {
            message_id: id,
            mailbox_id,
            messages,
            identities: identities.len(),
            changed,
        })
    }

    /// Show the conversation message `id` belongs to in listings again. One
    /// no Message-ID of which is muted is refused with
    /// `CONVERSATION_NOT_MUTED`.
    pub fn unmute_conversation(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<MuteChange, AppError> {
        let (mailbox_id, messages, identities) =
            self.conversation_identities(organization_id, id)?;
        let muted = self.database.muted_identities(organization_id)?;
        let held = muted
            .get(&mailbox_id)
            .is_some_and(|kept| identities.iter().any(|identity| kept.contains(identity)));
        if !held {
            return Err(AppError::conflict(
                "CONVERSATION_NOT_MUTED",
                format!("the conversation of message {id} is not muted"),
            ));
        }
        let changed = self.database.unmute_identities(mailbox_id, &identities)?;
        Ok(MuteChange {
            message_id: id,
            mailbox_id,
            messages,
            identities: identities.len(),
            changed,
        })
    }

    /// Mute, or with `mute: false` unmute, the conversation of every message
    /// the saved view `view` keeps, each conversation once: its filters with
    /// muted mail included, since a view's own listing leaves it out. A
    /// conversation already as asked is passed over; one that cannot be
    /// muted is named in `refused` with its code and sentence. An unknown view
    /// is `VIEW_NOT_FOUND`.
    pub fn mute_view(
        &self,
        organization_id: &str,
        view: &str,
        mute: bool,
    ) -> Result<MutedView, AppError> {
        let saved = self.view(organization_id, view)?;
        let filters = super::super::MessageFilters {
            include_muted: true,
            ..saved.filters.filters()
        };
        let muted = self.database.muted_identities(organization_id)?;
        let mut handled: HashSet<Uuid> = HashSet::new();
        let mut conversations = Vec::new();
        let mut refused = Vec::new();
        for message in self.list_messages(
            organization_id,
            saved.filters.mailbox_id,
            filters,
            None,
            None,
        )? {
            if handled.contains(&message.id) || is_muted(&message, &muted) == mute {
                continue;
            }
            handled.extend(
                self.message_thread(organization_id, message.id)?
                    .iter()
                    .map(|member| member.id),
            );
            let changed = match mute {
                true => self.mute_conversation(organization_id, message.id),
                false => self.unmute_conversation(organization_id, message.id),
            };
            match changed {
                Ok(change) => conversations.push(change),
                Err(refusal) => refused.push(RefusedAction {
                    message_id: message.id,
                    code: refusal.code,
                    message: refusal.message,
                }),
            }
        }
        Ok(MutedView {
            view: saved.name,
            conversations,
            refused,
        })
    }

    /// The mailbox of message `id`, how many stored messages its
    /// conversation holds, and every Message-ID they carry or name.
    fn conversation_identities(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<(Uuid, usize, Vec<String>), AppError> {
        let thread = self.message_thread(organization_id, id)?;
        let mailbox_id = self.database.get_message(organization_id, id)?.mailbox_id;
        let found: HashSet<String> = thread.iter().flat_map(identities).collect();
        Ok((mailbox_id, thread.len(), found.into_iter().collect()))
    }
}

/// Whether `message` carries or names a Message-ID muted in its mailbox.
pub(in crate::service) fn is_muted(
    message: &Message,
    muted: &HashMap<Uuid, HashSet<String>>,
) -> bool {
    muted
        .get(&message.mailbox_id)
        .is_some_and(|kept| identities(message).any(|identity| kept.contains(&identity)))
}
