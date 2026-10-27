//! Stored mail as a table: what `message list --csv` writes.

use super::{header, optional, row};
use crate::models::Message;

/// The table's header: the fields of a stored message a report reads.
const HEADER: &[&str] = &[
    "id", "mailbox_id", "received_at", "sent_at", "sender", "reply_to", "recipients", "cc", "subject",
    "message_id", "in_reply_to", "list_id", "snippet", "attachments", "tags", "note", "snoozed_until",
];

/// Every stored message as one row under the header, in the order given: a
/// header the message did not carry is empty, its text is left out but for
/// the snippet the listing prints, `attachments` names each recorded file
/// (an unnamed one by its type) and `tags` each tag, `; ` between them.
pub fn message_table(messages: &[Message]) -> String {
    let mut table = header(HEADER);
    for message in messages {
        let files: Vec<&str> = match &message.attachments {
            Some(files) => files
                .iter()
                .map(|file| match &file.filename {
                    Some(name) => name.as_str(),
                    None => file.content_type.as_str(),
                })
                .collect(),
            None => Vec::new(),
        };
        let list_id = match &message.mailing_list {
            Some(list) => optional(&list.list_id),
            None => String::new(),
        };
        table.push_str(&row(&[
            message.id.to_string(),
            message.mailbox_id.to_string(),
            message.received_at.clone(),
            optional(&message.sent_at),
            message.sender.clone(),
            optional(&message.reply_to),
            optional(&message.recipients),
            optional(&message.cc),
            optional(&message.subject),
            optional(&message.message_id),
            optional(&message.in_reply_to),
            list_id,
            message.snippet.clone(),
            files.join("; "),
            message.tags.join("; "),
            optional(&message.note),
            optional(&message.snoozed_until),
        ]));
    }
    table
}
