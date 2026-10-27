//! Logging in to IMAP with a password: proving a Gmail app password before it
//! is kept, and telling Google's refusal of it from a connection that failed.

use super::dependency_error;
use super::incoming::GMAIL_IMAP_HOST;
use super::ports::registered_tcp_port;
use crate::{error::AppError, gmail};

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
    let port = registered_tcp_port("imaps").map_err(|cause| PasswordLoginRefusal {
        code: "IMAP_PORT_UNREGISTERED",
        retryable: false,
        evidence: format!("the IMAP over TLS port for {GMAIL_IMAP_HOST} is unknown: {cause}"),
    })?;
    let client = imap::ClientBuilder::new(GMAIL_IMAP_HOST, port)
        .mode(imap::ConnectionMode::Tls)
        .tls_kind(imap::TlsKind::Native)
        .connect()
        .map_err(|error| PasswordLoginRefusal {
            code: "IMAP_UNAVAILABLE",
            retryable: true,
            evidence: format!("IMAP TLS connect to {GMAIL_IMAP_HOST}:{port}: {error:?}"),
        })?;
    let mut session = client
        .login(email, password)
        .map_err(|(error, _)| password_login_refusal(error, GMAIL_IMAP_HOST, password))?;
    // The login is the proof; a LOGOUT the server does not answer cleanly
    // leaves the proof standing and is said, not dropped.
    if let Err(error) = session.logout() {
        tracing::warn!(%error, host = GMAIL_IMAP_HOST, "IMAP LOGOUT after a proven login failed");
    }
    Ok(())
}

// Preserve the server's response code as well as its explanation. A socket
// failure is not evidence that Google rejected the password.
pub(super) fn password_login_refusal(
    error: imap::Error,
    host: &str,
    password: &str,
) -> PasswordLoginRefusal {
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
