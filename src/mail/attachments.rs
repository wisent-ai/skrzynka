//! The parts of a provider message that are files rather than its text. An
//! import records what each one is; a reader asking for one gets its decoded
//! bytes, read again from the provider's own copy of the message, because
//! Skrzynka keeps no attachment bytes of its own.

use mailparse::{DispositionType, ParsedMail};
use serde::{Deserialize, Serialize};

/// One file a message carries, as its MIME part describes it: its index
/// among the message's attachments (counted from zero, in MIME order), the
/// name the message gives it, its type, and its decoded size. What of the
/// part could not be read (its stated name, or its transfer encoding, which
/// then leaves it without a size) is said in `unreadable`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageAttachment {
    pub index: usize,
    pub filename: Option<String>,
    pub content_type: String,
    pub size_bytes: Option<u64>,
    pub unreadable: Option<String>,
}

/// One attachment of a message read whole, with its decoded bytes or the
/// reason its transfer encoding could not be decoded.
#[derive(Debug)]
pub struct AttachmentPart {
    pub attachment: MessageAttachment,
    pub content: Result<Vec<u8>, String>,
}

/// Every attachment of `parsed`, indexed in the order its MIME parts appear,
/// depth first. A part is an attachment when it is a leaf (not a multipart
/// container) and its Content-Disposition is `attachment`, it states a file
/// name, or it is a whole message (`message/rfc822`). The same message always
/// yields the same indexes, so an index recorded at import names the same
/// part when the message is read again.
pub(super) fn attachment_parts(parsed: &ParsedMail<'_>) -> Vec<AttachmentPart> {
    let mut leaves = Vec::new();
    collect(parsed, &mut leaves);
    leaves
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            let content = part
                .get_body_raw()
                .map_err(|error| format!("its transfer encoding could not be decoded: {error}"));
            let (size_bytes, content_unreadable) = match &content {
                Ok(bytes) => (Some(bytes.len() as u64), None),
                Err(reason) => (None, Some(reason.clone())),
            };
            let (filename, name_unreadable) = match filename(part) {
                Ok(name) => (name, None),
                Err(reason) => (None, Some(reason)),
            };
            let unreadable = match (name_unreadable, content_unreadable) {
                (Some(name), Some(content)) => Some(format!("{name}; {content}")),
                (Some(reason), None) | (None, Some(reason)) => Some(reason),
                (None, None) => None,
            };
            AttachmentPart {
                attachment: MessageAttachment {
                    index,
                    filename,
                    content_type: part.ctype.mimetype.to_ascii_lowercase(),
                    size_bytes,
                    unreadable,
                },
                content,
            }
        })
        .collect()
}

fn collect<'p, 'a>(part: &'p ParsedMail<'a>, leaves: &mut Vec<&'p ParsedMail<'a>>) {
    if part.subparts.is_empty() {
        if is_attachment(part) {
            leaves.push(part);
        }
        return;
    }
    for subpart in &part.subparts {
        collect(subpart, leaves);
    }
}

/// A part that states a file name is an attachment even when that name
/// cannot be read.
fn is_attachment(part: &ParsedMail<'_>) -> bool {
    matches!(
        part.get_content_disposition().disposition,
        DispositionType::Attachment
    ) || !matches!(filename(part), Ok(None))
        || part.ctype.mimetype.eq_ignore_ascii_case("message/rfc822")
}

/// The name the part gives its file: the Content-Disposition `filename`, or
/// else the Content-Type `name`, with RFC 2231 and RFC 2047 encodings read.
/// A blank name is no name; a name whose encoded words cannot be read is
/// refused with the name as written and why.
fn filename(part: &ParsedMail<'_>) -> Result<Option<String>, String> {
    let disposition = part.get_content_disposition();
    let Some(stated) = disposition
        .params
        .get("filename")
        .or_else(|| part.ctype.params.get("name"))
    else {
        return Ok(None);
    };
    let line = format!("Name: {stated}");
    let (header, _) = mailparse::parse_header(line.as_bytes())
        .map_err(|error| format!("its file name {stated:?} could not be read: {error}"))?;
    let name = header.get_value();
    let name = name.trim();
    Ok((!name.is_empty()).then(|| name.to_string()))
}
