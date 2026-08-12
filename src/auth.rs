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

const DEFAULT_SUPABASE_URL: &str = "https://alvaewvbyxpgwdpugnxy.supabase.co";
const DEFAULT_SUPABASE_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFsdmFld3ZieXhwZ3dkcHVnbnh5Iiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODEzOTc5NDcsImV4cCI6MjA5Njk3Mzk0N30.xkkJ36ZTwtqyVZLFju0vc9S25grTuKbj9ILKlsXdUPA";
const ORGANIZATION_HEADER: &str = "x-wisent-organization-id";

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub organization_id: String,
}

#[derive(Clone)]
pub struct AuthVerifier {
    client: Client,
    base_url: Url,
    anon_key: String,
}

#[derive(Deserialize)]
struct UserResponse {
    id: String,
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
        let bearer = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(unauthorized)?;
        let organization_id = headers
            .get(ORGANIZATION_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| valid_identifier(value))
            .ok_or_else(unauthorized)?
            .to_string();

        let user_url = self
            .base_url
            .join("auth/v1/user")
            .map_err(|_| AppError::internal("central identity URL could not be composed"))?;
        let user_response = self
            .client
            .get(user_url)
            .header("apikey", &self.anon_key)
            .bearer_auth(bearer)
            .send()
            .await
            .map_err(|_| identity_unavailable())?;
        if user_response.status() == StatusCode::UNAUTHORIZED {
            return Err(unauthorized());
        }
        if !user_response.status().is_success() {
            return Err(identity_unavailable());
        }
        let user: UserResponse = user_response
            .json()
            .await
            .map_err(|_| identity_unavailable())?;
        if !valid_identifier(&user.id) {
            return Err(identity_unavailable());
        }

        let membership_url = self
            .base_url
            .join("rest/v1/organization_members")
            .map_err(|_| AppError::internal("central identity URL could not be composed"))?;
        let membership_response = self
            .client
            .get(membership_url)
            .header("apikey", &self.anon_key)
            .bearer_auth(bearer)
            .query(&[
                ("select", "org_id"),
                ("user_id", &format!("eq.{}", user.id)),
                ("org_id", &format!("eq.{organization_id}")),
                ("limit", "1"),
            ])
            .send()
            .await
            .map_err(|_| identity_unavailable())?;
        if membership_response.status() == StatusCode::UNAUTHORIZED {
            return Err(unauthorized());
        }
        if !membership_response.status().is_success() {
            return Err(identity_unavailable());
        }
        let memberships: Vec<serde_json::Value> = membership_response
            .json()
            .await
            .map_err(|_| identity_unavailable())?;
        if memberships.is_empty() {
            return Err(forbidden());
        }
        Ok(AuthContext { organization_id })
    }
}

pub async fn require_auth(
    State(verifier): State<AuthVerifier>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let context = verifier.verify(request.headers()).await?;
    request.extensions_mut().insert(context);
    Ok(next.run(request).await)
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
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
    AppError::dependency(
        "IDENTITY_UNAVAILABLE",
        "central identity verification is unavailable",
        true,
    )
}
