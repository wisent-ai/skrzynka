//! `skrzynka message mark|move|delete|actions`: changes to a stored message
//! at its provider, and the record of every change asked of it. The four are
//! variants of `MessageCommand` itself, so every command is declared in one
//! place; these are their arguments.

use super::print_json;
use crate::{
    error::AppError,
    mail::{MarkState, ProviderAction, SpecialUse},
    service::AppState,
};
use clap::Args;
use uuid::Uuid;

/// Which messages a mark or move changes: one, every message a saved view
/// lists, or every message of one conversation; exactly one is named.
#[derive(Args)]
#[group(required = true, multiple = false)]
pub(super) struct Target {
    /// Message ID, as `skrzynka message list` prints it.
    id: Option<Uuid>,
    /// Every message this saved view lists instead of one.
    #[arg(long)]
    view: Option<String>,
    /// Every message of the conversation this message belongs to, as `message thread` lists it.
    #[arg(long)]
    thread: Option<Uuid>,
}

/// What `message mark` sets: on one message, a view's or a conversation's.
#[derive(Args)]
pub(super) struct MarkArgs {
    #[command(flatten)]
    target: Target,
    /// The state to set: seen, unseen, flagged (starred in Gmail) or unflagged.
    #[arg(long = "as", value_enum)]
    state: MarkState,
}

/// Where `message move` moves a message, a view's or a conversation's.
#[derive(Args)]
pub(super) struct MoveArgs {
    #[command(flatten)]
    target: Target,
    /// The folder as the provider lists it, such as an archive folder.
    #[arg(long, required_unless_present = "to", conflicts_with = "to")]
    folder: Option<String>,
    /// What the folder is for, as the provider declares it: archive, trash, junk or all.
    #[arg(long, value_enum)]
    to: Option<SpecialUse>,
}

/// What `message delete` deletes.
#[derive(Args)]
pub(super) struct DeleteArgs {
    /// Message ID, as `skrzynka message list` prints it.
    id: Uuid,
    /// Confirms the deletion, which the provider may not let be undone.
    #[arg(long)]
    confirm: bool,
}

/// One change asked of a message at its provider, or a read of them.
pub(super) enum ProviderCommand {
    Mark(MarkArgs),
    Move(MoveArgs),
    Delete(DeleteArgs),
    Actions(Uuid),
}

pub(super) async fn run_provider(
    state: AppState,
    organization: &str,
    command: ProviderCommand,
) -> Result<(), AppError> {
    match command {
        ProviderCommand::Actions(id) => print_json(&state.list_message_actions(organization, id)?),
        ProviderCommand::Mark(MarkArgs { target, state: mark }) => {
            let action = mark.action();
            match target {
                Target { id: Some(id), .. } => print_json(&state.act_on_message(organization, id, action).await?),
                Target { view: Some(view), .. } => {
                    let done = state.act_on_view(organization, &view, action).await?;
                    print_json(&done)?;
                    outcome(&done.refused, "VIEW_ACTION_INCOMPLETE", "the view's")
                }
                Target { thread: Some(thread), .. } => {
                    let done = state.act_on_thread(organization, thread, action).await?;
                    print_json(&done)?;
                    outcome(&done.refused, "THREAD_ACTION_INCOMPLETE", "the conversation's")
                }
                Target { .. } => Err(unnamed()),
            }
        }
        ProviderCommand::Move(MoveArgs { target, folder, to }) => match target {
            Target { id: Some(id), .. } => print_json(&state.move_message(organization, id, folder, to).await?),
            Target { view: Some(view), .. } => {
                let done = state.move_view(organization, &view, folder, to).await?;
                print_json(&done)?;
                outcome(&done.refused, "VIEW_ACTION_INCOMPLETE", "the view's")
            }
            Target { thread: Some(thread), .. } => {
                let done = state.move_thread(organization, thread, folder, to).await?;
                print_json(&done)?;
                outcome(&done.refused, "THREAD_ACTION_INCOMPLETE", "the conversation's")
            }
            Target { .. } => Err(unnamed()),
        },
        ProviderCommand::Delete(DeleteArgs { id, confirm: false }) => Err(AppError::invalid(
            "CONFIRMATION_REQUIRED",
            format!(
                "message delete {id} needs --confirm: the provider may not let the deletion be undone; Skrzynka keeps its stored copy either way"
            ),
        )),
        ProviderCommand::Delete(DeleteArgs { id, confirm: true }) => {
            print_json(&state.act_on_message(organization, id, ProviderAction::Delete).await?)
        }
    }
}

/// Clap requires exactly one of <ID>, --view and --thread; reaching here
/// with none is refused all the same.
fn unnamed() -> AppError {
    AppError::invalid("VIEW_ACTION_INVALID", "name a message ID, --view <NAME> or --thread <ID> to change")
}

/// A change of several messages is complete only when none of them was
/// refused; the printed outcome names each refused one.
fn outcome<T>(refused: &[T], code: &'static str, whose: &str) -> Result<(), AppError> {
    if refused.is_empty() {
        return Ok(());
    }
    Err(AppError::invalid(
        code,
        format!("{} of {whose} messages were not changed; each is listed under refused", refused.len()),
    ))
}
