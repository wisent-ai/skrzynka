//! Marking or moving at the provider every message a saved view lists, or
//! every message of one conversation: each message is its own recorded
//! action, so one that cannot be changed is named with its refusal and the
//! others still are.

use super::super::AppState;
use super::folders::unnamed_folder;
use crate::{
    error::AppError,
    mail::{MessageAction, ProviderAction, SpecialUse},
    models::Message,
};
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

/// What a change asked of a view's messages did.
#[derive(Debug, Serialize)]
pub struct ViewActions {
    pub view: String,
    /// The recorded action of every message the provider was asked to change.
    pub actions: Vec<MessageAction>,
    /// Every message refused before or while it was changed, and why.
    pub refused: Vec<RefusedAction>,
}

/// What a change asked of a conversation's messages did.
#[derive(Debug, Serialize)]
pub struct ThreadActions {
    /// The message whose conversation was changed, as it was named.
    pub thread: Uuid,
    pub actions: Vec<MessageAction>,
    pub refused: Vec<RefusedAction>,
}

/// One message that was not changed.
#[derive(Debug, Serialize)]
pub struct RefusedAction {
    pub message_id: Uuid,
    pub code: &'static str,
    pub message: String,
}

/// What asking the provider to change several messages did.
type Outcomes = (Vec<MessageAction>, Vec<RefusedAction>);

impl AppState {
    /// Ask the provider to apply `action` to every message the saved view
    /// lists. Deleting is refused here: each deletion is asked of one message.
    pub async fn act_on_view(
        &self,
        organization_id: &str,
        view: &str,
        action: ProviderAction,
    ) -> Result<ViewActions, AppError> {
        refuse_delete(&action)?;
        let messages = self.list_view(organization_id, view, None, None)?;
        let (actions, refused) = self.act_on_each(organization_id, messages, action).await;
        Ok(ViewActions { view: view.trim().to_string(), actions, refused })
    }

    /// Move every message the saved view lists to the folder `folder`
    /// names, or to the folder each message's provider declares for `to`,
    /// read once per mailbox. A mailbox whose provider declares none has each
    /// of its messages refused with `MESSAGE_FOLDER_UNDECLARED`.
    pub async fn move_view(
        &self,
        organization_id: &str,
        view: &str,
        folder: Option<String>,
        to: Option<SpecialUse>,
    ) -> Result<ViewActions, AppError> {
        let messages = self.list_view(organization_id, view, None, None)?;
        let (actions, refused) = self.move_each(organization_id, messages, folder, to).await?;
        Ok(ViewActions { view: view.trim().to_string(), actions, refused })
    }

    /// Ask the provider to apply `action` to every message of the
    /// conversation `id` belongs to, as `message thread` lists it. Deleting
    /// is refused here, as for a view.
    pub async fn act_on_thread(
        &self,
        organization_id: &str,
        id: Uuid,
        action: ProviderAction,
    ) -> Result<ThreadActions, AppError> {
        refuse_delete(&action)?;
        let messages = self.message_thread(organization_id, id)?;
        let (actions, refused) = self.act_on_each(organization_id, messages, action).await;
        Ok(ThreadActions { thread: id, actions, refused })
    }

    /// Move every message of the conversation `id` belongs to, as
    /// `move_view` moves a view's: archiving a whole conversation at once.
    pub async fn move_thread(
        &self,
        organization_id: &str,
        id: Uuid,
        folder: Option<String>,
        to: Option<SpecialUse>,
    ) -> Result<ThreadActions, AppError> {
        let messages = self.message_thread(organization_id, id)?;
        let (actions, refused) = self.move_each(organization_id, messages, folder, to).await?;
        Ok(ThreadActions { thread: id, actions, refused })
    }

    /// `action` asked of each message, each recorded or refused on its own.
    async fn act_on_each(&self, organization_id: &str, messages: Vec<Message>, action: ProviderAction) -> Outcomes {
        let mut done: Outcomes = (Vec::new(), Vec::new());
        for message in messages {
            match self.act_on_message(organization_id, message.id, action.clone()).await {
                Ok(recorded) => done.0.push(recorded),
                Err(error) => done.1.push(RefusedAction { message_id: message.id, code: error.code, message: error.message }),
            }
        }
        done
    }

    /// Each message moved to `folder`, or to the folder its mailbox's
    /// provider declares for `to`, read once per mailbox.
    async fn move_each(
        &self,
        organization_id: &str,
        messages: Vec<Message>,
        folder: Option<String>,
        to: Option<SpecialUse>,
    ) -> Result<Outcomes, AppError> {
        let special = match (folder, to) {
            (Some(folder), None) => {
                return Ok(self.act_on_each(organization_id, messages, ProviderAction::Move { folder }).await);
            }
            (None, Some(special)) => special,
            _ => return Err(unnamed_folder()),
        };
        let mut done: Outcomes = (Vec::new(), Vec::new());
        // Each mailbox's folder for `special`, or why it has none, read once.
        let mut folders: HashMap<Uuid, Result<String, (&'static str, String)>> = HashMap::new();
        for message in messages {
            if !folders.contains_key(&message.mailbox_id) {
                let found = self
                    .special_folder(organization_id, message.mailbox_id, special)
                    .await
                    .map_err(|error| (error.code, error.message));
                folders.insert(message.mailbox_id, found);
            }
            let outcome = match &folders[&message.mailbox_id] {
                Ok(folder) => self
                    .act_on_message(organization_id, message.id, ProviderAction::Move { folder: folder.clone() })
                    .await
                    .map_err(|error| (error.code, error.message)),
                Err(refusal) => Err(refusal.clone()),
            };
            match outcome {
                Ok(recorded) => done.0.push(recorded),
                Err((code, said)) => done.1.push(RefusedAction { message_id: message.id, code, message: said }),
            }
        }
        Ok(done)
    }
}

/// Deleting is asked of one message at a time, never of many together.
fn refuse_delete(action: &ProviderAction) -> Result<(), AppError> {
    if matches!(action, ProviderAction::Delete) {
        return Err(AppError::invalid(
            "VIEW_ACTION_INVALID",
            "several messages are marked or moved together; delete each message by its ID",
        ));
    }
    Ok(())
}
