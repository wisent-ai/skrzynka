//! Blocked sender routes: the senders whose mail listings leave out.

use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

mod suppressed;
pub(in crate::api) use suppressed::{
    check_recipients, export_suppressed, import_suppressed, list_suppressed, suppress, suppressed_csv, unsuppress,
};

/// `POST /v1/blocked` with `{address}`: an address, or `@domain`.
#[derive(Deserialize)]
pub(in crate::api) struct BlockSender {
    address: String,
}

/// `GET /v1/blocked?count=true&contains=`: each entry, or those holding the
/// text, with how much mail it leaves out when `count` is asked.
#[derive(Deserialize)]
pub(in crate::api) struct BlockedQuery {
    #[serde(default)]
    count: bool,
    contains: Option<String>,
}

pub(in crate::api) async fn list_blocked(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<BlockedQuery>,
) -> Result<Json<Value>, AppError> {
    let contains = query.contains.as_deref();
    Ok(Json(match query.count {
        true => json!(state.blocked_counts(&auth.organization_id, contains)?),
        false => json!(state.blocked_senders(&auth.organization_id, contains)?),
    }))
}

/// `GET /v1/blocked/export?contains=`: the entries as the text an import
/// reads, offered as `blocked.txt`, the bytes `message blocked --export` writes.
pub(in crate::api) async fn export_blocked(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<BlockedQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::{
        http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        response::IntoResponse,
    };
    let text = state.blocked_text(&auth.organization_id, query.contains.as_deref())?;
    Ok((
        [
            (CONTENT_TYPE, "text/plain; charset=utf-8"),
            (CONTENT_DISPOSITION, "attachment; filename=\"blocked.txt\""),
        ],
        text,
    )
        .into_response())
}

/// `GET /v1/blocked/csv?contains=`: the entries, each with the stored mail
/// it leaves out, as the CSV table `message blocked --csv` writes, offered as
/// `blocked.csv`.
pub(in crate::api) async fn blocked_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<BlockedQuery>,
) -> Result<axum::response::Response, AppError> {
    let counts = state.blocked_counts(&auth.organization_id, query.contains.as_deref())?;
    Ok(super::listing::csv_file("blocked.csv", crate::service::blocked_table(&counts)))
}

pub(in crate::api) async fn block_sender(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<BlockSender>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.block_sender(&auth.organization_id, &request.address)?
    )))
}

/// `POST /v1/blocked/import` with `{text}`: one address or `@domain` per
/// line of `text`, every line read before any is kept.
#[derive(Deserialize)]
pub(in crate::api) struct ImportBlocked {
    text: String,
}

pub(in crate::api) async fn import_blocked(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<ImportBlocked>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.import_blocked(&auth.organization_id, &request.text)?)))
}

/// `DELETE /v1/blocked/:address`: the address or `@domain`, URL-encoded.
pub(in crate::api) async fn unblock_sender(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(address): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.unblock_sender(&auth.organization_id, &address)?
    )))
}
