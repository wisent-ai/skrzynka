//! `skrzynka message view`: listing filters kept under a name. `message list
//! --save-view <NAME>` keeps one and `message list --view <NAME>` lists it.

use super::{arguments::ViewCommand, message::read_body_file, output::NewFile, print_json};
use crate::{error::AppError, service::AppState};

/// One view, named.
#[derive(clap::Args)]
pub(super) struct NameArg {
    /// The view's name, as `skrzynka message view list` prints it.
    name: String,
}

/// What `message view list` prints: the views, or each with its count.
#[derive(clap::Args)]
pub(super) struct ListArgs {
    /// Count how many stored messages each view lists now.
    #[arg(long)]
    count: bool,
    /// Only the views whose name holds this text, case aside.
    #[arg(long)]
    contains: Option<String>,
}

/// A view and the name it is to carry, or its copy is to carry.
#[derive(clap::Args)]
pub(super) struct RenameArgs {
    /// The view's name, as `skrzynka message view list` prints it.
    name: String,
    /// The name it is to carry; no other view may have it.
    #[arg(long)]
    to: String,
}

/// What `message view export` writes and which views.
#[derive(clap::Args)]
pub(super) struct ExportArgs {
    /// New file to write the views to, as JSON `[{name, mailbox, filters}]`; an existing path is never written over.
    #[arg(long)]
    output: std::path::PathBuf,
    /// Only the views whose name holds this text, case aside.
    #[arg(long)]
    contains: Option<String>,
}

/// What `message view import` reads.
#[derive(clap::Args)]
pub(super) struct ImportArgs {
    /// A file `message view export` wrote.
    #[arg(long)]
    input: std::path::PathBuf,
}

pub(super) fn run_view(
    state: &AppState,
    organization: &str,
    command: ViewCommand,
) -> Result<(), AppError> {
    match command {
        ViewCommand::List(ListArgs { count: true, contains }) => {
            print_json(&state.counted_views(organization, contains.as_deref())?)
        }
        ViewCommand::List(ListArgs { count: false, contains }) => print_json(&state.views(organization, contains.as_deref())?),
        ViewCommand::Show(NameArg { name }) => print_json(&state.view(organization, &name)?),
        ViewCommand::Rename(RenameArgs { name, to }) => {
            print_json(&state.rename_view(organization, &name, &to)?)
        }
        ViewCommand::Copy(RenameArgs { name, to }) => print_json(&state.copy_view(organization, &name, &to)?),
        ViewCommand::Delete(NameArg { name }) => print_json(&state.delete_view(organization, &name)?),
        ViewCommand::Export(ExportArgs { output, contains }) => {
            let file = NewFile { path: &output, exists: "VIEW_OUTPUT_EXISTS", invalid: "VIEW_OUTPUT_INVALID" };
            file.refuse_occupied()?;
            let views = state.views(organization, contains.as_deref())?.len();
            let document = state.view_document(organization, contains.as_deref())?;
            file.write(document.as_bytes())?;
            print_json(&serde_json::json!({ "path": output, "views": views, "size_bytes": document.len() }))
        }
        ViewCommand::Import(ImportArgs { input }) => {
            print_json(&state.import_views(organization, &read_body_file(&input, "VIEW_INVALID")?)?)
        }
    }
}
