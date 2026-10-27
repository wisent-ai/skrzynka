//! The suppression list as a table: what `message suppressed --csv` writes,
//! for a CRM or mailing tool to keep the same do-not-contact list.

use super::super::tables::{header, optional, row};
use crate::mail::Suppression;

/// The table's header, the field names `message suppressed` prints.
const HEADER: &[&str] = &["entry", "reason", "added_at"];

/// Every entry as one row under the header, in the order given; a reason
/// the entry was not given is empty.
pub fn suppression_table(entries: &[Suppression]) -> String {
    let mut table = header(HEADER);
    for entry in entries {
        table.push_str(&row(&[entry.entry.clone(), optional(&entry.reason), entry.added_at.clone()]));
    }
    table
}
