//! Mailbox credentials and configuration resolved from a Skarbiec item, and the
//! declaration that makes an item a mailbox at all.
//!
//! Skarbiec owns the account list. An item is one of Skrzynka's mailboxes
//! exactly when it carries [`MAILBOX_TAG`], the same way Brama reads an item
//! as a subscription only when it carries `brama:subscription`. Skrzynka keeps
//! mail and sync state for those items; it keeps no list of its own.

use super::{
    invalid_item, optional_port, optional_text, profile_error, required_text, validate_hostname,
    validate_item_id, ResolvedCredentials, SkarbiecResolver,
    GOOGLE_OAUTH_CLIENT_ITEM_ID, GOOGLE_SERVICE_ACCOUNT_ITEM_ID,
};
use crate::{
    db::MailboxConfig,
    error::AppError,
    models::{
        SmtpSecurity, MAX_DISPLAY_NAME_CHARS, MAX_POLL_INTERVAL_SECONDS, MIN_POLL_INTERVAL_SECONDS,
    },
};
use lettre::Address;
use serde_json::Value;
use std::str::FromStr;

/// The Skarbiec tag that declares an item one of Skrzynka's mailboxes.
pub const MAILBOX_TAG: &str = "skrzynka:mailbox";

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
    pub async fn set_mailbox_declared(&self, item_id: &str, declared: bool) -> Result<(), AppError> {
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
        if optional_text(fields.get("auth_method")).as_deref() == Some("oauth2") {
            if optional_text(fields.get("oauth_provider")).as_deref() != Some("google") {
                return Err(invalid_item("unsupported OAuth mail provider"));
            }
            let refresh_token = required_text(fields.get("refresh_token"), "refresh_token")?;
            if optional_text(fields.get("oauth_client_item_id")).as_deref()
                != Some(GOOGLE_OAUTH_CLIENT_ITEM_ID)
            {
                return Err(invalid_item(
                    "Gmail authorization does not reference Skrzynka's desktop OAuth client",
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
        if optional_text(fields.get("auth_method")).as_deref() == Some("oauth2_service_account") {
            if optional_text(fields.get("oauth_provider")).as_deref() != Some("google") {
                return Err(invalid_item("unsupported OAuth mail provider"));
            }
            if optional_text(fields.get("service_account_item_id")).as_deref()
                != Some(GOOGLE_SERVICE_ACCOUNT_ITEM_ID)
            {
                return Err(invalid_item(
                    "Gmail delegation does not reference Skrzynka's service account item",
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
    /// `poll_interval_seconds` is read from it too and defaults to the
    /// process setting.
    pub async fn resolve_mailbox_config(
        &self,
        item_id: &str,
        default_poll_interval_seconds: u64,
    ) -> Result<MailboxConfig, AppError> {
        validate_item_id(item_id)?;
        let payload = self.get_item(item_id).await?;
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_item("item has no canonical kind"))?;
        if !matches!(kind, "login" | "bundle") {
            return Err(invalid_item("item kind must be login or bundle"));
        }
        let fields = payload
            .get("fields")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid_item("item has no canonical fields object"))?;
        let username = required_text(fields.get("username"), "username")?;
        match optional_text(fields.get("auth_method")).as_deref() {
            Some("oauth2") => {
                required_text(fields.get("refresh_token"), "refresh_token")?;
            }
            Some("oauth2_service_account") => {
                required_text(
                    fields.get("service_account_item_id"),
                    "service_account_item_id",
                )?;
            }
            _ => {
                required_text(fields.get("password"), "password")?;
            }
        }

        let email = optional_text(fields.get("email"))
            .or_else(|| username.contains('@').then_some(username.clone()))
            .ok_or_else(|| profile_error("Skarbiec item must supply the mailbox email"))?;
        Address::from_str(&email).map_err(|_| profile_error("email is not a valid address"))?;

        let display_name =
            optional_text(fields.get("display_name")).unwrap_or_else(|| email.clone());
        let imap_host = optional_text(fields.get("imap_host"))
            .ok_or_else(|| profile_error("Skarbiec item must supply imap_host"))?;
        let smtp_host = optional_text(fields.get("smtp_host"))
            .ok_or_else(|| profile_error("Skarbiec item must supply smtp_host"))?;
        validate_hostname(&imap_host, "imap_host")?;
        validate_hostname(&smtp_host, "smtp_host")?;

        let smtp_security = match fields.get("smtp_security") {
            None => SmtpSecurity::Starttls,
            Some(value) => value
                .as_str()
                .and_then(|text| SmtpSecurity::from_str(text).ok())
                .ok_or_else(|| {
                    profile_error("Skarbiec item smtp_security must be starttls or tls")
                })?,
        };
        let port = |name: &str, default: u16| -> Result<u16, AppError> {
            match fields.get(name) {
                None => Ok(default),
                Some(value) => optional_port(Some(value))
                    .filter(|port| *port != 0)
                    .ok_or_else(|| {
                        profile_error(format!("Skarbiec item {name} must be between 1 and 65535"))
                    }),
            }
        };
        let imap_port = port("imap_port", 993)?;
        let smtp_port = port(
            "smtp_port",
            match smtp_security {
                SmtpSecurity::Starttls => 587,
                SmtpSecurity::Tls => 465,
            },
        )?;
        let smtp_skarbiec_item_id = optional_text(fields.get("smtp_skarbiec_item_id"));
        if let Some(item_id) = smtp_skarbiec_item_id.as_deref() {
            validate_item_id(item_id)?;
        }
        let poll_interval_seconds = match fields.get("poll_interval_seconds") {
            None => default_poll_interval_seconds,
            Some(value) => value.as_u64().ok_or_else(|| {
                profile_error("Skarbiec item poll_interval_seconds must be a whole number")
            })?,
        };
        if !(MIN_POLL_INTERVAL_SECONDS..=MAX_POLL_INTERVAL_SECONDS).contains(&poll_interval_seconds)
        {
            return Err(profile_error(
                "poll_interval_seconds must be between 15 and 86400",
            ));
        }
        let display_name = display_name.trim().to_string();
        if display_name.is_empty() || display_name.chars().count() > MAX_DISPLAY_NAME_CHARS {
            return Err(profile_error(
                "display_name must contain between 1 and 200 characters",
            ));
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
            poll_interval_seconds,
        })
    }
}
