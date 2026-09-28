//! The mailbox subcommands: read the mailboxes Skarbiec declares, and edit
//! the declaration itself.

use super::{arguments::MailboxCommand, print_json};
use crate::{error::AppError, service::AppState};
use serde_json::json;

pub(super) async fn run_mailbox(
    state: AppState,
    organization: &str,
    command: MailboxCommand,
) -> Result<(), AppError> {
    match command {
        MailboxCommand::Declare(args) => print_json(
            &state
                .declare_mailbox(organization, &args.skarbiec_item)
                .await?,
        ),
        MailboxCommand::Undeclare { id } => {
            print_json(&state.undeclare_mailbox(organization, id).await?)
        }
        MailboxCommand::List => print_json(&state.list_mailboxes(organization).await?),
        MailboxCommand::Show { id } => print_json(&state.get_mailbox(organization, id)?),
        MailboxCommand::Remove { id, confirm } => {
            if !confirm {
                return Err(AppError::invalid(
                    "CONFIRMATION_REQUIRED",
                    "mailbox removal requires --confirm",
                ));
            }
            state.delete_mailbox(organization, id)?;
            print_json(&json!({ "removed": id }))
        }
    }
}
