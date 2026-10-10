use super::Mailbox;

impl Mailbox {
    /// Reconstruct guidance from the stored failure, without contacting a provider
    /// or claiming that a readable credential has authenticated.
    pub(crate) fn recovery_command(&self) -> Option<String> {
        let code = self.last_error_code.as_deref()?;
        match code {
            "SKARBIEC_ITEM_INVALID" | "SKARBIEC_UNAVAILABLE" | "SKARBIEC_RESPONSE_INVALID" => {
                Some("skarbiec status".to_string())
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
