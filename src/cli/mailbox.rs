//! The mailbox subcommands: read the mailboxes Skarbiec declares, and edit
//! the declaration itself.

use super::{arguments::MailboxCommand, print_json, LOCAL_CLI_ORGANIZATION};
use crate::{error::AppError, service::AppState};
use serde_json::json;

pub(super) async fn run_mailbox(state: AppState, command: MailboxCommand) -> Result<(), AppError> {
    match command {
        MailboxCommand::Declare(args) => print_json(
            &state
                .declare_mailbox(LOCAL_CLI_ORGANIZATION, &args.skarbiec_item)
                .await?,
        ),
        MailboxCommand::Undeclare { id } => {
            print_json(&state.undeclare_mailbox(LOCAL_CLI_ORGANIZATION, id).await?)
        }
        MailboxCommand::List => print_json(&state.list_mailboxes(LOCAL_CLI_ORGANIZATION).await?),
        MailboxCommand::Show { id } => print_json(&state.get_mailbox(LOCAL_CLI_ORGANIZATION, id)?),
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
