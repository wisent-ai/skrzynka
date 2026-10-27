//! Outbound mail: claim, send and read.

use super::AppState;
use crate::{
    error::AppError,
    mail,
    models::{CreateOutboundRequest, DeliveryStatus, Mailbox, OutboundMessage},
};
use lettre::Address;
use std::str::FromStr;
use uuid::Uuid;

impl AppState {
    pub fn get_outbound(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<OutboundMessage, AppError> {
        self.database.get_outbound(organization_id, id)
    }

    /// A selector is either the mailbox id or the address itself. Operators
    /// know the address they send from; nothing should make them look up a
    /// UUID before they can use it.
    ///
    /// Two mailboxes can legitimately carry one address — the same account
    /// reached through a different Skarbiec item, say a provider relay beside a
    /// delegated Gmail row. Picking the first of them would send from whichever
    /// one sorted earlier, so an address that names more than one mailbox is
    /// refused with both ids instead.
    pub fn resolve_mailbox(
        &self,
        organization_id: &str,
        selector: &str,
    ) -> Result<Mailbox, AppError> {
        let selector = selector.trim();
        if let Ok(id) = Uuid::parse_str(selector) {
            return self.database.get_mailbox(organization_id, id);
        }
        let matches = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| mailbox.email.eq_ignore_ascii_case(selector))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Err(AppError::not_found("mailbox")),
            [mailbox] => Ok(mailbox.clone()),
            several => {
                let ids = several
                    .iter()
                    .map(|mailbox| mailbox.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(AppError::conflict(
                    "MAILBOX_SELECTOR_AMBIGUOUS",
                    format!(
                        "{selector} names {} mailboxes ({ids}); select one by id",
                        several.len()
                    ),
                ))
            }
        }
    }

    /// Originate mail from one mailbox. The row is claimed by its idempotency
    /// key before anything leaves this process, so repeating the same call
    /// returns the first attempt instead of handing the provider a second copy.
    /// `send_outbound` is the entry point; it adds the follow-up.
    pub(super) async fn originate(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        request: CreateOutboundRequest,
    ) -> Result<OutboundMessage, AppError> {
        let normalized = validate_outbound_request(&request)?;
        // A repeated key answers its first attempt whatever was learned since,
        // so only a key not claimed yet is held to the suppression list and
        // the bounce reports.
        let claimed = self.database.get_outbound_by_key(organization_id, request.idempotency_key.trim())?;
        if claimed.is_none() {
            let addressed: Vec<String> = [Some(normalized.recipients.as_str()), normalized.cc.as_deref(), normalized.bcc.as_deref()]
                .into_iter()
                .flatten()
                .flat_map(|header| header.split(", "))
                .map(str::to_lowercase)
                .collect();
            self.refuse_suppressed(organization_id, &addressed, "this send")?;
            if !request.allow_bounced {
                self.refuse_bounced(organization_id, &addressed)?;
            }
        }
        // Every file is checked before the row is claimed, so a file that
        // cannot be sent never leaves a claimed request behind.
        let files = mail::prepare_attachments(&request.attachments)?;
        let records: Vec<_> = files.iter().map(|file| file.record.clone()).collect();
        let (outbound, created) = self.database.begin_outbound(
            organization_id,
            mailbox_id,
            request.idempotency_key.trim(),
            &normalized.recipients,
            normalized.cc.as_deref(),
            normalized.bcc.as_deref(),
            &normalized.subject,
            &normalized.body,
            &records,
        )?;
        if !created {
            return Ok(outbound);
        }
        let outbound = self.database.update_outbound(
            outbound.id,
            DeliveryStatus::Sending,
            None,
            None,
            None,
        )?;
        let mailbox = match self.database.get_mailbox(organization_id, mailbox_id) {
            Ok(mailbox) => mailbox,
            Err(error) => {
                let recorded = self.database.update_outbound(
                    outbound.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error.with_unrecorded(recorded, "sending"));
            }
        };
        let credentials = match self
            .resolver
            .resolve_credentials(mailbox.outbound_skarbiec_item_id())
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let recorded = self.database.update_outbound(
                    outbound.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error.with_unrecorded(recorded, "sending"));
            }
        };
        let mut pending = outbound.clone();
        // The signature is the one the row was claimed with.
        pending.body = mail::signed(&pending.body, pending.signature.as_deref());
        let result = tokio::task::spawn_blocking(move || {
            mail::send_outbound(&mailbox, &credentials, &pending, &files)
        })
        .await;
        match result {
            Ok(Ok(provider_message_id)) => self.database.update_outbound(
                outbound.id,
                DeliveryStatus::Sent,
                Some(&provider_message_id),
                None,
                None,
            ),
            Ok(Err(error)) if error.code == "SMTP_UNCERTAIN" => self
                .database
                .update_outbound(
                    outbound.id,
                    DeliveryStatus::Uncertain,
                    None,
                    Some("OUTBOUND_UNCERTAIN"),
                    Some(&error.message),
                )
                .map_err(|write| error.with_unrecorded::<()>(Err(write), "sending")),
            Ok(Err(error)) => {
                let recorded = self.database.update_outbound(
                    outbound.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error.with_unrecorded(recorded, "sending"))
            }
            Err(stopped) => {
                let error = AppError::dependency(
                    "OUTBOUND_UNCERTAIN",
                    format!(
                        "send task stopped before terminal SMTP evidence was recorded: {stopped}"
                    ),
                    false,
                );
                self.database
                    .update_outbound(
                        outbound.id,
                        DeliveryStatus::Uncertain,
                        None,
                        Some(error.code),
                        Some(&error.message),
                    )
                    .map_err(|write| error.with_unrecorded::<()>(Err(write), "sending"))
            }
        }
    }
}

