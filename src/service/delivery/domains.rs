//! A mailbox's originated mail by recipient domain: how many recipients at
//! each domain the sends ended `sent` or `failed` for, and how many of them
//! a stored bounce report returned. Read beside each other, they show which
//! receiving provider turns the mailbox's mail away. Also the report's days
//! and domains as the CSV tables `mailbox delivery --csv` writes.

use super::super::{
    tables::{header, row},
    AppState,
};
use super::DeliveryReport;
use crate::{error::AppError, mail::StoredBounce, models::DeliveryStatus};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

/// One recipient domain's share of the mailbox's originated mail.
#[derive(Debug, Serialize)]
pub struct DomainDelivery {
    /// The part of the recipient addresses after `@`, in lower case.
    pub domain: String,
    /// Recipients (To, Cc and Bcc) at the domain of sends that ended `sent`.
    pub sent: u64,
    /// Recipients at the domain of sends that ended `failed`.
    pub failed: u64,
    /// Recipients at the domain a stored bounce report about one of the
    /// mailbox's originated messages says delivery to failed.
    pub bounced: u64,
}

/// The recipients seen at one domain, by how their send ended.
#[derive(Default)]
struct Tally<'a> {
    sent: Vec<&'a str>,
    failed: Vec<&'a str>,
    bounced: Vec<&'a str>,
}

impl AppState {
    /// Every recipient domain of the mailbox's originated mail claimed in
    /// `from`..`to`, most sent first, with the outbound bounces `bounces`
    /// holds counted against them.
    pub(super) fn delivery_domains(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        bounces: &[StoredBounce],
    ) -> Result<Vec<DomainDelivery>, AppError> {
        let rows = self.database.list_outbound(organization_id, Some(mailbox_id), None, None)?;
        let mut tallies: BTreeMap<String, Tally<'_>> = BTreeMap::new();
        for row in &rows {
            let claimed = DateTime::parse_from_rfc3339(&row.created_at)
                .map_err(|error| {
                    AppError::internal(format!(
                        "outbound message {} stores claim instant {:?}, which is not RFC 3339: {error}",
                        row.id, row.created_at
                    ))
                })?
                .with_timezone(&Utc);
            if from.is_some_and(|start| claimed < start) || to.is_some_and(|end| claimed >= end) {
                continue;
            }
            let addressed = [Some(row.recipients.as_str()), row.cc.as_deref(), row.bcc.as_deref()]
                .into_iter()
                .flatten()
                .flat_map(|header| header.split(','));
            for written in addressed {
                let Some(domain) = domain_of(written) else { continue };
                let tally = tallies.entry(domain).or_default();
                match row.status {
                    DeliveryStatus::Sent => tally.sent.push(written),
                    DeliveryStatus::Failed => tally.failed.push(written),
                    DeliveryStatus::Pending | DeliveryStatus::Sending | DeliveryStatus::Uncertain => {}
                }
            }
        }
        for bounce in bounces.iter().filter(|bounce| bounce.kind == "outbound") {
            let failed = bounce.action.as_deref().is_some_and(|action| action.trim().eq_ignore_ascii_case("failed"));
            let named = bounce.recipient.as_deref().and_then(|written| written.rsplit(';').next());
            if let (true, Some(address)) = (failed, named) {
                if let Some(domain) = domain_of(address) {
                    tallies.entry(domain).or_default().bounced.push(address);
                }
            }
        }
        let mut listed: Vec<DomainDelivery> = tallies
            .into_iter()
            .map(|(domain, tally)| DomainDelivery {
                domain,
                sent: tally.sent.len() as u64,
                failed: tally.failed.len() as u64,
                bounced: tally.bounced.len() as u64,
            })
            .collect();
        listed.sort_by(|left, right| right.sent.cmp(&left.sent).then_with(|| left.domain.cmp(&right.domain)));
        Ok(listed)
    }
}

/// The domain of one address as a header writes it (`Name <a@b>` or `a@b`),
/// lower case; none for an entry without `@`.
fn domain_of(written: &str) -> Option<String> {
    let address = written.trim().trim_end_matches('>');
    let (_, domain) = address.rsplit_once('@')?;
    let domain = domain.trim().to_lowercase();
    (!domain.is_empty()).then_some(domain)
}

impl DeliveryReport {
    /// The report's days as a CSV table (RFC 4180) under the header `day,
    /// total, sent, failed, uncertain, in_flight, bounced`, oldest first: a
    /// warm-up read in a spreadsheet.
    pub fn day_table(&self) -> String {
        let mut table = header(&["day", "total", "sent", "failed", "uncertain", "in_flight", "bounced"]);
        for day in &self.days {
            let counts = [day.total, day.sent, day.failed, day.uncertain, day.in_flight, day.bounced];
            let mut fields = vec![day.day.clone()];
            fields.extend(counts.iter().map(u64::to_string));
            table.push_str(&row(&fields));
        }
        table
    }

    /// The report's recipient domains as a CSV table under the header
    /// `domain, sent, failed, bounced`, most sent first.
    pub fn domain_table(&self) -> String {
        let mut table = header(&["domain", "sent", "failed", "bounced"]);
        for domain in &self.domains {
            table.push_str(&row(&[domain.domain.clone(), domain.sent.to_string(), domain.failed.to_string(), domain.bounced.to_string()]));
        }
        table
    }
}
