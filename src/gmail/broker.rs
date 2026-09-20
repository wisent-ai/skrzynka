//! The broker: construction, profiles, starting a flow, receiving the callback, status.

use super::{
    FlowRecord, GmailAuthorization, GmailOAuthBroker, GmailOAuthCallback, GmailOAuthFailure,
    GmailOAuthFlowSnapshot, GmailOAuthFlowStatus, GmailProfile, GmailRedirectProbe, PendingFlow,
    StartGmailOAuthRequest, StartGmailOAuthResponse, FLOW_LIFETIME_MINUTES, GMAIL_SCOPES,
    MAX_AUTHORIZATION_CODE_LENGTH,
};
use crate::{
    error::AppError,
    skarbiec::{GoogleOAuthClient, SkarbiecResolver},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use reqwest::{Client, Url};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

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

    /// What Google answers today about the fixed OAuth client and this
    /// process's own loopback callback.
    ///
    /// It starts no flow and stores no state: the URL is built exactly as a
    /// real authorization would build it, handed to Google once, and the code
    /// Google puts in the landing URL is read back. A `None` refusal means
    /// Google did not refuse at the authorization page, which is weaker than
    /// proof that the redirect is registered — only a completed flow proves
    /// that.
    pub async fn redirect_registration(&self) -> Result<GmailRedirectProbe, AppError> {
        let oauth_client = self.resolver.google_oauth_client().await?;
        let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let authorization_url = authorization_url(
            &oauth_client,
            &self.callback_url,
            &challenge,
            &Uuid::new_v4().to_string(),
            None,
        )?;
        Ok(GmailRedirectProbe {
            client_id: oauth_client.client_id,
            redirect_uri: self.callback_url.to_string(),
            refusal: super::diagnose_authorization(authorization_url.as_str()).await,
        })
    }

    pub async fn start(
        &self,
        organization_id: &str,
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
        let authorization_url = authorization_url(
            &oauth_client,
            &self.callback_url,
            &challenge,
            &flow_id.to_string(),
            Some(&email),
        )?;

        self.flows.lock().await.insert(
            flow_id,
            FlowRecord {
                organization_id: organization_id.to_string(),
                expires_at,
                status: GmailOAuthFlowStatus::Pending,
                pending: Some(PendingFlow {
                    organization_id: organization_id.to_string(),
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
            .filter(|value| !value.is_empty() && value.len() <= MAX_AUTHORIZATION_CODE_LENGTH)
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

    pub async fn status(
        &self,
        flow_id: Uuid,
        organization_id: &str,
    ) -> Result<GmailOAuthFlowSnapshot, AppError> {
        let mut flows = self.flows.lock().await;
        let record = flows
            .get_mut(&flow_id)
            .ok_or_else(|| AppError::not_found("Gmail OAuth flow"))?;
        if record.organization_id != organization_id {
            return Err(AppError::not_found("Gmail OAuth flow"));
        }
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
}

/// The authorization URL Google is handed for one client and one loopback
/// callback.
///
/// The started flow and the readiness probe both build it here, so a probe can
/// never ask Google about a URL the real flow would not have handed out.
fn authorization_url(
    oauth_client: &GoogleOAuthClient,
    callback_url: &Url,
    challenge: &str,
    state: &str,
    login_hint: Option<&str>,
) -> Result<Url, AppError> {
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
        .append_pair("redirect_uri", callback_url.as_str())
        .append_pair("response_type", "code")
        .append_pair("scope", GMAIL_SCOPES)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("include_granted_scopes", "true")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    if let Some(login_hint) = login_hint {
        authorization_url
            .query_pairs_mut()
            .append_pair("login_hint", login_hint);
    }
    Ok(authorization_url)
}
