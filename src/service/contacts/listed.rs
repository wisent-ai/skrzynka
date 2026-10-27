//! Putting the correspondents a narrowed contacts listing keeps on a list at
//! once: the suppression list, so no originated mail goes to them again (such
//! as everyone written to who never wrote back and went quiet), or the
//! blocked senders, so listings leave their mail out (such as every address
//! of a domain that only sends noise).

use super::{super::AppState, ContactFilter};
use crate::{error::AppError, mail::Suppression};
use serde::Serialize;

/// What `message contacts --suppress-listed` did: every entry added, in the
/// listing's order.
#[derive(Debug, Serialize)]
pub struct SuppressedContacts {
    pub entries: Vec<Suppression>,
}

/// What `message contacts --block-listed` did: every address blocked, in the
/// listing's order.
#[derive(Debug, Serialize)]
pub struct BlockedContacts {
    pub blocked: Vec<String>,
}

impl AppState {
    /// Suppress the address of every correspondent `filter` keeps that the
    /// list does not cover yet, with `reason` when given. A listing narrowed
    /// by no text, window, one-way filter, bounce or block is
    /// `SUPPRESSION_INVALID` before anything is read, so nothing suppresses
    /// everyone a mailbox corresponds with at once; so is a reason with no
    /// visible text. A listing keeping no correspondent the list does not
    /// cover yet is `NOT_FOUND`. Every entry is checked before any is kept.
    pub fn suppress_contacts(
        &self,
        organization_id: &str,
        filter: &ContactFilter,
        reason: Option<&str>,
    ) -> Result<SuppressedContacts, AppError> {
        narrowed(filter, "SUPPRESSION_INVALID", "suppresses")?;
        let reason = reason.map(str::trim);
        if reason.is_some_and(str::is_empty) {
            return Err(AppError::invalid(
                "SUPPRESSION_INVALID",
                "the reason for the suppressed contacts is empty; say why, or leave the reason out",
            ));
        }
        let addresses = self.listed(organization_id, filter, |contact| contact.suppressed, "contact not yet suppressed")?;
        let entries = addresses
            .iter()
            .map(|address| crate::mail::suppression_entry(address))
            .collect::<Result<Vec<_>, _>>()?;
        let mut kept = Vec::new();
        for entry in entries {
            kept.push(self.database.suppress(organization_id, &entry, reason)?);
        }
        Ok(SuppressedContacts { entries: kept })
    }

    /// Block the address of every correspondent `filter` keeps that no
    /// blocked entry covers yet. A listing narrowed by no text, window,
    /// one-way filter, bounce or block is `BLOCKED_SENDER_INVALID` before anything
    /// is read, so nothing hides all of a mailbox's mail at once; one keeping
    /// no correspondent not blocked yet is `NOT_FOUND`. Every entry is checked
    /// before any is kept.
    pub fn block_contacts(&self, organization_id: &str, filter: &ContactFilter) -> Result<BlockedContacts, AppError> {
        narrowed(filter, "BLOCKED_SENDER_INVALID", "blocks")?;
        let addresses = self.listed(organization_id, filter, |contact| contact.blocked, "contact not yet blocked")?;
        let entries = addresses
            .iter()
            .map(|address| super::super::blocked::blocked_entry(address))
            .collect::<Result<Vec<_>, _>>()?;
        for entry in &entries {
            self.database.block_sender(organization_id, entry)?;
        }
        Ok(BlockedContacts { blocked: entries })
    }

    /// The addresses `filter` keeps that `covered` says are not on the list
    /// yet, in the listing's order; none is `NOT_FOUND` naming `what`.
    fn listed(
        &self,
        organization_id: &str,
        filter: &ContactFilter,
        covered: fn(&super::Contact) -> bool,
        what: &'static str,
    ) -> Result<Vec<String>, AppError> {
        let addresses: Vec<String> = self
            .list_contacts(organization_id, filter)?
            .into_iter()
            .filter(|contact| !covered(contact))
            .map(|contact| contact.address)
            .collect();
        if addresses.is_empty() {
            return Err(AppError::not_found(what));
        }
        Ok(addresses)
    }
}

/// Refuse with `code` a listing narrowed by no text, window, one-way filter,
/// bounce or block: putting everyone a mailbox corresponds with on a list is
/// never what was meant.
fn narrowed(filter: &ContactFilter, code: &'static str, acts: &str) -> Result<(), AppError> {
    let bounded = [&filter.contains, &filter.since, &filter.until].iter().any(|bound| bound.is_some());
    if bounded || filter.never_written || filter.never_received || filter.bounced || filter.blocked {
        return Ok(());
    }
    Err(AppError::invalid(
        code,
        format!("narrow the contacts by text, window, one-way filter, bounce or block first; nothing {acts} everyone at once"),
    ))
}
