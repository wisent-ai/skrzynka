//! Follow-ups as a table: what `message followup list --csv` writes, the
//! list of who owes an answer to work through in a spreadsheet or a CRM.

use super::{
    super::tables::{header, optional, row},
    FollowUp, FollowUpState,
};

/// The table's header, the field names `message followup list` prints.
const HEADER: &[&str] = &[
    "outbound_id",
    "mailbox_id",
    "recipients",
    "subject",
    "sent_at",
    "due_at",
    "state",
    "answer",
    "answered_at",
];

/// Every follow-up as one row under the header, in the order given; an
/// instant or answer it does not hold is empty.
pub fn follow_up_table(follow_ups: &[FollowUp]) -> String {
    let mut table = header(HEADER);
    for follow_up in follow_ups {
        let state = match follow_up.state {
            FollowUpState::Waiting => "waiting",
            FollowUpState::Due => "due",
            FollowUpState::Answered => "answered",
        };
        let answer = match follow_up.answer {
            Some(id) => id.to_string(),
            None => String::new(),
        };
        table.push_str(&row(&[
            follow_up.outbound_id.to_string(),
            follow_up.mailbox_id.to_string(),
            follow_up.recipients.clone(),
            follow_up.subject.clone(),
            optional(&follow_up.sent_at),
            follow_up.due_at.clone(),
            state.to_string(),
            answer,
            optional(&follow_up.answered_at),
        ]));
    }
    table
}
