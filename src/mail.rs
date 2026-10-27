use crate::{
    error::AppError,
    models::{Mailbox, Message, OutboundMessage, SmtpSecurity},
    skarbiec::ResolvedCredentials,
};
use lettre::{
    message::{header::ContentType, Mailbox as LettreMailbox},
    transport::smtp::authentication::{Credentials, Mechanism},
    Message as OutgoingMessage, SmtpTransport, Transport,
};
use std::str::FromStr;
use uuid::Uuid;

// A message's files: described at import, read again from the provider.
mod attachments;
pub use attachments::{AttachmentPart, MessageAttachment};
pub use incoming::fetch_message_source;
pub use normalize::read_message;
// A mailing list's headers and its own way out.
mod lists;
pub use lists::{
    post_one_click, send_unsubscribe_mail, MailingList, Subscription, Subscriptions,
    UnsubscribeAttempt, UnsubscribeMethod, UnsubscribeRequest, UnsubscribeTarget,
};
// Delivery status notifications: which send bounced, and why.
mod reports;
pub use reports::{BounceReport, RecipientOutcome, StoredBounce};
// What the receiving provider concluded about a message's sender.
mod authentication;
pub use authentication::AuthenticationVerdict;
// What Skrzynka changes about one INBOX message at its provider.
mod actions;
pub use actions::{
    act_on_message, ActionStatus, MarkMessageRequest, MarkState, MessageAction,
    MoveMessageRequest, ProviderAction, SpecialUse,
};
// Files originated mail carries: checked, recorded and attached.
mod outgoing;
pub use outgoing::{
    prepare_attachments, quoted, signed, OutboundAttachment, OutgoingAttachment,
    PreparedAttachment, ReplyCopies,
};
use outgoing::{parse_recipient, split_addresses};
// Messages being written, kept in Skrzynka until they are sent.
mod drafts;
pub use drafts::{placeholders, Draft, DraftChange, MessageTemplate};
// The operator's rules that tag mail as it arrives.
mod rules;
pub use rules::{RuleSubject, TagRule};
// Recipients no originated mail may be sent to.
mod suppressions;
pub use suppressions::{suppression_entry, Suppression};
mod folders;
mod automation;
pub use automation::{machine_sent, AutoReply, AutoReplyDecision};
pub use folders::{append_message, list_folders, AppendOutcome, ProviderFolder};

mod incoming;
mod normalize;
mod password;
mod ports;
pub use incoming::{fetch_messages, wait_for_new_mail, GMAIL_IMAP_HOST};
use normalize::normalize_message;
pub use password::verify_gmail_app_password;
pub use ports::registered_tcp_port;

/// A reply through the mailbox that received `inbound`, threaded to it. With
/// files, the body is a `multipart/mixed` of the text and each file in the
/// order given. `copies` adds every To and Cc of the message, the addresses
/// asked for openly, and blind copies named in no header. An `automatic`
/// reply states `Auto-Submitted: auto-replied` (RFC 3834).
pub fn send_reply(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    inbound: &Message,
    body: &str,
    attachments: &[PreparedAttachment],
    copies: &ReplyCopies,
    automatic: bool,
) -> Result<String, AppError> {
    validate_body(body, "REPLY_BODY_INVALID")?;
    let from_address = mailbox.email.parse().map_err(|cause| {
        AppError::invalid(
            "MAILBOX_PROFILE_INVALID",
            format!(
                "mailbox sending address {:?} is invalid: {cause}",
                mailbox.email
            ),
        )
    })?;
    let from = LettreMailbox::new(Some(mailbox.display_name.clone()), from_address);
    // Reply-To names where the sender asked answers to go; a message that
    // names none is answered at its From address.
    let target = match inbound.reply_to.as_deref() {
        Some(reply_to) => reply_to,
        None => inbound.sender.as_str(),
    };
    let to = LettreMailbox::from_str(target).map_err(|cause| {
        AppError::invalid(
            "MESSAGE_REPLY_TARGET_INVALID",
            format!("the message's Reply-To or From address {target:?} is not valid: {cause}"),
        )
    })?;
    let provider_message_id = format!("<{}@skrzynka.local>", Uuid::new_v4());
    let mut builder = OutgoingMessage::builder()
        .from(from)
        .to(to)
        .message_id(Some(provider_message_id.clone()));
    // Every address is copied once: one --all already copies is not added again.
    let mut copied: Vec<String> = Vec::new();
    if copies.all {
        for everyone in outgoing::everyone_else(inbound, &mailbox.email, target)? {
            copied.push(everyone.email.to_string().to_lowercase());
            builder = builder.cc(everyone);
        }
    }
    for (addresses, blind) in [(&copies.cc, false), (&copies.bcc, true)] {
        for address in addresses.iter().filter(|address| !copied.contains(&address.to_lowercase())) {
            let recipient = parse_recipient(address)?;
            builder = if blind { builder.bcc(recipient) } else { builder.cc(recipient) };
        }
    }
    // A message that states no subject is answered with none: the reply
    // carries no subject its sender never wrote.
    match inbound.subject.as_deref() {
        Some(subject) if subject.to_ascii_lowercase().starts_with("re:") => {
            builder = builder.subject(subject);
        }
        Some(subject) => builder = builder.subject(format!("Re: {subject}")),
        None => {}
    }
    if let Some(message_id) = inbound.message_id.as_deref() {
        builder = builder.in_reply_to(message_id.to_string());
        let references = match inbound.references.as_deref() {
            Some(existing) => format!("{existing} {message_id}"),
            None => message_id.to_string(),
        };
        builder = builder.references(references);
    }
    if automatic {
        builder = builder.header(automation::AutoSubmitted::replied());
    }
    let built = if attachments.is_empty() {
        builder
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
    } else {
        builder.multipart(outgoing::body_with_attachments(body, attachments))
    };
    let outgoing = built.map_err(|cause| {
        AppError::invalid(
            "REPLY_MESSAGE_INVALID",
            format!("reply could not be encoded as an email message: {cause}"),
        )
    })?;
    deliver(mailbox, credentials, &outgoing)?;
    Ok(provider_message_id)
}

