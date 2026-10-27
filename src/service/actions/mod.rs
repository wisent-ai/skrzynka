//! Changes an operator asks of a stored message at its provider: mark it
//! seen, unseen, flagged or unflagged, move it to another folder, or delete it. Each request is
//! claimed as a pending record before the provider is asked, so a request
//! whose outcome is lost still leaves a row saying so.

use super::AppState;
use crate::{
    error::AppError,
    mail::{self, ActionStatus, MessageAction, ProviderAction},
};
use uuid::Uuid;

mod append;
mod folders;
mod views;
pub(in crate::service) use views::RefusedAction;

impl AppState {
    /// Apply `action` to the provider's copy of one stored message. A done or
    /// uncertain action is answered with its record; a refused one is the
    /// error, recorded as failed.
    pub async fn act_on_message(
        &self,
        organization_id: &str,
        message_id: Uuid,
        action: ProviderAction,
    ) -> Result<MessageAction, AppError> {
        let action = validated(action)?;
        let claimed = self
            .database
            .begin_message_action(organization_id, message_id, &action)?;
        let fail = |error: AppError| {
            let recorded = self.database.finish_message_action(
                organization_id,
                claimed.id,
                ActionStatus::Failed,
                Some(error.code),
                Some(&error.message),
            );
            error.with_unrecorded(recorded, "pending")
        };
        let source = self
            .database
            .get_message(organization_id, message_id)
            .and_then(|message| {
                self.database
                    .get_mailbox(organization_id, message.mailbox_id)
                    .map(|mailbox| (message, mailbox))
            });
        let (message, mailbox) = source.map_err(fail)?;
        let credentials = self
            .resolver
            .resolve_credentials(&mailbox.skarbiec_item_id)
            .await
            .map_err(fail)?;
        let requested = action.clone();
        let result = tokio::task::spawn_blocking(move || {
            mail::act_on_message(&mailbox, &credentials, &message, &requested)
        })
        .await;
        let uncertain = |error: AppError| {
            self.database
                .finish_message_action(
                    organization_id,
                    claimed.id,
                    ActionStatus::Uncertain,
                    Some(error.code),
                    Some(&error.message),
                )
                .map_err(|write| error.with_unrecorded::<()>(Err(write), "pending"))
        };
        match result {
            Ok(Ok(())) => self.database.finish_message_action(
                organization_id,
                claimed.id,
                ActionStatus::Done,
                None,
                None,
            ),
            Ok(Err(error)) if error.code == "IMAP_ACTION_UNCERTAIN" => uncertain(error),
            Ok(Err(error)) => Err(fail(error)),
            Err(stopped) => uncertain(AppError::dependency(
                "IMAP_ACTION_UNCERTAIN",
                format!(
                    "the {} task stopped before the provider's answer was observed: {stopped}",
                    action.verb()
                ),
                false,
            )),
        }
    }

    /// Every action asked of one stored message, newest first.
    pub fn list_message_actions(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<MessageAction>, AppError> {
        self.database
            .list_message_actions(organization_id, message_id)
    }
}

/// A move names the folder as the provider lists it, trimmed; INBOX, in any
/// case, is where the message already is (RFC 3501, 5.1).
fn validated(action: ProviderAction) -> Result<ProviderAction, AppError> {
    let ProviderAction::Move { folder } = action else {
        return Ok(action);
    };
    let folder = folder.trim().to_string();
    if folder.is_empty() {
        return Err(AppError::invalid(
            "MESSAGE_FOLDER_INVALID",
            "the folder to move to is empty; name a folder as the provider lists it",
        ));
    }
    if folder.eq_ignore_ascii_case("INBOX") {
        return Err(AppError::invalid(
            "MESSAGE_FOLDER_INVALID",
            "the message is already in INBOX; name another folder as the provider lists it",
        ));
    }
    Ok(ProviderAction::Move { folder })
}
