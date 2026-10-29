//! A delivery status notification (RFC 3464): the report a mail server sends
//! back when it could not, or could only partly, deliver a message. An import
//! reads which message it reports on and what happened to each recipient, so
//! a send Skrzynka recorded as accepted can be seen to have bounced later.

use mailparse::{MailHeaderMap, ParsedMail};
use serde::{Deserialize, Serialize};

/// RFC 6522: the container of a report.
const REPORT_TYPE: &str = "multipart/report";
/// RFC 3464: the report-type a delivery status notification declares.
const DELIVERY_STATUS: &str = "delivery-status";
/// RFC 3464: the part carrying the per-message and per-recipient fields.
const STATUS_PART: &str = "message/delivery-status";
/// RFC 3464 section 2: the returned message, whole or as its headers only,
/// travels in a part whose type names RFC 822.
const RETURNED_PART: &str = "rfc822";

/// What one report says: the Message-ID of the message it reports on, when
/// the returned part carries one, and each recipient's outcome as the
/// reporting server wrote it. A report whose parts cannot be read keeps why
/// in `unreadable`, with what could be read before it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BounceReport {
    pub original_message_id: Option<String>,
    pub recipients: Vec<RecipientOutcome>,
    pub unreadable: Option<String>,
}

/// One recipient's fields (RFC 3464 section 2.3): `Final-Recipient`,
/// `Action` (`failed`, `delayed`, `delivered`, `relayed`, `expanded`),
/// `Status` (an RFC 3463 code) and `Diagnostic-Code`, each as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientOutcome {
    pub recipient: Option<String>,
    pub action: Option<String>,
    pub status: Option<String>,
    pub diagnostic: Option<String>,
}

/// One recipient outcome of a stored delivery status notification about one
/// of a mailbox's sends: the kind (`reply`, `outbound`, `unsubscribe`) and ID
/// of the send, the stored report's message ID and when it arrived, and the
/// recipient, action, status and diagnostic the reporting server wrote
/// (absent fields stay absent; a report that names no recipient is one row
/// with all four absent), with why the report could not be read when it could
/// not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredBounce {
    pub kind: String,
    pub send_id: String,
    pub report_message_id: String,
    pub received_at: String,
    pub recipient: Option<String>,
    pub action: Option<String>,
    pub status: Option<String>,
    pub diagnostic: Option<String>,
    pub unreadable: Option<String>,
}

/// The delivery status notification `parsed` is, or `None` when it is not
/// one.
pub(in crate::mail) fn bounce_report(parsed: &ParsedMail<'_>) -> Option<BounceReport> {
    let declared = parsed.ctype.mimetype.eq_ignore_ascii_case(REPORT_TYPE)
        && parsed
            .ctype
            .params
            .get("report-type")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case(DELIVERY_STATUS));
    if !declared {
        return None;
    }
    let mut report = BounceReport::default();
    if let Err(reason) = read_report(parsed, &mut report) {
        report.unreadable = Some(reason);
    }
    Some(report)
}

/// Fill `report` from the parts of a delivery status notification, stopping
/// at the first part that cannot be read.
fn read_report(parsed: &ParsedMail<'_>, report: &mut BounceReport) -> Result<(), String> {
    // The returned message is read first, so a damaged status part still
    // leaves which send the report is about.
    for part in &parsed.subparts {
        if !part
            .ctype
            .mimetype
            .to_ascii_lowercase()
            .contains(RETURNED_PART)
        {
            continue;
        }
        let raw = part
            .get_body_raw()
            .map_err(|error| format!("its returned message could not be decoded: {error}"))?;
        let (headers, _) = mailparse::parse_headers(&raw).map_err(|error| {
            format!("its returned message's headers could not be read: {error}")
        })?;
        report.original_message_id = headers
            .get_first_value("Message-ID")
            .map(|value| value.trim().to_string());
    }
    let status = parsed
        .subparts
        .iter()
        .find(|part| part.ctype.mimetype.eq_ignore_ascii_case(STATUS_PART))
        .ok_or_else(|| {
            "the delivery status notification has no message/delivery-status part".to_string()
        })?;
    let text = status
        .get_body()
        .map_err(|error| format!("its delivery-status part could not be decoded: {error}"))?;
    let normalized = text.replace("\r\n", "\n");
    let mut blocks = normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty());
    // The first block holds the per-message fields; every later block is one
    // recipient's.
    if blocks.next().is_none() {
        return Err("its delivery-status part is empty".to_string());
    }
    for block in blocks {
        let (fields, _) = mailparse::parse_headers(block.as_bytes()).map_err(|error| {
            format!("a recipient block of its delivery-status part could not be read: {error}")
        })?;
        report.recipients.push(RecipientOutcome {
            recipient: fields.get_first_value("Final-Recipient"),
            action: fields.get_first_value("Action"),
            status: fields.get_first_value("Status"),
            diagnostic: fields.get_first_value("Diagnostic-Code"),
        });
    }
    Ok(())
}
