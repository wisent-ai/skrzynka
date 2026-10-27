//! Skarbiec items: the catalog, Google profiles, and the Gmail credentials written back.

use super::{
    invalid_item, validate_item_id, SkarbiecResolver, GOOGLE_OAUTH_CLIENT_ITEM_ID,
    GOOGLE_SERVICE_ACCOUNT_ITEM_ID,
};
use crate::{
    error::AppError, gmail::GmailProfile, mail::GMAIL_IMAP_HOST, models::SkarbiecItemMetadata,
};
use lettre::Address;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, str::FromStr};

impl SkarbiecResolver {
    pub async fn list_items(&self) -> Result<Vec<SkarbiecItemMetadata>, AppError> {
        let mut items = self.list_inventory().await?;
        items.retain(|item| matches!(item.kind.as_deref(), Some("login" | "bundle")));
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(items)
    }

    /// Complete non-secret metadata, including kinds unsuitable for mail accounts.
    pub(crate) async fn list_inventory(&self) -> Result<Vec<SkarbiecItemMetadata>, AppError> {
        let output = self.output(&["list"]).await?;
        if !output.status.success() {
            return Err(AppError::dependency(
                "SKARBIEC_UNAVAILABLE",
                format!(
                    "{} list failed with {}: {}",
                    self.binary.display(),
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                true,
            ));
        }
        let values: Vec<SkarbiecItemMetadata> =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                AppError::dependency(
                    "SKARBIEC_RESPONSE_INVALID",
                    format!(
                        "decode {} list metadata JSON: {error}",
                        self.binary.display()
                    ),
                    false,
                )
            })?;
        Ok(values)
    }

    /// Every Skarbiec login whose username is an email address, one per
    /// address. Whether the address is a Google account is Google's to say:
    /// the authorization it is offered for is started with it as the login
    /// hint, and the account Google returns is checked against it. No word
    /// in an item's id or notes decides it. Two logins for one address offer
    /// the one Skarbiec updated last.
    pub async fn list_google_profiles(&self) -> Result<Vec<GmailProfile>, AppError> {
        let items = self.list_items().await?;
        let mut profiles = HashMap::<String, (Option<String>, GmailProfile)>::new();
        for item in items {
            if item.kind.as_deref() != Some("login") {
                continue;
            }
            let payload = self.get_item(&item.id).await?;
            let Some(email) = payload
                .pointer("/fields/username")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| Address::from_str(value).is_ok())
            else {
                continue;
            };
            let key = email.to_ascii_lowercase();
            let candidate = GmailProfile {
                skarbiec_item_id: item.id,
                email: email.to_string(),
            };
            match profiles.get(&key) {
                Some((updated, _)) if *updated >= item.updated_at => {}
                _ => {
                    profiles.insert(key, (item.updated_at, candidate));
                }
            }
        }
        let mut profiles = profiles
            .into_values()
            .map(|(_, profile)| profile)
            .collect::<Vec<_>>();
        profiles.sort_by(|left, right| left.email.cmp(&right.email));
        Ok(profiles)
    }

    /// The address a Skarbiec login signs in with, for a Google authorization
    /// started with it as the login hint; refused when the item is no login
    /// or its username is no address.
    pub async fn resolve_google_identity(&self, item_id: &str) -> Result<String, AppError> {
        validate_item_id(item_id)?;
        let payload = self.get_item(item_id).await?;
        if payload.get("kind").and_then(Value::as_str) != Some("login") {
            return Err(invalid_item("Google profile must be a Skarbiec login item"));
        }
        let email = payload
            .pointer("/fields/username")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid_item("Google profile has no username"))?;
        Address::from_str(email).map_err(|cause| {
            invalid_item(format!(
                "Google profile username '{email}' is not an email address: {cause}"
            ))
        })?;
        Ok(email.to_string())
    }

    pub async fn save_gmail_authorization(
        &self,
        source_item_id: &str,
        email: &str,
        refresh_token: &str,
    ) -> Result<String, AppError> {
        validate_item_id(source_item_id)?;
        Address::from_str(email)
            .map_err(|_| invalid_item("authorized Google identity is not an email address"))?;
        let item_id = self.address_item(GMAIL_ITEM_PREFIX, email).await?;
        let payload = json!({
            "schema": "skarbiec.item.v2",
            "kind": "bundle",
            "fields": {
                "username": email,
                "email": email,
                "auth_method": "oauth2",
                "oauth_provider": "google",
                "oauth_client_item_id": GOOGLE_OAUTH_CLIENT_ITEM_ID,
                "refresh_token": refresh_token,
                "imap_host": GMAIL_IMAP_HOST,
                "smtp_host": "smtp.gmail.com",
                "smtp_security": "starttls"
            },
            "context": {
                "source_kind": "gmail_oauth",
                "source_item_id": source_item_id,
                "account_ref": email
            }
        });
        self.set_item(&item_id, "bundle", &payload).await?;
        self.token_cache.lock().await.remove(&item_id);
        Ok(item_id)
    }

    /// Persist a delegated Workspace mailbox credential. The bundle carries no
    /// secret of its own: access tokens are minted per connection from the
    /// service-account key referenced by `service_account_item_id`.
    pub async fn save_gmail_delegation(
        &self,
        email: &str,
        display_name: Option<&str>,
    ) -> Result<String, AppError> {
        Address::from_str(email)
            .map_err(|_| invalid_item("delegated Google identity is not an email address"))?;
        let item_id = self.address_item(GMAIL_ITEM_PREFIX, email).await?;
        let payload = json!({
            "schema": "skarbiec.item.v2",
            "kind": "bundle",
            "fields": {
                "username": email,
                "display_name": display_name.unwrap_or(email),
                "email": email,
                "auth_method": "oauth2_service_account",
                "oauth_provider": "google",
                "service_account_item_id": GOOGLE_SERVICE_ACCOUNT_ITEM_ID,
                "imap_host": GMAIL_IMAP_HOST,
                "smtp_host": "smtp.gmail.com",
                "smtp_security": "starttls"
            },
            "context": {
                "source_kind": "gmail_delegation",
                "source_item_id": GOOGLE_SERVICE_ACCOUNT_ITEM_ID,
                "account_ref": email
            }
        });
        self.set_item(&item_id, "bundle", &payload).await?;
        self.token_cache.lock().await.remove(&item_id);
        Ok(item_id)
    }

    /// The item an app-password connection keeps for `email`.
    pub async fn gmail_app_password_item(&self, email: &str) -> Result<String, AppError> {
        self.address_item(GMAIL_APP_PASSWORD_ITEM_PREFIX, email).await
    }

    /// Whether `item_id` is a credential item a Gmail connection path wrote
    /// for `email`.
    pub fn names_gmail_address(item_id: &str, email: &str) -> bool {
        let digest = address_digest(email);
        names_address(item_id, GMAIL_ITEM_PREFIX, &digest)
            || names_address(item_id, GMAIL_APP_PASSWORD_ITEM_PREFIX, &digest)
    }

    /// The item a connection writing under `prefix` keeps for `email`: the
    /// one Skarbiec already holds for that address, or, when it holds none,
    /// the prefix followed by the address's whole digest.
    async fn address_item(&self, prefix: &str, email: &str) -> Result<String, AppError> {
        Address::from_str(email)
            .map_err(|cause| invalid_item(format!("'{email}' is not an email address: {cause}")))?;
        let digest = address_digest(email);
        let found = self
            .list_inventory()
            .await?
            .into_iter()
            .find(|item| names_address(&item.id, prefix, &digest));
        Ok(match found {
            Some(item) => item.id,
            None => format!("{prefix}{digest}"),
        })
    }

    /// Persist a single-account Gmail credential after the caller has proved
    /// it against Google's IMAP endpoint.
    pub async fn save_gmail_app_password(
        &self,
        email: &str,
        password: &str,
        display_name: Option<&str>,
    ) -> Result<String, AppError> {
        let item_id = self.gmail_app_password_item(email).await?;
        if password.is_empty() {
            return Err(invalid_item("Google app-specific password is empty"));
        }
        let payload = json!({
            "schema": "skarbiec.item.v2",
            "kind": "bundle",
            "fields": {
                "username": email,
                "email": email,
                "display_name": display_name.unwrap_or(email),
                "password": password,
                "auth_method": "password",
                "oauth_provider": "google",
                "imap_host": GMAIL_IMAP_HOST,
                "smtp_host": "smtp.gmail.com",
                "smtp_security": "starttls"
            },
            "context": {
                "source_kind": "gmail_app_password",
                "account_ref": email
            }
        });
        self.set_item(&item_id, "bundle", &payload).await?;
        self.token_cache.lock().await.remove(&item_id);
        Ok(item_id)
    }
}

/// The id prefix of the item an OAuth or delegated Gmail connection writes.
const GMAIL_ITEM_PREFIX: &str = "skrzynka-gmail-";

/// The id prefix of the item an app-password Gmail connection writes.
const GMAIL_APP_PASSWORD_ITEM_PREFIX: &str = "skrzynka-gmail-app-password-";

/// The hex SHA-256 of an address, lowercased first: what a Gmail connection
/// names the item it writes for that address by.
fn address_digest(email: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(email.to_ascii_lowercase().as_bytes())
    )
}

/// Whether `id` is the item `prefix` names for the address whose digest is
/// `digest`: the prefix followed by that digest, or by the leading part of it
/// that items written before the whole digest was used carry. An id under
/// another prefix (an app-password item under the OAuth one) never matches,
/// since what follows its prefix is no part of a hex digest.
fn names_address(id: &str, prefix: &str, digest: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty() && digest.starts_with(rest))
}
