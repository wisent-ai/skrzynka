//! Inbound domains: the domains accounts are signed up on, whose every
//! address receives mail at a receiving provider. Skrzynka reads that mail
//! through wisent-integrations, stores each message once, and answers it by
//! recipient and sender, as it answers a mailbox's mail.

use super::{
    messages::{addressed_to, entry_address, filter_value, page_size},
    AppState,
};
use crate::{
    error::AppError,
    models::{InboundDomain, InboundMessage},
};
use chrono::Utc;
use lettre::Address;
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;

mod feed;
use feed::{Feed, Listed};

/// What one read of the receiving feed stored for a domain.
#[derive(Debug, Serialize)]
pub struct InboundSyncSummary {
    pub domain_id: Uuid,
    pub domain: String,
    pub received: usize,
    pub completed_at: String,
}

impl AppState {
    /// Declare a domain the organization receives mail for; declaring it
    /// again answers the same declaration.
    pub fn declare_inbound_domain(
        &self,
        organization_id: &str,
        domain: &str,
    ) -> Result<InboundDomain, AppError> {
        let domain = domain.trim().to_lowercase();
        // The postmaster address every domain has (RFC 5321) is how the
        // domain's syntax is checked: by the same parser that reads recipients.
        Address::new("postmaster", &domain).map_err(|error| {
            AppError::invalid(
                "INBOUND_DOMAIN_INVALID",
                format!("'{domain}' is not a mail domain: {error}"),
            )
        })?;
        self.database
            .declare_inbound_domain(organization_id, &domain)
    }

    pub fn list_inbound_domains(
        &self,
        organization_id: &str,
    ) -> Result<Vec<InboundDomain>, AppError> {
        self.database.list_inbound_domains(organization_id)
    }

    /// Delete a declaration and every message received for it; refused
    /// unless the caller confirms, because the mail cannot be restored.
    pub fn remove_inbound_domain(
        &self,
        organization_id: &str,
        id: Uuid,
        confirm: bool,
    ) -> Result<(), AppError> {
        if !confirm {
            return Err(AppError::invalid(
                "CONFIRMATION_REQUIRED",
                "removing an inbound domain deletes every message Skrzynka received for it; confirm the removal",
            ));
        }
        self.database.delete_inbound_domain(organization_id, id)
    }

    /// Read the receiving feed and store every message for the domain that
    /// is not stored yet. The outcome is kept on the domain: a failed read
    /// is recorded with its cause and refused, never answered as no mail.
    pub async fn sync_inbound_domain(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<InboundSyncSummary, AppError> {
        let domain = self.database.get_inbound_domain(organization_id, id)?;
        match self.receive(&domain).await {
            Ok(received) => {
                self.database.record_inbound_read(id, None)?;
                Ok(InboundSyncSummary {
                    domain_id: id,
                    domain: domain.domain,
                    received,
                    completed_at: Utc::now().to_rfc3339(),
                })
            }
            Err(error) => {
                self.database
                    .record_inbound_read(id, Some((error.code, &error.message)))?;
                Err(error)
            }
        }
    }

    /// Whether `recipient` is an address of a domain this organization
    /// declared inbound; a value that is not an address is not one.
    pub fn is_inbound_recipient(
        &self,
        organization_id: &str,
        recipient: &str,
    ) -> Result<bool, AppError> {
        match Address::from_str(recipient.trim()) {
            Ok(address) => Ok(self
                .database
                .inbound_domain_named(organization_id, address.domain())?
                .is_some()),
            Err(error) => {
                // Not an address, so no inbound domain holds it; the
                // mailboxes answer it, and their filter refuses what is blank.
                tracing::debug!(%error, recipient, "recipient is not a mail address");
                Ok(false)
            }
        }
    }

    /// The mail one address of a declared inbound domain received, newest
    /// first, after reading the feed for that domain; narrowed by a sender
    /// text and cut to `limit` when given.
    pub async fn read_inbound(
        &self,
        organization_id: &str,
        recipient: Option<&str>,
        sender: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<InboundMessage>, AppError> {
        page_size(limit)?;
        let recipient = filter_value(recipient, "recipient")?.ok_or_else(|| {
            AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                "name the recipient address whose mail to read; an inbound domain is read one address at a time",
            )
        })?;
        let sender = filter_value(sender, "sender")?;
        let address = Address::from_str(&recipient).map_err(|error| {
            AppError::invalid(
                "INBOUND_RECIPIENT_INVALID",
                format!("'{recipient}' is not a mail address: {error}"),
            )
        })?;
        let domain = self
            .database
            .inbound_domain_named(organization_id, address.domain())?
            .ok_or_else(|| {
                AppError::invalid(
                    "INBOUND_DOMAIN_NOT_DECLARED",
                    format!(
                        "{} is not an inbound domain of this organization; declare it with skrzynka mailbox declare --inbound-domain {}",
                        address.domain(),
                        address.domain()
                    ),
                )
            })?;
        self.sync_inbound_domain(organization_id, domain.id).await?;
        let matching = self
            .database
            .list_inbound_messages(domain.id)?
            .into_iter()
            .filter(|message| addressed_to(&message.recipients, &recipient))
            .filter(|message| match &sender {
                Some(part) => message.sender.to_lowercase().contains(part.as_str()),
                None => true,
            });
        Ok(match limit {
            Some(limit) => matching.take(limit as usize).collect(),
            None => matching.collect(),
        })
    }

    /// Walk the feed newest first until a message of this domain that is
    /// already stored, then store the new ones oldest first: a read that
    /// fails part way keeps what it stored, and the next read fills the rest.
    async fn receive(&self, domain: &InboundDomain) -> Result<usize, AppError> {
        let feed = Feed::from_vault(&self.resolver).await?;
        let suffix = format!("@{}", domain.domain.to_lowercase());
        let mut fresh: Vec<Listed> = Vec::new();
        let mut after: Option<String> = None;
        'pages: loop {
            let (page, more) = feed.page(after.as_deref()).await?;
            let last = page.last().map(|listed| listed.id.clone());
            for listed in page {
                let ours = listed
                    .recipients
                    .iter()
                    .any(|recipient| entry_address(recipient).to_lowercase().ends_with(&suffix));
                if !ours {
                    continue;
                }
                if self
                    .database
                    .inbound_message_stored(domain.id, &listed.id)?
                {
                    break 'pages;
                }
                fresh.push(listed);
            }
            match (more, last) {
                (true, Some(last)) => after = Some(last),
                _ => break,
            }
        }
        let received = fresh.len();
        for listed in fresh.into_iter().rev() {
            let body_text = feed.body(&listed.id).await?;
            self.database.store_inbound_message(&InboundMessage {
                id: Uuid::new_v4(),
                domain_id: domain.id,
                provider_id: listed.id,
                sender: listed.sender,
                recipients: listed.recipients.join(", "),
                subject: listed.subject,
                received_at: listed.received_at,
                body_text,
            })?;
        }
        Ok(received)
    }
}
