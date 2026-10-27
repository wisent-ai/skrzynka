//! How a mailbox's sends ended: every reply, originated message and
//! unsubscribe request Skrzynka recorded for it, counted by status, with each
//! refusal counted by its code, and every stored delivery status notification
//! that reported one of them back as undelivered.

use super::{filters::instant, AppState};
use crate::{error::AppError, mail::StoredBounce, models::DeliveryStatus};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

mod bounced;
pub(in crate::service) use bounced::BouncedAddress;
mod domains;
use domains::DomainDelivery;
mod replies;
pub use replies::ReplyFilter;
mod summary;

/// One kind of send and how its records ended.
#[derive(Debug, Default, Serialize)]
pub struct DeliveryKind {
    pub kind: String,
    pub total: u64,
    pub pending: u64,
    pub sending: u64,
    pub sent: u64,
    pub failed: u64,
    pub uncertain: u64,
    /// How many distinct sends of this kind a stored bounce report received
    /// in the window returned: compared with `sent`, the share that did not
    /// arrive, as far as reports tell.
    pub bounced: u64,
    /// Each refusal code the records carry, most frequent first.
    pub refusals: Vec<RefusalCount>,
}

#[derive(Debug, Serialize)]
pub struct RefusalCount {
    pub code: String,
    pub count: u64,
}

/// What `mailbox delivery` answers. `sent` means the provider's server
/// accepted the request. `bounces` lists each recipient outcome of every
/// delivery status notification (RFC 3464) stored in any of the
/// organization's mailboxes whose returned message carries the Message-ID a
/// send went out with, newest first. A bounce that has not been imported yet,
/// one that returns no headers, or a send that reached a spam folder is not
/// seen.
#[derive(Debug, Serialize)]
pub struct DeliveryReport {
    pub mailbox_id: Uuid,
    /// The window counted, as given: sends claimed and reports received at
    /// or after `since` and before `until`; absent for no bound.
    pub since: Option<String>,
    pub until: Option<String>,
    pub kinds: Vec<DeliveryKind>,
    /// Every UTC day a send was claimed on, oldest first, every kind together.
    pub days: Vec<DeliveryDay>,
    /// Every recipient domain of the originated mail, most sent first.
    pub domains: Vec<DomainDelivery>,
    pub bounces: Vec<StoredBounce>,
}

/// One UTC day of a mailbox's sending, every kind together: how many sends
/// were claimed on it and how each ended, and how many distinct sends the
/// bounce reports received that day returned. Read beside a warm-up, it
/// shows whether raising the limit raised the bounces.
#[derive(Debug, Default, Serialize)]
pub struct DeliveryDay {
    pub day: String,
    pub total: u64,
    pub sent: u64,
    pub failed: u64,
    pub uncertain: u64,
    pub in_flight: u64,
    pub bounced: u64,
}

impl AppState {
    /// How the mailbox's sends ended, those claimed within `since`..`until`
    /// (RFC 3339 instants or `YYYY-MM-DD` days, as `message list` takes them;
    /// either may be absent), with the bounce reports received in it. An
    /// unreadable bound, or `since` not before `until`, is
    /// `MESSAGE_FILTER_INVALID`.
    pub fn mailbox_delivery(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        since: Option<&str>,
        until: Option<&str>,
    ) -> Result<DeliveryReport, AppError> {
        let since = instant(since, "since")?;
        let until = instant(until, "until")?;
        if let (Some(from), Some(to)) = (since, until) {
            if from >= to {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("since {from} is not before until {to}; nothing could be sent between them"),
                ));
            }
        }
        let window = (since.map(|at| at.to_rfc3339()), until.map(|at| at.to_rfc3339()));
        let mut kinds: Vec<DeliveryKind> = Vec::new();
        for (kind, status, code, count) in self.database.delivery_counts(organization_id, mailbox_id, window.clone())? {
            let place = match kinds.iter().position(|entry| entry.kind == kind) {
                Some(place) => place,
                None => {
                    let place = kinds.len();
                    kinds.push(DeliveryKind {
                        kind,
                        ..DeliveryKind::default()
                    });
                    place
                }
            };
            let entry = &mut kinds[place];
            entry.total += count;
            match status {
                DeliveryStatus::Pending => entry.pending += count,
                DeliveryStatus::Sending => entry.sending += count,
                DeliveryStatus::Sent => entry.sent += count,
                DeliveryStatus::Failed => entry.failed += count,
                DeliveryStatus::Uncertain => entry.uncertain += count,
            }
            if let Some(code) = code {
                match entry.refusals.iter_mut().find(|refusal| refusal.code == code) {
                    Some(refusal) => refusal.count += count,
                    None => entry.refusals.push(RefusalCount { code, count }),
                }
            }
        }
        for entry in &mut kinds {
            entry
                .refusals
                .sort_by(|left, right| right.count.cmp(&left.count));
        }
        let bounces = self.database.mailbox_bounces(organization_id, mailbox_id, window.clone())?;
        for entry in &mut kinds {
            let returned: BTreeSet<&str> = bounces
                .iter()
                .filter(|bounce| bounce.kind == entry.kind)
                .map(|bounce| bounce.send_id.as_str())
                .collect();
            entry.bounced = returned.len() as u64;
        }
        let mut days: BTreeMap<String, DeliveryDay> = BTreeMap::new();
        for (day, status, count) in self.database.delivery_days(organization_id, mailbox_id, window.clone())? {
            let entry = days.entry(day.clone()).or_insert_with(|| DeliveryDay { day, ..DeliveryDay::default() });
            entry.total += count;
            match status {
                DeliveryStatus::Sent => entry.sent += count,
                DeliveryStatus::Failed => entry.failed += count,
                DeliveryStatus::Uncertain => entry.uncertain += count,
                DeliveryStatus::Pending | DeliveryStatus::Sending => entry.in_flight += count,
            }
        }
        // A report's day is the UTC day it was received on: the RFC 3339
        // instant Skrzynka stored, cut to its date.
        let mut returned: BTreeMap<String, BTreeSet<(&str, &str)>> = BTreeMap::new();
        for bounce in &bounces {
            let day = chrono::DateTime::parse_from_rfc3339(&bounce.received_at)
                .map_err(|error| {
                    AppError::internal(format!(
                        "bounce report {} stores the receive instant {:?}, which is not RFC 3339: {error}",
                        bounce.report_message_id, bounce.received_at
                    ))
                })?
                .with_timezone(&chrono::Utc)
                .date_naive()
                .to_string();
            returned.entry(day).or_default().insert((bounce.kind.as_str(), bounce.send_id.as_str()));
        }
        for (day, sends) in returned {
            let entry = days.entry(day.clone()).or_insert_with(|| DeliveryDay { day, ..DeliveryDay::default() });
            entry.bounced = sends.len() as u64;
        }
        let domains = self.delivery_domains(organization_id, mailbox_id, since, until, &bounces)?;
        Ok(DeliveryReport {
            mailbox_id,
            since: window.0,
            until: window.1,
            kinds,
            days: days.into_values().collect(),
            domains,
            bounces,
        })
    }
}
