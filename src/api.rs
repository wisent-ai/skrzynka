use crate::{
    error::AppError,
    models::{CreateMailboxRequest, CreateReplyRequest, HealthResponse, UpdateMailboxRequest},
    service::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/status", get(status))
        .route("/v1/skarbiec/items", get(list_skarbiec_items))
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

async fn status(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.status().await?)))
}

async fn list_skarbiec_items(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_skarbiec_items().await?)))
}

async fn list_mailboxes(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_mailboxes()?)))
}

async fn create_mailbox(
    State(state): State<AppState>,
    Json(request): Json<CreateMailboxRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let mailbox = state.create_mailbox(request).await?;
    Ok((StatusCode::CREATED, Json(json!(mailbox))))
}

async fn get_mailbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.get_mailbox(parse_uuid(&id)?)?)))
}

async fn update_mailbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateMailboxRequest>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(
        json!(state.update_mailbox(parse_uuid(&id)?, request)?),
    ))
}

#[derive(Deserialize)]
struct DeleteQuery {
    confirm: Option<bool>,
}

async fn delete_mailbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, AppError> {
    if query.confirm != Some(true) {
        return Err(AppError::invalid(
            "CONFIRMATION_REQUIRED",
            "mailbox removal requires confirm=true",
        ));
    }
    state.delete_mailbox(parse_uuid(&id)?)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn sync_mailbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.sync_mailbox(parse_uuid(&id)?).await?)))
}

async fn sync_all(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.sync_all().await?)))
}

#[derive(Deserialize)]
struct MessageQuery {
    mailbox_id: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_messages(
    State(state): State<AppState>,
    Query(query): Query<MessageQuery>,
) -> Result<Json<Value>, AppError> {
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    Ok(Json(json!(state.list_messages(
        mailbox_id,
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )?)))
}

async fn get_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.get_message(parse_uuid(&id)?)?)))
}

async fn list_replies(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_replies(parse_uuid(&id)?)?)))
}

async fn create_reply(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<CreateReplyRequest>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.reply(parse_uuid(&id)?, request).await?)))
}

fn parse_uuid(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(Into::into)
}
