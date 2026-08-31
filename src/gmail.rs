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
/// The OAuth error code Google put in the landing URL it sent a browser to,
/// or `None` when that URL carries none.
///
/// Google encodes it as base64url in `authError`, so the code an operator needs
/// is unreadable without decoding. Split out from the request that fetched the
/// URL so the decode is exercised against a real captured error rather than a
/// stubbed server.
pub fn oauth_error_code(landing_url: &str) -> Option<String> {
    let parsed = Url::parse(landing_url).ok()?;
    let encoded = parsed
        .query_pairs()
        .find(|(name, _)| name == "authError")
        .map(|(_, value)| value.into_owned())?;
    if encoded.is_empty() {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(encoded.trim_end_matches('=')).ok()?;
    // The payload is an undocumented blob whose first field is the code as
    // plain text, so the code is the leading run of code-shaped bytes rather
    // than the result of parsing a format Google does not publish.
    let text = String::from_utf8_lossy(&decoded);
    let code: String = text
        .chars()
        .skip_while(|character| !character.is_ascii_alphabetic())
        .take_while(|character| character.is_ascii_alphabetic() || *character == '_')
        .collect();
    (!code.is_empty()).then_some(code)
}

/// The refusal an unregistered loopback redirect deserves.
///
/// Google refuses the authorization inside the browser, so no callback ever
/// reaches this process and the flow spends its whole ten-minute lifetime
/// saying nothing. The client id, the redirect URI presented and the one
/// setting that fixes it are the sentence an operator needs, and none of them
/// were anywhere in this product's output.
pub fn redirect_not_registered(client_id: &str, redirect_uri: &str) -> AppError {
    AppError::dependency(
        "GMAIL_OAUTH_REDIRECT_NOT_REGISTERED",
        format!(
            "the OAuth client {client_id} has no loopback redirect URI registered, so Google \
             refused this authorization with redirect_uri_mismatch before showing any consent \
             screen; it was presented {redirect_uri}. Register a loopback redirect URI for that \
             client in the Google Cloud Console, or issue a Desktop app client which accepts any \
             loopback port, and retry"
        ),
        false,
    )
}

/// The refusal when Google IMAP receives an ordinary password instead of OAuth or an app password.
///
/// Google disabled ordinary password IMAP access in May 2022. When IMAP authentication fails
/// for a Gmail account with a Password credential, the operator needs to know they must use
/// either the app's OAuth flow or an app-specific password. The mailbox email, credential
/// item name, and the two paths forward are the sentence an operator needs.
pub fn google_imap_password_rejected(
    mailbox_email: &str,
    skarbiec_item_id: &str,
) -> AppError {
    AppError::dependency(
        "GMAIL_IMAP_PASSWORD_REJECTED",
        format!(
            "mailbox {mailbox_email} could not authenticate to imap.gmail.com with the password in Skarbiec item '{skarbiec_item_id}': Google does not accept an ordinary account password over IMAP. Authorize with `skrzynka gmail authorize --skarbiec-item {skarbiec_item_id}`, or store a Google app-specific password in that item — note that `skarbiec set-json {skarbiec_item_id}` reads the whole payload from stdin and replaces it, so supply the complete item."
        ),
        false,
    )
}

/// The `client_id` and `redirect_uri` an authorization URL carries, for the
/// refusal above. Read back from the URL that was actually handed out rather
/// than recomputed, so the sentence names what Google was really given.
pub fn authorization_operands(authorization_url: &str) -> Option<(String, String)> {
    let parsed = Url::parse(authorization_url).ok()?;
    let mut client_id = None;
    let mut redirect_uri = None;
    for (name, value) in parsed.query_pairs() {
        match name.as_ref() {
            "client_id" => client_id = Some(value.into_owned()),
            "redirect_uri" => redirect_uri = Some(value.into_owned()),
            _ => {}
        }
    }
    Some((client_id?, redirect_uri?))
}

/// Ask Google what it says about this authorization, for use only AFTER a flow
/// has already failed.
///
/// Never a pre-flight gate: Google shows a sign-in page before validating the
/// redirect for some forms, so a response that is not an error page does not
/// mean the redirect is registered. This only explains a failure that has
/// already happened.
pub async fn diagnose_authorization(authorization_url: &str) -> Option<String> {
    let response = Client::new()
        .get(authorization_url)
        .header("user-agent", "Mozilla/5.0")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .ok()?;
    oauth_error_code(response.url().as_str())
}

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
            organization_id: pending.organization_id.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A REAL landing URL captured from Google while authorizing client
    /// 903183433368-...ap3l9 against a loopback redirect it does not have
    /// registered. Not a stubbed server: the bytes Google actually sent.
    const CAPTURED_ERROR: &str = "https://accounts.google.com/signin/oauth/error?authError=\
                                  ChVyZWRpcmVjdF91cmlfbWlzbWF0Y2gSsAEKWW91IGNhbid0IHNpZ24gaW4gdG8gdGhpcyBhcHA";

    #[test]
    fn the_captured_google_error_decodes_to_its_code() {
        assert_eq!(
            oauth_error_code(CAPTURED_ERROR).as_deref(),
            Some("redirect_uri_mismatch")
        );
    }

    #[test]
    fn a_landing_url_without_an_error_carries_no_code() {
        // A completed flow must never be reported as a registration problem.
        assert!(oauth_error_code("http://127.0.0.1:8788/v1/gmail/oauth/callback?code=x&state=y")
            .is_none());
        assert!(oauth_error_code("https://accounts.google.com/signin/oauth/consent").is_none());
        assert!(oauth_error_code("https://accounts.google.com/x?authError=").is_none());
        assert!(oauth_error_code("not a url").is_none());
    }

    #[test]
    fn the_operands_come_from_the_url_that_was_handed_out() {
        let url = "https://accounts.google.com/o/oauth2/auth?client_id=abc.apps.googleusercontent.com\
                   &redirect_uri=http%3A%2F%2F127.0.0.1%3A8788%2Fv1%2Fgmail%2Foauth%2Fcallback\
                   &response_type=code";
        let (client_id, redirect_uri) = authorization_operands(url).expect("operands");
        assert_eq!(client_id, "abc.apps.googleusercontent.com");
        assert_eq!(redirect_uri, "http://127.0.0.1:8788/v1/gmail/oauth/callback");
        // A URL missing either operand yields nothing rather than half a
        // sentence naming an empty client.
        assert!(authorization_operands("https://accounts.google.com/o/oauth2/auth").is_none());
    }

    #[test]
    fn the_refusal_names_the_client_the_uri_and_the_setting() {
        let error = redirect_not_registered(
            "903183433368-5nt0jdbqtli8rm39oh2s0limiljap3l9.apps.googleusercontent.com",
            "http://127.0.0.1:8788/v1/gmail/oauth/callback",
        );
        assert_eq!(error.code, "GMAIL_OAUTH_REDIRECT_NOT_REGISTERED");
        assert!(!error.retryable, "registering a redirect URI is not a retry");
        assert_eq!(
            error.message,
            "the OAuth client 903183433368-5nt0jdbqtli8rm39oh2s0limiljap3l9.apps.googleusercontent.com \
has no loopback redirect URI registered, so Google refused this authorization with \
redirect_uri_mismatch before showing any consent screen; it was presented \
http://127.0.0.1:8788/v1/gmail/oauth/callback. Register a loopback redirect URI for that client \
in the Google Cloud Console, or issue a Desktop app client which accepts any loopback port, and \
retry"
        );
    }

    #[test]
    fn google_imap_password_rejected_names_mailbox_and_credential_item() {
        let error = google_imap_password_rejected(
            "user@gmail.com",
            "gmail-personal",
        );
        assert_eq!(error.code, "GMAIL_IMAP_PASSWORD_REJECTED");
        assert!(!error.retryable, "fixing a password with OAuth or app password is not a retry");
        // Message must be exactly as specified, with operands interpolated
        assert_eq!(
            error.message,
            "mailbox user@gmail.com could not authenticate to imap.gmail.com with the password in Skarbiec item 'gmail-personal': Google does not accept an ordinary account password over IMAP. Authorize with `skrzynka gmail authorize --skarbiec-item gmail-personal`, or store a Google app-specific password in that item — note that `skarbiec set-json gmail-personal` reads the whole payload from stdin and replaces it, so supply the complete item."
        );
        // Reject any argv-secret patterns: password= or =< constructions must never appear
        assert!(!error.message.contains("password="), "message must not suggest password= argv form; secrets cannot be passed on command line");
        assert!(!error.message.contains("=<"), "message must not contain =< placeholder; all guidance must be concrete");
    }

    #[test]
    fn google_imap_password_rejected_enforces_gmail_host_boundary() {
        // The error function itself is Gmail-specific. The caller (mail.rs::fetch_messages)
        // must check that mailbox.imap_host.contains("gmail.com") before invoking this.
        // Non-Gmail hosts must get the generic "IMAP authentication was refused" message.
        let error = google_imap_password_rejected("user@example.invalid", "example-inbox");
        assert_eq!(error.code, "GMAIL_IMAP_PASSWORD_REJECTED");
        // Error message is Gmail-focused; mail.rs must enforce the host boundary
        assert!(error.message.contains("Google") || error.message.contains("Gmail"), 
            "error message is Gmail-specific; caller must detect gmail.com hosts first");
    }
}
