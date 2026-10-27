//! The operator's own handling of one stored message: snoozing it out of
//! sight until an instant, and keeping a note on it. Neither changes
//! anything at the provider.

use super::AppState;
use crate::{error::AppError, models::Message};
use chrono::{DateTime, SecondsFormat, Utc};
use uuid::Uuid;

/// What `message snooze --view` did: the view, the instant, and every
/// message it listed.
#[derive(Debug, serde::Serialize)]
pub struct SnoozedView {
    pub view: String,
    pub until: String,
    pub messages: Vec<Uuid>,
}

/// What `message wake --view` did: the view, and every snoozed message of it
/// shown again.
#[derive(Debug, serde::Serialize)]
pub struct WokenView {
    pub view: String,
    pub messages: Vec<Uuid>,
}

/// What `message snooze --thread` or `message wake --thread` did: the
/// message named, the instant (absent for a wake), and every message of its
/// conversation changed.
#[derive(Debug, serde::Serialize)]
pub struct SnoozedThread {
    pub thread: Uuid,
    pub until: Option<String>,
    pub messages: Vec<Uuid>,
}

/// A snooze instant as stored, in the fixed width [`instant`] writes, from
/// either an RFC 3339 instant ahead of now or a span from now written `+`,
/// a whole number and a unit: `m` minutes, `h` hours, `d` days, `w` weeks
/// (`+2h`, `+3d`). A span that does not reach past now is refused as an
/// instant that has passed.
fn snooze_instant(until: &str) -> Result<String, AppError> {
    let written = until.trim();
    let at = match written.strip_prefix('+') {
        Some(span) => Utc::now().checked_add_signed(span_from_now(span, '+', "SNOOZE_INVALID")?).ok_or_else(|| {
            AppError::invalid(
                "SNOOZE_INVALID",
                format!("+{span} reaches past the last instant Skrzynka can store; snooze for less"),
            )
        })?,
        None => DateTime::parse_from_rfc3339(written)
            .map_err(|error| {
                AppError::invalid(
                    "SNOOZE_INVALID",
                    format!("{until:?} is neither an RFC 3339 instant ({error}) nor a span from now such as +2h or +3d"),
                )
            })?
            .with_timezone(&Utc),
    };
    if at <= Utc::now() {
        return Err(AppError::invalid(
            "SNOOZE_INVALID",
            format!("{at} has already passed; snooze until a later instant"),
        ));
    }
    Ok(instant(at))
}

/// A span such as `2h`: a whole number and one unit letter, written after
/// `sign` (`+` ahead of now, `-` back from it). A span that does not read so
/// is refused with `code`, the refusal of the command that took it.
pub(super) fn span_from_now(span: &str, sign: char, code: &'static str) -> Result<chrono::TimeDelta, AppError> {
    let invalid = |why: String| {
        AppError::invalid(
            code,
            format!("{sign}{span} is not a span from now: {why}; write {sign} a whole number and m, h, d or w, such as {sign}2h"),
        )
    };
    let Some(unit) = span.chars().last() else {
        return Err(invalid("it is empty".to_string()));
    };
    let count: i64 = span[..span.len() - unit.len_utf8()]
        .parse()
        .map_err(|error| invalid(format!("its count does not read as a whole number ({error})")))?;
    let delta = match unit {
        'm' => chrono::TimeDelta::try_minutes(count),
        'h' => chrono::TimeDelta::try_hours(count),
        'd' => chrono::TimeDelta::try_days(count),
        'w' => chrono::TimeDelta::try_weeks(count),
        other => return Err(invalid(format!("{other:?} is not a unit"))),
    };
    delta.ok_or_else(|| invalid("it is longer than an instant can reach".to_string()))
}

impl AppState {
    /// Snooze the message until `until` (RFC 3339, or a span from now such as
    /// `+2h`), which must lie ahead.
    pub fn snooze_message(
        &self,
        organization_id: &str,
        message_id: Uuid,
        until: &str,
    ) -> Result<Message, AppError> {
        let at = snooze_instant(until)?;
        self.database
            .snooze_message(organization_id, message_id, Some(at.as_str()))
    }

