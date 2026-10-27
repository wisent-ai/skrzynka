//! What went out as tables: the outbound log, which `message outbound
//! --csv` writes, and the reply attempts, which `message replies --csv`
//! writes.

use super::{header, optional, row};
use crate::models::{OutboundMessage, ReplyAttempt};

/// The table's header: the fields of a sent message a report reads.
const HEADER: &[&str] = &[
    "id", "mailbox_id", "recipients", "cc", "bcc", "subject", "status", "created_at", "sent_at",
    "provider_message_id", "error_code", "error_message", "attachments",
];

/// Every outbound message as one row under the header, in the order given:
/// a field the row does not hold is empty, the body is left out and
/// `attachments` names each file the message carried, `; ` between them.
pub fn outbound_table(messages: &[OutboundMessage]) -> String {
    let mut table = header(HEADER);
    for message in messages {
        let files: Vec<&str> = message.attachments.iter().map(|file| file.filename.as_str()).collect();
        table.push_str(&row(&[
            message.id.to_string(),
            message.mailbox_id.to_string(),
            message.recipients.clone(),
            optional(&message.cc),
            optional(&message.bcc),
            message.subject.clone(),
            message.status.as_str().to_string(),
            message.created_at.clone(),
            optional(&message.sent_at),
            optional(&message.provider_message_id),
            optional(&message.error_code),
            optional(&message.error_message),
            files.join("; "),
        ]));
    }
    table
}

/// The reply table's header: the fields of a reply attempt a report reads,
/// `message_id` the stored message it answers.
const REPLY_HEADER: &[&str] = &[
    "id", "message_id", "cc", "bcc", "status", "created_at", "sent_at", "provider_message_id", "error_code",
    "error_message", "attachments",
];

/// Every reply attempt as one row under its header, in the order given: a
/// field it does not hold is empty, its text is left out and `attachments`
/// names each file it carried, `; ` between them.
pub fn reply_table(replies: &[ReplyAttempt]) -> String {
    let mut table = header(REPLY_HEADER);
    for reply in replies {
        let files: Vec<&str> = reply.attachments.iter().map(|file| file.filename.as_str()).collect();
        table.push_str(&row(&[
            reply.id.to_string(),
            reply.message_id.to_string(),
            optional(&reply.cc),
            optional(&reply.bcc),
            reply.status.as_str().to_string(),
            reply.created_at.clone(),
            optional(&reply.sent_at),
            optional(&reply.provider_message_id),
            optional(&reply.error_code),
            optional(&reply.error_message),
            files.join("; "),
        ]));
    }
    table
}
