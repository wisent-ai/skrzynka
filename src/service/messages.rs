//! Stored messages and the replies sent to them.

use super::{
    blocked::is_blocked, filters::MessageFilters, snoozes::{self, is_snoozed}, threads::mutes::is_muted, AppState,
};
use crate::{
    error::AppError,
    mail,
    models::{CreateReplyRequest, DeliveryStatus, Message, ReplyAttempt},
};
use uuid::Uuid;

impl AppState {
    /// Stored messages, newest first: every one from `offset`, or a page of
    /// `limit` when the caller names one. Zero is refused, never read as a
    /// different page size. `filters` narrow the listing before the page is
    /// cut; with none, the database pages directly.
    pub fn list_messages(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        filters: MessageFilters<'_>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Message>, AppError> {
        page_size(limit)?;
        let blocked = match filters.include_blocked {
            true => Vec::new(),
            false => self.database.blocked_senders(organization_id)?,
        };
        let now = snoozes::instant(chrono::Utc::now());
        // Snoozed mail is left out unless it is kept anyway, or is all that is asked for.
        let leaves_snoozed = !filters.include_snoozed && !filters.snoozed;
        let snoozing = leaves_snoozed && self.database.any_snoozed(organization_id, &now)?;
        let matcher = filters.matcher()?;
        // Messages a reply was sent to, read once when only unanswered, or only answered, ones are kept.
        let answered = match filters.unanswered || filters.answered {
            true => Some(self.database.answered_messages(organization_id)?),
            false => None,
        };
        // Muted Message-IDs by mailbox, read unless muted mail is kept anyway;
        // read too when muted mail is all that is asked for.
        let muted = match filters.include_muted && !filters.muted {
            true => std::collections::HashMap::new(),
            false => self.database.muted_identities(organization_id)?,
        };
        let quiet = matcher.is_none() && blocked.is_empty() && !snoozing && muted.is_empty();
        if quiet && answered.is_none() && !filters.oldest_first && !filters.snoozed && !filters.muted {
            return self
                .database
                .list_messages(organization_id, mailbox_id, limit, offset);
        }
        // The filters read the stored messages, so the page is cut after
        // them: every stored message is read, from the first one.
        let mut matching = Vec::new();
        for message in self
            .database
            .list_messages(organization_id, mailbox_id, None, None)?
        {
            // A muted conversation is left out, unless it is all that is asked for.
            let muting = is_muted(&message, &muted);
            if is_blocked(&message, &blocked) || (snoozing && is_snoozed(&message, &now)) || muting != filters.muted {
                continue;
            }
            if answered.as_ref().is_some_and(|answered| answered.contains(&message.id) != filters.answered) {
                continue;
            }
            if filters.snoozed && !is_snoozed(&message, &now) {
                continue;
            }
            if let Some(matcher) = &matcher {
                if !matcher.keeps(&message)? {
                    continue;
                }
            }
            matching.push(message);
        }
        // The database answers newest first; oldest first is that order turned
        // round before the page is cut.
        if filters.oldest_first {
            matching.reverse();
        }
        let after_offset: Vec<Message> = match offset {
            Some(offset) => matching.into_iter().skip(offset as usize).collect(),
            None => matching,
        };
        Ok(match limit {
            Some(limit) => after_offset.into_iter().take(limit as usize).collect(),
            None => after_offset,
        })
    }

    pub fn get_message(&self, organization_id: &str, id: Uuid) -> Result<Message, AppError> {
        self.database.get_message(organization_id, id)
    }

