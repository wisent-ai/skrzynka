//! Google OAuth clients, service accounts and the access tokens minted from them.

use super::{invalid_item, optional_text, required_text, CachedAccessToken, GoogleOAuthClient, GoogleServiceAccount, SkarbiecResolver, GMAIL_DELEGATION_SCOPE, GOOGLE_ADMIN_DELEGATION_URL, GOOGLE_OAUTH_CLIENT_ITEM_ID, GOOGLE_SERVICE_ACCOUNT_ITEM_ID, GOOGLE_TOKEN_URI, TOKEN_EXPIRY_MARGIN_SECONDS};
use crate::error::AppError;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{json, Value};

impl SkarbiecResolver {
    pub(super) async fn refresh_google_access_token(
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
            if cached.expires_at > Utc::now() + ChronoDuration::seconds(TOKEN_EXPIRY_MARGIN_SECONDS)
            {
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

    pub(crate) async fn google_service_account(&self) -> Result<GoogleServiceAccount, AppError> {
        let payload = self.get_item(GOOGLE_SERVICE_ACCOUNT_ITEM_ID).await?;
        let wrapped = payload
            .pointer("/fields/value")
            .ok_or_else(|| invalid_item("Google service account item has no fields.value"))?;
        let raw = wrapped
            .get("value")
            .and_then(Value::as_str)
            .or_else(|| wrapped.as_str())
            .ok_or_else(|| invalid_item("Google service account value is not text"))?;
        let document: Value = serde_json::from_str(raw)
            .map_err(|_| invalid_item("Google service account value is invalid JSON"))?;
        if document.get("type").and_then(Value::as_str) != Some("service_account") {
            return Err(invalid_item(
                "Google service account JSON is not a service_account key",
            ));
        }
        let token_uri = document
            .get("token_uri")
            .and_then(Value::as_str)
            .unwrap_or(GOOGLE_TOKEN_URI)
            .to_string();
        if token_uri != GOOGLE_TOKEN_URI {
            return Err(invalid_item(
                "Google service account token endpoint is not canonical",
            ));
        }
        let fields = document
            .as_object()
            .ok_or_else(|| invalid_item("Google service account JSON is not an object"))?;
        Ok(GoogleServiceAccount {
            client_email: required_text(fields.get("client_email"), "client_email")?,
            client_id: required_text(fields.get("client_id"), "client_id")?,
            private_key: required_text(fields.get("private_key"), "private_key")?,
            private_key_id: optional_text(fields.get("private_key_id")),
            token_uri,
        })
    }

    /// Mint a delegated access token for `user_email` through the RFC 7523
    /// JWT-bearer grant. Google authorizes it against the domain-wide
    /// delegation table of the user's Workspace domain — there is no consent
    /// screen and no refresh token; every token is minted from the key.
    pub(crate) async fn delegated_access_token(
        &self,
        cache_key: &str,
        user_email: &str,
    ) -> Result<String, AppError> {
        if let Some(cached) = self.token_cache.lock().await.get(cache_key).cloned() {
            if cached.expires_at > Utc::now() + ChronoDuration::seconds(TOKEN_EXPIRY_MARGIN_SECONDS)
            {
                return Ok(cached.value);
            }
        }
        let account = self.google_service_account().await?;
        let now = Utc::now().timestamp();
        let claims = json!({
            "iss": account.client_email,
            "sub": user_email,
            "aud": account.token_uri,
            "scope": GMAIL_DELEGATION_SCOPE,
            "iat": now,
            "exp": now + 3600,
        });
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = account.private_key_id.clone();
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(account.private_key.as_bytes()).map_err(
            |_| invalid_item("Google service account private key is not a valid RSA PEM"),
        )?;
        let assertion = jsonwebtoken::encode(&header, &claims, &key)
            .map_err(|_| AppError::internal("delegation assertion could not be signed"))?;
        let response = self
            .client
            .post(&account.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|_| {
                AppError::dependency(
                    "GOOGLE_TOKEN_UNAVAILABLE",
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
            let code = payload.get("error").and_then(Value::as_str).unwrap_or("");
            return Err(match code {
                "unauthorized_client" | "access_denied" => AppError::dependency(
                    "GOOGLE_DELEGATION_NOT_GRANTED",
                    format!(
                        "The Workspace admin has not granted domain-wide delegation to this \
                         service account. Open {GOOGLE_ADMIN_DELEGATION_URL}, add client ID \
                         {} with scope {GMAIL_DELEGATION_SCOPE}, then retry.",
                        account.client_id
                    ),
                    false,
                ),
                "invalid_grant" => AppError::dependency(
                    "GOOGLE_DELEGATION_REJECTED",
                    format!(
                        "Google refused delegated access for {user_email}; verify the address \
                         is an active user in a Workspace domain that granted client ID {}.",
                        account.client_id
                    ),
                    false,
                ),
                _ => AppError::dependency(
                    "GMAIL_TOKEN_RESPONSE_INVALID",
                    format!("Google rejected the delegation grant with HTTP {status}"),
                    false,
                ),
            });
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
            cache_key.to_string(),
            CachedAccessToken {
                value: access_token.clone(),
                expires_at: Utc::now() + ChronoDuration::seconds(expires_in),
            },
        );
        Ok(access_token)
    }

}
