//! The people a mailbox corresponds with, read from what Skrzynka stored:
//! every address that sent a stored message, and every address originated
//! mail went to. Nothing is kept apart from the mail; the list is derived on
//! each read, so it always agrees with the messages and outbound rows.

use super::{
    messages::{entry_address, filter_value},
    AppState,
};
use crate::{error::AppError, models::DeliveryStatus};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

mod listed;
mod table;
mod vcard;
pub use table::contact_table;
pub use vcard::vcards;

/// One correspondent: the address (lower case), each display name it was
/// written with, how many stored messages came from it and when the latest
/// was received, how many originated messages not recorded `failed`
/// went to it (To or Cc) and when the latest was claimed, and whether a
/// stored bounce report says delivery to it failed for good.
#[derive(Debug, Serialize)]
pub struct Contact {
    pub address: String,
    pub names: Vec<String>,
    pub received: usize,
    pub last_received_at: Option<String>,
    pub written: usize,
    pub last_written_at: Option<String>,
    /// The address is one `message bounced` lists, read across every mailbox
    /// of the organization: a send to it is refused unless allowed.
    pub bounced: bool,
    /// The address is one `message suppressed` covers, itself or by its
    /// domain: no originated mail may be sent to it.
    pub suppressed: bool,
    /// The address is one `message blocked` lists, itself or by its domain:
    /// listings leave its mail out.
    pub blocked: bool,
}

/// What the read gathers for one address before it is reported.
#[derive(Default)]
struct Tally {
    names: Vec<String>,
    received: Vec<String>,
    written: Vec<String>,
}

/// Which correspondents a contacts read keeps and in what order, as
/// `message contacts` takes them; the default keeps every one, most
/// exchanged first.
#[derive(Debug, Default, Clone)]
pub struct ContactFilter {
    /// Only this mailbox's correspondents.
    pub mailbox_id: Option<Uuid>,
    /// Only those whose address or a name they were written with contains this.
    pub contains: Option<String>,
    /// The latest exchanged with first.
    pub recent: bool,
    /// Only those `message bounced` lists.
    pub bounced: bool,
    /// Only those an entry of `message suppressed` covers.
    pub suppressed: bool,
    /// Only those a blocked entry covers, to review whom listings leave out.
    pub blocked: bool,
    /// Only those exchanged with at or after this RFC 3339 instant or UTC day.
    pub since: Option<String>,
    /// Only those last exchanged with before this RFC 3339 instant or UTC
    /// day: the correspondents who went quiet.
    pub until: Option<String>,
    /// Only those who wrote and were never written to: who waits on an answer.
    pub never_written: bool,
    /// Only those written to who never wrote back: who has not answered.
    pub never_received: bool,
}

impl AppState {
    /// The same correspondents as vCards (RFC 6350), in the listing's order,
    /// for an address book to import.
    pub fn contacts_vcard(
        &self,
        organization_id: &str,
        filter: &ContactFilter,
    ) -> Result<String, AppError> {
        Ok(vcard::vcards(&self.list_contacts(organization_id, filter)?))
    }