/// Mail this mailbox originates, from the stored outbound row; no threading
/// headers are written. Blind copies are envelope recipients only: the built
/// message drops its Bcc header. With files, the body is `multipart/mixed`.
pub fn send_outbound(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    outbound: &OutboundMessage,
    attachments: &[PreparedAttachment],
) -> Result<String, AppError> {
    validate_body(&outbound.body, "OUTBOUND_BODY_INVALID")?;
    let recipients = split_addresses(&outbound.recipients);
    if recipients.is_empty() {
        return Err(AppError::invalid(
            "OUTBOUND_RECIPIENT_INVALID",
            "outbound message must name at least one recipient",
        ));
    }
    let from_address = mailbox.email.parse().map_err(|cause| {
        AppError::invalid(
            "MAILBOX_PROFILE_INVALID",
            format!(
                "mailbox sending address {:?} is invalid: {cause}",
                mailbox.email
            ),
        )
    })?;
    let provider_message_id = format!("<{}@skrzynka.local>", Uuid::new_v4());
    let mut builder = OutgoingMessage::builder()
        .from(LettreMailbox::new(
            Some(mailbox.display_name.clone()),
            from_address,
        ))
        .subject(outbound.subject.clone())
        .message_id(Some(provider_message_id.clone()));
    for recipient in recipients {
        builder = builder.to(parse_recipient(recipient)?);
    }
    for (copies, blind) in [(outbound.cc.as_deref(), false), (outbound.bcc.as_deref(), true)] {
        for recipient in copies.map(split_addresses).into_iter().flatten() {
            let recipient = parse_recipient(recipient)?;
            builder = if blind { builder.bcc(recipient) } else { builder.cc(recipient) };
        }
    }
    let built = if attachments.is_empty() {
        builder
            .header(ContentType::TEXT_PLAIN)
            .body(outbound.body.clone())
    } else {
        builder.multipart(outgoing::body_with_attachments(&outbound.body, attachments))
    };
    let outgoing = built.map_err(|cause| {
        AppError::invalid(
            "OUTBOUND_MESSAGE_INVALID",
            format!("outbound message could not be encoded as an email message: {cause}"),
        )
    })?;
    deliver(mailbox, credentials, &outgoing)?;
    Ok(provider_message_id)
}

/// One transport for every send: the mailbox's own host, port, security mode
/// and the secret resolved for this one operation.
fn deliver(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    outgoing: &OutgoingMessage,
) -> Result<(), AppError> {
    let builder = match mailbox.smtp_security {
        SmtpSecurity::Starttls => SmtpTransport::starttls_relay(&mailbox.smtp_host),
        SmtpSecurity::Tls => SmtpTransport::relay(&mailbox.smtp_host),
    }
    .map_err(|error| {
        dependency_error(
            "SMTP_TLS_FAILED",
            format!("SMTP TLS configuration was rejected: {error}"),
            false,
        )
    })?
    .port(mailbox.smtp_port);
    let builder = match credentials {
        ResolvedCredentials::Password { username, password } => {
            builder.credentials(Credentials::new(username.clone(), password.clone()))
        }
        ResolvedCredentials::OAuth2 {
            username,
            access_token,
        } => builder
            .credentials(Credentials::new(username.clone(), access_token.clone()))
            .authentication(vec![Mechanism::Xoauth2]),
    };
    let transport = builder
        // No client limit (cli.md rule 8): the submission ends with the
        // server's answer or the connection's own error.
        .timeout(None)
        .build();
    let server = format!("{}:{}", mailbox.smtp_host, mailbox.smtp_port);
    stado_wait::blocking(
        stado_wait::Kind::Network,
        format!("SMTP submission from {}", mailbox.email),
        &server,
        || transport.send(outgoing),
    )
    .map_err(|error| {
        // The server's own sentence is the only thing that says why it
        // refused. A fixed message sends the operator hunting through logs
        // Skrzynka does not keep for a reason the provider already stated.
        if error.is_transient() || error.is_permanent() {
            dependency_error(
                "SMTP_REJECTED",
                format!("SMTP explicitly rejected the message: {error}"),
                error.is_transient(),
            )
        } else {
            dependency_error(
                "SMTP_UNCERTAIN",
                format!(
                    "SMTP acceptance is uncertain ({error}); inspect provider Sent mail before \
                     another attempt"
                ),
                false,
            )
        }
    })?;
    Ok(())
}

/// A body's size is the provider's to bound: its SMTP server refuses what it
/// will not carry, and that refusal is reported as the delivery's failure.
fn validate_body(body: &str, empty_code: &'static str) -> Result<(), AppError> {
    if body.trim().is_empty() {
        return Err(AppError::invalid(
            empty_code,
            "message body must not be empty",
        ));
    }
    Ok(())
}

fn dependency_error(code: &'static str, message: impl Into<String>, retryable: bool) -> AppError {
    AppError::dependency(code, message, retryable)
}
