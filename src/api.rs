use crate::{
    auth::{require_auth, AuthContext, OrganizationRole},
    error::AppError,
    gmail::{GmailOAuthCallback, StartGmailOAuthRequest},
    models::{
        CreateMailboxRequest, CreateOutboundRequest, CreateReplyRequest, HealthResponse,
        UpdateMailboxRequest,
    },
    service::AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    middleware,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/v1/status", get(status))
        .route("/v1/skarbiec/items", get(list_skarbiec_items))
        .route("/v1/gmail/profiles", get(list_gmail_profiles))
        .route("/v1/gmail/oauth/start", post(start_gmail_oauth))
        .route("/v1/gmail/oauth/:flow_id", get(gmail_oauth_status))
        .route(
            "/v1/gmail/delegation",
            get(gmail_delegation_status_handler),
        )
        .route("/v1/gmail/delegate", post(connect_gmail_delegated))
        .route("/v1/mailboxes", get(list_mailboxes).post(create_mailbox))
        .route(
            "/v1/mailboxes/:id",
            get(get_mailbox)
                .patch(update_mailbox)
                .delete(delete_mailbox),
        )
        .route("/v1/mailboxes/:id/sync", post(sync_mailbox))
        .route("/v1/sync", post(sync_all))
        .route("/v1/messages", get(list_messages))
        .route("/v1/messages/:id", get(get_message))
        .route(
            "/v1/messages/:id/replies",
            get(list_replies).post(create_reply),
        )
        .route(
            "/v1/mailboxes/:id/outbound",
            get(list_mailbox_outbound).post(create_outbound),
        )
        .route("/v1/outbound", get(list_outbound))
        .route("/v1/outbound/:id", get(get_outbound))
        .route_layer(middleware::from_fn_with_state(
            state.auth_verifier.clone(),
            require_auth,
        ));

    Router::new()
        .route("/healthz", get(health))
        .route("/v1/gmail/oauth/callback", get(gmail_oauth_callback))
        .merge(protected)
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ready",
        product: "skrzynka",
        version: env!("CARGO_PKG_VERSION"),
        schema_version: crate::db::SCHEMA_VERSION,
    })
}

async fn status(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.status(&auth.organization_id).await?)))
}

async fn list_skarbiec_items(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_skarbiec_items().await?)))
}

async fn list_gmail_profiles(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_gmail_profiles().await?)))
}

async fn start_gmail_oauth(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<StartGmailOAuthRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .start_gmail_oauth(&auth.organization_id, request)
            .await?
    )))
}

async fn gmail_delegation_status_handler(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.gmail_delegation_status().await)))
}

#[derive(Deserialize)]
struct DelegateGmailRequest {
    email: String,
    display_name: Option<String>,
}

async fn connect_gmail_delegated(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<DelegateGmailRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .connect_gmail_delegated(&auth.organization_id, &request.email, request.display_name)
            .await?
    )))
}

async fn gmail_oauth_callback(
    State(state): State<AppState>,
    Query(callback): Query<GmailOAuthCallback>,
) -> (StatusCode, Html<&'static str>) {
    match state.complete_gmail_oauth_callback(callback).await {
        Ok(_) => (
            StatusCode::OK,
            Html("<!doctype html><meta charset=utf-8><title>Gmail connected</title><p>Gmail is connected to Skrzynka. You can close this window and return to Skrzynka Desktop.</p>"),
        ),
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Html("<!doctype html><meta charset=utf-8><title>Gmail connection failed</title><p>Gmail could not be connected. Return to Skrzynka Desktop for the exact error.</p>"),
        ),
    }
}

async fn gmail_oauth_status(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(flow_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state
            .gmail_oauth_status(&auth.organization_id, parse_uuid(&flow_id)?)
            .await?
    )))
}

async fn list_mailboxes(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_mailboxes(&auth.organization_id)?)))
}

async fn create_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<CreateMailboxRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    let mailbox = state.create_mailbox(&auth.organization_id, request).await?;
    Ok((StatusCode::CREATED, Json(json!(mailbox))))
}

async fn get_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_mailbox(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

async fn update_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<UpdateMailboxRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(state.update_mailbox(
        &auth.organization_id,
        parse_uuid(&id)?,
        request,
    )?)))
}

#[derive(Deserialize)]
struct DeleteQuery {
    confirm: Option<bool>,
}

async fn delete_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    if query.confirm != Some(true) {
        return Err(AppError::invalid(
            "CONFIRMATION_REQUIRED",
            "mailbox removal requires confirm=true",
        ));
    }
    state.delete_mailbox(&auth.organization_id, parse_uuid(&id)?)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn sync_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .sync_mailbox(&auth.organization_id, parse_uuid(&id)?)
            .await?
    )))
}

async fn sync_all(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.sync_all(&auth.organization_id).await?)))
}

#[derive(Deserialize)]
struct MessageQuery {
    mailbox_id: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_messages(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<MessageQuery>,
) -> Result<Json<Value>, AppError> {
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    Ok(Json(json!(state.list_messages(
        &auth.organization_id,
        mailbox_id,
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )?)))
}

async fn get_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_message(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

async fn list_replies(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.list_replies(&auth.organization_id, parse_uuid(&id)?,)?
    )))
}

async fn create_reply(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<CreateReplyRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .reply(&auth.organization_id, parse_uuid(&id)?, request)
            .await?
    )))
}

#[derive(Deserialize)]
struct OutboundQuery {
    mailbox_id: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<OutboundQuery>,
) -> Result<Json<Value>, AppError> {
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    Ok(Json(json!(state.list_outbound(
        &auth.organization_id,
        mailbox_id,
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )?)))
}

async fn list_mailbox_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(query): Query<OutboundQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_outbound(
        &auth.organization_id,
        Some(parse_uuid(&id)?),
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )?)))
}

async fn get_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_outbound(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

async fn create_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<CreateOutboundRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .send_outbound(&auth.organization_id, parse_uuid(&id)?, request)
            .await?
    )))
}

fn parse_uuid(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(Into::into)
}
