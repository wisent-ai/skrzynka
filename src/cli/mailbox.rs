//! The mailbox subcommands: read the mailboxes Skarbiec declares, and edit
//! the declaration itself.

use super::{arguments::MailboxCommand, output::NewFile, print_json};
use crate::{
    error::AppError,
    models::Mailbox,
    service::{AppState, WarmUpRequest},
};
use clap::Args;
use serde_json::json;

/// What `mailbox declare` declares: a Skarbiec item, or an inbound domain.
#[derive(Args)]
#[group(required = true, multiple = false)]
pub(super) struct DeclareMailboxArgs {
    /// Skarbiec item that holds the mailbox's credentials.
    #[arg(long)]
    pub(super) skarbiec_item: Option<String>,
    /// A mail domain, as it follows the @ of its addresses, whose mail Skrzynka
    /// reads through wisent-integrations with the Skarbiec item playing role
    /// skrzynka-integration-api (fields token and url).
    #[arg(long)]
    pub(super) inbound_domain: Option<String>,
}

/// One mailbox, named by its ID.
#[derive(Args)]
pub(super) struct MailboxRef {
    /// Mailbox ID, as `skrzynka mailbox list` prints it.
    id: uuid::Uuid,
}

/// What `mailbox delivery` and `mailbox summary` count: one mailbox's sends
/// or stored mail, within a window when given.
#[derive(Args)]
pub(super) struct WindowArgs {
    /// Mailbox ID, as `skrzynka mailbox list` prints it.
    id: uuid::Uuid,
    /// Only what was sent or received at or after this RFC 3339 instant or UTC day.
    #[arg(long, allow_hyphen_values = true)]
    since: Option<String>,
    /// Only what was sent or received before this RFC 3339 instant or UTC day.
    #[arg(long, allow_hyphen_values = true)]
    until: Option<String>,
}

/// What `mailbox delivery` counts, and the new files its days or its
/// recipient domains are written to as CSV tables.
#[derive(Args)]
pub(super) struct DeliveryArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Write the report's days (day, total, sent, failed, uncertain, in_flight, bounced) to this new file as a CSV table.
    #[arg(long)]
    days_csv: Option<std::path::PathBuf>,
    /// Write the report's recipient domains (domain, sent, failed, bounced) to this new file as a CSV table.
    #[arg(long, conflicts_with = "days_csv")]
    domains_csv: Option<std::path::PathBuf>,
}

/// What `mailbox summary` counts, and the new file its days are written to
/// as a CSV table.
#[derive(Args)]
pub(super) struct SummaryArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Write the summary's days (day, received) to this new file as a CSV table.
    #[arg(long)]
    days_csv: Option<std::path::PathBuf>,
}

/// What `skrzynka sync` reads: every declared mailbox, one, or one inbound domain.
#[derive(Args)]
pub(super) struct SyncArgs {
    /// Import only this mailbox, by ID or by its own address.
    #[arg(long, conflicts_with = "inbound_domain")]
    pub(super) mailbox: Option<String>,
    /// Read only this inbound domain, as `mailbox list --inbound` prints its ID.
    #[arg(long)]
    pub(super) inbound_domain: Option<uuid::Uuid>,
}

/// What `mailbox list` lists.
#[derive(Args)]
pub(super) struct ListArgs {
    /// List the inbound domains instead of the mailboxes.
    #[arg(long, conflicts_with = "state")]
    inbound: bool,
    /// Only the mailboxes in this state: failing (declared, last sync errored), undeclared, or unsynchronized.
    #[arg(long, value_enum)]
    state: Option<crate::service::MailboxState>,
    /// Write the mailboxes listed to this new file as a CSV table: ID,
    /// address, name, whether enabled, hosts, last UID, last sync and its
    /// error, created; never a credential.
    #[arg(long, conflicts_with = "inbound")]
    csv: Option<std::path::PathBuf>,
}

/// What `mailbox remove` deletes.
#[derive(Args)]
pub(super) struct RemoveArgs {
    /// Mailbox ID, or inbound domain ID with --inbound, as `mailbox list` prints it.
    id: uuid::Uuid,
    /// The ID names an inbound domain.
    #[arg(long)]
    inbound: bool,
    /// Required: the local mail is deleted and cannot be restored.
    #[arg(long)]
    confirm: bool,
}

/// What `mailbox limit` reads or changes.
#[derive(Args)]
pub(super) struct LimitArgs {
    /// Mailbox ID, as `skrzynka mailbox list` prints it.
    id: uuid::Uuid,
    /// Messages the mailbox may send in one UTC day; with --grow-by, the count a warm-up starts from today.
    #[arg(long, conflicts_with = "clear")]
    per_day: Option<std::num::NonZeroU32>,
    /// Warm up: raise the limit by this many messages every UTC day after today.
    #[arg(long, requires = "per_day", requires = "up_to")]
    grow_by: Option<std::num::NonZeroU32>,
    /// The count a warm-up stops growing at; above --per-day.
    #[arg(long, requires = "grow_by")]
    up_to: Option<std::num::NonZeroU32>,
    /// Remove the limit.
    #[arg(long)]
    clear: bool,
}

