//! Checking recipients before writing to them: for each address, whether
//! the suppression list covers it and whether a stored bounce report says
//! delivery to it failed for good — the two reasons a send is refused.

use super::super::{delivery::BouncedAddress, outbound::normalize_addresses, AppState};
use crate::{error::AppError, mail::Suppression};
use serde::Serialize;

/// One address as a send would meet it.
#[derive(Debug, Serialize)]
pub struct RecipientCheck {
    /// The address as written, trimmed.
    pub address: String,
    /// The entry that covers it, itself or by its domain: a send naming it is
    /// `OUTBOUND_RECIPIENT_SUPPRESSED`, with no override.
    pub suppressed: Option<Suppression>,
    /// What the stored reports say about it: a send naming it is
    /// `OUTBOUND_RECIPIENT_BOUNCED` unless `allow_bounced` is given.
    pub bounced: Option<BouncedAddress>,
    /// Whether a send naming only this address would pass both checks.
    pub sendable: bool,
}

impl AppState {
    /// Each address of `written` (comma-separated values allowed, repeats
    /// once), in the order given, as a send would meet it; nothing is sent
    /// or claimed. An address a send would refuse as unreadable is
    /// `OUTBOUND_RECIPIENT_INVALID`, and so is naming none.
    pub fn check_recipients(
        &self,
        organization_id: &str,
        written: &[String],
    ) -> Result<Vec<RecipientCheck>, AppError> {
        let addresses = normalize_addresses(written)?;
        if addresses.is_empty() {
            return Err(AppError::invalid(
                "OUTBOUND_RECIPIENT_INVALID",
                "name at least one address to check",
            ));
        }
        let held = self.suppressions(organization_id)?;
        let bounced = self.bounced_addresses(organization_id)?;
        Ok(addresses
            .into_iter()
            .map(|address| {
                let lower = address.to_lowercase();
                let suppressed = held.iter().find(|kept| kept.covers(&lower)).cloned();
                let reported = bounced.iter().find(|entry| entry.address == lower).cloned();
                RecipientCheck {
                    sendable: suppressed.is_none() && reported.is_none(),
                    address,
                    suppressed,
                    bounced: reported,
                }
            })
            .collect())
    }
}
