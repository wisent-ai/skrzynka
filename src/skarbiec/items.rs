//! Skarbiec items: the catalog, Google profiles, and the Gmail credentials written back.

use super::{cli::bounded_stdout, invalid_item, looks_like_google_profile, profile_preference, validate_item_id, SkarbiecResolver, GOOGLE_OAUTH_CLIENT_ITEM_ID, GOOGLE_SERVICE_ACCOUNT_ITEM_ID};
use crate::{error::AppError, gmail::GmailProfile, models::SkarbiecItemMetadata};
use lettre::Address;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, str::FromStr};

impl SkarbiecResolver {
    pub async fn list_items(&self) -> Result<Vec<SkarbiecItemMetadata>, AppError> {
        let output = self.output(&["list"]).await?;
        if !output.status.success() {
            return Err(AppError::dependency(
                "SKARBIEC_UNAVAILABLE",
                "Skarbiec metadata listing failed",
                true,
            ));
        }
        bounded_stdout(&output.stdout)?;
        let values: Vec<Value> = serde_json::from_slice(&output.stdout).map_err(|_| {
            AppError::dependency(
                "SKARBIEC_RESPONSE_INVALID",
                "Skarbiec returned invalid metadata JSON",
                false,
            )
        })?;
        let mut items = values
            .into_iter()
            .filter_map(|value| {
                let object = value.as_object()?;
                let id = object.get("id")?.as_str()?.to_string();
                let tags = object
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                Some(SkarbiecItemMetadata {
                    id,
                    kind: object
                        .get("kind")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    state: object
                        .get("state")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    tags,
                    versions: object.get("versions").and_then(Value::as_u64),
                    updated_at: object
                        .get("updated_at")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
            })
            .filter(|item| matches!(item.kind.as_deref(), Some("login" | "bundle")))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(items)
    }

    pub async fn list_google_profiles(&self) -> Result<Vec<GmailProfile>, AppError> {
        let items = self.list_items().await?;
        let mut profiles = HashMap::<String, GmailProfile>::new();
        for item in items {
            if item.kind.as_deref() != Some("login") {
                continue;
            }
            let Ok(payload) = self.get_item(&item.id).await else {
                continue;
            };
            let Some(email) = payload
                .pointer("/fields/username")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| value.contains('@'))
            else {
                continue;
            };
            if !looks_like_google_profile(&item.id, email, &payload) {
                continue;
            }
            let key = email.to_ascii_lowercase();
            let candidate = GmailProfile {
                skarbiec_item_id: item.id,
                email: email.to_string(),
            };
            match profiles.get(&key) {
                Some(current)
                    if profile_preference(&current.skarbiec_item_id)
                        >= profile_preference(&candidate.skarbiec_item_id) => {}
                _ => {
                    profiles.insert(key, candidate);
                }
            }
        }
        let mut profiles = profiles.into_values().collect::<Vec<_>>();
        profiles.sort_by(|left, right| left.email.cmp(&right.email));
        Ok(profiles)
    }

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
        Address::from_str(email)
            .map_err(|_| invalid_item("Google profile username is not an email address"))?;
        if !looks_like_google_profile(item_id, email, &payload) {
            return Err(invalid_item(
                "selected Skarbiec login is not a Google identity",
            ));
        }
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
        let digest = format!(
            "{:x}",
            Sha256::digest(email.to_ascii_lowercase().as_bytes())
        );
        let item_id = format!("skrzynka-gmail-{}", &digest[..20]);
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
                "imap_host": "imap.gmail.com",
                "imap_port": 993,
                "smtp_host": "smtp.gmail.com",
                "smtp_port": 587,
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
    pub async fn save_gmail_delegation(&self, email: &str) -> Result<String, AppError> {
        Address::from_str(email)
            .map_err(|_| invalid_item("delegated Google identity is not an email address"))?;
        let digest = format!(
            "{:x}",
            Sha256::digest(email.to_ascii_lowercase().as_bytes())
        );
        let item_id = format!("skrzynka-gmail-{}", &digest[..20]);
        let payload = json!({
            "schema": "skarbiec.item.v2",
            "kind": "bundle",
            "fields": {
                "username": email,
                "email": email,
                "auth_method": "oauth2_service_account",
                "oauth_provider": "google",
                "service_account_item_id": GOOGLE_SERVICE_ACCOUNT_ITEM_ID,
                "imap_host": "imap.gmail.com",
                "imap_port": 993,
                "smtp_host": "smtp.gmail.com",
                "smtp_port": 587,
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

    pub fn gmail_app_password_item_id(email: &str) -> Result<String, AppError> {
        Address::from_str(email)
            .map_err(|_| invalid_item("Google app-password identity is not an email address"))?;
        let digest = format!(
            "{:x}",
            Sha256::digest(email.to_ascii_lowercase().as_bytes())
        );
        Ok(format!("skrzynka-gmail-app-password-{}", &digest[..20]))
    }

    /// Persist a single-account Gmail credential after the caller has proved
    /// it against Google's IMAP endpoint.
    pub async fn save_gmail_app_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<String, AppError> {
        let item_id = Self::gmail_app_password_item_id(email)?;
        if password.is_empty() {
            return Err(invalid_item("Google app-specific password is empty"));
        }
        let payload = json!({
            "schema": "skarbiec.item.v2",
            "kind": "bundle",
            "fields": {
                "username": email,
                "email": email,
                "password": password,
                "auth_method": "password",
                "oauth_provider": "google",
                "imap_host": "imap.gmail.com",
                "imap_port": 993,
                "smtp_host": "smtp.gmail.com",
                "smtp_port": 587,
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
