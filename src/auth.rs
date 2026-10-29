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

const ORGANIZATION_HEADER: &str = "x-wisent-organization-id";

/// An organization role, declared from least to most privileged so the
/// derived order is the privilege order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrganizationRole {
    Member,
    Admin,
    Owner,
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
        self >= required
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
        // The identity authority is configuration: Stado's service catalog sets
        // SUPABASE_URL and SUPABASE_ANON_KEY for the unit. Nothing is compiled in.
        let configured = |name: &str| match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => Ok(value),
            Ok(_) | Err(std::env::VarError::NotPresent) => Err(AppError::internal(format!(
                "{name} is not set; the central identity authority has no address"
            ))),
            Err(std::env::VarError::NotUnicode(raw)) => Err(AppError::internal(format!(
                "{name} is not UTF-8 ({raw:?}); the identity authority cannot be read"
            ))),
        };
        let base_url = configured("SUPABASE_URL")?;
        let anon_key = configured("SUPABASE_ANON_KEY")?;
        let base_url = Url::parse(base_url.trim())
            .map_err(|cause| AppError::internal(format!("SUPABASE_URL is not a URL: {cause}")))?;
        if base_url.scheme() != "https" {
            return Err(AppError::internal(format!(
                "SUPABASE_URL must be https, got {}",
                base_url.scheme()
            )));
        }
        if anon_key.trim().is_empty() {
            return Err(AppError::internal("SUPABASE_ANON_KEY is empty"));
        }
        Ok(Self {
            client: Client::new(),
            base_url,
            anon_key,
        })
    }

    async fn verify(&self, headers: &HeaderMap) -> Result<AuthContext, AppError> {
        let bearer = bearer_token(headers).ok_or_else(unauthorized)?;
        let stated = single_header_value(headers, ORGANIZATION_HEADER).ok_or_else(invalid_organization)?;
        let target_organization_id = stated.parse::<Uuid>().map_err(|cause| {
            AppError::invalid(
                "INVALID_ORGANIZATION",
                format!("X-Wisent-Organization-ID '{stated}' is not an organization UUID: {cause}"),
            )
        })?;

        let authorization_url = self
            .base_url
            .join("rest/v1/rpc/authorize_organization")
            .map_err(|cause| {
                AppError::internal(format!(
                    "central identity URL {} could not be composed: {cause}",
                    self.base_url
                ))
            })?;
        let response = stado_wait::request(
            self.client
                .post(authorization_url)
                .header("apikey", &self.anon_key)
                .bearer_auth(bearer)
                .header(ORGANIZATION_HEADER, target_organization_id.to_string())
                .json(&serde_json::json!({
                    "target_org_id": target_organization_id,
                })),
        )
        .await
        .map_err(|error| {
            // The caller sees only "unavailable"; the operator's log keeps why.
            tracing::warn!(%error, "central identity request failed");
            identity_unavailable()
        })?;

        match response.status() {
            StatusCode::UNAUTHORIZED => return Err(unauthorized()),
            StatusCode::FORBIDDEN => return Err(forbidden()),
            status if !status.is_success() => {
                tracing::warn!(%status, "central identity refused the authorization request");
                return Err(identity_unavailable());
            }
            _ => {}
        }

        let authorizations: Vec<AuthorizationResponse> =
            response.json().await.map_err(|error| {
                tracing::warn!(%error, "central identity answered with an unreadable authorization");
                identity_unavailable()
            })?;
        let rows = authorizations.len();
        let mut answered = authorizations.into_iter();
        let authorization = match (answered.next(), answered.next()) {
            (None, _) => return Err(forbidden()),
            (Some(only), None) => only,
            (Some(_), Some(_)) => {
                tracing::warn!(
                    rows,
                    "central identity answered more than one authorization"
                );
                return Err(identity_unavailable());
            }
        };
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
    // The caller is answered with the generic refusal an auth failure gets;
    // the cause is kept in the service log, as the identity failures are.
    match value.to_str() {
        Ok(text) => Some(text),
        Err(error) => {
            tracing::warn!(%error, header = name, "request header is not visible ASCII");
            None
        }
    }
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