    /// Every correspondent of the organization's mailboxes, or of one, most
    /// exchanged first, or with `recent` the latest exchanged with first;
    /// with `contains`, only those whose address or a name they were written
    /// with contains it, case aside, with `bounced` only those a stored
    /// bounce report says delivery to failed for good, with `suppressed`
    /// only those the suppression list covers, with `blocked` only those a
    /// blocked entry covers, with `since` only
    /// those whose latest exchange is at or after it and with `until` only
    /// those whose latest exchange is before it; `never_written` keeps those
    /// who wrote and were never written to and `never_received` those
    /// written to who never wrote. A blank `contains`, an unreadable instant,
    /// a window whose start is not before its end, or both one-way filters
    /// at once is refused with `MESSAGE_FILTER_INVALID`.
    pub fn list_contacts(
        &self,
        organization_id: &str,
        filter: &ContactFilter,
    ) -> Result<Vec<Contact>, AppError> {
        let ContactFilter {
            mailbox_id, recent, ..
        } = *filter;
        if filter.never_written && filter.never_received {
            return Err(AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                "never_written and never_received together keep no one; name one of them",
            ));
        }
        let contains = filter_value(filter.contains.as_deref(), "contains")?;
        let since = super::filters::instant(filter.since.as_deref(), "since")?;
        let until = super::filters::instant(filter.until.as_deref(), "until")?;
        if let (Some(from), Some(to)) = (since, until) {
            if from >= to {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("since {from} is not before until {to}; no latest exchange could fall between them"),
                ));
            }
        }
        let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
        for message in self
            .database
            .list_messages(organization_id, mailbox_id, None, None)?
        {
            tally(&mut tallies, &message.sender)
                .received
                .push(message.received_at);
        }
        for outbound in self
            .database
            .list_outbound(organization_id, mailbox_id, None, None)?
        {
            if outbound.status == DeliveryStatus::Failed {
                continue;
            }
            let addressed = [
                Some(outbound.recipients.as_str()),
                outbound.cc.as_deref(),
                outbound.bcc.as_deref(),
            ];
            for header in addressed.into_iter().flatten() {
                for recipient in header.split(',').filter(|part| !part.trim().is_empty()) {
                    tally(&mut tallies, recipient)
                        .written
                        .push(outbound.created_at.clone());
                }
            }
        }
        let bounced: BTreeSet<String> = self
            .bounced_addresses(organization_id)?
            .into_iter()
            .map(|entry| entry.address)
            .collect();
        let suppressed = self.suppressions(organization_id)?;
        let blocked = self.database.blocked_senders(organization_id)?;
        let mut contacts: Vec<Contact> = tallies
            .into_iter()
            .map(|(address, tally)| Contact {
                bounced: bounced.contains(&address),
                suppressed: suppressed.iter().any(|kept| kept.covers(&address)),
                blocked: blocked.iter().any(|entry| {
                    *entry == address
                        || (entry.starts_with('@') && address.ends_with(entry.as_str()))
                }),
                address,
                names: tally.names,
                received: tally.received.len(),
                last_received_at: tally.received.iter().max().cloned(),
                written: tally.written.len(),
                last_written_at: tally.written.iter().max().cloned(),
            })
            .filter(|contact| contact.bounced || !filter.bounced)
            .filter(|contact| contact.suppressed || !filter.suppressed)
            .filter(|contact| contact.blocked || !filter.blocked)
            .filter(|contact| {
                !filter.never_written
                    || (contact.last_received_at.is_some() && contact.last_written_at.is_none())
            })
            .filter(|contact| {
                !filter.never_received
                    || (contact.last_written_at.is_some() && contact.last_received_at.is_none())
            })
            .filter(|contact| {
                contains.as_deref().is_none_or(|part| {
                    contact.address.contains(part)
                        || contact
                            .names
                            .iter()
                            .any(|name| name.to_lowercase().contains(part))
                })
            })
            .collect();
        if since.is_some() || until.is_some() {
            let mut inside = Vec::new();
            for contact in contacts {
                let exchanged = [&contact.last_received_at, &contact.last_written_at]
                    .into_iter()
                    .flatten()
                    .map(|stored| {
                        chrono::DateTime::parse_from_rfc3339(stored).map_err(|error| {
                            AppError::internal(format!(
                                "contact {} has the stored instant {stored:?}, which is not RFC 3339: {error}",
                                contact.address
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, AppError>>()?;
                let kept = exchanged.into_iter().max().is_some_and(|latest| {
                    since.is_none_or(|from| latest >= from) && until.is_none_or(|to| latest < to)
                });
                if kept {
                    inside.push(contact);
                }
            }
            contacts = inside;
        }
        // Both instants are stored by Skrzynka as RFC 3339 in UTC, so their
        // text orders as their time does.
        let latest = |contact: &Contact| {
            contact
                .last_received_at
                .clone()
                .max(contact.last_written_at.clone())
        };
        contacts.sort_by(|left, right| match recent {
            true => latest(right)
                .cmp(&latest(left))
                .then_with(|| left.address.cmp(&right.address)),
            false => (right.received + right.written)
                .cmp(&(left.received + left.written))
                .then_with(|| left.address.cmp(&right.address)),
        });
        Ok(contacts)
    }
}

/// The tally of the address one header entry names, created on first sight,
/// with the entry's display name recorded when it states one not seen before.
fn tally<'a>(tallies: &'a mut BTreeMap<String, Tally>, written: &str) -> &'a mut Tally {
    let tally = tallies
        .entry(entry_address(written).to_lowercase())
        .or_default();
    if let Some(open) = written.rfind('<') {
        let name = written[..open].trim().trim_matches('"').trim();
        if !name.is_empty() && !tally.names.iter().any(|known| known == name) {
            tally.names.push(name.to_string());
        }
    }
    tally
}