    /// Snooze every message the saved view `view` lists until `until`, and
    /// answer which messages they were. The instant is checked once first.
    pub fn snooze_view(
        &self,
        organization_id: &str,
        view: &str,
        until: &str,
    ) -> Result<SnoozedView, AppError> {
        let at = snooze_instant(until)?;
        let mut messages = Vec::new();
        for message in self.list_view(organization_id, view, None, None)? {
            self.database
                .snooze_message(organization_id, message.id, Some(at.as_str()))?;
            messages.push(message.id);
        }
        Ok(SnoozedView {
            view: view.trim().to_string(),
            until: at,
            messages,
        })
    }

    /// Show the message in listings again now.
    pub fn wake_message(&self, organization_id: &str, message_id: Uuid) -> Result<Message, AppError> {
        self.database.snooze_message(organization_id, message_id, None)
    }

    /// Show every message the saved view `view` keeps that is snoozed now:
    /// its filters with snoozed mail included, since a view's own listing
    /// leaves it out. An unknown view is `VIEW_NOT_FOUND`.
    pub fn wake_view(&self, organization_id: &str, view: &str) -> Result<WokenView, AppError> {
        let saved = self.view(organization_id, view)?;
        let filters = super::MessageFilters { include_snoozed: true, ..saved.filters.filters() };
        let now = instant(Utc::now());
        let mut messages = Vec::new();
        for message in self.list_messages(organization_id, saved.filters.mailbox_id, filters, None, None)? {
            if is_snoozed(&message, &now) {
                self.database.snooze_message(organization_id, message.id, None)?;
                messages.push(message.id);
            }
        }
        Ok(WokenView { view: saved.name, messages })
    }

    /// Snooze every message of the conversation `id` belongs to, as
    /// `message thread` lists it, until `until`; the instant is checked once
    /// first. A reply arriving later is not snoozed by this.
    pub fn snooze_thread(&self, organization_id: &str, id: Uuid, until: &str) -> Result<SnoozedThread, AppError> {
        let at = snooze_instant(until)?;
        let mut messages = Vec::new();
        for message in self.message_thread(organization_id, id)? {
            self.database.snooze_message(organization_id, message.id, Some(at.as_str()))?;
            messages.push(message.id);
        }
        Ok(SnoozedThread { thread: id, until: Some(at), messages })
    }

    /// Show every snoozed message of the conversation `id` belongs to again now.
    pub fn wake_thread(&self, organization_id: &str, id: Uuid) -> Result<SnoozedThread, AppError> {
        let now = instant(Utc::now());
        let mut messages = Vec::new();
        for message in self.message_thread(organization_id, id)? {
            if is_snoozed(&message, &now) {
                self.database.snooze_message(organization_id, message.id, None)?;
                messages.push(message.id);
            }
        }
        Ok(SnoozedThread { thread: id, until: None, messages })
    }
}

impl AppState {
    /// Keep the operator's note on a message, replacing its note, or remove
    /// it with `None`. A note that holds no visible text is refused rather
    /// than kept as an empty one.
    pub fn set_message_note(
        &self,
        organization_id: &str,
        message_id: Uuid,
        note: Option<&str>,
    ) -> Result<Message, AppError> {
        let note = note.map(str::trim_end);
        if note.is_some_and(|text| text.trim().is_empty()) {
            return Err(AppError::invalid(
                "MESSAGE_NOTE_INVALID",
                "the note holds no visible text; write one, or remove the note",
            ));
        }
        self.database.set_message_note(organization_id, message_id, note)
    }

    /// Add `text` to the message's note on a line of its own, or start the
    /// note with it when there is none: a running log kept on the message.
    /// Text with nothing visible is `MESSAGE_NOTE_INVALID`.
    pub fn append_message_note(
        &self,
        organization_id: &str,
        message_id: Uuid,
        text: &str,
    ) -> Result<Message, AppError> {
        if text.trim().is_empty() {
            return Err(AppError::invalid(
                "MESSAGE_NOTE_INVALID",
                "the text to add holds no visible text; write some, or leave the note as it is",
            ));
        }
        let note = match self.database.get_message(organization_id, message_id)?.note {
            Some(kept) => format!("{}\n{}", kept.trim_end(), text.trim_end()),
            None => text.to_string(),
        };
        self.set_message_note(organization_id, message_id, Some(&note))
    }
}

/// An instant as snoozes store and compare it: one fixed RFC 3339 width in
/// UTC, so the text order the database and listings compare is time order.
pub(super) fn instant(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Whether listings leave `message` out at `now`, written by [`instant`].
pub(super) fn is_snoozed(message: &Message, now: &str) -> bool {
    message
        .snoozed_until
        .as_deref()
        .is_some_and(|until| until > now)
}
