//! What a failed Google authorization means, in the words an operator needs.

use super::AUTHORIZATION_PROBE_TIMEOUT_SECONDS;
use crate::error::AppError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::{Client, Url};

/// The OAuth error code Google put in the landing URL it sent a browser to,
/// or `None` when that URL carries none.
///
/// Google encodes it as base64url in `authError`, so the code an operator needs
/// is unreadable without decoding. Split out from the request that fetched the
/// URL so the decode is exercised against a real captured error rather than a
/// stubbed server.
pub fn oauth_error_code(landing_url: &str) -> Option<String> {
    let parsed = Url::parse(landing_url).ok()?;
    let encoded = parsed
        .query_pairs()
        .find(|(name, _)| name == "authError")
        .map(|(_, value)| value.into_owned())?;
    if encoded.is_empty() {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(encoded.trim_end_matches('=')).ok()?;
    // The payload is an undocumented blob whose first field is the code as
    // plain text, so the code is the leading run of code-shaped bytes rather
    // than the result of parsing a format Google does not publish.
    let text = String::from_utf8_lossy(&decoded);
    let code: String = text
        .chars()
        .skip_while(|character| !character.is_ascii_alphabetic())
        .take_while(|character| character.is_ascii_alphabetic() || *character == '_')
        .collect();
    (!code.is_empty()).then_some(code)
}

/// The refusal an unregistered loopback redirect deserves.
///
/// Google refuses the authorization inside the browser, so no callback ever
/// reaches this process and the flow spends its whole ten-minute lifetime
/// saying nothing. The client id, the redirect URI presented and the one
/// setting that fixes it are the sentence an operator needs, and none of them
/// were anywhere in this product's output.
pub fn redirect_not_registered(client_id: &str, redirect_uri: &str) -> AppError {
    AppError::dependency(
        "GMAIL_OAUTH_REDIRECT_NOT_REGISTERED",
        format!(
            "the OAuth client {client_id} has no loopback redirect URI registered, so Google \
             refused this authorization with redirect_uri_mismatch before showing any consent \
             screen; it was presented {redirect_uri}. Register a loopback redirect URI for that \
             client in the Google Cloud Console, or issue a Desktop app client which accepts any \
             loopback port, and retry"
        ),
        false,
    )
}

/// The command that obtains an app-specific password for `account` without a
/// person: Weles signs the account's Skarbiec Google login in, creates the
/// password on Google's App passwords page, and pipes it to
/// `skrzynka gmail app-password`. Every refusal that needs an app password
/// names this one command rather than asking somebody to make one by hand.
pub fn app_password_action(account: &str) -> String {
    format!("weles app-password --login-item <Skarbiec Google login of {account}>")
}

/// The refusal when Google IMAP rejects a password credential.
///
/// The same refusal covers an ordinary account password and an invalid or
/// revoked app-specific password. It names the command that creates an app
/// password and hands it back here, and then the report that says which
/// paths this account can actually use — it does not recommend authorizing,
/// because whether OAuth can complete here is measured, not assumed. It never
/// places a secret in argv.
pub fn google_imap_password_rejected(mailbox_email: &str, skarbiec_item_id: &str) -> AppError {
    AppError::dependency(
        "GMAIL_IMAP_PASSWORD_REJECTED",
        format!(
            "Google refused IMAP authentication for mailbox {mailbox_email} using the password credential associated with Skarbiec item '{skarbiec_item_id}'. Create an app-specific password and store it here with `{}`. Run `skrzynka gmail connection --email {mailbox_email}` for which connection paths this account can actually use.",
            app_password_action(mailbox_email)
        ),
        false,
    )
}

/// The `client_id` and `redirect_uri` an authorization URL carries, for the
/// refusal above. Read back from the URL that was actually handed out rather
/// than recomputed, so the sentence names what Google was really given.
pub fn authorization_operands(authorization_url: &str) -> Option<(String, String)> {
    let parsed = Url::parse(authorization_url).ok()?;
    let mut client_id = None;
    let mut redirect_uri = None;
    for (name, value) in parsed.query_pairs() {
        match name.as_ref() {
            "client_id" => client_id = Some(value.into_owned()),
            "redirect_uri" => redirect_uri = Some(value.into_owned()),
            _ => {}
        }
    }
    Some((client_id?, redirect_uri?))
}

/// Ask Google what it says about this authorization, for use only AFTER a flow
/// has already failed.
///
/// Never a pre-flight gate: Google shows a sign-in page before validating the
/// redirect for some forms, so a response that is not an error page does not
/// mean the redirect is registered. This only explains a failure that has
/// already happened.
pub async fn diagnose_authorization(authorization_url: &str) -> Option<String> {
    let response = Client::new()
        .get(authorization_url)
        .header("user-agent", "Mozilla/5.0")
        .timeout(std::time::Duration::from_secs(
            AUTHORIZATION_PROBE_TIMEOUT_SECONDS,
        ))
        .send()
        .await
        .ok()?;
    oauth_error_code(response.url().as_str())
}

#[cfg(test)]
#[path = "../../tests/gmail/diagnosis.rs"]
mod tests;
