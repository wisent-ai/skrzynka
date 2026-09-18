//! The mailbox subcommands.

use super::{arguments::MailboxCommand, print_json, LOCAL_CLI_ORGANIZATION};
use crate::{error::AppError, service::AppState};
use serde_json::json;

pub(super) async fn run_mailbox(state: AppState, command: MailboxCommand) -> Result<(), AppError> {
    match command {
        MailboxCommand::Add(args) => print_json(
            &state
                .create_mailbox(LOCAL_CLI_ORGANIZATION, args.into_request())
                .await?,
        ),
        MailboxCommand::Import(args) => {
            let result = state
                .import_mailbox(LOCAL_CLI_ORGANIZATION, args.into_request())
                .await?;
            print_json(&result)
        }
        MailboxCommand::List => print_json(&state.list_mailboxes(LOCAL_CLI_ORGANIZATION)?),
        MailboxCommand::Show { id } => print_json(&state.get_mailbox(LOCAL_CLI_ORGANIZATION, id)?),
        MailboxCommand::Enable { id } => print_json(&state.update_mailbox(
            LOCAL_CLI_ORGANIZATION,
            id,
            crate::models::UpdateMailboxRequest {
                enabled: Some(true),
                ..Default::default()
            },
        )?),
        MailboxCommand::Disable { id } => print_json(&state.update_mailbox(
            LOCAL_CLI_ORGANIZATION,
            id,
            crate::models::UpdateMailboxRequest {
                enabled: Some(false),
                ..Default::default()
            },
        )?),
        MailboxCommand::Remove { id, confirm } => {
            if !confirm {
                return Err(AppError::invalid(
                    "CONFIRMATION_REQUIRED",
                    "mailbox removal requires --confirm",
                ));
            }
            state.delete_mailbox(LOCAL_CLI_ORGANIZATION, id)?;
            print_json(&json!({ "removed": id }))
        }
    }
}
