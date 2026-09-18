//! The HTTP API: the router and the handlers that need no organization context; the
//! Gmail, mailbox and message handlers live in their own modules.

use crate::{
    auth::{require_auth, AuthContext},
    error::AppError,
    models::HealthResponse,
    service::AppState,
};
use axum::{
    extract::{Extension, State},
    middleware,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

mod gmail;
mod mailboxes;
mod messages;

use gmail::{
    connect_gmail_app_password, connect_gmail_delegated, gmail_delegation_status_handler,
    gmail_oauth_callback, gmail_oauth_status, start_gmail_oauth,
};
use mailboxes::{
    create_mailbox, delete_mailbox, get_mailbox, import_mailbox, list_mailboxes, sync_all,
    sync_mailbox, update_mailbox,
};
use messages::{
    create_outbound, create_reply, get_message, get_outbound, list_mailbox_outbound,
    list_messages, list_outbound, list_replies,
};

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/v1/status", get(status))
        .route("/v1/skarbiec/items", get(list_skarbiec_items))
        .route("/v1/gmail/profiles", get(list_gmail_profiles))
        .route("/v1/gmail/oauth/start", post(start_gmail_oauth))
        .route("/v1/gmail/oauth/:flow_id", get(gmail_oauth_status))
        .route("/v1/gmail/delegation", get(gmail_delegation_status_handler))
        .route("/v1/gmail/delegate", post(connect_gmail_delegated))
        .route("/v1/gmail/app-password", post(connect_gmail_app_password))
        .route("/v1/mailboxes", get(list_mailboxes).post(create_mailbox))
        .route("/v1/imports/mailbox", post(import_mailbox))
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

pub(super) fn parse_uuid(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(Into::into)
}
