//! Recipients no originated mail may be sent to: an address, or `@domain`
//! for every address of a domain, kept with why.

use crate::error::AppError;
use serde::Serialize;

/// One entry no originated mail may be addressed to.
#[derive(Debug, Clone, Serialize)]
pub struct Suppression {
    /// A whole address, or `@domain`, in lower case.
    pub entry: String,
    /// Why, as the operator wrote it; absent when none was given.
    pub reason: Option<String>,
    pub added_at: String,
}

impl Suppression {
    /// Whether this entry suppresses `address` (lower case): it is the
    /// address, or the `@domain` the address ends with.
    pub fn covers(&self, address: &str) -> bool {
        self.entry == address || (self.entry.starts_with('@') && address.ends_with(self.entry.as_str()))
    }
}

/// An entry as stored: a whole address, or `@domain`, trimmed and lower case.
/// Anything else is `SUPPRESSION_INVALID`.
pub fn suppression_entry(written: &str) -> Result<String, AppError> {
    let entry = written.trim().to_lowercase();
    let valid = match entry.split_once('@') {
        Some((_, domain)) => {
            !domain.is_empty()
                && !domain.contains('@')
                && !entry.chars().any(|character| character.is_whitespace() || character == ',' || character == '<')
        }
        None => false,
    };
    if !valid {
        return Err(AppError::invalid(
            "SUPPRESSION_INVALID",
            format!("{written:?} is neither an address nor @domain; write one, such as person@example.com or @example.com"),
        ));
    }
    Ok(entry)
}
