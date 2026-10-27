//! A mailbox's signature: the text its replies, originated messages and
//! forwards carry after an RFC 3676 `-- ` line. Unsubscribe mail is a
//! request to a machine and carries none.

use super::AppState;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What `mailbox signature` answers.
#[derive(Debug, Serialize)]
pub struct MailboxSignature {
    pub mailbox_id: Uuid,
    pub signature: Option<String>,
}

/// `PUT /v1/mailboxes/:id/signature`: `signature` sets it, `null` or an
/// absent field removes it.
#[derive(Debug, Deserialize)]
pub struct SignatureRequest {
    pub signature: Option<String>,
}

impl AppState {
    pub fn mailbox_signature(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
    ) -> Result<MailboxSignature, AppError> {
        Ok(MailboxSignature {
            mailbox_id,
            signature: self.database.signature(organization_id, mailbox_id)?,
        })
    }

    /// Set the signature, or remove it with `None`. A signature that holds
    /// no visible text is refused rather than stored as an empty one.
    pub fn set_mailbox_signature(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        signature: Option<&str>,
    ) -> Result<MailboxSignature, AppError> {
        let signature = match signature.map(str::trim_end) {
            Some(text) if text.trim().is_empty() => {
                return Err(AppError::invalid(
                    "SIGNATURE_INVALID",
                    "the signature holds no visible text; write one, or remove the signature",
                ));
            }
            other => other,
        };
        Ok(MailboxSignature {
            mailbox_id,
            signature: self
                .database
                .set_signature(organization_id, mailbox_id, signature)?,
        })
    }
}
