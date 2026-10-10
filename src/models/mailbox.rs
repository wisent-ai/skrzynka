use super::Mailbox;
use std::path::Path;

impl Mailbox {
    /// Reconstruct guidance from the stored failure, without contacting a provider
    /// or claiming that a readable credential has authenticated. A credential
    /// failure names the Skarbiec executable the resolver actually runs
    /// (`--skarbiec-bin`); with an explicit credentials file there is no
    /// Skarbiec to ask, and `credential_source` names that file instead.
    pub(crate) fn recovery_command(&self, skarbiec: Option<&Path>) -> Option<String> {
        let code = self.last_error_code.as_deref()?;
        match code {
            "SKARBIEC_ITEM_INVALID" | "SKARBIEC_UNAVAILABLE" | "SKARBIEC_RESPONSE_INVALID" => {
                let binary = skarbiec?.display().to_string();
                Some(format!("{} status", quoted(&binary)))
            }
            "GMAIL_IMAP_PASSWORD_REJECTED" => Some(format!(
                "skrzynka --organization {} account app-password --provider gmail --email {} --mailbox {}",
                quoted(&self.organization_id), quoted(&self.email), self.id
            )),
            _ => None,
        }
    }
}

fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
