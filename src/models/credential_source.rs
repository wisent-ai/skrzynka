use serde::{Deserialize, Serialize};

/// Non-secret vault metadata observed for a mailbox read. This is not an
/// authentication verdict and never changes the stored synchronization error.
/// `item_held` answers whether this vault's item list carries the mailbox's
/// bound item; `item_error` says why that list could not be read, which is
/// never reported as an absent item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSource {
    pub vault: Option<String>,
    pub route: Option<String>,
    pub endpoint: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub item_held: Option<bool>,
    #[serde(default)]
    pub item_error: Option<String>,
}

impl CredentialSource {
    /// Record whether the vault's item list holds `item_id`, or why the list
    /// could not be read. Only the item list is read, never a payload.
    pub fn observe_item(
        &mut self,
        item_id: &str,
        items: &Result<Vec<super::SkarbiecItemMetadata>, crate::error::AppError>,
    ) {
        match items {
            Ok(items) => self.item_held = Some(items.iter().any(|item| item.id == item_id)),
            Err(error) => self.item_error = Some(format!("{}: {}", error.code, error.message)),
        }
    }

    /// The vault in words a refusal can carry: its identity and route, or why
    /// the vault could not be observed, so a missing item is never reported
    /// without the vault it was looked for in.
    pub fn describe(&self) -> String {
        match (&self.vault, &self.error) {
            (Some(vault), _) => {
                let mut text = format!("vault '{vault}'");
                if let Some(route) = &self.route {
                    text.push_str(&format!(" via {route}"));
                }
                if let Some(endpoint) = &self.endpoint {
                    text.push_str(&format!(" at {endpoint}"));
                }
                text
            }
            (None, Some(error)) => format!("an unidentified vault ({error})"),
            (None, None) => "an unidentified vault".to_string(),
        }
    }
}
