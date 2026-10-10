use crate::{
    db::MailboxConfig,
    models::{Mailbox, MailboxDeclarationRefusal},
};

pub(super) fn mailbox_matches_config(mailbox: &Mailbox, config: &MailboxConfig) -> bool {
    mailbox.organization_id == config.organization_id
        && mailbox.skarbiec_item_id == config.skarbiec_item_id
        && mailbox.smtp_skarbiec_item_id == config.smtp_skarbiec_item_id
        && mailbox.display_name == config.display_name
        && mailbox.email == config.email
        && mailbox.imap_host == config.imap_host
        && mailbox.imap_port == config.imap_port
        && mailbox.smtp_host == config.smtp_host
        && mailbox.smtp_port == config.smtp_port
        && mailbox.smtp_security == config.smtp_security
}

/// The retained UID cursor belongs to the original receiving endpoint.
pub(super) fn endpoint_conflict(
    mailbox: &Mailbox,
    config: &MailboxConfig,
) -> Option<MailboxDeclarationRefusal> {
    (mailbox.email != config.email
        || mailbox.imap_host != config.imap_host
        || mailbox.imap_port != config.imap_port)
        .then(|| MailboxDeclarationRefusal {
            skarbiec_item_id: config.skarbiec_item_id.clone(),
            code: "MAILBOX_IMPORT_PROFILE_CONFLICT".to_string(),
            message: "the Skarbiec profile changes the receiving address or IMAP endpoint; the retained UID cursor cannot be reused and no import data was changed".to_string(),
        })
}

pub(super) fn apply_config(mailbox: &mut Mailbox, config: &MailboxConfig) {
    mailbox.smtp_skarbiec_item_id = config.smtp_skarbiec_item_id.clone();
    mailbox.display_name = config.display_name.clone();
    mailbox.smtp_host = config.smtp_host.clone();
    mailbox.smtp_port = config.smtp_port;
    mailbox.smtp_security = config.smtp_security;
}
