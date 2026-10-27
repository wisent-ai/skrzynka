//! Many suppression entries at once, from a text such as an exported
//! do-not-contact list: one entry per line, its reason after it; and the
//! list written back as that same text, so it can be kept or imported
//! elsewhere.

use super::{super::AppState, SuppressionFilter};
use crate::{
    error::AppError,
    mail::{suppression_entry, Suppression},
};

/// What `message suppressed --import` did: every entry added or whose
/// reason was replaced, in line order.
#[derive(Debug, serde::Serialize)]
pub struct ImportedSuppressions {
    pub entries: Vec<Suppression>,
}

impl AppState {
    /// Suppress every entry `text` holds: one per line, an address or
    /// `@domain`, then optionally whitespace and the reason for that line;
    /// a line without one takes `reason`. Blank lines and lines starting `#`
    /// are skipped. Every line is read before any is kept, so one unreadable
    /// entry is `SUPPRESSION_INVALID` quoting its line and nothing changes;
    /// a text with no entry is refused the same way.
    pub fn import_suppressions(
        &self,
        organization_id: &str,
        text: &str,
        reason: Option<&str>,
    ) -> Result<ImportedSuppressions, AppError> {
        let reason = reason.map(str::trim);
        if reason.is_some_and(str::is_empty) {
            return Err(AppError::invalid(
                "SUPPRESSION_INVALID",
                "the reason for the imported entries is empty; say why, or leave the reason out",
            ));
        }
        let mut read = Vec::new();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (written, own) = match line.split_once(char::is_whitespace) {
                Some((written, own)) => (written, Some(own.trim())),
                None => (line, None),
            };
            let entry = suppression_entry(written).map_err(|refusal| {
                AppError::invalid(
                    "SUPPRESSION_INVALID",
                    format!("the import line {line:?}: {}; nothing was imported", refusal.message),
                )
            })?;
            read.push((entry, own.filter(|own| !own.is_empty()).or(reason)));
        }
        if read.is_empty() {
            return Err(AppError::invalid(
                "SUPPRESSION_INVALID",
                "the import holds no entry; write one address or @domain per line",
            ));
        }
        let mut entries = Vec::new();
        for (entry, reason) in read {
            entries.push(self.database.suppress(organization_id, &entry, reason)?);
        }
        Ok(ImportedSuppressions { entries })
    }

    /// The entries `filter` keeps as the text `import_suppressions` reads:
    /// one entry per line in entry order, its reason after a space when it
    /// has one, under a `#` line saying what the file is.
    pub fn suppression_text(&self, organization_id: &str, filter: &SuppressionFilter) -> Result<String, AppError> {
        let mut text = String::from("# Skrzynka suppression list: an address or @domain per line, its reason after it\n");
        for kept in self.find_suppressions(organization_id, filter)? {
            text.push_str(&kept.entry);
            if let Some(reason) = kept.reason.as_deref() {
                text.push(' ');
                text.push_str(reason);
            }
            text.push('\n');
        }
        Ok(text)
    }
}
