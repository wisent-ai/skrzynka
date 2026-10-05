use super::{dependency_error, normalize_message};
use crate::{
    error::AppError,
    gmail,
    models::{Mailbox, NewMessage},
    skarbiec::ResolvedCredentials,
};
use std::collections::BTreeMap;

/// Gmail's own IMAP endpoint: the only host the Gmail connection paths speak
/// for, and the boundary that decides whether a refusal is Google's.
pub const GMAIL_IMAP_HOST: &str = "imap.gmail.com";

/// What one pass read from the provider besides the messages it handed to
/// `commit`: what it could not import and why, and the cursor it reached.
#[derive(Debug)]
pub struct FetchedMessages {
    pub skipped: usize,
    pub rejected_by_reason: BTreeMap<String, usize>,
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

/// Why an IMAP password login did not complete: the code this product raises,
/// whether retrying can help, and the provider's own words.
///
/// The two halves are kept apart because two callers need different ones. The
/// synchronizer records the full refusal, guidance included, on the mailbox.
/// The connection report supplies its own next step and would otherwise print
/// an instruction to run the command the operator has just run.
pub struct PasswordLoginRefusal {
    pub code: &'static str,
    pub retryable: bool,
    pub evidence: String,
}

impl PasswordLoginRefusal {
    /// The refusal a caller raises or persists: what happened, what to do, and
    /// the provider's words at the end.
    pub fn into_error(self, organization: &str, email: &str, skarbiec_item_id: &str) -> AppError {
        let mut failure = match self.code {
            "GMAIL_IMAP_PASSWORD_REJECTED" => {
                gmail::google_imap_password_rejected(organization, email, skarbiec_item_id)
            }
            code => dependency_error(
                code,
                "IMAP LOGIN did not complete; inspect the reported provider or connection error",
                self.retryable,
            ),
        };
        failure.message.push(' ');
        failure.message.push_str(&self.evidence);
        failure
    }
}

/// Prove a Gmail password credential without selecting or reading the inbox.
/// The caller persists only after this login succeeds.
pub fn verify_gmail_app_password(email: &str, password: &str) -> Result<(), PasswordLoginRefusal> {
    let client = imap::ClientBuilder::new(GMAIL_IMAP_HOST, 993)
        .mode(imap::ConnectionMode::Tls)
        .tls_kind(imap::TlsKind::Native)
        .connect()
        .map_err(|error| PasswordLoginRefusal {
            code: "IMAP_UNAVAILABLE",
            retryable: true,
            evidence: format!("IMAP TLS connect to {GMAIL_IMAP_HOST}: {error:?}"),
        })?;
    let mut session = client
        .login(email, password)
        .map_err(|(error, _)| password_login_refusal(error, GMAIL_IMAP_HOST, password))?;
    let _ = session.logout();
    Ok(())
}

// Preserve the server's response code as well as its explanation. A socket
// failure is not evidence that Google rejected the password.
fn password_login_refusal(error: imap::Error, host: &str, password: &str) -> PasswordLoginRefusal {
    let rejected_by_google =
        matches!(&error, imap::Error::No(_)) && host.eq_ignore_ascii_case(GMAIL_IMAP_HOST);
    let detail = format!("{error:?}");
    let detail = if password.is_empty() {
        detail
    } else {
        detail.replace(password, "[redacted]")
    };
    PasswordLoginRefusal {
        code: if rejected_by_google {
            "GMAIL_IMAP_PASSWORD_REJECTED"
        } else {
            "IMAP_AUTHENTICATION_FAILED"
        },
        retryable: !rejected_by_google
            && matches!(&error, imap::Error::Io(_) | imap::Error::ConnectionLost),
        evidence: format!("IMAP LOGIN at {host}: {detail}"),
    }
}

/// Block until the provider reports new mail in INBOX (IMAP IDLE, RFC 2177).
/// The connection re-issues IDLE on the RFC's own refresh boundary by itself;
/// no interval is chosen here. Returns when an `EXISTS` arrives, or with the
/// error that ended the connection.
pub fn wait_for_new_mail(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
) -> Result<(), AppError> {
    let mut session = open_inbox(mailbox, credentials)?;
    session
        .idle()
        .wait_while(|response| !matches!(response, imap::types::UnsolicitedResponse::Exists(_)))
        .map_err(|error| {
            dependency_error(
                "IMAP_IDLE_FAILED",
                format!("IMAP IDLE on INBOX ended: {error:?}"),
                true,
            )
        })?;
    Ok(())
}

/// Connect over TLS, authenticate with the mailbox's credentials and select INBOX.
fn open_inbox(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
) -> Result<imap::Session<imap::Connection>, AppError> {
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
                password_login_refusal(error, &mailbox.imap_host, password).into_error(
                    &mailbox.organization_id,
                    &mailbox.email,
                    &mailbox.skarbiec_item_id,
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
    Ok(session)
}

/// Read every message the provider holds past the mailbox cursor, oldest UID
/// first, and hand each to `commit` with the cursor it reaches. A pass imports
/// everything there is, one message at a time: no batch size decides how much
/// mail arrives, only one message is held at once, and a failure part way
/// keeps what was committed before it, so the next pass continues from there.
/// A UID that yields no importable message is committed as an empty slice, so
/// the cursor still passes it.
pub fn fetch_messages(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    mut commit: impl FnMut(&[NewMessage], u32) -> Result<(), AppError>,
) -> Result<FetchedMessages, AppError> {
    let mut session = open_inbox(mailbox, credentials)?;

    let first_uid = mailbox.last_uid.saturating_add(1).max(1);
    let query = format!("UID {first_uid}:*");
    let mut uids = session
        .uid_search(query)
        .map_err(|_| dependency_error("IMAP_SEARCH_FAILED", "IMAP UID search failed", true))?
        .into_iter()
        .filter(|uid| *uid >= first_uid)
        .collect::<Vec<_>>();
    // SEARCH returns a HashSet. Advance through UIDs in order, so a pass that
    // stops part way never leaves an older unread row behind its cursor.
    uids.sort_unstable();

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
            commit(&[], last_uid)?;
            continue;
        };
        let uid = fetch.uid.unwrap_or(requested_uid);
        last_uid = last_uid.max(uid);
        let Some(body) = fetch.body() else {
            skipped += 1;
            *rejected_by_reason
                .entry("message_body_missing".to_string())
                .or_default() += 1;
            commit(&[], last_uid)?;
            continue;
        };
        match normalize_message(uid, body) {
            Ok(message) => commit(std::slice::from_ref(&message), last_uid)?,
            Err(error) => {
                skipped += 1;
                *rejected_by_reason
                    .entry(error.code.to_ascii_lowercase())
                    .or_default() += 1;
                commit(&[], last_uid)?;
            }
        }
    }
    let _ = session.logout();
    Ok(FetchedMessages {
        skipped,
        rejected_by_reason,
        last_uid,
    })
}
