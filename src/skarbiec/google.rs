//! Google OAuth clients, service accounts and the access tokens minted from them.

use super::{
    invalid_item, CachedAccessToken, SkarbiecResolver, GMAIL_DELEGATION_SCOPE,
    GOOGLE_ADMIN_DELEGATION_URL,
};
use crate::error::AppError;
use chrono::Utc;
use serde_json::{json, Value};
mod credentials;
mod token_response;
use token_response::{credential_fingerprint, refresh_refusal, token_expiry};

impl SkarbiecResolver {
    pub(super) async fn refresh_google_access_token(
        &self,
        credential_item_id: &str,
        refresh_token: &str,
    ) -> Result<String, AppError> {
        let oauth_client = self.google_oauth_client().await?;
        let fingerprint = credential_fingerprint([
            "refresh",
            refresh_token,
            &oauth_client.client_id,
            &oauth_client.client_secret,
            &oauth_client.token_uri,
        ]);
        {
            let cache = self.token_cache.lock().await;
            if let Some(cached) = cache.get(credential_item_id) {
                // Google's stated lifetime and the current credentials both govern reuse.
                if cached.expires_at > Utc::now() && cached.credential_fingerprint == fingerprint {
                    return Ok(cached.value.clone());
                }
            }
        }
        let requested_at = Utc::now();
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
            .map_err(|error| {
                AppError::dependency(
                    "GMAIL_TOKEN_REFRESH_UNAVAILABLE",
                    format!("Google refresh-token request failed: {error}"),
                    true,
                )
            })?;
        let status = response.status();
        let payload: Value = response.json().await.map_err(|error| {
            AppError::dependency(
                "GMAIL_TOKEN_RESPONSE_INVALID",
                format!("decode Google refresh-token response after HTTP {status}: {error}"),
                status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
            )
        })?;
        if !status.is_success() {
            return Err(refresh_refusal(status, &payload));
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
        // Cached only for the lifetime Google states; an answer without
        // expires_in is used for this call and asked for again next time.
        if let Some(expires_at) = token_expiry(&payload, requested_at)? {
            self.token_cache.lock().await.insert(
                credential_item_id.to_string(),
                CachedAccessToken {
                    value: access_token.clone(),
                    expires_at,
                    credential_fingerprint: fingerprint,
                },
            );
        }
        Ok(access_token)
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
        let account = self.google_service_account().await?;
        let fingerprint = credential_fingerprint(
            [
                "delegated",
                user_email,
                account.client_email.as_str(),
                account.client_id.as_str(),
                account.private_key.as_str(),
                account.token_uri.as_str(),
                GMAIL_DELEGATION_SCOPE,
            ]
            .into_iter()
            .chain(account.private_key_id.as_deref()),
        );
        {
            let cache = self.token_cache.lock().await;
            if let Some(cached) = cache.get(cache_key) {
                if cached.expires_at > Utc::now() && cached.credential_fingerprint == fingerprint {
                    return Ok(cached.value.clone());
                }
            }
        }
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
            |error| {
                invalid_item(&format!(
                    "Google service account private key is not a valid RSA PEM: {error}"
                ))
            },
        )?;
        let assertion = jsonwebtoken::encode(&header, &claims, &key).map_err(|error| {
            AppError::internal(format!("delegation assertion could not be signed: {error}"))
        })?;
        let requested_at = Utc::now();
        let response = self
            .client
            .post(&account.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|error| {
                AppError::dependency(
                    "GOOGLE_TOKEN_UNAVAILABLE",
                    format!("Google delegated-token request failed: {error}"),
                    true,
                )
            })?;
        let status = response.status();
        let payload: Value = response.json().await.map_err(|error| {
            AppError::dependency(
                "GMAIL_TOKEN_RESPONSE_INVALID",
                format!("decode Google delegated-token response after HTTP {status}: {error}"),
                status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
            )
        })?;
        if !status.is_success() {
            let code = payload.get("error").and_then(Value::as_str);
            let description = payload.get("error_description").and_then(Value::as_str);
            let observed =
                format!("HTTP {status}; error={code:?}; error_description={description:?}");
            return Err(match code {
                Some("unauthorized_client" | "access_denied") => AppError::dependency(
                    "GOOGLE_DELEGATION_NOT_GRANTED",
                    format!(
                        "Google refused domain-wide delegation: {observed}. Check the Workspace \
                         grant at {GOOGLE_ADMIN_DELEGATION_URL} for client ID \
                         {} with scope {GMAIL_DELEGATION_SCOPE}.",
                        account.client_id
                    ),
                    false,
                ),
                Some("invalid_grant") => AppError::dependency(
                    "GOOGLE_DELEGATION_REJECTED",
                    format!(
                        "Google refused delegated access for {user_email}: {observed}; verify the address \
                         is an active user in a Workspace domain that granted client ID {}.",
                        account.client_id
                    ),
                    false,
                ),
                _ => AppError::dependency(
                    "GMAIL_TOKEN_RESPONSE_INVALID",
                    format!("Google rejected the delegation grant: {observed}"),
                    status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
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
        // Cached only for the lifetime Google states, as above.
        if let Some(expires_at) = token_expiry(&payload, requested_at)? {
            self.token_cache.lock().await.insert(
                cache_key.to_string(),
                CachedAccessToken {
                    value: access_token.clone(),
                    expires_at,
                    credential_fingerprint: fingerprint,
                },
            );
        }
        Ok(access_token)
    }
}
