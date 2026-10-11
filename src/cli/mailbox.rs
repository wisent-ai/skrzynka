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
        MailboxCommand::Declare(args) => match (args.skarbiec_item, args.inbound_domain) {
            (_, Some(domain)) => print_json(&state.declare_inbound_domain(organization, &domain)?),
            (Some(item), None) => print_json(&state.declare_mailbox(organization, &item).await?),
            (None, None) => Err(AppError::invalid(
                "MAILBOX_DECLARATION_INVALID",
                "name --skarbiec-item for a mailbox or --inbound-domain for an inbound domain",
            )),
        },
        MailboxCommand::Undeclare { id } => {
            print_json(&state.undeclare_mailbox(organization, id).await?)
        }
        MailboxCommand::List { inbound: true } => {
            print_json(&state.list_inbound_domains(organization)?)
        }
        MailboxCommand::List { inbound: false } => {
            print_json(&state.list_mailboxes(organization).await?)
        }
        MailboxCommand::Show { id } => print_json(&state.get_mailbox(organization, id).await?),
        MailboxCommand::Remove {
            id,
            inbound: true,
            confirm,
        } => {
            state.remove_inbound_domain(organization, id, confirm)?;
            print_json(&json!({ "removed": id }))
        }
        MailboxCommand::Remove {
            id,
            inbound: false,
            confirm,
        } => {
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
