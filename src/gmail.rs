use crate::{
    error::AppError,
    skarbiec::{GoogleOAuthClient, SkarbiecResolver},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

const GMAIL_SCOPES: &str = "openid email https://mail.google.com/";
const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const FLOW_LIFETIME_MINUTES: i64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailProfile {
    pub skarbiec_item_id: String,
    pub email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartGmailOAuthRequest {
    pub skarbiec_item_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartGmailOAuthResponse {
    pub flow_id: Uuid,
    pub authorization_url: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GmailOAuthCallback {
    pub state: Option<String>,
    pub code: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GmailAuthorization {
    pub credential_item_id: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct GmailOAuthFailure {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone)]
pub enum GmailOAuthFlowStatus {
    Pending,
    Processing,
    Completed(GmailAuthorization),
    Failed(GmailOAuthFailure),
}

#[derive(Debug, Clone)]
pub struct GmailOAuthFlowSnapshot {
    pub flow_id: Uuid,
    pub expires_at: chrono::DateTime<Utc>,
    pub status: GmailOAuthFlowStatus,
}

#[derive(Clone)]
pub struct GmailOAuthBroker {
    resolver: SkarbiecResolver,
    client: Client,
    flows: Arc<Mutex<HashMap<Uuid, FlowRecord>>>,
    callback_url: Url,
}

#[derive(Clone)]
struct PendingFlow {
    verifier: String,
    source_item_id: String,
    expected_email: String,
    oauth_client: GoogleOAuthClient,
}

#[derive(Clone)]
struct FlowRecord {
    expires_at: chrono::DateTime<Utc>,
    status: GmailOAuthFlowStatus,
    pending: Option<PendingFlow>,
}

impl GmailOAuthBroker {
    pub fn new(resolver: SkarbiecResolver, callback_base_url: &str) -> Result<Self, AppError> {
        let mut callback_url = Url::parse(callback_base_url)
            .map_err(|_| AppError::internal("Gmail OAuth callback base URL is invalid"))?;
        if callback_url.scheme() != "http"
            || !callback_url
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|address| address.is_loopback())
            || callback_url.username() != ""
            || callback_url.password().is_some()
            || callback_url.query().is_some()
            || callback_url.fragment().is_some()
        {
            return Err(AppError::internal(
                "Gmail OAuth callback must use credential-free loopback HTTP",
            ));
        }
        callback_url.set_path("/v1/gmail/oauth/callback");
        Ok(Self {
            resolver,
            client: Client::new(),
            flows: Arc::new(Mutex::new(HashMap::new())),
            callback_url,
        })
    }

    pub async fn profiles(&self) -> Result<Vec<GmailProfile>, AppError> {
        self.resolver.list_google_profiles().await
    }

    pub async fn start(
        &self,
        request: StartGmailOAuthRequest,
    ) -> Result<StartGmailOAuthResponse, AppError> {
        let email = self
            .resolver
            .resolve_google_identity(&request.skarbiec_item_id)
            .await?;
        let oauth_client = self.resolver.google_oauth_client().await?;
        let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let flow_id = Uuid::new_v4();
        let expires_at = Utc::now() + Duration::minutes(FLOW_LIFETIME_MINUTES);
        let mut authorization_url = Url::parse(&oauth_client.auth_uri).map_err(|_| {
            AppError::dependency(
                "GMAIL_OAUTH_CLIENT_INVALID",
                "Google OAuth client has an invalid authorization endpoint",
                false,
            )
        })?;
        if authorization_url.scheme() != "https"
            || authorization_url.host_str() != Some("accounts.google.com")
        {
            return Err(AppError::dependency(
                "GMAIL_OAUTH_CLIENT_INVALID",
                "Google OAuth client authorization endpoint is not trusted",
                false,
            ));
        }
        authorization_url
            .query_pairs_mut()
            .append_pair("client_id", &oauth_client.client_id)
            .append_pair("redirect_uri", self.callback_url.as_str())
            .append_pair("response_type", "code")
            .append_pair("scope", GMAIL_SCOPES)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent")
            .append_pair("include_granted_scopes", "true")
            .append_pair("login_hint", &email)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &flow_id.to_string());

        self.flows.lock().await.insert(
            flow_id,
            FlowRecord {
                expires_at,
                status: GmailOAuthFlowStatus::Pending,
                pending: Some(PendingFlow {
                    verifier,
                    source_item_id: request.skarbiec_item_id,
                    expected_email: email,
                    oauth_client,
                }),
            },
        );
        Ok(StartGmailOAuthResponse {
            flow_id,
            authorization_url: authorization_url.to_string(),
            expires_at: expires_at.to_rfc3339(),
        })
    }

    pub async fn complete_callback(
        &self,
        callback: GmailOAuthCallback,
    ) -> Result<GmailAuthorization, AppError> {
        let flow_id = callback
            .state
            .as_deref()
            .ok_or_else(|| {
                AppError::invalid(
                    "GMAIL_OAUTH_STATE_MISSING",
                    "Google returned no OAuth state",
                )
            })
            .and_then(|state| {
                Uuid::parse_str(state).map_err(|_| {
                    AppError::invalid(
                        "GMAIL_OAUTH_STATE_INVALID",
                        "Google returned an invalid OAuth state",
                    )
                })
            })?;
        if callback.error.is_some() {
            let message = callback
                .error_description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("Google authorization was declined")
                .to_string();
            let error = AppError::invalid("GMAIL_OAUTH_REJECTED", message);
            self.fail(flow_id, &error).await;
            return Err(error);
        }
        let code = callback
            .code
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 4096)
            .ok_or_else(|| {
                AppError::invalid(
                    "GMAIL_OAUTH_CODE_INVALID",
                    "Google returned no valid authorization code",
                )
            })?
            .to_string();
        let pending = self.begin_completion(flow_id).await?;
        let result = self.exchange_code(&pending, &code).await;
        match result {
            Ok(authorization) => {
                let mut flows = self.flows.lock().await;
                if let Some(record) = flows.get_mut(&flow_id) {
                    record.status = GmailOAuthFlowStatus::Completed(authorization.clone());
                    record.pending = None;
                }
                Ok(authorization)
            }
            Err(error) => {
                self.fail(flow_id, &error).await;
                Err(error)
            }
        }
    }

    pub async fn status(&self, flow_id: Uuid) -> Result<GmailOAuthFlowSnapshot, AppError> {
        let mut flows = self.flows.lock().await;
        let record = flows
            .get_mut(&flow_id)
            .ok_or_else(|| AppError::not_found("Gmail OAuth flow"))?;
        if record.expires_at < Utc::now()
            && matches!(
                record.status,
                GmailOAuthFlowStatus::Pending | GmailOAuthFlowStatus::Processing
            )
        {
            record.status = GmailOAuthFlowStatus::Failed(GmailOAuthFailure {
                code: "GMAIL_OAUTH_FLOW_EXPIRED",
                message: "Gmail authorization flow expired".to_string(),
                retryable: true,
            });
            record.pending = None;
        }
        Ok(GmailOAuthFlowSnapshot {
            flow_id,
            expires_at: record.expires_at,
            status: record.status.clone(),
        })
    }

    async fn begin_completion(&self, flow_id: Uuid) -> Result<PendingFlow, AppError> {
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

    async fn exchange_code(
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
            credential_item_id,
            email: authorized_email.to_string(),
        })
    }

    async fn fail(&self, flow_id: Uuid, error: &AppError) {
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
