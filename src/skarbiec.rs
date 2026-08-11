use crate::{
    db::MailboxConfig,
    error::AppError,
    models::{CreateMailboxRequest, SkarbiecItemMetadata, SmtpSecurity},
};
use lettre::Address;
use serde_json::Value;
use std::{path::PathBuf, str::FromStr, time::Duration};
use tokio::process::Command;

const MAX_SKARBIEC_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub struct ResolvedCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Clone)]
pub struct SkarbiecResolver {
    binary: PathBuf,
}

impl SkarbiecResolver {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub async fn is_available(&self) -> bool {
        self.output(&["version"])
            .await
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

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
        let password = required_text(fields.get("password"), "password")?;
        Ok(ResolvedCredentials { username, password })
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
        let _password = required_text(fields.get("password"), "password")?;

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
        let poll_interval_seconds = request.poll_interval_seconds.unwrap_or(60);
        if !(15..=86_400).contains(&poll_interval_seconds) {
            return Err(profile_error(
                "poll_interval_seconds must be between 15 and 86400",
            ));
        }
        let display_name = display_name.trim().to_string();
        if display_name.is_empty() || display_name.chars().count() > 200 {
            return Err(profile_error(
                "display_name must contain between 1 and 200 characters",
            ));
        }

        Ok(MailboxConfig {
            skarbiec_item_id: request.skarbiec_item_id.clone(),
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

    async fn get_item(&self, item_id: &str) -> Result<Value, AppError> {
        let output = self.output(&["get", item_id]).await?;
        if !output.status.success() {
            return Err(invalid_item(
                "selected Skarbiec item is missing, unreadable, or unavailable",
            ));
        }
        bounded_stdout(&output.stdout)?;
        serde_json::from_slice(&output.stdout).map_err(|_| {
            AppError::dependency(
                "SKARBIEC_RESPONSE_INVALID",
                "Skarbiec returned invalid item JSON",
                false,
            )
        })
    }

    async fn output(&self, arguments: &[&str]) -> Result<std::process::Output, AppError> {
        let mut command = Command::new(&self.binary);
        command.args(arguments);
        command.kill_on_drop(true);
        tokio::time::timeout(Duration::from_secs(15), command.output())
            .await
            .map_err(|_| {
                AppError::dependency(
                    "SKARBIEC_TIMEOUT",
                    "Skarbiec did not finish within 15 seconds",
                    true,
                )
            })?
            .map_err(|_| {
                AppError::dependency(
                    "SKARBIEC_UNAVAILABLE",
                    "Skarbiec could not be started from the configured path",
                    true,
                )
            })
    }
}

fn bounded_stdout(stdout: &[u8]) -> Result<(), AppError> {
    if stdout.len() > MAX_SKARBIEC_RESPONSE_BYTES {
        return Err(AppError::dependency(
            "SKARBIEC_RESPONSE_TOO_LARGE",
            "Skarbiec response exceeded the 2 MiB safety limit",
            false,
        ));
    }
    Ok(())
}

fn required_text(value: Option<&Value>, name: &str) -> Result<String, AppError> {
    let value = value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_item(format!("item field {name} is required and must be text")))?;
    Ok(value.to_string())
}

fn optional_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_port(value: Option<&Value>) -> Option<u16> {
    value.and_then(|value| {
        value
            .as_u64()
            .and_then(|number| u16::try_from(number).ok())
            .or_else(|| value.as_str().and_then(|text| text.parse::<u16>().ok()))
    })
}

fn validate_item_id(item_id: &str) -> Result<(), AppError> {
    if item_id.is_empty() || item_id.len() > 256 || item_id.chars().any(char::is_whitespace) {
        return Err(profile_error(
            "skarbiec_item_id must contain 1 to 256 non-whitespace characters",
        ));
    }
    Ok(())
}

fn validate_hostname(value: &str, field: &str) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > 253
        || value.contains("://")
        || value.chars().any(char::is_whitespace)
    {
        return Err(profile_error(format!(
            "{field} must be a hostname without a URL scheme"
        )));
    }
    Ok(())
}

fn invalid_item(message: impl Into<String>) -> AppError {
    AppError::dependency("SKARBIEC_ITEM_INVALID", message, false)
}

fn profile_error(message: impl Into<String>) -> AppError {
    AppError::invalid("MAILBOX_PROFILE_INVALID", message)
}
