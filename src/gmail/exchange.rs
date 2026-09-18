//! Completing a flow: claiming the pending record, exchanging the code, reading the identity.

use super::{
    GmailAuthorization, GmailOAuthBroker, GmailOAuthFailure, GmailOAuthFlowStatus, PendingFlow,
    GOOGLE_USERINFO_URL,
};
use crate::error::AppError;
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

impl GmailOAuthBroker {
    pub(super) async fn begin_completion(&self, flow_id: Uuid) -> Result<PendingFlow, AppError> {
        let mut flows = self.flows.lock().await;
        let record = flows
            .get_mut(&flow_id)
            .ok_or_else(|| AppError::not_found("Gmail OAuth flow"))?;
        if record.expires_at < Utc::now() {
            record.pending = None;
            record.status = GmailOAuthFlowStatus::Failed(GmailOAuthFailure {
                code: "GMAIL_OAUTH_FLOW_EXPIRED",
                message: "Gmail authorization flow expired".to_string(),
                retryable: true,
            });
            return Err(AppError::invalid(
                "GMAIL_OAUTH_FLOW_EXPIRED",
                "Gmail authorization flow expired",
            ));
        }
        if !matches!(record.status, GmailOAuthFlowStatus::Pending) {
            return Err(AppError::conflict(
                "GMAIL_OAUTH_FLOW_CONSUMED",
                "Gmail authorization flow is already being processed or completed",
            ));
        }
        let pending = record.pending.clone().ok_or_else(|| {
            AppError::conflict(
                "GMAIL_OAUTH_FLOW_CONSUMED",
                "Gmail authorization flow has no pending authorization",
            )
        })?;
        record.status = GmailOAuthFlowStatus::Processing;
        Ok(pending)
    }

    pub(super) async fn exchange_code(
        &self,
        pending: &PendingFlow,
        code: &str,
    ) -> Result<GmailAuthorization, AppError> {
        let response = self
            .client
            .post(&pending.oauth_client.token_uri)
            .form(&[
                ("client_id", pending.oauth_client.client_id.as_str()),
                ("client_secret", pending.oauth_client.client_secret.as_str()),
                ("code", code),
                ("code_verifier", pending.verifier.as_str()),
                ("redirect_uri", self.callback_url.as_str()),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await
            .map_err(|_| {
                AppError::dependency(
                    "GMAIL_OAUTH_UNAVAILABLE",
                    "Google token service is unavailable",
                    true,
                )
            })?;
        let status = response.status();
        let payload: Value = response.json().await.map_err(|_| {
            AppError::dependency(
                "GMAIL_OAUTH_RESPONSE_INVALID",
                "Google token service returned invalid JSON",
                false,
            )
        })?;
        if !status.is_success() {
            return Err(AppError::dependency(
                "GMAIL_OAUTH_REJECTED",
                "Google rejected Gmail authorization",
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
                    "GMAIL_OAUTH_RESPONSE_INVALID",
                    "Google returned no access token",
                    false,
                )
            })?;
        let refresh_token = payload
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::dependency(
                    "GMAIL_REFRESH_TOKEN_MISSING",
                    "Google returned no durable Gmail authorization; reconnect the profile",
                    false,
                )
            })?;
        let userinfo_response = self
            .client
            .get(GOOGLE_USERINFO_URL)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| {
                AppError::dependency(
                    "GMAIL_IDENTITY_UNAVAILABLE",
                    "Google identity service is unavailable",
                    true,
                )
            })?;
        let userinfo_status = userinfo_response.status();
        let userinfo: Value = userinfo_response.json().await.map_err(|_| {
            AppError::dependency(
                "GMAIL_OAUTH_RESPONSE_INVALID",
                "Google identity service returned invalid JSON",
                false,
            )
        })?;
        if !userinfo_status.is_success() {
            return Err(AppError::dependency(
                "GMAIL_IDENTITY_UNAVAILABLE",
                "Google rejected the identity lookup",
                false,
            ));
        }
        let authorized_email = userinfo
            .get("email")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::dependency(
                    "GMAIL_OAUTH_IDENTITY_MISSING",
                    "Google authorization returned no account email",
                    false,
                )
            })?;
        if !authorized_email.eq_ignore_ascii_case(&pending.expected_email) {
            return Err(AppError::invalid(
                "GMAIL_OAUTH_ACCOUNT_MISMATCH",
                format!(
                    "Google authorized {authorized_email}, but the selected Skarbiec profile is {}",
                    pending.expected_email
                ),
            ));
        }
        let credential_item_id = self
            .resolver
            .save_gmail_authorization(&pending.source_item_id, authorized_email, refresh_token)
            .await?;
        Ok(GmailAuthorization {
            organization_id: pending.organization_id.clone(),
            credential_item_id,
            email: authorized_email.to_string(),
        })
    }

    pub(super) async fn fail(&self, flow_id: Uuid, error: &AppError) {
        let mut flows = self.flows.lock().await;
        if let Some(record) = flows.get_mut(&flow_id) {
            record.status = GmailOAuthFlowStatus::Failed(GmailOAuthFailure {
                code: error.code,
                message: error.message.clone(),
                retryable: error.retryable,
            });
            record.pending = None;
        }
    }
}
