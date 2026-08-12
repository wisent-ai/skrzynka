use crate::{
    db::MailboxConfig,
    error::AppError,
    gmail::GmailProfile,
    models::{CreateMailboxRequest, SkarbiecItemMetadata, SmtpSecurity},
};
use chrono::{Duration as ChronoDuration, Utc};
use lettre::Address;
use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap, path::PathBuf, process::Stdio, str::FromStr, sync::Arc, time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command, sync::Mutex};

const MAX_SKARBIEC_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const GOOGLE_OAUTH_CLIENT_ITEM_ID: &str = "skrzynka-google-oauth-desktop";

pub enum ResolvedCredentials {
    Password {
        username: String,
        password: String,
    },
    OAuth2 {
        username: String,
        access_token: String,
    },
}

#[derive(Clone)]
struct CachedAccessToken {
    value: String,
    expires_at: chrono::DateTime<Utc>,
}

#[derive(Clone)]
pub(crate) struct GoogleOAuthClient {
    pub client_id: String,
    pub client_secret: String,
    pub auth_uri: String,
    pub token_uri: String,
}

#[derive(Clone)]
pub struct SkarbiecResolver {
    binary: PathBuf,
    client: Client,
    token_cache: Arc<Mutex<HashMap<String, CachedAccessToken>>>,
}

impl SkarbiecResolver {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            client: Client::new(),
            token_cache: Arc::new(Mutex::new(HashMap::new())),
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
        if optional_text(fields.get("auth_method")).as_deref() == Some("oauth2") {
            required_text(fields.get("refresh_token"), "refresh_token")?;
        } else {
            required_text(fields.get("password"), "password")?;
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
            organization_id: String::new(),
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

    async fn refresh_google_access_token(
        &self,
        credential_item_id: &str,
        refresh_token: &str,
    ) -> Result<String, AppError> {
        if let Some(cached) = self
            .token_cache
            .lock()
            .await
            .get(credential_item_id)
            .cloned()
        {
            if cached.expires_at > Utc::now() + ChronoDuration::seconds(60) {
                return Ok(cached.value);
            }
        }
        let oauth_client = self.google_oauth_client().await?;
        let response = self
            .client
            .post(&oauth_client.token_uri)
            .form(&[
                ("client_id", oauth_client.client_id.as_str()),
                ("client_secret", oauth_client.client_secret.as_str()),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .map_err(|_| {
                AppError::dependency(
                    "GMAIL_TOKEN_REFRESH_UNAVAILABLE",
                    "Google token service is unavailable",
                    true,
                )
            })?;
        let status = response.status();
        let payload: Value = response.json().await.map_err(|_| {
            AppError::dependency(
                "GMAIL_TOKEN_RESPONSE_INVALID",
                "Google token service returned invalid JSON",
                false,
            )
        })?;
        if !status.is_success() {
            return Err(AppError::dependency(
                "GMAIL_AUTHORIZATION_EXPIRED",
                "Google rejected the saved Gmail authorization; reconnect the profile",
                false,
            ));
        }
        let access_token = payload
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::dependency(
                    "GMAIL_TOKEN_RESPONSE_INVALID",
                    "Google token response contained no access token",
                    false,
                )
            })?
            .to_string();
        let expires_in = payload
            .get("expires_in")
            .and_then(Value::as_i64)
            .unwrap_or(3600)
            .clamp(60, 86_400);
        self.token_cache.lock().await.insert(
            credential_item_id.to_string(),
            CachedAccessToken {
                value: access_token.clone(),
                expires_at: Utc::now() + ChronoDuration::seconds(expires_in),
            },
        );
        Ok(access_token)
    }

    pub(crate) async fn google_oauth_client(&self) -> Result<GoogleOAuthClient, AppError> {
        let payload = self.get_item(GOOGLE_OAUTH_CLIENT_ITEM_ID).await?;
        let wrapped = payload
            .pointer("/fields/value")
            .ok_or_else(|| invalid_item("Google OAuth client item has no fields.value"))?;
        let raw = wrapped
            .get("value")
            .and_then(Value::as_str)
            .or_else(|| wrapped.as_str())
            .ok_or_else(|| invalid_item("Google OAuth client value is not text"))?;
        let document: Value = serde_json::from_str(raw)
            .map_err(|_| invalid_item("Google OAuth client value is invalid JSON"))?;
        let profile = document
            .get("installed")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                invalid_item("Google OAuth client is not a desktop application client")
            })?;
        let auth_uri = optional_text(profile.get("auth_uri"))
            .unwrap_or_else(|| "https://accounts.google.com/o/oauth2/auth".to_string());
        let token_uri = optional_text(profile.get("token_uri"))
            .unwrap_or_else(|| "https://oauth2.googleapis.com/token".to_string());
        if auth_uri != "https://accounts.google.com/o/oauth2/auth"
            || token_uri != "https://oauth2.googleapis.com/token"
        {
            return Err(invalid_item(
                "Google OAuth client endpoints are not canonical",
            ));
        }
        Ok(GoogleOAuthClient {
            client_id: required_text(profile.get("client_id"), "client_id")?,
            client_secret: required_text(profile.get("client_secret"), "client_secret")?,
            auth_uri,
            token_uri,
        })
    }

    async fn set_item(&self, item_id: &str, kind: &str, payload: &Value) -> Result<(), AppError> {
        validate_item_id(item_id)?;
        let bytes = serde_json::to_vec(payload)
            .map_err(|_| AppError::internal("Gmail credential payload could not be encoded"))?;
        let mut command = Command::new(&self.binary);
        command
            .args(["set-json", item_id, "--type", kind])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| {
            AppError::dependency(
                "SKARBIEC_UNAVAILABLE",
                "Skarbiec could not be started from the configured path",
                true,
            )
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::internal("Skarbiec input pipe was unavailable"))?;
        stdin.write_all(&bytes).await.map_err(|_| {
            AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                "Gmail authorization could not be sent to Skarbiec",
                false,
            )
        })?;
        drop(stdin);
        let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
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
                    "SKARBIEC_WRITE_FAILED",
                    "Skarbiec did not persist Gmail authorization",
                    false,
                )
            })?;
        if !output.status.success() {
            return Err(AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                "Skarbiec rejected the Gmail authorization item",
                false,
            ));
        }
        Ok(())
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

fn looks_like_google_profile(item_id: &str, email: &str, payload: &Value) -> bool {
    let id = item_id.to_ascii_lowercase();
    let domain = email
        .rsplit_once('@')
        .map(|(_, domain)| domain.to_ascii_lowercase());
    let context_mentions_google = payload
        .get("context")
        .and_then(|value| serde_json::to_string(value).ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("google"));
    domain.as_deref() == Some("gmail.com")
        || id.contains("google")
        || id.contains("gmail")
        || context_mentions_google
}

fn profile_preference(item_id: &str) -> u8 {
    let id = item_id.to_ascii_lowercase();
    u8::from(id.contains("gmail")) * 2 + u8::from(id.contains("google"))
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
