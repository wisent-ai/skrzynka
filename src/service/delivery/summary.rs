//! What a mailbox holds, counted from the messages Skrzynka stored for it:
//! how many there are, how many no reply was sent to, how many are put aside
//! (snoozed, from blocked senders, of muted conversations), tagged, noted, carry files, are bounce
//! reports or failed the receiving provider's sender checks, from how many
//! senders, and the first and last instants they were received. Each count
//! uses the predicate the matching `message list` filter uses. Its days are
//! also the CSV table `mailbox summary --days-csv` writes.

use super::super::blocked::is_blocked;
use super::super::filters::{carries_files, doubts_sender, instant, received_instant};
use super::super::snoozes::{self, is_snoozed};
use super::super::threads::mutes::is_muted;
use super::super::{tables, AppState, MessageFilters};
use crate::{error::AppError, models::Message};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

/// One mailbox's stored mail, counted.
#[derive(Debug, Serialize)]
pub struct MailboxSummary {
    pub mailbox_id: Uuid,
    /// The receive window counted, as the instants the bounds stood for
    /// (a span such as `-7d` read when the summary was); absent for no bound.
    pub since: Option<String>,
    pub until: Option<String>,
    pub messages: usize,
    pub unanswered: usize,
    pub snoozed: usize,
    pub blocked: usize,
    pub muted: usize,
    pub tagged: usize,
    pub noted: usize,
    pub with_attachments: usize,
    /// The recorded sizes of every file the counted mail carries, added up;
    /// a file whose size was not recorded adds nothing.
    pub file_bytes: u64,
    pub bounces: usize,
    pub failed_authentication: usize,
    pub senders: usize,
    pub first_received_at: Option<String>,
    pub last_received_at: Option<String>,
    /// How many of the counted messages were received on each UTC day that
    /// received any, oldest first: the mailbox's inbound volume over time.
    pub days: Vec<ReceivedDay>,
}

/// One UTC day's received mail.
#[derive(Debug, Serialize)]
pub struct ReceivedDay {
    /// `YYYY-MM-DD`.
    pub day: String,
    pub received: usize,
}

impl AppState {
    /// Count the mailbox's stored mail now, or that received within
    /// `since`..`until` (as `message list --since --until` takes them);
    /// nothing is kept apart from it and no provider is contacted. An unknown
    /// mailbox is `NOT_FOUND`, an unreadable window `MESSAGE_FILTER_INVALID`.
    pub fn mailbox_summary(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        since: Option<&str>,
        until: Option<&str>,
    ) -> Result<MailboxSummary, AppError> {
        self.database.get_mailbox(organization_id, mailbox_id)?;
        let every = MessageFilters {
            include_blocked: true,
            include_snoozed: true,
            include_muted: true,
            since,
            until,
            ..MessageFilters::default()
        };
        let messages = self.list_messages(organization_id, Some(mailbox_id), every, None, None)?;
        let answered = self.database.answered_messages(organization_id)?;
        let blocked = self.database.blocked_senders(organization_id)?;
        let muted = self.database.muted_identities(organization_id)?;
        let now = snoozes::instant(chrono::Utc::now());
        let count = |kept: &dyn Fn(&Message) -> bool| {
            messages.iter().filter(|message| kept(message)).count()
        };
        let senders: HashSet<String> = messages
            .iter()
            .map(|message| message.sender.to_lowercase())
            .collect();
        let mut first: Option<(chrono::DateTime<chrono::Utc>, &Message)> = None;
        let mut last: Option<(chrono::DateTime<chrono::Utc>, &Message)> = None;
        let mut days: BTreeMap<chrono::NaiveDate, Vec<&Message>> = BTreeMap::new();
        for message in &messages {
            let received = received_instant(message)?;
            days.entry(received.date_naive()).or_default().push(message);
            if first.is_none_or(|(earliest, _)| received < earliest) {
                first = Some((received, message));
            }
            if last.is_none_or(|(latest, _)| received > latest) {
                last = Some((received, message));
            }
        }
        Ok(MailboxSummary {
            mailbox_id,
            since: instant(since, "since")?.map(|at| at.to_rfc3339()),
            until: instant(until, "until")?.map(|at| at.to_rfc3339()),
            messages: messages.len(),
            unanswered: count(&|message| !answered.contains(&message.id)),
            snoozed: count(&|message| is_snoozed(message, &now)),
            blocked: count(&|message| is_blocked(message, &blocked)),
            muted: count(&|message| is_muted(message, &muted)),
            tagged: count(&|message| !message.tags.is_empty()),
            noted: count(&|message| message.note.is_some()),
            with_attachments: count(&|message| carries_files(message)),
            file_bytes: messages
                .iter()
                .flat_map(|message| message.attachments.iter().flatten())
                .filter_map(|file| file.size_bytes)
                .sum(),
            bounces: count(&|message| message.bounce.is_some()),
            failed_authentication: count(&|message| doubts_sender(message)),
            senders: senders.len(),
            first_received_at: first.map(|(_, message)| message.received_at.clone()),
            last_received_at: last.map(|(_, message)| message.received_at.clone()),
            days: days
                .into_iter()
                .map(|(day, received)| ReceivedDay {
                    day: day.to_string(),
                    received: received.len(),
                })
                .collect(),
        })
    }
}

impl MailboxSummary {
    /// The summary's days as a CSV table (RFC 4180) under the header `day,
    /// received`, oldest first: how much mail arrived on each UTC day.
    pub fn day_table(&self) -> String {
        let mut table = tables::header(&["day", "received"]);
        for day in &self.days {
            table.push_str(&tables::row(&[day.day.clone(), day.received.to_string()]));
        }
        table
    }
}
