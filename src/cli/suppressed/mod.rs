//! `skrzynka message suppressed`: recipients no originated mail may be sent
//! to, listed, added with why, imported from a file, written to one, or
//! removed.

use super::{message::read_body_file, output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{suppression_table, AppState, SuppressionFilter},
};
use std::path::PathBuf;

/// What `message suppressed` changes; without a flag it lists the entries.
#[derive(clap::Args)]
pub(super) struct SuppressedArgs {
    /// An address, or @domain for every address of a domain, no originated mail may be sent to.
    #[arg(long, group = "adding", conflicts_with = "remove", conflicts_with = "import")]
    add: Option<String>,
    /// UTF-8 file of entries, one address or @domain per line with its own reason after it; blank and # lines are skipped.
    #[arg(long, group = "adding", conflicts_with = "remove")]
    import: Option<PathBuf>,
    /// Why, such as "asked not to be written to"; kept with the entry, or with each imported line that names none.
    #[arg(long, requires = "adding")]
    reason: Option<String>,
    /// An entry the list holds, taken off it: sends to it are no longer refused.
    #[arg(long)]
    remove: Option<String>,
    /// Without --add, --import or --remove: list only the entries whose entry or reason contains this, case aside.
    #[arg(long, conflicts_with = "adding", conflicts_with = "remove")]
    contains: Option<String>,
    /// Without --add, --import or --remove: list only the entries added at or after an RFC 3339 instant, YYYY-MM-DD day or span back such as -7d.
    #[arg(long, conflicts_with = "adding", conflicts_with = "remove", allow_hyphen_values = true)]
    since: Option<String>,
    /// Without --add, --import or --remove: list only the entries added before this.
    #[arg(long, conflicts_with = "adding", conflicts_with = "remove", allow_hyphen_values = true)]
    until: Option<String>,
    /// Without --add, --import or --remove: write the entries kept to this new file, as --import reads them.
    #[arg(long, conflicts_with = "adding", conflicts_with = "remove")]
    export: Option<PathBuf>,
    /// Without --add, --import or --remove: write the entries kept to this new file as a CSV table (entry, reason, added_at).
    #[arg(long, conflicts_with = "adding", conflicts_with = "remove", conflicts_with = "export")]
    csv: Option<PathBuf>,
}

pub(super) fn suppressed(state: &AppState, organization: &str, args: SuppressedArgs) -> Result<(), AppError> {
    match args {
        SuppressedArgs { add: Some(entry), reason, .. } => print_json(&state.suppress(organization, &entry, reason.as_deref())?),
        SuppressedArgs { import: Some(path), reason, .. } => {
            let text = read_body_file(&path, "SUPPRESSION_INVALID")?;
            print_json(&state.import_suppressions(organization, &text, reason.as_deref())?)
        }
        SuppressedArgs { remove: Some(entry), .. } => print_json(&state.unsuppress(organization, &entry)?),
        SuppressedArgs { add: None, import: None, remove: None, contains, since, until, export, csv, .. } => {
            let filter = SuppressionFilter { contains, since, until };
            let (path, as_table) = match (export, csv) {
                (Some(path), _) => (path, false),
                (None, Some(path)) => (path, true),
                (None, None) => return print_json(&state.find_suppressions(organization, &filter)?),
            };
            let file = NewFile { path: &path, exists: "SUPPRESSION_OUTPUT_EXISTS", invalid: "SUPPRESSION_OUTPUT_INVALID" };
            file.refuse_occupied()?;
            let entries = state.find_suppressions(organization, &filter)?;
            let text = match as_table {
                true => suppression_table(&entries),
                false => state.suppression_text(organization, &filter)?,
            };
            file.write(text.as_bytes())?;
            print_json(&serde_json::json!({ "path": path, "entries": entries.len(), "size_bytes": text.len() }))
        }
    }
}

/// The addresses `message recipients` checks.
#[derive(clap::Args)]
pub(super) struct RecipientsArgs {
    /// Addresses to check, as a send's --to, --cc or --bcc takes them; comma-separated values are split.
    #[arg(required = true)]
    addresses: Vec<String>,
}

/// Print how a send would meet each address, in the order given.
pub(super) fn check_recipients(state: &AppState, organization: &str, args: RecipientsArgs) -> Result<(), AppError> {
    print_json(&state.check_recipients(organization, &args.addresses)?)
}
