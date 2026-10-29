//! Many blocked senders at once, from a text such as a spam list exported
//! from another mail program: one address or `@domain` per line; and the
//! list written back as that same text.

use super::{super::AppState, blocked_entry};
use crate::error::AppError;
use serde::Serialize;

/// What `message blocked --import` did: the entries the text named, in line
/// order, once each, and every blocked entry after it.
#[derive(Debug, Serialize)]
pub struct ImportedBlocks {
    pub imported: Vec<String>,
    pub blocked: Vec<String>,
}

impl AppState {
    /// Block every entry `text` holds: one per line, an address or
    /// `@domain`; anything after the entry on its line is a note for the
    /// reader and is not kept. Blank lines and lines starting `#` are
    /// skipped, and an entry already blocked stays blocked. Every line is
    /// read before any is kept, so one unreadable entry is
    /// `BLOCKED_SENDER_INVALID` quoting its line and nothing changes; a text
    /// with no entry is refused the same way.
    pub fn import_blocked(
        &self,
        organization_id: &str,
        text: &str,
    ) -> Result<ImportedBlocks, AppError> {
        let mut imported: Vec<String> = Vec::new();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let written = match line.split_once(char::is_whitespace) {
                Some((written, _)) => written,
                None => line,
            };
            let entry = blocked_entry(written).map_err(|refusal| {
                AppError::invalid(
                    "BLOCKED_SENDER_INVALID",
                    format!(
                        "the import line {line:?}: {}; nothing was imported",
                        refusal.message
                    ),
                )
            })?;
            if !imported.contains(&entry) {
                imported.push(entry);
            }
        }
        if imported.is_empty() {
            return Err(AppError::invalid(
                "BLOCKED_SENDER_INVALID",
                "the import holds no entry; write one address or @domain per line",
            ));
        }
        let already = self.database.blocked_senders(organization_id)?;
        for entry in imported.iter().filter(|entry| !already.contains(entry)) {
            self.database.block_sender(organization_id, entry)?;
        }
        Ok(ImportedBlocks {
            imported,
            blocked: self.database.blocked_senders(organization_id)?,
        })
    }

    /// Every blocked entry, or those holding `contains`, as the text
    /// `import_blocked` reads: one per line in address order, under a `#`
    /// line saying what the file is.
    pub fn blocked_text(
        &self,
        organization_id: &str,
        contains: Option<&str>,
    ) -> Result<String, AppError> {
        let mut text = String::from("# Skrzynka blocked senders: an address or @domain per line\n");
        for entry in self.blocked_senders(organization_id, contains)?.blocked {
            text.push_str(&entry);
            text.push('\n');
        }
        Ok(text)
    }
}