/// What `mailbox signature` reads or changes.
#[derive(Args)]
pub(super) struct SignatureArgs {
    /// Mailbox ID, as `skrzynka mailbox list` prints it.
    id: uuid::Uuid,
    /// UTF-8 file holding the signature to set.
    #[arg(long, conflicts_with = "clear")]
    file: Option<std::path::PathBuf>,
    /// Remove the signature.
    #[arg(long)]
    clear: bool,
}

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
        MailboxCommand::Undeclare(MailboxRef { id }) => {
            print_json(&state.undeclare_mailbox(organization, id).await?)
        }
        MailboxCommand::List(ListArgs { inbound: true, .. }) => {
            print_json(&state.list_inbound_domains(organization)?)
        }
        MailboxCommand::List(ListArgs { inbound: false, state: wanted, csv: None }) => {
            print_json(&state.list_mailboxes(organization, wanted).await?)
        }
        MailboxCommand::List(ListArgs { inbound: false, state: wanted, csv: Some(path) }) => {
            let file = NewFile { path: &path, exists: "MAILBOX_OUTPUT_EXISTS", invalid: "MAILBOX_OUTPUT_INVALID" };
            file.refuse_occupied()?;
            let mailboxes = state.list_mailboxes(organization, wanted).await?;
            let table = Mailbox::table(&mailboxes);
            file.write(table.as_bytes())?;
            print_json(&json!({ "path": path, "mailboxes": mailboxes.len(), "size_bytes": table.len() }))
        }
        MailboxCommand::Show(MailboxRef { id }) => {
            print_json(&state.get_mailbox(organization, id).await?)
        }
        MailboxCommand::Delivery(DeliveryArgs { window: WindowArgs { id, since, until }, days_csv, domains_csv }) => {
            let written = match (days_csv, domains_csv) {
                (Some(path), _) => Some((path, true)),
                (None, Some(path)) => Some((path, false)),
                (None, None) => None,
            };
            let Some((path, days)) = written else {
                return print_json(&state.mailbox_delivery(organization, id, since.as_deref(), until.as_deref())?);
            };
            let file = NewFile { path: &path, exists: "DELIVERY_OUTPUT_EXISTS", invalid: "DELIVERY_OUTPUT_INVALID" };
            file.refuse_occupied()?;
            let report = state.mailbox_delivery(organization, id, since.as_deref(), until.as_deref())?;
            let (table, rows) = match days {
                true => (report.day_table(), report.days.len()),
                false => (report.domain_table(), report.domains.len()),
            };
            file.write(table.as_bytes())?;
            print_json(&json!({ "path": path, "rows": rows, "size_bytes": table.len() }))
        }
        MailboxCommand::Summary(SummaryArgs { window: WindowArgs { id, since, until }, days_csv }) => {
            let Some(path) = days_csv else {
                return print_json(&state.mailbox_summary(organization, id, since.as_deref(), until.as_deref())?);
            };
            let file = NewFile { path: &path, exists: "SUMMARY_OUTPUT_EXISTS", invalid: "SUMMARY_OUTPUT_INVALID" };
            file.refuse_occupied()?;
            let summary = state.mailbox_summary(organization, id, since.as_deref(), until.as_deref())?;
            let table = summary.day_table();
            file.write(table.as_bytes())?;
            print_json(&json!({ "path": path, "rows": summary.days.len(), "size_bytes": table.len() }))
        }
        MailboxCommand::Folders(MailboxRef { id }) => {
            print_json(&state.mailbox_folders(organization, id).await?)
        }
        MailboxCommand::Limit(LimitArgs {
            id,
            per_day: Some(per_day),
            grow_by,
            up_to,
            clear: false,
        }) => {
            let warm_up = grow_by.zip(up_to).map(|(grow_by, up_to)| WarmUpRequest { grow_by, up_to });
            print_json(&state.set_mailbox_send_limit(organization, id, Some(per_day), warm_up)?)
        }
        MailboxCommand::Limit(LimitArgs {
            id, clear: true, ..
        }) => print_json(&state.set_mailbox_send_limit(organization, id, None, None)?),
        MailboxCommand::Limit(LimitArgs { id, .. }) => {
            print_json(&state.mailbox_send_limit(organization, id)?)
        }
        MailboxCommand::Signature(SignatureArgs {
            id,
            file: Some(file),
            ..
        }) => {
            let text = super::message::read_body_file(&file, "SIGNATURE_INVALID")?;
            print_json(&state.set_mailbox_signature(organization, id, Some(&text))?)
        }
        MailboxCommand::Signature(SignatureArgs { id, clear: true, .. }) => {
            print_json(&state.set_mailbox_signature(organization, id, None)?)
        }
        MailboxCommand::Signature(SignatureArgs { id, .. }) => {
            print_json(&state.mailbox_signature(organization, id)?)
        }
        MailboxCommand::Responder(args) => super::auto_replies::run_auto_reply(&state, organization, args).await,
        MailboxCommand::Remove(RemoveArgs {
            id,
            inbound: true,
            confirm,
        }) => {
            state.remove_inbound_domain(organization, id, confirm)?;
            print_json(&json!({ "removed": id }))
        }
        MailboxCommand::Remove(RemoveArgs {
            id,
            inbound: false,
            confirm,
        }) => {
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
