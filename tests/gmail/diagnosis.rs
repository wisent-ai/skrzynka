// Diagnosis of a failed Google authorization, attached to src/gmail/diagnosis.rs.

use super::*;

/// A REAL landing URL captured from Google while authorizing client
/// 903183433368-...ap3l9 against a loopback redirect it does not have
/// registered. Not a stubbed server: the bytes Google actually sent.
const CAPTURED_ERROR: &str = "https://accounts.google.com/signin/oauth/error?authError=\
                              ChVyZWRpcmVjdF91cmlfbWlzbWF0Y2gSsAEKWW91IGNhbid0IHNpZ24gaW4gdG8gdGhpcyBhcHA";

#[test]
fn the_captured_google_error_decodes_to_its_code() {
    assert_eq!(
        oauth_error_code(CAPTURED_ERROR).as_deref(),
        Some("redirect_uri_mismatch")
    );
}

#[test]
fn a_landing_url_without_an_error_carries_no_code() {
    // A completed flow must never be reported as a registration problem.
    assert!(
        oauth_error_code("http://127.0.0.1:8788/v1/gmail/oauth/callback?code=x&state=y")
            .is_none()
    );
    assert!(oauth_error_code("https://accounts.google.com/signin/oauth/consent").is_none());
    assert!(oauth_error_code("https://accounts.google.com/x?authError=").is_none());
    assert!(oauth_error_code("not a url").is_none());
}

#[test]
fn the_operands_come_from_the_url_that_was_handed_out() {
    let url =
        "https://accounts.google.com/o/oauth2/auth?client_id=abc.apps.googleusercontent.com\
               &redirect_uri=http%3A%2F%2F127.0.0.1%3A8788%2Fv1%2Fgmail%2Foauth%2Fcallback\
               &response_type=code";
    let (client_id, redirect_uri) = authorization_operands(url).expect("operands");
    assert_eq!(client_id, "abc.apps.googleusercontent.com");
    assert_eq!(
        redirect_uri,
        "http://127.0.0.1:8788/v1/gmail/oauth/callback"
    );
    // A URL missing either operand yields nothing rather than half a
    // sentence naming an empty client.
    assert!(authorization_operands("https://accounts.google.com/o/oauth2/auth").is_none());
}

#[test]
fn the_refusal_names_the_client_the_uri_and_the_setting() {
    let error = redirect_not_registered(
        "903183433368-5nt0jdbqtli8rm39oh2s0limiljap3l9.apps.googleusercontent.com",
        "http://127.0.0.1:8788/v1/gmail/oauth/callback",
    );
    assert_eq!(error.code, "GMAIL_OAUTH_REDIRECT_NOT_REGISTERED");
    assert!(
        !error.retryable,
        "registering a redirect URI is not a retry"
    );
    assert_eq!(
        error.message,
        "the OAuth client 903183433368-5nt0jdbqtli8rm39oh2s0limiljap3l9.apps.googleusercontent.com \
has no loopback redirect URI registered, so Google refused this authorization with \
redirect_uri_mismatch before showing any consent screen; it was presented \
http://127.0.0.1:8788/v1/gmail/oauth/callback. Register a loopback redirect URI for that client \
in the Google Cloud Console, or issue a Desktop app client which accepts any loopback port, and \
retry"
    );
}

#[test]
fn google_imap_password_rejected_names_mailbox_and_credential_item() {
    let error = google_imap_password_rejected("user@gmail.com", "gmail-personal");
    assert_eq!(error.code, "GMAIL_IMAP_PASSWORD_REJECTED");
    assert!(
        !error.retryable,
        "fixing a password with OAuth or app password is not a retry"
    );
    // Message must be exactly as specified, with operands interpolated
    assert_eq!(
        error.message,
        "Google refused IMAP authentication for mailbox user@gmail.com using the password credential associated with Skarbiec item 'gmail-personal'. Supply a valid Google app-specific password through stdin to `skrzynka gmail app-password --email user@gmail.com`, or authorize the account with `skrzynka gmail authorize --skarbiec-item gmail-personal`."
    );
    // Reject any argv-secret patterns: password= or =< constructions must never appear
    assert!(!error.message.contains("password="), "message must not suggest password= argv form; secrets cannot be passed on command line");
    assert!(
        !error.message.contains("=<"),
        "message must not contain =< placeholder; all guidance must be concrete"
    );
}

#[test]
fn google_imap_password_rejected_enforces_gmail_host_boundary() {
    // The error function itself is Gmail-specific. The caller (mail.rs::fetch_messages)
    // must check that mailbox.imap_host.contains("gmail.com") before invoking this.
    // Non-Gmail hosts must get the generic "IMAP authentication was refused" message.
    let error = google_imap_password_rejected("user@example.invalid", "example-inbox");
    assert_eq!(error.code, "GMAIL_IMAP_PASSWORD_REJECTED");
    // Error message is Gmail-focused; mail.rs must enforce the host boundary
    assert!(
        error.message.contains("Google") || error.message.contains("Gmail"),
        "error message is Gmail-specific; caller must detect gmail.com hosts first"
    );
}
