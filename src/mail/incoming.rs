use super::{dependency_error, normalize_message};
use crate::{
    error::AppError,
    gmail,
    models::{Mailbox, NewMessage},
    skarbiec::ResolvedCredentials,
};
use std::collections::BTreeMap;

const MAX_RAW_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_MESSAGES_PER_SYNC: usize = 200;

#[derive(Debug)]
pub struct FetchedMessages {
    pub messages: Vec<NewMessage>,
    pub skipped: usize,
    pub rejected_by_reason: BTreeMap<String, usize>,
    pub last_uid: u32,
    pub has_more: bool,
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

/// Prove a Gmail password credential without selecting or reading the inbox.
/// The caller persists only after this login succeeds.
pub fn verify_gmail_app_password(
    email: &str,
    password: &str,
    skarbiec_item_id: &str,
) -> Result<(), AppError> {
    let client = imap::ClientBuilder::new("imap.gmail.com", 993)
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
    let mut session = client.login(email, password).map_err(|(error, _)| {
        password_login_error(error, "imap.gmail.com", email, skarbiec_item_id, password)
    })?;
    let _ = session.logout();
    Ok(())
}

// Preserve the server's response code as well as its explanation. A socket
// failure is not evidence that Google rejected the password.
fn password_login_error(
    error: imap::Error,
    host: &str,
    email: &str,
    item: &str,
    password: &str,
) -> AppError {
    let mut failure =
        if matches!(&error, imap::Error::No(_)) && host.eq_ignore_ascii_case("imap.gmail.com") {
            gmail::google_imap_password_rejected(email, item)
        } else {
            dependency_error(
                "IMAP_AUTHENTICATION_FAILED",
                "IMAP LOGIN did not complete; inspect the reported provider or connection error",
                matches!(&error, imap::Error::Io(_) | imap::Error::ConnectionLost),
            )
        };
    let detail = format!("{error:?}");
    let detail = if password.is_empty() {
        detail
    } else {
        detail.replace(password, "[redacted]")
    };
    failure
        .message
        .push_str(&format!(" IMAP LOGIN at {host}: {detail}"));
    failure
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
            client.login(username, password).map_err(|(error, _)| {
                password_login_error(
                    error,
                    &mailbox.imap_host,
                    &mailbox.email,
                    &mailbox.skarbiec_item_id,
                    password,
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
    // SEARCH returns a HashSet. Advance only through the oldest remaining UIDs:
    // truncating arbitrary hash order would permanently jump over unread rows.
    uids.sort_unstable();
    let has_more = uids.len() > MAX_MESSAGES_PER_SYNC;
    if has_more {
        uids.truncate(MAX_MESSAGES_PER_SYNC);
    }

    let mut messages = Vec::with_capacity(uids.len());
    let mut skipped = 0usize;
    let mut rejected_by_reason = BTreeMap::new();
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
            *rejected_by_reason
                .entry("provider_row_missing".to_string())
                .or_default() += 1;
            last_uid = last_uid.max(requested_uid);
            continue;
        };
        let uid = fetch.uid.unwrap_or(requested_uid);
        last_uid = last_uid.max(uid);
        let Some(body) = fetch.body() else {
            skipped += 1;
            *rejected_by_reason
                .entry("message_body_missing".to_string())
                .or_default() += 1;
            continue;
        };
        if body.len() > MAX_RAW_MESSAGE_BYTES {
            skipped += 1;
            *rejected_by_reason
                .entry("message_exceeds_2_mib".to_string())
                .or_default() += 1;
            continue;
        }
        match normalize_message(uid, body) {
            Ok(message) => messages.push(message),
            Err(error) => {
                skipped += 1;
                *rejected_by_reason
                    .entry(error.code.to_ascii_lowercase())
                    .or_default() += 1;
            }
        }
    }
    let _ = session.logout();
    Ok(FetchedMessages {
        messages,
        skipped,
        rejected_by_reason,
        last_uid,
        has_more,
    })
}
