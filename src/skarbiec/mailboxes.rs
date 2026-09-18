//! Mailbox credentials and configuration resolved from a Skarbiec item.

use super::{invalid_item, optional_port, optional_text, profile_error, required_text, validate_hostname, validate_item_id, ResolvedCredentials, SkarbiecResolver, DEFAULT_POLL_INTERVAL_SECONDS, GOOGLE_OAUTH_CLIENT_ITEM_ID, GOOGLE_SERVICE_ACCOUNT_ITEM_ID};
use crate::{
    db::MailboxConfig,
    error::AppError,
    models::{CreateMailboxRequest, SmtpSecurity, MAX_DISPLAY_NAME_CHARS, MAX_POLL_INTERVAL_SECONDS, MIN_POLL_INTERVAL_SECONDS},
};
use lettre::Address;
use serde_json::Value;
use std::str::FromStr;

impl SkarbiecResolver {
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

    pub async fn resolve_mailbox_config(
        &self,
        request: &CreateMailboxRequest,
    ) -> Result<MailboxConfig, AppError> {
        validate_item_id(&request.skarbiec_item_id)?;
        let payload = self.get_item(&request.skarbiec_item_id).await?;
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

        let email = request
            .email
            .clone()
            .or_else(|| optional_text(fields.get("email")))
            .or_else(|| username.contains('@').then_some(username.clone()))
            .ok_or_else(|| profile_error("email is required"))?;
        Address::from_str(&email).map_err(|_| profile_error("email is not a valid address"))?;

        let display_name = request
            .display_name
            .clone()
            .or_else(|| optional_text(fields.get("display_name")))
            .unwrap_or_else(|| email.clone());
        let imap_host = request
            .imap_host
            .clone()
            .or_else(|| optional_text(fields.get("imap_host")))
            .ok_or_else(|| profile_error("imap_host is required"))?;
        let smtp_host = request
            .smtp_host
            .clone()
            .or_else(|| optional_text(fields.get("smtp_host")))
            .ok_or_else(|| profile_error("smtp_host is required"))?;
        validate_hostname(&imap_host, "imap_host")?;
        validate_hostname(&smtp_host, "smtp_host")?;

        let smtp_security = request
            .smtp_security
            .or_else(|| {
                optional_text(fields.get("smtp_security"))
                    .and_then(|value| SmtpSecurity::from_str(&value).ok())
            })
            .unwrap_or(SmtpSecurity::Starttls);
        let imap_port = request
            .imap_port
            .or_else(|| optional_port(fields.get("imap_port")))
            .unwrap_or(993);
        let smtp_port = request
            .smtp_port
            .or_else(|| optional_port(fields.get("smtp_port")))
            .unwrap_or(match smtp_security {
                SmtpSecurity::Starttls => 587,
                SmtpSecurity::Tls => 465,
            });
        if imap_port == 0 || smtp_port == 0 {
            return Err(profile_error("mail server ports must be nonzero"));
        }
        let poll_interval_seconds = request
            .poll_interval_seconds
            .unwrap_or(DEFAULT_POLL_INTERVAL_SECONDS);
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
            skarbiec_item_id: request.skarbiec_item_id.clone(),
            smtp_skarbiec_item_id: None,
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
