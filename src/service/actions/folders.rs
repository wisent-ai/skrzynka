//! The folders a mailbox's provider holds, read from the provider now, and
//! the folder a move goes to: named, or found by what the provider says it is
//! for (RFC 6154).

use super::super::AppState;
use crate::{
    error::AppError,
    mail::{self, MessageAction, ProviderAction, ProviderFolder, SpecialUse},
};
use uuid::Uuid;

impl AppState {
    /// Every folder the mailbox's provider lists, read over IMAP now: the
    /// names `message move --folder` takes.
    pub async fn mailbox_folders(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
    ) -> Result<Vec<ProviderFolder>, AppError> {
        let mailbox = self.database.get_mailbox(organization_id, mailbox_id)?;
        let credentials = self
            .resolver
            .resolve_credentials(&mailbox.skarbiec_item_id)
            .await?;
        let email = mailbox.email.clone();
        tokio::task::spawn_blocking(move || mail::list_folders(&mailbox, &credentials))
            .await
            .map_err(|error| {
                AppError::internal(format!(
                    "listing the folders of {email} stopped before the provider answered: {error}"
                ))
            })?
    }

    /// The selectable folder the mailbox's provider declares for `special`,
    /// read now. A provider that declares none is `MESSAGE_FOLDER_UNDECLARED`,
    /// naming the folders it lists, so the operator can name one.
    pub async fn special_folder(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        special: SpecialUse,
    ) -> Result<String, AppError> {
        let folders = self.mailbox_folders(organization_id, mailbox_id).await?;
        let declared = folders.iter().find(|folder| {
            folder.selectable && folder.special_use.as_deref() == Some(special.as_str())
        });
        match declared {
            Some(folder) => Ok(folder.name.clone()),
            None => Err(AppError::conflict(
                "MESSAGE_FOLDER_UNDECLARED",
                format!(
                    "the provider of mailbox {mailbox_id} declares no {} folder (RFC 6154); name one with --folder from {:?}",
                    special.as_str(),
                    folders.iter().map(|folder| folder.name.as_str()).collect::<Vec<_>>()
                ),
            )),
        }
    }

    /// The move a request names: its folder, or the provider's folder for
    /// what `to` names; exactly one of the two.
    pub async fn move_action(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        folder: Option<String>,
        to: Option<SpecialUse>,
    ) -> Result<ProviderAction, AppError> {
        match (folder, to) {
            (Some(folder), None) => Ok(ProviderAction::Move { folder }),
            (None, Some(special)) => Ok(ProviderAction::Move {
                folder: self
                    .special_folder(organization_id, mailbox_id, special)
                    .await?,
            }),
            _ => Err(unnamed_folder()),
        }
    }

    /// Move one stored message to the folder `folder` names, or to its
    /// provider's folder for `to`.
    pub async fn move_message(
        &self,
        organization_id: &str,
        message_id: Uuid,
        folder: Option<String>,
        to: Option<SpecialUse>,
    ) -> Result<MessageAction, AppError> {
        let mailbox_id = self
            .database
            .get_message(organization_id, message_id)?
            .mailbox_id;
        let action = self
            .move_action(organization_id, mailbox_id, folder, to)
            .await?;
        self.act_on_message(organization_id, message_id, action)
            .await
    }
}

/// A move that names neither a folder nor what one is for, or both.
pub(super) fn unnamed_folder() -> AppError {
    AppError::invalid(
        "MESSAGE_FOLDER_INVALID",
        "name the folder to move to, or what it is for with to (archive, trash, junk or all); one of the two",
    )
}
