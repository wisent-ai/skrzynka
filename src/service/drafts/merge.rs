//! Mail merge: one draft per row of a CSV table, each the template filled
//! with that row's values and addressed to that row's `to`. Every row is
//! read, addressed and filled before any draft is kept, so a table with one
//! bad row drafts nothing; nothing is sent, the drafts wait to be read.

use super::{
    super::{outbound::normalize_addresses, tables::read_rows, AppState},
    drafted,
};
use crate::{
    error::AppError,
    mail::{Draft, DraftChange},
};
use std::collections::BTreeMap;

/// The column holding each row's recipients; every other column names a
/// `{{key}}` placeholder of the template and holds its value.
const RECIPIENTS: &str = "to";

impl AppState {
    /// One draft from the mailbox `selector` names per row of `table`, the
    /// template `template` filled with the row's other columns and addressed
    /// to its `to`. A table without a header, a `to` column or a row, a
    /// column named twice, or a row of another length than the header or
    /// with no `to` address is `DRAFT_MERGE_INVALID`; an unreadable address
    /// is `OUTBOUND_RECIPIENT_INVALID`, and a column the template does not
    /// name or a placeholder no column fills the template's own refusal;
    /// each names the row's `to`. A recipient a send would refuse — one the
    /// suppression list covers or a stored bounce report says failed for
    /// good, as `message recipients` reads them — is
    /// `DRAFT_MERGE_INVALID` naming every such address and why, so the table
    /// is corrected before anything is drafted. None of them keeps a draft.
    pub fn merge_drafts(
        &self,
        organization_id: &str,
        selector: &str,
        template: &str,
        table: &str,
    ) -> Result<Vec<Draft>, AppError> {
        let invalid = |message: String| {
            AppError::invalid(
                "DRAFT_MERGE_INVALID",
                format!("{message}; nothing was drafted"),
            )
        };
        let mailbox = self.resolve_mailbox(organization_id, selector)?;
        let mut rows = read_rows(table)
            .map_err(|why| invalid(format!("the table cannot be read: {why}")))?
            .into_iter();
        let Some(header) = rows.next() else {
            return Err(invalid(format!("the table is empty; write a header row naming `{RECIPIENTS}` and the template's placeholders")));
        };
        let header: Vec<String> = header
            .iter()
            .map(|name| name.trim().to_lowercase())
            .collect();
        let Some(recipients_at) = header.iter().position(|name| name == RECIPIENTS) else {
            return Err(invalid(format!(
                "the header {} names no `{RECIPIENTS}` column",
                header.join(",")
            )));
        };
        for (at, name) in header.iter().enumerate() {
            if header[..at].contains(name) {
                return Err(invalid(format!(
                    "the header names the column `{name}` more than once"
                )));
            }
        }
        let mut drafts = Vec::new();
        let mut addressed = Vec::new();
        for row in rows {
            let to = match row.get(recipients_at) {
                Some(written) => written.trim().to_string(),
                None => String::new(),
            };
            if row.len() != header.len() {
                return Err(invalid(format!(
                    "the row for {to:?} holds {} fields and the header {}",
                    row.len(),
                    header.len()
                )));
            }
            let named = |error: AppError| AppError {
                message: format!("the row for {to:?}: {}", error.message),
                ..error
            };
            let recipients = normalize_addresses(&[to.clone()]).map_err(named)?;
            if recipients.is_empty() {
                return Err(invalid(format!("a row names no `{RECIPIENTS}` address")));
            }
            let values: BTreeMap<String, String> = header
                .iter()
                .zip(row.iter())
                .filter(|(name, _)| name.as_str() != RECIPIENTS)
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            let filled = self
                .render_template(organization_id, template, &values, None)
                .map_err(named)?;
            let change = DraftChange {
                to: Some(recipients.clone()),
                cc: None,
                bcc: None,
                subject: filled.subject,
                body: Some(filled.body),
                attachments: None,
            };
            drafts.push(drafted(mailbox.id, change)?);
            addressed.extend(recipients);
        }
        if drafts.is_empty() {
            return Err(invalid("the table holds a header and no row".to_string()));
        }
        let refused = self.unsendable(organization_id, &addressed)?;
        if !refused.is_empty() {
            return Err(invalid(format!(
                "a send would refuse {}; take them out of the table",
                refused.join(", ")
            )));
        }
        drafts
            .iter()
            .map(|draft| {
                self.database
                    .create_draft(organization_id, mailbox.id, draft)
            })
            .collect()
    }
}
