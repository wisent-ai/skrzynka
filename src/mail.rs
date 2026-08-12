use crate::{
    error::AppError,
    models::{Mailbox, Message, NewMessage, SmtpSecurity},
    skarbiec::ResolvedCredentials,
};
use lettre::{
    message::{header::ContentType, Mailbox as LettreMailbox},
    transport::smtp::authentication::{Credentials, Mechanism},
    Message as OutgoingMessage, SmtpTransport, Transport,
};
use mailparse::{MailHeaderMap, ParsedMail};
use std::{str::FromStr, time::Duration};
use uuid::Uuid;

const MAX_RAW_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_REPLY_BYTES: usize = 256 * 1024;
const MAX_MESSAGES_PER_SYNC: usize = 200;

#[derive(Debug)]
pub struct FetchedMessages {
    pub messages: Vec<NewMessage>,
    pub skipped: usize,
    pub last_uid: u32,
}

struct OAuth2Authenticator<'a> {
    username: &'a str,
    access_token: &'a str,
}

impl imap::Authenticator for OAuth2Authenticator<'_> {
    type Response = String;

    fn process(&self, _: &[u8]) -> Self::Response {
        format!(
            "user={}\u{1}auth=Bearer {}\u{1}\u{1}",
            self.username, self.access_token
        )
    }
}

pub fn fetch_messages(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
) -> Result<FetchedMessages, AppError> {
    let client = imap::ClientBuilder::new(mailbox.imap_host.as_str(), mailbox.imap_port)
        .mode(imap::ConnectionMode::Tls)
        .tls_kind(imap::TlsKind::Native)
        .connect()
        .map_err(|_| {
            dependency_error(
                "IMAP_UNAVAILABLE",
                "IMAP server could not be reached over TLS",
                true,
            )
        })?;
    let mut session = match credentials {
        ResolvedCredentials::Password { username, password } => {
            client.login(username, password).map_err(|_| {
                dependency_error(
                    "IMAP_AUTHENTICATION_FAILED",
                    "IMAP authentication was refused; inspect the selected Skarbiec item",
                    false,
                )
            })?
        }
        ResolvedCredentials::OAuth2 {
            username,
            access_token,
        } => client
            .authenticate(
                "XOAUTH2",
                &OAuth2Authenticator {
                    username,
                    access_token,
                },
            )
            .map_err(|_| {
                dependency_error(
                    "IMAP_AUTHENTICATION_FAILED",
                    "Google refused the saved Gmail authorization; reconnect the profile",
                    false,
                )
            })?,
    };
    session.select("INBOX").map_err(|_| {
        dependency_error(
            "IMAP_INBOX_UNAVAILABLE",
            "the provider did not make INBOX available",
            true,
        )
    })?;

    let first_uid = mailbox.last_uid.saturating_add(1).max(1);
    let query = format!("UID {first_uid}:*");
    let mut uids = session
        .uid_search(query)
        .map_err(|_| dependency_error("IMAP_SEARCH_FAILED", "IMAP UID search failed", true))?
        .into_iter()
        .filter(|uid| *uid >= first_uid)
        .collect::<Vec<_>>();
    uids.sort_unstable();
    if uids.len() > MAX_MESSAGES_PER_SYNC {
        uids.truncate(MAX_MESSAGES_PER_SYNC);
    }

    let mut messages = Vec::with_capacity(uids.len());
    let mut skipped = 0usize;
    let mut last_uid = mailbox.last_uid;
    for requested_uid in uids {
        let fetches = session
            .uid_fetch(requested_uid.to_string(), "(UID BODY.PEEK[])")
            .map_err(|_| {
                dependency_error(
                    "IMAP_FETCH_FAILED",
                    format!("IMAP fetch failed at UID {requested_uid}"),
                    true,
                )
            })?;
        let Some(fetch) = fetches.iter().next() else {
            skipped += 1;
            last_uid = last_uid.max(requested_uid);
            continue;
        };
        let uid = fetch.uid.unwrap_or(requested_uid);
        last_uid = last_uid.max(uid);
        let Some(body) = fetch.body() else {
            skipped += 1;
            continue;
        };
        if body.len() > MAX_RAW_MESSAGE_BYTES {
            skipped += 1;
            continue;
        }
        match normalize_message(uid, body) {
            Ok(message) => messages.push(message),
            Err(_) => skipped += 1,
        }
    }
    let _ = session.logout();
    Ok(FetchedMessages {
        messages,
        skipped,
        last_uid,
    })
}

