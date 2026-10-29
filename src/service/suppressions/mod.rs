//! Recipients no originated mail may be sent to: an address, or `@domain`
//! for every address of a domain, kept with why, such as a person who asked
//! not to be written to. A send, forward, draft send or schedule naming one
//! is refused before anything is claimed; there is no override, because the
//! list is the operator's own word. Replies are not held to it.

use super::AppState;
use crate::{
    error::AppError,
    mail::{suppression_entry, Suppression},
};
use axum::http::StatusCode;

mod filter;
mod import;
mod recipients;
mod table;
pub use filter::SuppressionFilter;
pub use import::ImportedSuppressions;
pub use table::suppression_table;

impl AppState {
    /// Every suppressed entry of the organization, in entry order.
    pub fn suppressions(&self, organization_id: &str) -> Result<Vec<Suppression>, AppError> {
        self.database.suppressions(organization_id)
    }

    /// The suppressed entries `filter` admits, in entry order: those whose
    /// entry or reason contains a text, case aside, or those added within a
    /// window. A blank text, an unreadable instant or a backwards window is
    /// `MESSAGE_FILTER_INVALID`.
    pub fn find_suppressions(
        &self,
        organization_id: &str,
        filter: &SuppressionFilter,
    ) -> Result<Vec<Suppression>, AppError> {
        filter.narrow(self.suppressions(organization_id)?)
    }

    /// Suppress an address or `@domain`, stored trimmed and in lower case,
    /// with why; suppressing one already there replaces its reason. A reason
    /// with no visible text is `SUPPRESSION_INVALID`.
    pub fn suppress(
        &self,
        organization_id: &str,
        written: &str,
        reason: Option<&str>,
    ) -> Result<Suppression, AppError> {
        let entry = suppression_entry(written)?;
        let reason = reason.map(str::trim);
        if reason.is_some_and(str::is_empty) {
            return Err(AppError::invalid(
                "SUPPRESSION_INVALID",
                format!(
                    "the reason for suppressing {entry} is empty; say why, or leave the reason out"
                ),
            ));
        }
        self.database.suppress(organization_id, &entry, reason)
    }

    /// Take an entry off the list and answer what it was; one the list does
    /// not hold is `SUPPRESSION_NOT_FOUND`, naming the entries it does.
    pub fn unsuppress(
        &self,
        organization_id: &str,
        written: &str,
    ) -> Result<Suppression, AppError> {
        let entry = suppression_entry(written)?;
        match self.database.unsuppress(organization_id, &entry)? {
            Some(removed) => Ok(removed),
            None => {
                let held: Vec<String> = self
                    .suppressions(organization_id)?
                    .into_iter()
                    .map(|kept| kept.entry)
                    .collect();
                Err(AppError::new(
                    StatusCode::NOT_FOUND,
                    "SUPPRESSION_NOT_FOUND",
                    format!(
                        "{entry} is not suppressed; suppressed are [{}]",
                        held.join(", ")
                    ),
                    false,
                ))
            }
        }
    }

    /// `OUTBOUND_RECIPIENT_SUPPRESSED` when an address of `addressed` (lower
    /// case) is suppressed, itself or by its domain, naming each with its
    /// entry and reason; `doing` says what was refused. Nothing is claimed.
    pub(super) fn refuse_suppressed(
        &self,
        organization_id: &str,
        addressed: &[String],
        doing: &str,
    ) -> Result<(), AppError> {
        let held = self.suppressions(organization_id)?;
        let named: Vec<String> = addressed
            .iter()
            .filter_map(|address| {
                held.iter()
                    .find(|kept| kept.covers(address))
                    .map(|kept| match &kept.reason {
                        Some(reason) => format!("{address} ({}: {reason})", kept.entry),
                        None => format!("{address} ({})", kept.entry),
                    })
            })
            .collect();
        if named.is_empty() {
            return Ok(());
        }
        Err(AppError::conflict(
            "OUTBOUND_RECIPIENT_SUPPRESSED",
            format!(
                "{doing} names {}, which the suppression list says no mail may be sent to; leave the address out, or take it off the list with `skrzynka message suppressed --remove <ENTRY>`",
                named.join(", ")
            ),
        ))
    }
}
