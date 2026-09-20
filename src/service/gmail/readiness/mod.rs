//! Which Gmail connection paths this installation can actually use, measured.
//!
//! Gmail offers three connection paths and Skrzynka implements all three, so
//! before this report an operator learned which one was available by running
//! them one at a time and reading three different refusals. Worse, the refusal
//! a rejected IMAP password produced recommended the OAuth path unconditionally
//! — a path whose fixed client has been refusing `redirect_uri_mismatch` since
//! it was first measured.
//!
//! Every verdict here comes from confronting a declaration with the world: a
//! real IMAP login for the stored password, Google's own answer about the
//! stored OAuth client and this process's loopback redirect, and a real
//! delegated token mint for a Workspace address. A readable Skarbiec item is
//! never by itself reported as a working path.

use super::super::{AppState, GmailConnectionReadiness};
use crate::{error::AppError, models::Mailbox};
use lettre::Address;
use std::str::FromStr;

mod app_password;
mod delegation;
mod oauth;

/// The domains Google runs for individuals. Domain-wide delegation is a
/// Workspace feature, so an address in one of these can never use it.
const CONSUMER_DOMAINS: [&str; 2] = ["gmail.com", "googlemail.com"];

const APP_PASSWORD: &str = "app_password";
const OAUTH: &str = "oauth";
const DELEGATION: &str = "delegation";

/// A path that authenticated against the provider just now.
const USABLE: &str = "usable";
/// A path the provider, the vault or this product refused just now.
const REFUSED: &str = "refused";
/// A path nothing here can decide: no credential is stored, no account was
/// named, or the provider did not answer the question.
const UNPROVEN: &str = "unproven";

impl AppState {
    /// Report every Gmail connection path for one account, or the
    /// account-independent state of all three when no account is named.
    ///
    /// Reading is the whole effect: no mailbox, credential, flow or cursor is
    /// written, and the report carries no secret. The only outbound calls are
    /// the three checks themselves.
    pub async fn gmail_connection_readiness(
        &self,
        organization_id: &str,
        email: Option<&str>,
    ) -> Result<GmailConnectionReadiness, AppError> {
        let account = match email {
            Some(email) => Some(validated_account(email)?),
            None => None,
        };
        let mailbox = match account.as_deref() {
            Some(account) => self.mailbox_named(organization_id, account)?,
            None => None,
        };
        let paths = vec![
            self.app_password_path(account.as_deref(), mailbox.as_ref())
                .await,
            self.oauth_path().await,
            self.delegation_path(account.as_deref()).await,
        ];
        Ok(GmailConnectionReadiness {
            account,
            mailbox_id: mailbox.as_ref().map(|mailbox| mailbox.id),
            receiving_skarbiec_item_id: mailbox
                .as_ref()
                .map(|mailbox| mailbox.skarbiec_item_id.clone()),
            usable_paths: paths.iter().filter(|path| path.verdict == USABLE).count(),
            paths,
        })
    }

    /// The one mailbox this address names, or none. Two mailboxes for one
    /// address is the ambiguity every other selector refuses, and this report
    /// refuses it rather than measuring an arbitrary one of them.
    fn mailbox_named(
        &self,
        organization_id: &str,
        account: &str,
    ) -> Result<Option<Mailbox>, AppError> {
        let mut matches = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| mailbox.email.eq_ignore_ascii_case(account))
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            let ids = matches
                .iter()
                .map(|mailbox| mailbox.id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AppError::conflict(
                "MAILBOX_SELECTOR_AMBIGUOUS",
                format!(
                    "{account} names {} mailboxes ({ids}); select one by id",
                    matches.len()
                ),
            ));
        }
        Ok(matches.pop())
    }
}

/// The address this report is about, refused rather than guessed when it is not
/// an address at all.
fn validated_account(email: &str) -> Result<String, AppError> {
    let email = email.trim();
    Address::from_str(email)
        .map_err(|_| AppError::invalid("GMAIL_PROFILE_INVALID", "email is not a valid address"))?;
    Ok(email.to_string())
}

/// Whether Google runs this address for an individual rather than a Workspace
/// domain.
fn is_consumer_account(email: &str) -> bool {
    email.rsplit_once('@').is_some_and(|(_, domain)| {
        CONSUMER_DOMAINS
            .iter()
            .any(|consumer| domain.eq_ignore_ascii_case(consumer))
    })
}

#[cfg(test)]
#[path = "../../../../tests/gmail/readiness.rs"]
mod tests;
