//! Drafts: messages written over time from one mailbox and kept in Skrzynka
//! until they are sent. Sending originates the draft exactly as
//! `message send` does (claimed by its key, under the send limit, signed);
//! the draft is removed once the provider accepted it, and kept otherwise so
//! it can be corrected and sent again.

use super::AppState;
use crate::{
    error::AppError,
    mail::{prepare_attachments, Draft, DraftChange},
    models::{CreateOutboundRequest, DeliveryStatus, OutboundMessage},
};
use serde::Deserialize;
use uuid::Uuid;

mod listing;
mod merge;
mod schedule;
mod staggered;
pub use listing::DraftFilter;

/// How a draft is sent: `POST /v1/drafts/:id/send`, `message draft send`.
#[derive(Debug, Deserialize)]
pub struct DraftSendRequest {
    /// Key that makes a repeated send the same send.
    pub idempotency_key: String,
    /// Send even to an address a stored bounce report says delivery to failed for good.
    #[serde(default)]
    pub allow_bounced: bool,
    /// Expect an answer by this RFC 3339 instant or span from now once sent.
    #[serde(default)]
    pub follow_up: Option<String>,
}

impl AppState {
    /// Start a draft from the mailbox `selector` names (its ID or address).
    pub fn create_draft(
        &self,
        organization_id: &str,
        selector: &str,
        change: DraftChange,
    ) -> Result<Draft, AppError> {
        let mailbox = self.resolve_mailbox(organization_id, selector)?;
        let draft = drafted(mailbox.id, change)?;
        self.database
            .create_draft(organization_id, mailbox.id, &draft)
    }

    /// Change the fields `change` names and keep the others.
    pub fn update_draft(
        &self,
        organization_id: &str,
        id: Uuid,
        change: DraftChange,
    ) -> Result<Draft, AppError> {
        let mut draft = self.database.get_draft(organization_id, id)?;
        apply(&mut draft, change)?;
        self.database.update_draft(organization_id, &draft)
    }

    /// Start a draft from an originated message: its mailbox, To, Cc, Bcc,
    /// subject and body, whatever the message's status. The row keeps only
    /// the names and digests of the files it carried, not their bytes, so
    /// the draft carries none; they are attached again with `update`.
    pub fn draft_from_outbound(
        &self,
        organization_id: &str,
        outbound_id: Uuid,
    ) -> Result<Draft, AppError> {
        let sent = self.get_outbound(organization_id, outbound_id)?;
        let addresses = super::outbound::stored_addresses;
        let change = DraftChange {
            to: Some(addresses(&sent.recipients)),
            cc: sent.cc.as_deref().map(addresses),
            bcc: sent.bcc.as_deref().map(addresses),
            subject: Some(sent.subject),
            body: Some(sent.body),
            attachments: None,
        };
        self.create_draft(organization_id, &sent.mailbox_id.to_string(), change)
    }

    /// A new draft holding the same addresses, subject, body and files as
    /// draft `id`, written from the mailbox `mailbox` names (its ID or
    /// address) or from the same mailbox; the copy is not scheduled, and the
    /// original is left as it was.
    pub fn copy_draft(
        &self,
        organization_id: &str,
        id: Uuid,
        mailbox: Option<&str>,
    ) -> Result<Draft, AppError> {
        let draft = self.database.get_draft(organization_id, id)?;
        let mailbox_id = match mailbox {
            Some(selector) => self.resolve_mailbox(organization_id, selector)?.id,
            None => draft.mailbox_id,
        };
        self.database
            .create_draft(organization_id, mailbox_id, &draft)
    }

    pub fn get_draft(&self, organization_id: &str, id: Uuid) -> Result<Draft, AppError> {
        self.database.get_draft(organization_id, id)
    }

    pub fn delete_draft(&self, organization_id: &str, id: Uuid) -> Result<(), AppError> {
        self.database.delete_draft(organization_id, id)
    }