    pub fn list_replies(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<ReplyAttempt>, AppError> {
        self.database.list_replies(organization_id, message_id)
    }

    pub async fn reply(
        &self,
        organization_id: &str,
        message_id: Uuid,
        request: CreateReplyRequest,
    ) -> Result<ReplyAttempt, AppError> {
        validate_reply_request(&request)?;
        // Every file is checked before the attempt is claimed, so a file that
        // cannot be sent never leaves a claimed reply behind.
        let files = mail::prepare_attachments(&request.attachments)?;
        let records: Vec<_> = files.iter().map(|file| file.record.clone()).collect();
        // A quoted reply carries the message's text below the reply's own;
        // the attempt records the body as sent, quote included.
        let body = match request.quote {
            true => mail::quoted(
                request.body.trim_end(),
                &self.database.get_message(organization_id, message_id)?,
            ),
            false => request.body.trim_end().to_string(),
        };
        // Copies are bare addresses, each once, checked before anything is claimed.
        let copies = mail::ReplyCopies {
            all: request.reply_all,
            cc: super::outbound::normalize_addresses(&request.cc)?,
            bcc: super::outbound::normalize_addresses(&request.bcc)?,
        };
        let (attempt, created) = self.database.begin_reply(
            organization_id,
            message_id,
            request.idempotency_key.trim(),
            &body,
            &records,
            &copies,
        )?;
        if !created {
            return Ok(attempt);
        }
        let attempt =
            self.database
                .update_reply(attempt.id, DeliveryStatus::Sending, None, None, None)?;
        let source = self
            .database
            .get_message(organization_id, message_id)
            .and_then(|message| {
                self.database
                    .get_mailbox(organization_id, message.mailbox_id)
                    .map(|mailbox| (message, mailbox))
            });
        let (message, mailbox) = match source {
            Ok(source) => source,
            Err(error) => {
                let recorded = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error.with_unrecorded(recorded, "sending"));
            }
        };
        let credentials = match self
            .resolver
            .resolve_credentials(mailbox.outbound_skarbiec_item_id())
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let recorded = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error.with_unrecorded(recorded, "sending"));
            }
        };
        // The signature is the one the attempt was claimed with.
        let body = mail::signed(&body, attempt.signature.as_deref());
        let automatic = request.automatic;
        let result = tokio::task::spawn_blocking(move || {
            mail::send_reply(&mailbox, &credentials, &message, &body, &files, &copies, automatic)
        })
        .await;
        match result {
            Ok(Ok(provider_message_id)) => self.database.update_reply(
                attempt.id,
                DeliveryStatus::Sent,
                Some(&provider_message_id),
                None,
                None,
            ),
            Ok(Err(error)) if error.code == "SMTP_UNCERTAIN" => self
                .database
                .update_reply(
                    attempt.id,
                    DeliveryStatus::Uncertain,
                    None,
                    Some("REPLY_UNCERTAIN"),
                    Some(&error.message),
                )
                .map_err(|write| error.with_unrecorded::<()>(Err(write), "sending")),
            Ok(Err(error)) => {
                let recorded = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error.with_unrecorded(recorded, "sending"))
            }
            Err(stopped) => {
                let error = AppError::dependency(
                    "REPLY_UNCERTAIN",
                    format!(
                        "send task stopped before terminal SMTP evidence was recorded: {stopped}"
                    ),
                    false,
                );
                self.database
                    .update_reply(
                        attempt.id,
                        DeliveryStatus::Uncertain,
                        None,
                        Some(error.code),
                        Some(&error.message),
                    )
                    .map_err(|write| error.with_unrecorded::<()>(Err(write), "sending"))
            }
        }
    }
}

fn validate_reply_request(request: &CreateReplyRequest) -> Result<(), AppError> {
    super::outbound::checked_key(&request.idempotency_key)?;
    if request.body.trim().is_empty() {
        return Err(AppError::invalid(
            "REPLY_BODY_INVALID",
            "reply body must not be empty",
        ));
    }
    Ok(())
}

/// A page size of zero names no page; every listing refuses it the same way.
pub(super) fn page_size(limit: Option<u32>) -> Result<(), AppError> {
    match limit.map(std::num::NonZeroU32::new) {
        Some(None) => Err(AppError::invalid(
            "LIMIT_INVALID",
            "limit must be at least one; omit it to list every message",
        )),
        Some(Some(_)) | None => Ok(()),
    }
}

/// A listing filter as given, lowercased, or refused when it is blank.
pub(super) fn filter_value(value: Option<&str>, name: &str) -> Result<Option<String>, AppError> {
    match value.map(str::trim) {
        None => Ok(None),
        Some("") => Err(AppError::invalid(
            "MESSAGE_FILTER_INVALID",
            &format!("the {name} filter is empty; name what it must match or leave the filter out"),
        )),
        Some(value) => Ok(Some(value.to_lowercase())),
    }
}

/// The address of one recipient entry: the bracketed part of a display-name
/// entry, or the entry itself, trimmed.
pub(super) fn entry_address(entry: &str) -> &str {
    let entry = entry.trim();
    let address = match (entry.rfind('<'), entry.rfind('>')) {
        (Some(open), Some(close)) if open < close => entry[open..close].trim_start_matches('<'),
        _ => entry,
    };
    address.trim()
}

/// Whether the To header names `address` whole: each comma-separated entry,
/// a display name with the address in angle brackets or a bare address,
/// compared without case.
pub(super) fn addressed_to(header: &str, address: &str) -> bool {
    header
        .split(',')
        .any(|entry| entry_address(entry).eq_ignore_ascii_case(address))
}
