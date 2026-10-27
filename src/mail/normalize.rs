//! A provider message read into what Skrzynka stores: its headers as the
//! message states them and its plain-text body. Nothing is written in for a
//! header the message leaves out, and a header it states unreadably is kept
//! with why, so a stored row never claims what the sender did not write.

use super::attachments::{attachment_parts, AttachmentPart};
use super::authentication::authentication_verdicts;
use super::lists::MailingList;
use super::reports::bounce_report;
use super::dependency_error;
use crate::{error::AppError, models::NewMessage};
use mailparse::{MailHeaderMap, ParsedMail};

pub(super) fn normalize_message(uid: u32, bytes: &[u8]) -> Result<NewMessage, AppError> {
    read_message(uid, bytes).map(|(message, _)| message)
}

/// A provider message read whole: what Skrzynka stores of it, attachments
/// described, and each attachment with its decoded bytes.
pub fn read_message(
    uid: u32,
    bytes: &[u8],
) -> Result<(NewMessage, Vec<AttachmentPart>), AppError> {
    let parsed = mailparse::parse_mail(bytes).map_err(|error| {
        dependency_error(
            "MESSAGE_MALFORMED",
            format!("provider message {uid} could not be parsed: {error}"),
            false,
        )
    })?;
    let headers = &parsed.headers;
    let sender = headers
        .get_first_value("From")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            dependency_error(
                "MESSAGE_MALFORMED",
                "provider message has no From header",
                false,
            )
        })?;
    // To and Subject are kept as the message states them: absent stays absent.
    let recipients = headers.get_first_value("To");
    let subject = headers.get_first_value("Subject");
    let text = extract_text(&parsed).map_err(|error| {
        dependency_error(
            "MESSAGE_BODY_UNDECODABLE",
            format!("provider message {uid} has a text part that cannot be decoded: {error}"),
            false,
        )
    })?;
    // A message with no text/plain part (HTML only) has no text body.
    let body_text = match text {
        Some(text) => text.trim().to_string(),
        None => String::new(),
    };
    let snippet = body_text.split_whitespace().collect::<Vec<_>>().join(" ");
    // A Date header that does not read as an instant is kept with the
    // reader's reason, so a misdated message is told from an undated one.
    let (sent_at, sent_at_unreadable) = match headers.get_first_value("Date") {
        None => (None, None),
        Some(header) => match mailparse::dateparse(&header) {
            Ok(seconds) => match chrono::DateTime::<chrono::Utc>::from_timestamp_secs(seconds) {
                Some(instant) => (Some(instant.to_rfc3339()), None),
                None => {
                    let error =
                        format!("{seconds} seconds from the epoch is no representable instant");
                    (None, Some(NewMessage::unreadable_date(header, error)))
                }
            },
            Err(error) => (
                None,
                Some(NewMessage::unreadable_date(header, error.to_string())),
            ),
        },
    };
    let parts = attachment_parts(&parsed);
    let attachments = parts.iter().map(|part| part.attachment.clone()).collect();
    let message = NewMessage {
        external_uid: uid,
        message_id: headers.get_first_value("Message-ID"),
        in_reply_to: headers.get_first_value("In-Reply-To"),
        references: headers.get_first_value("References"),
        sender,
        reply_to: headers.get_first_value("Reply-To"),
        recipients,
        subject,
        sent_at,
        sent_at_unreadable,
        body_text,
        snippet,
        attachments,
        mailing_list: MailingList::of(headers),
        bounce: bounce_report(&parsed),
        authentication: authentication_verdicts(headers),
        cc: headers.get_first_value("Cc"),
    };
    Ok((message, parts))
}

/// The first text/plain part's decoded body, `None` when the message has no
/// such part, or the decoding failure. A part whose charset or transfer
/// encoding failed used to read as "no text part", so the message was stored
/// with an empty body as if it had none; it is now rejected and counted under
/// its own reason in the sync summary.
fn extract_text(parsed: &ParsedMail<'_>) -> Result<Option<String>, mailparse::MailParseError> {
    if parsed.subparts.is_empty() {
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/plain") {
            return parsed.get_body().map(Some);
        }
        return Ok(None);
    }
    for part in &parsed.subparts {
        if let Some(text) = extract_text(part)? {
            return Ok(Some(text));
        }
    }
    Ok(None)
}
