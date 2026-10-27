//! Reading an export directory back: its manifest, and each file it lists
//! checked against the SHA-256 the export recorded. `message verify`
//! checks a whole directory this way without contacting a provider, and
//! `message restore` checks each file this way before appending it.

use super::super::print_json;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// What `message verify` reads.
#[derive(clap::Args)]
pub(in crate::cli) struct VerifyArgs {
    /// A directory `skrzynka message export` wrote, holding its manifest.json.
    #[arg(long)]
    input: PathBuf,
}

/// One file an export manifest lists, with the digest it was written with.
#[derive(Deserialize)]
pub(super) struct ExportedFile {
    pub(super) file: String,
    pub(super) sha256: String,
}

/// The part of an export manifest that is read back: every original, and
/// the mbox when the export wrote one.
#[derive(Deserialize)]
pub(super) struct ExportManifest {
    pub(super) exported: Vec<ExportedFile>,
    #[serde(default)]
    pub(super) mbox: Option<ExportedFile>,
}

/// Why a listed file does not hold what the export wrote.
pub(super) enum FileFault {
    /// The manifest names something that is not a file of the directory, or
    /// it could not be read.
    Unreadable(String),
    /// It reads, but its SHA-256 is not the one the export recorded.
    Changed(String),
}

/// Read `<input>/manifest.json`; one that cannot be read or is not an
/// export manifest is refused with `code`.
pub(super) fn read_manifest(input: &Path, code: &'static str) -> Result<ExportManifest, AppError> {
    let listing = input.join("manifest.json");
    let written = std::fs::read(&listing).map_err(|error| {
        AppError::invalid(code, format!("{} could not be read: {error}; name a directory `message export` wrote", listing.display()))
    })?;
    serde_json::from_slice(&written)
        .map_err(|error| AppError::invalid(code, format!("{} is not an export manifest: {error}", listing.display())))
}

/// The bytes of one listed file, when they are still what the export wrote.
pub(super) fn checked_file(input: &Path, entry: &ExportedFile) -> Result<Vec<u8>, FileFault> {
    // A name is one file of the export directory, never a path out of it.
    if entry.file.contains(['/', '\\']) || entry.file.starts_with('.') {
        return Err(FileFault::Unreadable(format!("the manifest names {:?}, which is not a file of the export", entry.file)));
    }
    let source = std::fs::read(input.join(&entry.file))
        .map_err(|error| FileFault::Unreadable(format!("{} could not be read: {error}", entry.file)))?;
    let digest = format!("{:x}", Sha256::digest(&source));
    if !digest.eq_ignore_ascii_case(&entry.sha256) {
        return Err(FileFault::Changed(format!(
            "{} has SHA-256 {digest}, not the {} the export recorded; it was changed since",
            entry.file, entry.sha256
        )));
    }
    Ok(source)
}

/// One file that does not hold what the export wrote, and why.
#[derive(Serialize)]
struct Damaged {
    file: String,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct VerifyReport {
    directory: PathBuf,
    /// Every listed file, the mbox included, that still holds what was written.
    verified: Vec<String>,
    damaged: Vec<Damaged>,
}

/// Check every file an export's manifest lists, the mbox included, print
/// what holds and what does not, and end with `EXPORT_DAMAGED` when any
/// does not, so a script cannot take a damaged backup for a whole one.
pub(in crate::cli) fn verify_export(VerifyArgs { input }: VerifyArgs) -> Result<(), AppError> {
    let manifest = read_manifest(&input, "EXPORT_INPUT_INVALID")?;
    let mut report = VerifyReport { directory: input.clone(), verified: Vec::new(), damaged: Vec::new() };
    for entry in manifest.exported.iter().chain(manifest.mbox.as_ref()) {
        match checked_file(&input, entry) {
            Ok(_) => report.verified.push(entry.file.clone()),
            Err(FileFault::Unreadable(message)) => {
                report.damaged.push(Damaged { file: entry.file.clone(), code: "EXPORT_FILE_MISSING", message })
            }
            Err(FileFault::Changed(message)) => {
                report.damaged.push(Damaged { file: entry.file.clone(), code: "EXPORT_FILE_CHANGED", message })
            }
        }
    }
    print_json(&report)?;
    if report.damaged.is_empty() {
        return Ok(());
    }
    Err(AppError::invalid(
        "EXPORT_DAMAGED",
        format!(
            "{} of the export's files do not hold what the export wrote; each is listed under damaged",
            report.damaged.len()
        ),
    ))
}
