//! Outbound mail: claim, send and read.

use super::AppState;
use crate::{
    error::AppError,
    mail,
    models::{
        CreateOutboundRequest, DeliveryStatus, Mailbox, OutboundMessage, MAX_BODY_BYTES,
        MAX_IDEMPOTENCY_KEY_LENGTH, MAX_SUBJECT_CHARS,
    },
};
use lettre::Address;
use std::str::FromStr;
use uuid::Uuid;

impl AppState {
    pub fn list_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<OutboundMessage>, AppError> {
        self.database
            .list_outbound(organization_id, mailbox_id, limit.clamp(1, 500), offset)
    }

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
        let mut matches = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| mailbox.email.eq_ignore_ascii_case(selector))
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            let ids = matches
                .iter()
                .map(|mailbox| mailbox.id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AppError::conflict(
                "MAILBOX_SELECTOR_AMBIGUOUS",
                format!(
                    "{selector} names {} mailboxes ({ids}); select one by id",
                    matches.len()
                ),
            ));
        }
        matches.pop().ok_or_else(|| AppError::not_found("mailbox"))
    }

    /// Originate mail from one mailbox. The row is claimed by its idempotency
    /// key before anything leaves this process, so repeating the same call
    /// returns the first attempt instead of handing the provider a second copy.
    pub async fn send_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        request: CreateOutboundRequest,
    ) -> Result<OutboundMessage, AppError> {
        let normalized = validate_outbound_request(&request)?;
        let (outbound, created) = self.database.begin_outbound(
            organization_id,
            mailbox_id,
            request.idempotency_key.trim(),
            &normalized.recipients,
            normalized.cc.as_deref(),
            &normalized.subject,
            &normalized.body,
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
        let mailbox = self.database.get_mailbox(organization_id, mailbox_id)?;
        let credentials = match self
            .resolver
            .resolve_credentials(mailbox.outbound_skarbiec_item_id())
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let _ = self.database.update_outbound(
                    outbound.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error);
            }
        };
        let pending = outbound.clone();
        let result = tokio::task::spawn_blocking(move || {
            mail::send_outbound(&mailbox, &credentials, &pending)
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
            Ok(Err(error)) if error.code == "SMTP_UNCERTAIN" => self.database.update_outbound(
                outbound.id,
                DeliveryStatus::Uncertain,
                None,
                Some("OUTBOUND_UNCERTAIN"),
                Some(&error.message),
            ),
            Ok(Err(error)) => {
                let _ = self.database.update_outbound(
                    outbound.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error)
            }
            Err(_) => self.database.update_outbound(
                outbound.id,
                DeliveryStatus::Uncertain,
                None,
                Some("OUTBOUND_UNCERTAIN"),
                Some("send task stopped before terminal SMTP evidence was recorded"),
            ),
        }
    }
}

struct NormalizedOutbound {
    recipients: String,
    cc: Option<String>,
    subject: String,
    body: String,
}

fn validate_outbound_request(
    request: &CreateOutboundRequest,
) -> Result<NormalizedOutbound, AppError> {
    let key = request.idempotency_key.trim();
    if key.is_empty()
        || key.len() > MAX_IDEMPOTENCY_KEY_LENGTH
        || key.chars().any(char::is_whitespace)
    {
        return Err(AppError::invalid(
            "IDEMPOTENCY_KEY_INVALID",
            "idempotency_key must contain 1 to 200 non-whitespace characters",
        ));
    }
    let subject = request.subject.trim().to_string();
    if subject.is_empty() || subject.chars().count() > MAX_SUBJECT_CHARS {
        return Err(AppError::invalid(
            "OUTBOUND_SUBJECT_INVALID",
            "subject must contain between 1 and 500 characters",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(AppError::invalid(
            "OUTBOUND_BODY_INVALID",
            "outbound body must not be empty",
        ));
    }
    if request.body.len() > MAX_BODY_BYTES {
        return Err(AppError::invalid(
            "OUTBOUND_BODY_TOO_LARGE",
            "outbound body exceeds the 256 KiB limit",
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
    Ok(NormalizedOutbound {
        recipients: recipients.join(", "),
        cc: (!cc.is_empty()).then(|| cc.join(", ")),
        subject,
        body: request.body.trim_end().to_string(),
    })
}

/// Recipients are bare addresses, deduplicated in the order given. A display
/// name is refused here rather than at the provider, where the refusal would
/// arrive after the row was already claimed.
fn normalize_addresses(values: &[String]) -> Result<Vec<String>, AppError> {
    let mut addresses = Vec::with_capacity(values.len());
    for value in values {
        for candidate in value.split(',') {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            Address::from_str(candidate).map_err(|_| {
                AppError::invalid(
                    "OUTBOUND_RECIPIENT_INVALID",
                    format!("{candidate} is not a valid email address"),
                )
            })?;
            if !addresses.iter().any(|existing| existing == candidate) {
                addresses.push(candidate.to_string());
            }
        }
    }
    Ok(addresses)
}
