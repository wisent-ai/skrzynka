//! `skrzynka message bounced`: the addresses a stored report says delivery
//! to failed for good, one cleared when it works again or every one listed,
//! or the listing written as a CSV table.

use super::super::{output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{bounced_table, AppState},
};
use std::path::PathBuf;

/// What `message bounced` prints, or the address it clears.
#[derive(clap::Args)]
pub(in crate::cli) struct BouncedArgs {
    /// An address that works again: reports received until now no longer refuse sends to it.
    #[arg(long)]
    clear: Option<String>,
    /// Clear every address the listing keeps instead, such as --contains @example.com once its server is fixed.
    #[arg(long, conflicts_with = "clear", conflicts_with = "csv")]
    clear_listed: bool,
    /// Only the addresses whose newest report was received at or after this RFC 3339 instant, UTC day or span back such as -7d.
    #[arg(long, conflicts_with = "clear", allow_hyphen_values = true)]
    since: Option<String>,
    /// Only the addresses whose newest report was received before this: what bounced long ago.
    #[arg(long, conflicts_with = "clear", allow_hyphen_values = true)]
    until: Option<String>,
    /// Only the addresses containing this text, case aside, such as @example.com.
    #[arg(long, conflicts_with = "clear")]
    contains: Option<String>,
    /// Write the addresses listed as a CSV table (RFC 4180) to this new file, for a CRM or mailing tool.
    #[arg(long, conflicts_with = "clear")]
    csv: Option<PathBuf>,
}

/// Clear one address, print the addresses listed, or write them as a CSV
/// table to a new file and print what was written.
pub(in crate::cli) fn bounced(
    state: &AppState,
    organization: &str,
    args: BouncedArgs,
) -> Result<(), AppError> {
    if let Some(address) = args.clear {
        return print_json(&state.clear_bounced(organization, &address)?);
    }
    if args.clear_listed {
        let (since, until, contains) = (
            args.since.as_deref(),
            args.until.as_deref(),
            args.contains.as_deref(),
        );
        return print_json(&state.clear_bounced_listed(organization, since, until, contains)?);
    }
    let (since, until, contains) = (
        args.since.as_deref(),
        args.until.as_deref(),
        args.contains.as_deref(),
    );
    let Some(path) = args.csv else {
        return print_json(&state.find_bounced(organization, since, until, contains)?);
    };
    let file = NewFile {
        path: &path,
        exists: "BOUNCED_OUTPUT_EXISTS",
        invalid: "BOUNCED_OUTPUT_INVALID",
    };
    file.refuse_occupied()?;
    let addresses = state.find_bounced(organization, since, until, contains)?;
    let table = bounced_table(&addresses);
    file.write(table.as_bytes())?;
    print_json(
        &serde_json::json!({ "path": path, "addresses": addresses.len(), "size_bytes": table.len() }),
    )
}
