//! Addresses that bounced for good: every recipient a stored delivery status
//! notification (RFC 3464) reports with the action `failed`, read from the
//! stored mail of the organization's mailboxes. Mail to one of them did not
//! arrive, and sending it more harms the mailbox's standing with providers.

use super::super::AppState;
use crate::{error::AppError, mail::RecipientOutcome, models::Message};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

/// One address reported `failed`: how many reports said so, and what the
/// newest of them said.
#[derive(Debug, Clone, Serialize)]
pub struct BouncedAddress {
    /// The address, lower case, as the report's `Final-Recipient` names it
    /// after its address type (`rfc822;`).
    pub address: String,
    pub reports: usize,
    pub last_report_id: Uuid,
    pub last_reported_at: String,
    pub status: Option<String>,
    pub diagnostic: Option<String>,
}

impl AppState {
    /// Every address a stored report says delivery to failed for good, in
    /// address order. Reports not imported yet, those that state no
    /// recipient, and those received before the address was cleared name
    /// nobody.
    pub fn bounced_addresses(
        &self,
        organization_id: &str,
    ) -> Result<Vec<BouncedAddress>, AppError> {
        let messages = self
            .database
            .list_messages(organization_id, None, None, None)?;
        let cleared = self.database.bounce_clearances(organization_id)?;
        // Each address's reports, newest first, as the stored mail is listed.
        // Both instants are stored by Skrzynka as RFC 3339 in UTC, so their
        // text orders as their time does.
        let mut reported: BTreeMap<String, Vec<(&Message, &RecipientOutcome)>> = BTreeMap::new();
        for message in &messages {
            for (address, outcome) in failed_recipients(message) {
                if cleared
                    .get(&address)
                    .is_some_and(|at| message.received_at <= *at)
                {
                    continue;
                }
                reported
                    .entry(address)
                    .or_default()
                    .push((message, outcome));
            }
        }
        Ok(reported
            .into_iter()
            .filter_map(|(address, reports)| {
                let (newest, outcome) = *reports.first()?;
                Some(BouncedAddress {
                    address,
                    reports: reports.len(),
                    last_report_id: newest.id,
                    last_reported_at: newest.received_at.clone(),
                    status: outcome.status.clone(),
                    diagnostic: outcome.diagnostic.clone(),
                })
            })
            .collect())
    }

    /// `OUTBOUND_RECIPIENT_BOUNCED` when a stored bounce report says delivery
    /// to an address of `addressed` (lower case) failed for good, naming each
    /// one and the report; nothing is claimed. `allow_bounced` sends anyway.
    pub(in crate::service) fn refuse_bounced(
        &self,
        organization_id: &str,
        addressed: &[String],
    ) -> Result<(), AppError> {
        let bounced: Vec<String> = self
            .bounced_addresses(organization_id)?
            .into_iter()
            .filter(|bounced| addressed.contains(&bounced.address))
            .map(|bounced| {
                format!(
                    "{} (report {}, {})",
                    bounced.address, bounced.last_report_id, bounced.last_reported_at
                )
            })
            .collect();
        if bounced.is_empty() {
            return Ok(());
        }
        Err(AppError::conflict(
            "OUTBOUND_RECIPIENT_BOUNCED",
            format!(
                "delivery to {} failed for good, as a stored bounce report says; leave the address out, or send anyway with allow_bounced (--allow-bounced)",
                bounced.join(", ")
            ),
        ))
    }

