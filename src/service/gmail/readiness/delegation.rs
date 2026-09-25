//! Whether domain-wide delegation can carry this address.
//!
//! A readable service-account key is not a grant, and reporting it as
//! `configured` is what this check replaces: the grant lives in a Workspace
//! admin console, and the only thing that proves it is a token Google actually
//! mints for the address. A consumer account is refused without calling Google
//! at all, because no administrator can grant delegation for one.

use super::super::super::{AppState, GmailConnectionPath};
use super::{is_consumer_account, DELEGATION, REFUSED, UNPROVEN, USABLE};
use crate::skarbiec::{GMAIL_DELEGATION_SCOPE, GOOGLE_ADMIN_DELEGATION_URL};
use std::collections::BTreeMap;

impl AppState {
    pub(super) async fn delegation_path(&self, account: Option<&str>) -> GmailConnectionPath {
        let service_account = match self.resolver.google_service_account().await {
            Ok(service_account) => service_account,
            Err(error) => {
                return GmailConnectionPath {
                    path: DELEGATION,
                    verdict: REFUSED,
                    code: Some(error.code.to_string()),
                    detail: error.message,
                    action: "Store a Google service-account key in the Skarbiec item \
                             skrzynka-google-service-account."
                        .to_string(),
                    observed: BTreeMap::new(),
                }
            }
        };
        let observed = BTreeMap::from([
            ("service_account", service_account.client_email.clone()),
            ("client_id", service_account.client_id.clone()),
            ("scope", GMAIL_DELEGATION_SCOPE.to_string()),
            ("admin_console_url", GOOGLE_ADMIN_DELEGATION_URL.to_string()),
        ]);
        let grant_action = format!(
            "Grant client ID {} the scope {GMAIL_DELEGATION_SCOPE} at \
             {GOOGLE_ADMIN_DELEGATION_URL}",
            service_account.client_id
        );
        let Some(account) = account else {
            return GmailConnectionPath {
                path: DELEGATION,
                verdict: UNPROVEN,
                code: None,
                detail: format!(
                    "The service account {} is readable. A grant exists per Workspace domain, so \
                     naming an address with --email is what proves it.",
                    service_account.client_email
                ),
                action: grant_action,
                observed,
            };
        };
        if is_consumer_account(account) {
            return GmailConnectionPath {
                path: DELEGATION,
                verdict: REFUSED,
                code: Some("GOOGLE_DELEGATION_NOT_APPLICABLE".to_string()),
                detail: format!(
                    "{account} is a consumer Google account, and domain-wide delegation exists \
                     only inside a Workspace domain. No administrator can grant it for this \
                     address."
                ),
                action: crate::gmail::app_password_action(account),
                observed,
            };
        }
        match self
            .resolver
            .delegated_access_token(&format!("readiness-probe:{account}"), account)
            .await
        {
            Ok(_) => GmailConnectionPath {
                path: DELEGATION,
                verdict: USABLE,
                code: None,
                detail: format!(
                    "Google minted a delegated {GMAIL_DELEGATION_SCOPE} token for {account} \
                     through service account {}.",
                    service_account.client_email
                ),
                action: format!("skrzynka gmail delegate --email {account}"),
                observed,
            },
            Err(error) => GmailConnectionPath {
                path: DELEGATION,
                verdict: REFUSED,
                code: Some(error.code.to_string()),
                detail: error.message,
                action: grant_action,
                observed,
            },
        }
    }
}
