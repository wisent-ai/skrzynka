//! `skrzynka message blocked`: the blocked senders listed, counted, found,
//! imported from a file, written to one in the form an import reads, or
//! written as a table of each entry and the mail it hides.

use super::super::{message::read_body_file, output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{blocked_table, AppState},
};
use std::path::PathBuf;

/// What `message blocked` prints: the entries, or each with the mail it
/// leaves out; or a file of entries it blocks, or one it writes.
#[derive(clap::Args)]
pub(in crate::cli) struct BlockedArgs {
    /// Count how many stored messages each entry leaves out of listings now.
    #[arg(long)]
    count: bool,
    /// List only the entries holding this text, case aside, such as a domain.
    #[arg(long)]
    contains: Option<String>,
    /// UTF-8 file of entries to block, one address or @domain per line; blank and # lines are skipped.
    #[arg(long, conflicts_with = "count", conflicts_with = "contains", conflicts_with = "export")]
    import: Option<PathBuf>,
    /// Write the entries listed to this new file, one per line, as --import reads them.
    #[arg(long, conflicts_with = "count")]
    export: Option<PathBuf>,
    /// Write the entries listed, each with the stored messages it leaves out, to this new file as a CSV table.
    #[arg(long, conflicts_with = "count", conflicts_with = "export", conflicts_with = "import")]
    csv: Option<PathBuf>,
}

pub(in crate::cli) fn blocked(state: &AppState, organization: &str, args: BlockedArgs) -> Result<(), AppError> {
    let BlockedArgs { count, contains, import, export, csv } = args;
    if let Some(path) = import {
        return print_json(&state.import_blocked(organization, &read_body_file(&path, "BLOCKED_SENDER_INVALID")?)?);
    }
    if let Some(path) = export {
        let file = NewFile { path: &path, exists: "BLOCKED_OUTPUT_EXISTS", invalid: "BLOCKED_OUTPUT_INVALID" };
        file.refuse_occupied()?;
        let entries = state.blocked_senders(organization, contains.as_deref())?.blocked.len();
        let text = state.blocked_text(organization, contains.as_deref())?;
        file.write(text.as_bytes())?;
        return print_json(&serde_json::json!({ "path": path, "entries": entries, "size_bytes": text.len() }));
    }
    if let Some(path) = csv {
        let file = NewFile { path: &path, exists: "BLOCKED_OUTPUT_EXISTS", invalid: "BLOCKED_OUTPUT_INVALID" };
        file.refuse_occupied()?;
        let counts = state.blocked_counts(organization, contains.as_deref())?;
        let table = blocked_table(&counts);
        file.write(table.as_bytes())?;
        return print_json(&serde_json::json!({ "path": path, "entries": counts.len(), "size_bytes": table.len() }));
    }
    match count {
        true => print_json(&state.blocked_counts(organization, contains.as_deref())?),
        false => print_json(&state.blocked_senders(organization, contains.as_deref())?),
    }
}
