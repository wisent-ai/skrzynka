//! The blocked senders as a table: what `message blocked --csv` writes, each
//! entry with how many stored messages it leaves out of listings now, for a
//! spreadsheet or a review of what blocking hides.

use super::super::tables::{header, row};
use super::BlockedCount;

/// Every counted entry as one CSV row (RFC 4180) under the header `entry,
/// messages`, the fields `message blocked --count` prints, in the order given.
pub fn blocked_table(counts: &[BlockedCount]) -> String {
    let mut table = header(&["entry", "messages"]);
    for counted in counts {
        table.push_str(&row(&[counted.entry.clone(), counted.messages.to_string()]));
    }
    table
}
