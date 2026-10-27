//! Drafts as a table: what `message draft list --csv` writes, to plan and
//! review the mail being written in a spreadsheet.

use super::{header, optional, row};
use crate::mail::Draft;

/// The table's header: the fields of a draft a review reads.
const HEADER: &[&str] = &[
    "id", "mailbox_id", "recipients", "cc", "bcc", "subject", "created_at", "updated_at", "send_at",
    "schedule_error", "attachments",
];

/// Every draft as one row under the header, in the order given: its body is
/// left out, an instant or error it does not hold is empty, and
/// `attachments` names each file kept with it, `; ` between them.
pub fn draft_table(drafts: &[Draft]) -> String {
    let mut table = header(HEADER);
    for draft in drafts {
        let files: Vec<&str> = draft.attachments.iter().map(|file| file.filename.as_str()).collect();
        table.push_str(&row(&[
            draft.id.to_string(),
            draft.mailbox_id.to_string(),
            draft.recipients.clone(),
            draft.cc.clone(),
            draft.bcc.clone(),
            draft.subject.clone(),
            draft.created_at.clone(),
            draft.updated_at.clone(),
            optional(&draft.send_at),
            optional(&draft.schedule_error),
            files.join("; "),
        ]));
    }
    table
}
