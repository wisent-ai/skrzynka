use crate::error::AppError;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use reqwest::{Client, Url};
use serde::Deserialize;
use uuid::Uuid;

const DEFAULT_SUPABASE_URL: &str = "https://alvaewvbyxpgwdpugnxy.supabase.co";
const DEFAULT_SUPABASE_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFsdmFld3ZieXhwZ3dkcHVnbnh5Iiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODEzOTc5NDcsImV4cCI6MjA5Njk3Mzk0N30.xkkJ36ZTwtqyVZLFju0vc9S25grTuKbj9ILKlsXdUPA";
const ORGANIZATION_HEADER: &str = "x-wisent-organization-id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganizationRole {
    Owner,
    Admin,
    Member,
}

impl OrganizationRole {
    fn from_rpc(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            _ => None,
        }
    }

    fn permits(self, required: Self) -> bool {
        self.rank() >= required.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Self::Member => 0,
            Self::Admin => 1,
            Self::Owner => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user_id: Uuid,
    pub organization_id: String,
    pub role: OrganizationRole,
}

impl AuthContext {
    pub fn require_role(&self, required: OrganizationRole) -> Result<(), AppError> {
        if self.role.permits(required) {
            Ok(())
        } else {
            Err(forbidden())
        }
    }
}

#[derive(Clone)]
pub struct AuthVerifier {
    client: Client,
    base_url: Url,
    anon_key: String,
}

#[derive(Deserialize)]
struct AuthorizationResponse {
    user_id: Uuid,
    organization_id: Uuid,
    role: Option<String>,
}

impl AuthVerifier {
    pub fn from_environment() -> Result<Self, AppError> {
        let base_url =
            std::env::var("SUPABASE_URL").unwrap_or_else(|_| DEFAULT_SUPABASE_URL.to_string());
        let anon_key = std::env::var("SUPABASE_ANON_KEY")
            .unwrap_or_else(|_| DEFAULT_SUPABASE_ANON_KEY.to_string());
        let base_url = Url::parse(base_url.trim())
            .map_err(|_| AppError::internal("central identity URL is invalid"))?;
        if base_url.scheme() != "https" || anon_key.trim().is_empty() {
            return Err(AppError::internal(
                "central identity configuration is incomplete",
            ));
        }
        Ok(Self {
            client: Client::new(),
            base_url,
            anon_key,
        })
    }

    async fn verify(&self, headers: &HeaderMap) -> Result<AuthContext, AppError> {
        let bearer = bearer_token(headers).ok_or_else(unauthorized)?;
        let target_organization_id = single_header_value(headers, ORGANIZATION_HEADER)
            .ok_or_else(invalid_organization)?
            .parse::<Uuid>()
            .map_err(|_| invalid_organization())?;

        let authorization_url = self
            .base_url
            .join("rest/v1/rpc/authorize_organization")
            .map_err(|_| AppError::internal("central identity URL could not be composed"))?;
        let response = self
            .client
            .post(authorization_url)
            .header("apikey", &self.anon_key)
            .bearer_auth(bearer)
            .header(ORGANIZATION_HEADER, target_organization_id.to_string())
            .json(&serde_json::json!({
                "target_org_id": target_organization_id,
            }))
            .send()
            .await
            .map_err(|_| identity_unavailable())?;

        match response.status() {
            StatusCode::UNAUTHORIZED => return Err(unauthorized()),
            StatusCode::FORBIDDEN => return Err(forbidden()),
            status if !status.is_success() => return Err(identity_unavailable()),
            _ => {}
        }

        let mut authorizations: Vec<AuthorizationResponse> = response
            .json()
            .await
            .map_err(|_| identity_unavailable())?;
        if authorizations.len() != 1 {
            return if authorizations.is_empty() {
                Err(forbidden())
            } else {
                Err(identity_unavailable())
            };
        }
        let authorization = authorizations
            .pop()
            .expect("authorization row count was checked");
        if authorization.organization_id != target_organization_id {
            return Err(forbidden());
        }
        let role = authorization
            .role
            .as_deref()
            .and_then(OrganizationRole::from_rpc)
            .ok_or_else(forbidden)?;

        Ok(AuthContext {
            user_id: authorization.user_id,
            organization_id: authorization.organization_id.to_string(),
            role,
        })
    }
}

pub async fn require_auth(
    State(verifier): State<AuthVerifier>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let context = verifier.verify(request.headers()).await?;
    tracing::debug!(
        user_id = %context.user_id,
        organization_id = %context.organization_id,
        role = ?context.role,
        "authorized organization request"
    );
    request.extensions_mut().insert(context);
    Ok(next.run(request).await)
}
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = single_header_value(headers, AUTHORIZATION.as_str())?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return None;
    }
    Some(token)
}

fn single_header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    value.to_str().ok()
}

fn invalid_organization() -> AppError {
    AppError::invalid(
        "INVALID_ORGANIZATION",
        "X-Wisent-Organization-ID must contain a valid organization UUID",
    )
}
fn unauthorized() -> AppError {
    AppError::new(
        StatusCode::UNAUTHORIZED,
        "AUTHENTICATION_REQUIRED",
        "a valid Wisent session is required",
        false,
    )
}

fn forbidden() -> AppError {
    AppError::new(
        StatusCode::FORBIDDEN,
        "ORGANIZATION_ACCESS_DENIED",
        "the signed-in account does not belong to the selected organization",
        false,
    )
}

fn identity_unavailable() -> AppError {
    AppError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "IDENTITY_UNAVAILABLE",
        "central identity verification is unavailable",
        true,
    )
}
