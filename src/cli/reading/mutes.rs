//! `skrzynka message mute` and `message unmute`: one message's conversation,
//! or every conversation a saved view keeps.

use super::super::print_json;
use crate::{error::AppError, service::AppState};
use uuid::Uuid;

/// What `message mute` or `message unmute` changes: one conversation, or
/// every conversation of a saved view.
#[derive(clap::Args)]
pub(in crate::cli) struct MuteArgs {
    /// Message ID, as `skrzynka message list` prints it; its whole conversation is changed.
    #[arg(required_unless_present = "view", conflicts_with = "view")]
    id: Option<Uuid>,
    /// Change every conversation of a message this saved view keeps instead of one.
    #[arg(long)]
    view: Option<String>,
}

/// Mute (`mute: true`) or unmute what `args` names and print what changed.
pub(in crate::cli) fn mute(state: &AppState, organization: &str, args: MuteArgs, mute: bool) -> Result<(), AppError> {
    match (args, mute) {
        (MuteArgs { id: Some(id), .. }, true) => print_json(&state.mute_conversation(organization, id)?),
        (MuteArgs { id: Some(id), .. }, false) => print_json(&state.unmute_conversation(organization, id)?),
        (MuteArgs { view: Some(view), .. }, mute) => print_json(&state.mute_view(organization, &view, mute)?),
        // Clap requires one of <ID> and --view, and refuses both.
        (MuteArgs { .. }, _) => Err(AppError::invalid(
            "CONVERSATION_UNMUTABLE",
            "name a message ID or --view <NAME> whose conversations to change",
        )),
    }
}