    /// Remove every draft mentioning `filter.contains` (of its mailbox when
    /// it names one), such as a mail merge made by mistake, and answer the
    /// drafts removed as they stood. A filter naming no text is
    /// `MESSAGE_FILTER_INVALID`, so nothing removes every draft at once;
    /// none listed is `NOT_FOUND`.
    pub fn delete_drafts(
        &self,
        organization_id: &str,
        filter: &DraftFilter,
    ) -> Result<Vec<Draft>, AppError> {
        if filter.contains.is_none() {
            return Err(AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                "name the text the drafts to delete contain, or delete one draft by its ID",
            ));
        }
        let listed = self.list_drafts(organization_id, filter)?;
        if listed.is_empty() {
            return Err(AppError::not_found("draft"));
        }
        for draft in &listed {
            self.database.delete_draft(organization_id, draft.id)?;
        }
        Ok(listed)
    }

    /// Originate the draft from its mailbox as `request` asks and answer the
    /// outbound row. A row the provider accepted removes the draft; any
    /// other outcome keeps it.
    pub async fn send_draft(
        &self,
        organization_id: &str,
        id: Uuid,
        request: DraftSendRequest,
    ) -> Result<OutboundMessage, AppError> {
        let draft = self.database.get_draft(organization_id, id)?;
        let sent = self
            .send_outbound(
                organization_id,
                draft.mailbox_id,
                CreateOutboundRequest {
                    idempotency_key: request.idempotency_key,
                    to: vec![draft.recipients.clone()],
                    cc: match draft.cc.trim() {
                        "" => Vec::new(),
                        cc => vec![cc.to_string()],
                    },
                    bcc: match draft.bcc.trim() {
                        "" => Vec::new(),
                        bcc => vec![bcc.to_string()],
                    },
                    subject: draft.subject.clone(),
                    body: draft.body.clone(),
                    attachments: draft.files.clone(),
                    allow_bounced: request.allow_bounced,
                    follow_up: request.follow_up,
                },
            )
            .await?;
        if sent.status == DeliveryStatus::Sent {
            self.database.delete_draft(organization_id, id).map_err(|error| {
                AppError::internal(format!(
                    "draft {id} was sent as outbound message {} but could not be removed: {}; delete it with `skrzynka message draft delete {id}`",
                    sent.id, error.message
                ))
            })?;
        }
        Ok(sent)
    }
}

/// Every address the draft's To, Cc and Bcc name, bare and in lower case, as
/// `message bounced` writes them.
fn addressed(draft: &Draft) -> impl Iterator<Item = String> + '_ {
    [&draft.recipients, &draft.cc, &draft.bcc]
        .into_iter()
        .flat_map(|header| header.split(','))
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| super::messages::entry_address(entry).trim().to_lowercase())
}

impl AppState {
    /// Each of `addresses` a send would refuse — one the suppression list
    /// covers or a stored bounce report says failed for good, as `message
    /// recipients` reads them — written with why, for a refusal that
    /// drafts many at once to name them all.
    pub(in crate::service) fn unsendable(
        &self,
        organization_id: &str,
        addresses: &[String],
    ) -> Result<Vec<String>, AppError> {
        Ok(self
            .check_recipients(organization_id, addresses)?
            .into_iter()
            .filter(|check| !check.sendable)
            .map(|check| match check.suppressed {
                Some(entry) => format!(
                    "{} (the suppression list holds {})",
                    check.address, entry.entry
                ),
                None => format!(
                    "{} (a stored bounce report says delivery to it failed for good)",
                    check.address
                ),
            })
            .collect())
    }
}

/// A new draft from mailbox `mailbox_id` holding what `change` names and
/// nothing else, not yet kept.
pub(in crate::service) fn drafted(
    mailbox_id: Uuid,
    change: DraftChange,
) -> Result<Draft, AppError> {
    let mut draft = Draft {
        id: Uuid::nil(),
        mailbox_id,
        recipients: String::new(),
        cc: String::new(),
        bcc: String::new(),
        subject: String::new(),
        body: String::new(),
        files: Vec::new(),
        attachments: Vec::new(),
        created_at: String::new(),
        updated_at: String::new(),
        send_at: None,
        send_key: None,
        schedule_error: None,
    };
    apply(&mut draft, change)?;
    Ok(draft)
}

/// Write each field `change` names into `draft`. Files are checked as a send
/// checks them, so a file that could never be sent is refused when it is
/// added, with `OUTBOUND_ATTACHMENT_INVALID`, not when the draft is sent.
fn apply(draft: &mut Draft, change: DraftChange) -> Result<(), AppError> {
    if let Some(to) = change.to {
        draft.recipients = to.join(", ");
    }
    if let Some(cc) = change.cc {
        draft.cc = cc.join(", ");
    }
    if let Some(bcc) = change.bcc {
        draft.bcc = bcc.join(", ");
    }
    if let Some(subject) = change.subject {
        draft.subject = subject;
    }
    if let Some(body) = change.body {
        draft.body = body;
    }
    if let Some(files) = change.attachments {
        prepare_attachments(&files)?;
        draft.files = files;
    }
    Ok(())
}
