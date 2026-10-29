//! `skrzynka message followup`: sent messages the operator expects an
//! answer to by an instant, and which of them are owed one.

use super::{arguments::FollowupCommand, output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{follow_up_table, AppState, FollowUpFilter, FollowUpState},
};
use std::path::PathBuf;
use uuid::Uuid;

/// What `message followup set` records.
#[derive(clap::Args)]
pub(super) struct SetArgs {
    /// Outbound message ID, as `skrzynka message outbound` prints it.
    id: Uuid,
    /// RFC 3339 instant by which an answer is expected, or a span from now such as +3d or +1w (m, h, d, w).
    #[arg(long)]
    by: String,
}

/// What `message followup list` shows.
#[derive(clap::Args)]
pub(super) struct ListArgs {
    /// Only the follow-ups standing so: waiting (unanswered, instant ahead), due (unanswered, instant passed) or answered.
    #[arg(long, value_enum)]
    state: Option<FollowUpState>,
    /// Only the follow-ups of messages this mailbox sent, by ID or address.
    #[arg(long)]
    mailbox: Option<String>,
    /// Only the follow-ups whose recipients contain this text, case aside.
    #[arg(long)]
    to: Option<String>,
    /// Only the follow-ups whose sent message's subject or body contains this text, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// Only the follow-ups expecting an answer before this: +3d (coming due), an RFC 3339 instant, a YYYY-MM-DD day or -1d.
    #[arg(long, allow_hyphen_values = true)]
    due_by: Option<String>,
    /// Write the follow-ups listed as a CSV table (RFC 4180), one row each, to this new file.
    #[arg(long)]
    csv: Option<PathBuf>,
}

/// One outbound message, named by its ID.
#[derive(clap::Args)]
pub(super) struct OutboundRef {
    /// Outbound message ID, as `skrzynka message outbound` prints it.
    id: Uuid,
}

/// What `message followup clear` stops expecting: one sent message's
/// answer, or every follow-up standing so, narrowed as `list` narrows.
#[derive(clap::Args)]
pub(super) struct ClearArgs {
    /// Outbound message ID, as `skrzynka message outbound` prints it.
    #[arg(required_unless_present = "state", conflicts_with = "state")]
    id: Option<Uuid>,
    /// Clear every follow-up standing so instead: waiting, due or answered (such as every answered one).
    #[arg(long, value_enum)]
    state: Option<FollowUpState>,
    /// With --state, only those of messages this mailbox sent, by ID or address.
    #[arg(long, requires = "state")]
    mailbox: Option<String>,
    /// With --state, only those whose recipients contain this text, case aside.
    #[arg(long, requires = "state")]
    to: Option<String>,
    /// With --state, only those whose sent message's subject or body contains this text, case aside.
    #[arg(long, requires = "state")]
    contains: Option<String>,
    /// With --state, only those expecting an answer before this, as `list --due-by` reads it.
    #[arg(long, requires = "state", allow_hyphen_values = true)]
    due_by: Option<String>,
}

/// What `message followup nudge` drafts: a reminder from a template to
/// every follow-up still owed an answer that the filters keep.
#[derive(clap::Args)]
pub(super) struct NudgeArgs {
    /// Template whose text each reminder holds; {{subject}}, {{recipients}}, {{sent_at}} and {{due_at}} are filled from the sent message.
    #[arg(long)]
    template: String,
    /// Only the follow-ups standing so: waiting or due; answered ones are owed nothing and refused.
    #[arg(long, value_enum)]
    state: Option<FollowUpState>,
    /// Only the follow-ups of messages this mailbox sent, by ID or address.
    #[arg(long)]
    mailbox: Option<String>,
    /// Only the follow-ups whose recipients contain this text, case aside.
    #[arg(long)]
    to: Option<String>,
    /// Only the follow-ups whose sent message's subject or body contains this text, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// Only the follow-ups expecting an answer before this, as `list --due-by` reads it.
    #[arg(long, allow_hyphen_values = true)]
    due_by: Option<String>,
}

pub(super) fn run_follow_up(
    state: &AppState,
    organization: &str,
    command: FollowupCommand,
) -> Result<(), AppError> {
    match command {
        FollowupCommand::Set(SetArgs { id, by }) => {
            print_json(&state.set_follow_up(organization, id, &by)?)
        }
        FollowupCommand::List(ListArgs {
            state: standing,
            mailbox,
            to,
            contains,
            due_by,
            csv,
        }) => {
            let mailbox_id = super::reading::chosen_mailbox(state, organization, mailbox)?;
            let filter = FollowUpFilter {
                state: standing,
                mailbox_id,
                to,
                contains,
                due_by,
            };
            let Some(path) = csv else {
                return print_json(&state.follow_ups(organization, &filter)?);
            };
            let file = NewFile {
                path: &path,
                exists: "FOLLOW_UP_OUTPUT_EXISTS",
                invalid: "FOLLOW_UP_OUTPUT_INVALID",
            };
            file.refuse_occupied()?;
            let follow_ups = state.follow_ups(organization, &filter)?;
            let table = follow_up_table(&follow_ups);
            file.write(table.as_bytes())?;
            print_json(
                &serde_json::json!({ "path": path, "follow_ups": follow_ups.len(), "size_bytes": table.len() }),
            )
        }
        FollowupCommand::Show(OutboundRef { id }) => {
            print_json(&state.get_follow_up(organization, id)?)
        }
        FollowupCommand::Clear(ClearArgs { id: Some(id), .. }) => {
            print_json(&state.clear_follow_up(organization, id)?)
        }
        FollowupCommand::Clear(ClearArgs {
            id: None,
            state: standing,
            mailbox,
            to,
            contains,
            due_by,
        }) => {
            let mailbox_id = super::reading::chosen_mailbox(state, organization, mailbox)?;
            let filter = FollowUpFilter {
                state: standing,
                mailbox_id,
                to,
                contains,
                due_by,
            };
            print_json(&state.clear_follow_ups(organization, &filter)?)
        }
        FollowupCommand::Nudge(NudgeArgs {
            template,
            state: standing,
            mailbox,
            to,
            contains,
            due_by,
        }) => {
            let mailbox_id = super::reading::chosen_mailbox(state, organization, mailbox)?;
            let filter = FollowUpFilter {
                state: standing,
                mailbox_id,
                to,
                contains,
                due_by,
            };
            print_json(&state.nudge_follow_ups(organization, &filter, &template)?)
        }
    }
}
