use serde::{Deserialize, Serialize};

/// Non-secret vault metadata observed for a mailbox read. This is not an
/// authentication verdict and never changes the stored synchronization error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSource {
    pub vault: Option<String>,
    pub route: Option<String>,
    pub endpoint: Option<String>,
    pub error: Option<String>,
}
