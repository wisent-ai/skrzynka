//! Gmail OAuth: the flow types and the broker, whose methods live in `broker` and
//! `exchange`; `diagnosis` explains a failed authorization.

use crate::skarbiec::{GoogleOAuthClient, SkarbiecResolver};
use chrono::Utc;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

mod broker;
mod diagnosis;
mod exchange;

pub use diagnosis::{
    app_password_action, authorization_operands, diagnose_authorization,
    google_imap_password_rejected, redirect_not_registered,
};

const GMAIL_SCOPES: &str = "openid email https://mail.google.com/";
/// A diagnosis reads Google's authorization page once; it does not wait longer than this.
const AUTHORIZATION_PROBE_TIMEOUT_SECONDS: u64 = 20;
/// An OAuth authorization code is short; anything longer is not one.
const MAX_AUTHORIZATION_CODE_LENGTH: usize = 4096;
const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const FLOW_LIFETIME_MINUTES: i64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailProfile {
    pub skarbiec_item_id: String,
    pub email: String,
}

/// What Google answered about one OAuth client and one loopback redirect.
///
/// `refusal` is the OAuth error code Google put in the landing URL it sent the
/// browser to, so `redirect_uri_mismatch` here is Google's own word and not
/// this product's inference.
#[derive(Debug, Clone, Serialize)]
pub struct GmailRedirectProbe {
    pub client_id: String,
    pub redirect_uri: String,
    pub refusal: Option<String>,
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
    pub organization_id: String,
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
    organization_id: String,
    verifier: String,
    source_item_id: String,
    expected_email: String,
    oauth_client: GoogleOAuthClient,
}

#[derive(Clone)]
struct FlowRecord {
    organization_id: String,
    expires_at: chrono::DateTime<Utc>,
    status: GmailOAuthFlowStatus,
    pending: Option<PendingFlow>,
}
