//! Mailbox credentials and configuration resolved from a Skarbiec item, and the
//! declaration that makes an item a mailbox at all.
//!
//! Skarbiec owns the account list. An item is one of Skrzynka's mailboxes
//! exactly when it carries [`MAILBOX_TAG`], the same way Brama reads an item
//! as a subscription only when it carries `brama:subscription`. Skrzynka keeps
//! mail and sync state for those items; it keeps no list of its own.

use super::{
    invalid_item, optional_text, port_field, profile_error, required_text, validate_hostname,
    validate_item_id, ResolvedCredentials, SkarbiecResolver, GOOGLE_OAUTH_CLIENT_ITEM_ID,
    GOOGLE_SERVICE_ACCOUNT_ITEM_ID,
};
use crate::{db::MailboxConfig, error::AppError, mail::registered_tcp_port, models::SmtpSecurity};
use lettre::{
    transport::smtp::{SUBMISSIONS_PORT, SUBMISSION_PORT},
    Address,
};
use serde_json::Value;
use std::str::FromStr;

/// The Skarbiec tag that declares an item one of Skrzynka's mailboxes.
pub const MAILBOX_TAG: &str = "skrzynka:mailbox";

#[derive(PartialEq, Eq)]
enum MailAuthentication {
    Password,
    GoogleOAuth,
    GoogleDelegation,
}

fn authentication_method(
    fields: &serde_json::Map<String, Value>,
) -> Result<MailAuthentication, AppError> {
    let Some(value) = fields.get("auth_method") else {
        return Ok(MailAuthentication::Password);
    };
    match value.as_str().map(str::trim) {
        Some("password") => Ok(MailAuthentication::Password),
        Some("oauth2") => Ok(MailAuthentication::GoogleOAuth),
        Some("oauth2_service_account") => Ok(MailAuthentication::GoogleDelegation),
        _ => Err(invalid_item(
            "auth_method must be password, oauth2, or oauth2_service_account when present; an invalid declaration does not select password authentication",
        )),
    }
}

