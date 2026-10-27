//! Mailing lists as a table: what `message subscriptions --csv` writes, to
//! audit in a spreadsheet which lists write to which mailbox and which were
//! left.

use super::super::tables::{header, optional, row};
use crate::mail::{Subscription, UnsubscribeTarget};

/// The table's header: the fields of a list `message subscriptions` prints,
/// its ways out and its last request to leave flattened to text.
const HEADER: &[&str] = &[
    "list_id", "sender", "mailbox_id", "messages", "latest_message_id", "latest_received_at", "ways_out",
    "last_attempt_status", "last_attempt_at",
];

/// Every list as one row under the header, in the order given: `ways_out`
/// names each way by its `method` as the listing prints it, then its
/// target: `one_click <URL>`, `mailto <ADDRESS>` or `page <URL>`,
/// `; ` between them, and the last request's status and claim instant are
/// empty when none was sent.
pub fn subscription_table(lists: &[Subscription]) -> String {
    let mut table = header(HEADER);
    for list in lists {
        let ways: Vec<String> = list
            .ways_out
            .iter()
            .map(|way| match way {
                UnsubscribeTarget::OneClick { url } => format!("one_click {url}"),
                UnsubscribeTarget::Mailto { address, .. } => format!("mailto {address}"),
                UnsubscribeTarget::Page { url } => format!("page {url}"),
            })
            .collect();
        let (status, at) = match &list.last_attempt {
            Some(attempt) => (attempt.status.as_str().to_string(), attempt.created_at.clone()),
            None => (String::new(), String::new()),
        };
        table.push_str(&row(&[
            optional(&list.list_id),
            list.sender.clone(),
            list.mailbox_id.to_string(),
            list.messages.to_string(),
            list.latest_message_id.to_string(),
            list.latest_received_at.clone(),
            ways.join("; "),
            status,
            at,
        ]));
    }
    table
}
