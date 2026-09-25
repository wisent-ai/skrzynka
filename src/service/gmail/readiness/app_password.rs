//! Does Gmail accept the password this mailbox already carries?
//!
//! The login performed here is the one the synchronizer performs, against
//! Google, with the credential the mailbox actually names. A `usable` verdict
//! therefore means mail can be received now, and a `refused` verdict carries
//! Google's own sentence rather than a guess about it.

use super::super::super::{AppState, GmailConnectionPath};
use super::{APP_PASSWORD, REFUSED, UNPROVEN, USABLE};
use crate::{
    mail::{self, GMAIL_IMAP_HOST},
    models::Mailbox,
    skarbiec::ResolvedCredentials,
};
use std::collections::BTreeMap;

impl AppState {
    pub(super) async fn app_password_path(
        &self,
        account: Option<&str>,
        mailbox: Option<&Mailbox>,
    ) -> GmailConnectionPath {
        let (Some(account), Some(mailbox)) = (account, mailbox) else {
            return nothing_to_authenticate(account);
        };
        let mut observed = BTreeMap::from([
            ("skarbiec_item_id", mailbox.skarbiec_item_id.clone()),
            ("imap_host", mailbox.imap_host.clone()),
        ]);
        if mailbox.imap_host != GMAIL_IMAP_HOST {
            return GmailConnectionPath {
                path: APP_PASSWORD,
                verdict: UNPROVEN,
                code: None,
                detail: format!(
                    "Mailbox {account} receives from {} rather than {GMAIL_IMAP_HOST}, so the \
                     Gmail connection paths do not decide whether it can receive.",
                    mailbox.imap_host
                ),
                action: connect_action(Some(account)),
                observed,
            };
        }
        let credentials = match self
            .resolver
            .resolve_credentials(&mailbox.skarbiec_item_id)
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                return GmailConnectionPath {
                    path: APP_PASSWORD,
                    verdict: REFUSED,
                    code: Some(error.code.to_string()),
                    detail: error.message,
                    action: connect_action(Some(account)),
                    observed,
                }
            }
        };
        let ResolvedCredentials::Password { password, .. } = credentials else {
            observed.insert("auth_method", "oauth2".to_string());
            return GmailConnectionPath {
                path: APP_PASSWORD,
                verdict: UNPROVEN,
                code: None,
                detail: format!(
                    "Mailbox {account} authenticates with a Google token rather than a password, \
                     so it holds no app-specific password to authenticate."
                ),
                action: connect_action(Some(account)),
                observed,
            };
        };
        let item = mailbox.skarbiec_item_id.clone();
        let login = {
            let account = account.to_string();
            tokio::task::spawn_blocking(move || {
                mail::verify_gmail_app_password(&account, &password)
            })
            .await
        };
        match login {
            Ok(Ok(())) => GmailConnectionPath {
                path: APP_PASSWORD,
                verdict: USABLE,
                code: None,
                detail: format!(
                    "{GMAIL_IMAP_HOST} accepted the password credential in Skarbiec item \
                     '{item}' for {account}."
                ),
                action: format!("skrzynka sync --mailbox {}", mailbox.id),
                observed,
            },
            // The provider's own words, without the guidance the refusal
            // carries elsewhere: this report supplies its own next step and
            // must not tell the operator to run the command they just ran.
            Ok(Err(refusal)) => GmailConnectionPath {
                path: APP_PASSWORD,
                verdict: REFUSED,
                code: Some(refusal.code.to_string()),
                detail: format!(
                    "Google refused the login for {account} with the credential in Skarbiec item \
                     '{item}'. {}",
                    refusal.evidence
                ),
                action: connect_action(Some(account)),
                observed,
            },
            Err(_) => GmailConnectionPath {
                path: APP_PASSWORD,
                verdict: UNPROVEN,
                code: None,
                detail:
                    "Gmail credential verification stopped unexpectedly before Google answered."
                        .to_string(),
                action: connect_action(Some(account)),
                observed,
            },
        }
    }
}

/// No mailbox names this account, so there is no credential to authenticate.
/// The path is still the recommended one, because an app-specific password
/// needs neither a Workspace administrator nor an OAuth client.
fn nothing_to_authenticate(account: Option<&str>) -> GmailConnectionPath {
    GmailConnectionPath {
        path: APP_PASSWORD,
        verdict: UNPROVEN,
        code: Some("GMAIL_APP_PASSWORD_NOT_STORED".to_string()),
        detail: match account {
            Some(account) => format!(
                "No mailbox is named {account}, so no stored credential could be authenticated. \
                 An app-specific password connects this account without a Workspace \
                 administrator or an OAuth client."
            ),
            None => "No account was named, so no stored credential could be authenticated. Name \
                     one with --email to have its password authenticated against Gmail."
                .to_string(),
        },
        action: connect_action(account),
        observed: BTreeMap::new(),
    }
}

/// The exact command that connects this account: Weles creates the app
/// password and hands it to Skrzynka on stdin, so no secret is typed or placed
/// in argv.
fn connect_action(account: Option<&str>) -> String {
    crate::gmail::app_password_action(account.unwrap_or("<address>"))
}
