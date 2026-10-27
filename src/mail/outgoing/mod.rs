//! Files a message Skrzynka originates carries: as a caller hands them over
//! (a name, a type and the bytes in base64), as they are recorded on the
//! outbound row (name, type, size and SHA-256, never the bytes), and as they
//! are attached to the SMTP message.

use crate::error::AppError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use lettre::message::{header::ContentType, Attachment, MultiPart, SinglePart};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One file as a caller hands it to `message send` or
/// `POST /v1/mailboxes/:id/outbound`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingAttachment {
    pub filename: String,
    pub content_type: String,
    pub content_base64: String,
}

/// One file as the outbound row records it. A repeated request with the same
/// idempotency key must carry the same files, compared by these fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutboundAttachment {
    pub filename: String,
    pub content_type: String,
    pub size_bytes: usize,
    pub sha256: String,
}

/// A checked file: what is recorded, and the bytes that are sent.
#[derive(Debug, Clone)]
pub struct PreparedAttachment {
    pub record: OutboundAttachment,
    pub content_type: ContentType,
    pub bytes: Vec<u8>,
}

/// Who a reply goes to besides its target: every To and Cc address of the
/// message (`all`), and the addresses the request copies openly (`cc`) or
/// blindly (`bcc`), each bare and once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplyCopies {
    pub all: bool,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
}

/// The addresses of a comma-separated header value, trimmed, blanks left out.
pub(crate) fn split_addresses(value: &str) -> Vec<&str> {
    value.split(',').map(str::trim).filter(|address| !address.is_empty()).collect()
}

/// One recipient as the mail builder takes it; one that does not read as an
/// address is `OUTBOUND_RECIPIENT_INVALID`.
pub(crate) fn parse_recipient(value: &str) -> Result<lettre::message::Mailbox, AppError> {
    value.parse().map_err(|cause| {
        AppError::invalid(
            "OUTBOUND_RECIPIENT_INVALID",
            format!("outbound recipient {value:?} is not a valid email address: {cause}"),
        )
    })
}

/// Check every file before anything is claimed: a name that is a plain file
/// name, a type SMTP can carry, and bytes that decode.
pub fn prepare_attachments(
    attachments: &[OutgoingAttachment],
) -> Result<Vec<PreparedAttachment>, AppError> {
    attachments
        .iter()
        .enumerate()
        .map(|(position, attachment)| prepare(position, attachment))
        .collect()
}

fn prepare(
    position: usize,
    attachment: &OutgoingAttachment,
) -> Result<PreparedAttachment, AppError> {
    let filename = attachment.filename.trim();
    if filename.is_empty()
        || filename.contains(['/', '\\'])
        || filename.chars().any(char::is_control)
    {
        return Err(invalid(format!(
            "attachment {position} names the file {:?}; a file name must be non-empty, without a path separator or a control character",
            attachment.filename
        )));
    }
    let content_type = ContentType::parse(attachment.content_type.trim()).map_err(|cause| {
        invalid(format!(
            "attachment {position} ({filename}) has the type {:?}, which is not a MIME type: {cause}",
            attachment.content_type
        ))
    })?;
    let bytes = STANDARD
        .decode(attachment.content_base64.trim())
        .map_err(|cause| {
            invalid(format!(
                "attachment {position} ({filename}) is not base64 (RFC 4648, standard alphabet): {cause}"
            ))
        })?;
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(PreparedAttachment {
        record: OutboundAttachment {
            filename: filename.to_string(),
            content_type: attachment.content_type.trim().to_string(),
            size_bytes: bytes.len(),
            sha256,
        },
        content_type,
        bytes,
    })
}

/// The SMTP body of a message with files: a `multipart/mixed` of the text
/// followed by each file in the order given.
pub fn body_with_attachments(text: &str, attachments: &[PreparedAttachment]) -> MultiPart {
    attachments.iter().fold(
        MultiPart::mixed().singlepart(SinglePart::plain(text.to_string())),
        |body, attachment| {
            body.singlepart(
                Attachment::new(attachment.record.filename.clone())
                    .body(attachment.bytes.clone(), attachment.content_type.clone()),
            )
        },
    )
}

fn invalid(message: String) -> AppError {
    AppError::invalid("OUTBOUND_ATTACHMENT_INVALID", message)
}

/// The text a send carries: `body`, then the mailbox's `signature` after the
/// RFC 3676 signature separator line (`-- `), or `body` alone without one.
pub fn signed(body: &str, signature: Option<&str>) -> String {
    match signature.map(str::trim_end) {
        Some(signature) if !signature.trim().is_empty() => {
            format!("{}\n\n-- \n{signature}", body.trim_end())
        }
        _ => body.to_string(),
    }
}

/// A reply's text with the message it answers quoted below it: a line naming
/// when and from whom the message came, then each of its lines after `> `
/// (`>` alone for an empty one), as mail programs quote.
pub fn quoted(body: &str, inbound: &crate::models::Message) -> String {
    let when = match inbound.sent_at.as_deref() {
        Some(sent_at) => sent_at,
        None => inbound.received_at.as_str(),
    };
    let lines: Vec<String> = inbound
        .body_text
        .trim_end()
        .lines()
        .map(|line| match line.is_empty() {
            true => ">".to_string(),
            false => format!("> {line}"),
        })
        .collect();
    format!(
        "{}\n\nOn {when}, {} wrote:\n{}",
        body.trim_end(),
        inbound.sender,
        lines.join("\n")
    )
}

/// The Cc of a reply to everyone: each address of the message's To and Cc
/// headers, each once, except the mailbox's own address and the reply's
/// target. A header that does not read as a list of addresses refuses the
/// reply, naming it, rather than leaving someone out unsaid.
pub fn everyone_else(
    inbound: &crate::models::Message,
    own: &str,
    target: &str,
) -> Result<Vec<lettre::message::Mailbox>, AppError> {
    use std::str::FromStr;
    let target = lettre::message::Mailbox::from_str(target)
        .map(|mailbox| mailbox.email.to_string().to_lowercase())
        .map_err(|cause| {
            AppError::invalid(
                "MESSAGE_REPLY_TARGET_INVALID",
                format!("the reply target {target:?} is not valid: {cause}"),
            )
        })?;
    let mut skipped = vec![own.trim().to_lowercase(), target];
    let mut copied = Vec::new();
    let headers = [("To", inbound.recipients.as_deref()), ("Cc", inbound.cc.as_deref())];
    for (name, header) in headers {
        let Some(header) = header else { continue };
        let listed = lettre::message::Mailboxes::from_str(header).map_err(|cause| {
            AppError::invalid(
                "MESSAGE_REPLY_ALL_INVALID",
                format!("the message's {name} header {header:?} is not a list of addresses: {cause}"),
            )
        })?;
        for mailbox in listed {
            let address = mailbox.email.to_string().to_lowercase();
            if !skipped.contains(&address) {
                skipped.push(address);
                copied.push(mailbox);
            }
        }
    }
    Ok(copied)
}