    /// The addresses `bounced_addresses` lists, only those whose newest
    /// report was received at or after `since` and before `until` (each an
    /// RFC 3339 instant, a `YYYY-MM-DD` day or a span back such as `-7d`)
    /// when given, what bounced lately or long ago, and only those
    /// containing `contains`, case aside, such as one domain. An unreadable
    /// instant, a window whose start is not before its end, or a blank
    /// `contains` is `MESSAGE_FILTER_INVALID`.
    pub fn find_bounced(
        &self,
        organization_id: &str,
        since: Option<&str>,
        until: Option<&str>,
        contains: Option<&str>,
    ) -> Result<Vec<BouncedAddress>, AppError> {
        let contains = crate::service::messages::filter_value(contains, "contains")?;
        let from = super::super::filters::instant(since, "since")?;
        let to = super::super::filters::instant(until, "until")?;
        if let (Some(from), Some(to)) = (from, to) {
            if from >= to {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("since {from} is not before until {to}; no report could have been received between them"),
                ));
            }
        }
        let listed: Vec<BouncedAddress> = self
            .bounced_addresses(organization_id)?
            .into_iter()
            .filter(|entry| {
                contains
                    .as_deref()
                    .is_none_or(|part| entry.address.contains(part))
            })
            .collect();
        if from.is_none() && to.is_none() {
            return Ok(listed);
        }
        let mut inside = Vec::new();
        for entry in listed {
            let newest = chrono::DateTime::parse_from_rfc3339(&entry.last_reported_at).map_err(|error| {
                AppError::internal(format!(
                    "bounce report {} stores the receive instant {:?}, which is not RFC 3339: {error}",
                    entry.last_report_id, entry.last_reported_at
                ))
            })?;
            if from.is_none_or(|from| newest >= from) && to.is_none_or(|to| newest < to) {
                inside.push(entry);
            }
        }
        Ok(inside)
    }

    /// Say `address` works again: the reports received until now no longer
    /// count, so sends to it are no longer refused; a later report counts
    /// again. An address `message bounced` does not list is
    /// `BOUNCE_NOT_FOUND`, naming the ones it does.
    pub fn clear_bounced(
        &self,
        organization_id: &str,
        address: &str,
    ) -> Result<ClearedBounce, AppError> {
        let address = address.trim().to_lowercase();
        let listed = self.bounced_addresses(organization_id)?;
        if !listed.iter().any(|entry| entry.address == address) {
            let named: Vec<&str> = listed.iter().map(|entry| entry.address.as_str()).collect();
            return Err(AppError::new(
                axum::http::StatusCode::NOT_FOUND,
                "BOUNCE_NOT_FOUND",
                format!(
                    "no stored bounce report says delivery to {address:?} failed for good; the addresses that bounced are [{}]",
                    named.join(", ")
                ),
                false,
            ));
        }
        let cleared_at = chrono::Utc::now().to_rfc3339();
        self.database
            .clear_bounced(organization_id, &address, &cleared_at)?;
        Ok(ClearedBounce {
            address,
            cleared_at,
        })
    }

    /// Say every address `find_bounced` keeps for `since`, `until` and
    /// `contains` works again, such as a whole domain whose server is fixed,
    /// at one instant, and answer each cleared. A listing narrowed by none of
    /// them is `MESSAGE_FILTER_INVALID`, so nothing clears every address at
    /// once; one keeping none is `BOUNCE_NOT_FOUND`.
    pub fn clear_bounced_listed(
        &self,
        organization_id: &str,
        since: Option<&str>,
        until: Option<&str>,
        contains: Option<&str>,
    ) -> Result<Vec<ClearedBounce>, AppError> {
        if since.is_none() && until.is_none() && contains.is_none() {
            return Err(AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                "name the addresses to clear by a text they contain or a window, or clear one address by name",
            ));
        }
        let listed = self.find_bounced(organization_id, since, until, contains)?;
        if listed.is_empty() {
            return Err(AppError::new(
                axum::http::StatusCode::NOT_FOUND,
                "BOUNCE_NOT_FOUND",
                "no address that bounced matches; nothing was cleared",
                false,
            ));
        }
        let cleared_at = chrono::Utc::now().to_rfc3339();
        listed
            .into_iter()
            .map(|entry| {
                self.database
                    .clear_bounced(organization_id, &entry.address, &cleared_at)?;
                Ok(ClearedBounce {
                    address: entry.address,
                    cleared_at: cleared_at.clone(),
                })
            })
            .collect()
    }
}

/// What `message bounced --clear` answers: the address and when it was cleared.
#[derive(Debug, Serialize)]
pub struct ClearedBounce {
    pub address: String,
    pub cleared_at: String,
}

/// The addresses `message`, when it is a delivery status notification,
/// reports failed, with the outcome that says so.
fn failed_recipients(message: &Message) -> Vec<(String, &RecipientOutcome)> {
    let Some(report) = message.bounce.as_ref() else {
        return Vec::new();
    };
    report
        .recipients
        .iter()
        .filter(|outcome| {
            outcome
                .action
                .as_deref()
                .is_some_and(|action| action.trim().eq_ignore_ascii_case("failed"))
        })
        .filter_map(|outcome| {
            let written = outcome.recipient.as_deref()?;
            let address = written.rsplit(';').next()?.trim().to_lowercase();
            (!address.is_empty()).then_some((address, outcome))
        })
        .collect()
}
