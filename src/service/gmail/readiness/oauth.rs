//! What Google says today about the stored OAuth client and this process's
//! loopback callback.
//!
//! The probe hands Google the URL a real authorization would hand it and reads
//! the code Google puts in the landing URL. A non-refusal is reported as
//! unproven rather than usable, because Google shows a sign-in page before
//! validating the redirect for some clients: only a completed authorization
//! proves the redirect is registered.

use super::super::super::{AppState, GmailConnectionPath};
use super::{OAUTH, REFUSED, UNPROVEN};
use crate::gmail::redirect_not_registered;
use std::collections::BTreeMap;

/// Google's own word for a redirect URI the client does not carry.
const REDIRECT_MISMATCH: &str = "redirect_uri_mismatch";

impl AppState {
    pub(super) async fn oauth_path(&self) -> GmailConnectionPath {
        let probe = match self.gmail_oauth.redirect_registration().await {
            Ok(probe) => probe,
            Err(error) => {
                return GmailConnectionPath {
                    path: OAUTH,
                    verdict: REFUSED,
                    code: Some(error.code.to_string()),
                    detail: error.message,
                    action: "Store a Google Desktop-app OAuth client in the Skarbiec item \
                             skrzynka-google-oauth-desktop."
                        .to_string(),
                    observed: BTreeMap::new(),
                }
            }
        };
        let observed = BTreeMap::from([
            ("oauth_client_id", probe.client_id.clone()),
            ("redirect_uri", probe.redirect_uri.clone()),
        ]);
        match probe.refusal.as_deref() {
            Some(REDIRECT_MISMATCH) => {
                let refusal = redirect_not_registered(&probe.client_id, &probe.redirect_uri);
                GmailConnectionPath {
                    path: OAUTH,
                    verdict: REFUSED,
                    code: Some(refusal.code.to_string()),
                    detail: refusal.message,
                    action: "Register a loopback redirect URI for that client in the Google \
                             Cloud Console, or issue a Desktop app client, and store it in \
                             skrzynka-google-oauth-desktop."
                        .to_string(),
                    observed,
                }
            }
            Some(refusal) => GmailConnectionPath {
                path: OAUTH,
                verdict: REFUSED,
                code: Some("GMAIL_OAUTH_REJECTED".to_string()),
                detail: format!(
                    "Google refused the authorization for client {} with {refusal} before any \
                     consent screen.",
                    probe.client_id
                ),
                action: "Repair that client in the Google Cloud Console and store it in \
                         skrzynka-google-oauth-desktop."
                    .to_string(),
                observed,
            },
            None => GmailConnectionPath {
                path: OAUTH,
                verdict: UNPROVEN,
                code: None,
                detail: format!(
                    "Google did not refuse client {} with redirect {} at the authorization page. \
                     That is not proof the redirect is registered: only a completed \
                     authorization proves it.",
                    probe.client_id, probe.redirect_uri
                ),
                action: "skrzynka gmail authorize --skarbiec-item <item-id>".to_string(),
                observed,
            },
        }
    }
}
