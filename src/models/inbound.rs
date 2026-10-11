//! Inbound domains and the mail Skrzynka received for them.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A domain whose every address receives mail at a receiving provider, which
/// Skrzynka reads through wisent-integrations. The last read's outcome is kept
/// on it, as a mailbox keeps its last sync: a failed read is never an empty one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundDomain {
    pub id: Uuid,
    #[serde(skip_serializing)]
    pub organization_id: String,
    pub domain: String,
    pub last_sync_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// One message received for an address of an inbound domain, stored once by
/// the receiving provider's own id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub id: Uuid,
    pub domain_id: Uuid,
    pub provider_id: String,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub received_at: String,
    pub body_text: String,
}