pub fn send_reply(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    inbound: &Message,
    body: &str,
) -> Result<String, AppError> {
    if body.trim().is_empty() {
        return Err(AppError::invalid(
            "REPLY_BODY_INVALID",
            "reply body must not be empty",
        ));
    }
    if body.len() > MAX_REPLY_BYTES {
        return Err(AppError::invalid(
            "REPLY_BODY_TOO_LARGE",
            "reply body exceeds the 256 KiB limit",
        ));
    }
    let from_address = mailbox.email.parse().map_err(|_| {
        AppError::invalid(
            "MAILBOX_PROFILE_INVALID",
            "mailbox sending address is invalid",
        )
    })?;
    let from = LettreMailbox::new(Some(mailbox.display_name.clone()), from_address);
    let target = inbound
        .reply_to
        .as_deref()
        .unwrap_or(inbound.sender.as_str());
    let to = LettreMailbox::from_str(target).map_err(|_| {
        AppError::invalid(
            "MESSAGE_REPLY_TARGET_INVALID",
            "message has no valid Reply-To or From address",
        )
    })?;
    let subject = if inbound.subject.to_ascii_lowercase().starts_with("re:") {
        inbound.subject.clone()
    } else {
        format!("Re: {}", inbound.subject)
    };
    let provider_message_id = format!("<{}@skrzynka.local>", Uuid::new_v4());
    let mut builder = OutgoingMessage::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .message_id(Some(provider_message_id.clone()))
        .header(ContentType::TEXT_PLAIN);
    if let Some(message_id) = inbound.message_id.as_deref() {
        builder = builder.in_reply_to(message_id.to_string());
        let references = inbound
            .references
            .as_deref()
            .map(|existing| format!("{existing} {message_id}"))
            .unwrap_or_else(|| message_id.to_string());
        builder = builder.references(references);
    }
    let outgoing = builder.body(body.to_string()).map_err(|_| {
        AppError::invalid(
            "REPLY_MESSAGE_INVALID",
            "reply could not be encoded as an email message",
        )
    })?;
    let builder = match mailbox.smtp_security {
        SmtpSecurity::Starttls => SmtpTransport::starttls_relay(&mailbox.smtp_host),
        SmtpSecurity::Tls => SmtpTransport::relay(&mailbox.smtp_host),
    }
    .map_err(|_| {
        dependency_error(
            "SMTP_TLS_FAILED",
            "SMTP TLS configuration was rejected",
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
    let transport = builder.timeout(Some(Duration::from_secs(30))).build();
    transport.send(&outgoing).map_err(|error| {
        if error.is_transient() || error.is_permanent() {
            dependency_error(
                "SMTP_REJECTED",
                "SMTP explicitly rejected the reply; inspect mailbox status before retrying",
                error.is_transient(),
            )
        } else {
            dependency_error(
                "SMTP_UNCERTAIN",
                "SMTP acceptance is uncertain; inspect provider Sent mail before another attempt",
                false,
            )
        }
    })?;
    Ok(provider_message_id)
}

fn normalize_message(uid: u32, bytes: &[u8]) -> Result<NewMessage, AppError> {
    let parsed = mailparse::parse_mail(bytes).map_err(|_| {
        dependency_error(
            "MESSAGE_MALFORMED",
            "provider message could not be parsed",
            false,
        )
    })?;
    let headers = &parsed.headers;
    let sender = headers
        .get_first_value("From")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            dependency_error(
                "MESSAGE_MALFORMED",
                "provider message has no From header",
                false,
            )
        })?;
    let recipients = headers.get_first_value("To").unwrap_or_default();
    let subject = headers
        .get_first_value("Subject")
        .unwrap_or_else(|| "(no subject)".to_string());
    let body_text = extract_text(&parsed).unwrap_or_default().trim().to_string();
    let snippet = body_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect();
    let sent_at = headers
        .get_first_value("Date")
        .and_then(|value| mailparse::dateparse(&value).ok())
        .and_then(|seconds| chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0))
        .map(|value| value.to_rfc3339());
    Ok(NewMessage {
        external_uid: uid,
        message_id: headers.get_first_value("Message-ID"),
        in_reply_to: headers.get_first_value("In-Reply-To"),
        references: headers.get_first_value("References"),
        sender,
        reply_to: headers.get_first_value("Reply-To"),
        recipients,
        subject,
        sent_at,
        body_text,
        snippet,
    })
}

fn extract_text(parsed: &ParsedMail<'_>) -> Option<String> {
    if parsed.subparts.is_empty() {
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/plain") {
            return parsed.get_body().ok();
        }
        return None;
    }
    parsed.subparts.iter().find_map(extract_text)
}

fn dependency_error(code: &'static str, message: impl Into<String>, retryable: bool) -> AppError {
    AppError::dependency(code, message, retryable)
}