struct NormalizedOutbound {
    recipients: String,
    cc: Option<String>,
    bcc: Option<String>,
    subject: String,
    body: String,
}

/// An idempotency key as every claim reads it, trimmed; one that is blank or
/// holds whitespace is `IDEMPOTENCY_KEY_INVALID`, before anything is claimed.
pub(in crate::service) fn checked_key(key: &str) -> Result<&str, AppError> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return Err(AppError::invalid(
            "IDEMPOTENCY_KEY_INVALID",
            "idempotency_key must contain non-whitespace characters only",
        ));
    }
    Ok(key)
}

/// The addresses of a header an outbound row or a draft stores
/// comma-separated, each as written, blanks left out: what sending the row
/// again, drafting from it or reminding its recipients addresses.
pub(in crate::service) fn stored_addresses(stored: &str) -> Vec<String> {
    stored.split(',').map(str::trim).filter(|address| !address.is_empty()).map(String::from).collect()
}

fn validate_outbound_request(
    request: &CreateOutboundRequest,
) -> Result<NormalizedOutbound, AppError> {
    checked_key(&request.idempotency_key)?;
    let subject = request.subject.trim().to_string();
    if subject.is_empty() {
        return Err(AppError::invalid(
            "OUTBOUND_SUBJECT_INVALID",
            "subject must not be empty",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(AppError::invalid(
            "OUTBOUND_BODY_INVALID",
            "outbound body must not be empty",
        ));
    }
    let recipients = normalize_addresses(&request.to)?;
    if recipients.is_empty() {
        return Err(AppError::invalid(
            "OUTBOUND_RECIPIENT_INVALID",
            "at least one recipient address is required",
        ));
    }
    let cc = normalize_addresses(&request.cc)?;
    let bcc = normalize_addresses(&request.bcc)?;
    Ok(NormalizedOutbound {
        recipients: recipients.join(", "),
        cc: (!cc.is_empty()).then(|| cc.join(", ")),
        bcc: (!bcc.is_empty()).then(|| bcc.join(", ")),
        subject,
        body: request.body.trim_end().to_string(),
    })
}

/// Recipients are bare addresses, deduplicated in the order given. A display
/// name is refused here rather than at the provider, where the refusal would
/// arrive after the row was already claimed.
pub(super) fn normalize_addresses(values: &[String]) -> Result<Vec<String>, AppError> {
    let mut addresses = Vec::with_capacity(values.len());
    for value in values {
        for candidate in value.split(',') {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            Address::from_str(candidate).map_err(|cause| {
                AppError::invalid(
                    "OUTBOUND_RECIPIENT_INVALID",
                    format!("{candidate} is not a valid email address: {cause}"),
                )
            })?;
            if !addresses.iter().any(|existing| existing == candidate) {
                addresses.push(candidate.to_string());
            }
        }
    }
    Ok(addresses)
}