impl SkarbiecResolver {
    /// Every item the vault declares a mailbox, by id.
    pub async fn declared_mailbox_items(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .list_items()
            .await?
            .into_iter()
            .filter(|item| item.tags.iter().any(|tag| tag == MAILBOX_TAG))
            .map(|item| item.id)
            .collect())
    }

    /// Add or remove [`MAILBOX_TAG`] on one item, keeping every other tag it
    /// carries. Nothing else about the item changes.
    pub async fn set_mailbox_declared(
        &self,
        item_id: &str,
        declared: bool,
    ) -> Result<(), AppError> {
        validate_item_id(item_id)?;
        let item = self
            .list_items()
            .await?
            .into_iter()
            .find(|item| item.id == item_id)
            .ok_or_else(|| {
                invalid_item("selected Skarbiec item is missing, unreadable, or unavailable")
            })?;
        let mut tags = item
            .tags
            .into_iter()
            .filter(|tag| tag != MAILBOX_TAG)
            .collect::<Vec<_>>();
        if declared {
            tags.push(MAILBOX_TAG.to_string());
        }
        let joined = tags.join(",");
        let output = self.output(&["retag", item_id, "--tags", &joined]).await?;
        if !output.status.success() {
            return Err(AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                format!(
                    "Skarbiec refused to retag item '{item_id}': {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                false,
            ));
        }
        Ok(())
    }

    pub async fn resolve_credentials(
        &self,
        item_id: &str,
    ) -> Result<ResolvedCredentials, AppError> {
        validate_item_id(item_id)?;
        let payload = self.get_item(item_id).await?;
        let fields = payload
            .get("fields")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid_item("item has no canonical fields object"))?;
        let username = required_text(fields.get("username"), "username")?;
        let method = authentication_method(fields)?;
        if method == MailAuthentication::GoogleOAuth {
            if optional_text(fields.get("oauth_provider")).as_deref() != Some("google") {
                return Err(invalid_item("unsupported OAuth mail provider"));
            }
            let refresh_token = required_text(fields.get("refresh_token"), "refresh_token")?;
            let items = self.list_items().await?;
            if !super::reference_names(
                optional_text(fields.get("oauth_client_item_id")).as_deref(),
                GOOGLE_OAUTH_CLIENT_ITEM_ID,
                &items,
            ) {
                return Err(invalid_item(
                    "Gmail authorization does not reference the item playing Skrzynka's desktop OAuth client role",
                ));
            }
            let access_token = self
                .refresh_google_access_token(item_id, &refresh_token)
                .await?;
            return Ok(ResolvedCredentials::OAuth2 {
                username,
                access_token,
            });
        }
        if method == MailAuthentication::GoogleDelegation {
            if optional_text(fields.get("oauth_provider")).as_deref() != Some("google") {
                return Err(invalid_item("unsupported OAuth mail provider"));
            }
            let items = self.list_items().await?;
            if !super::reference_names(
                optional_text(fields.get("service_account_item_id")).as_deref(),
                GOOGLE_SERVICE_ACCOUNT_ITEM_ID,
                &items,
            ) {
                return Err(invalid_item(
                    "Gmail delegation does not reference the item playing Skrzynka's service account role",
                ));
            }
            let access_token = self.delegated_access_token(item_id, &username).await?;
            return Ok(ResolvedCredentials::OAuth2 {
                username,
                access_token,
            });
        }
        let password = required_text(fields.get("password"), "password")?;
        Ok(ResolvedCredentials::Password { username, password })
    }

    /// The mailbox profile an item declares. Every value comes from the item;
    /// which items are mailboxes is the tag's to say, read by `list_items`.
    pub async fn resolve_mailbox_config(&self, item_id: &str) -> Result<MailboxConfig, AppError> {
        validate_item_id(item_id)?;
        let payload = self.get_item(item_id).await?;
        let fields = payload
            .get("fields")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid_item("item has no canonical fields object"))?;
        let username = required_text(fields.get("username"), "username")?;
        match authentication_method(fields)? {
            MailAuthentication::GoogleOAuth => {
                required_text(fields.get("refresh_token"), "refresh_token")?;
            }
            MailAuthentication::GoogleDelegation => {
                required_text(
                    fields.get("service_account_item_id"),
                    "service_account_item_id",
                )?;
            }
            MailAuthentication::Password => {
                required_text(fields.get("password"), "password")?;
            }
        }

        let email = optional_text(fields.get("email"))
            .or_else(|| username.contains('@').then_some(username.clone()))
            .ok_or_else(|| profile_error("Skarbiec item must supply the mailbox email"))?;
        Address::from_str(&email).map_err(|cause| {
            profile_error(format!("email '{email}' is not a valid address: {cause}"))
        })?;

        // A mailbox that states no display name is shown by its address.
        let display_name = match optional_text(fields.get("display_name")) {
            Some(name) => name,
            None => email.clone(),
        };
        let imap_host = optional_text(fields.get("imap_host"))
            .ok_or_else(|| profile_error("Skarbiec item must supply imap_host"))?;
        let smtp_host = optional_text(fields.get("smtp_host"))
            .ok_or_else(|| profile_error("Skarbiec item must supply smtp_host"))?;
        validate_hostname(&imap_host, "imap_host")?;
        validate_hostname(&smtp_host, "smtp_host")?;

        let smtp_security = match fields.get("smtp_security") {
            None => SmtpSecurity::Starttls,
            Some(value) => {
                let text = value.as_str().ok_or_else(|| {
                    profile_error(format!(
                        "Skarbiec item smtp_security must be text, not {value}"
                    ))
                })?;
                SmtpSecurity::from_str(text).map_err(|cause| {
                    profile_error(format!("Skarbiec item {cause}, not '{text}'"))
                })?
            }
        };
        let imap_port = match fields.get("imap_port") {
            Some(value) => port_field(value, "imap_port")?,
            None => registered_tcp_port("imaps")
                .map_err(|cause| profile_error(format!("the item states no port and {cause}")))?,
        };
        let smtp_port = match fields.get("smtp_port") {
            Some(value) => port_field(value, "smtp_port")?,
            None => match smtp_security {
                SmtpSecurity::Starttls => SUBMISSION_PORT,
                SmtpSecurity::Tls => SUBMISSIONS_PORT,
            },
        };
        let smtp_skarbiec_item_id = match fields.get("smtp_skarbiec_item_id") {
            None | Some(Value::Null) => None,
            Some(value) => Some(required_text(Some(value), "smtp_skarbiec_item_id")?),
        };
        if let Some(item_id) = smtp_skarbiec_item_id.as_deref() {
            validate_item_id(item_id)?;
        }
        let display_name = display_name.trim().to_string();
        if display_name.is_empty() {
            return Err(profile_error("display_name must not be empty"));
        }

        Ok(MailboxConfig {
            organization_id: String::new(),
            skarbiec_item_id: item_id.to_string(),
            smtp_skarbiec_item_id,
            display_name,
            email,
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            smtp_security,
        })
    }
}
