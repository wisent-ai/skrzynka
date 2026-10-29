//! `/v1/suppressed`: recipients no originated mail may be sent to, as
//! `message suppressed` lists, adds and removes them.

use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, SuppressionFilter},
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `POST /v1/suppressed` with `{entry, reason}`; `reason` optional.
#[derive(Deserialize)]
pub(in crate::api) struct SuppressRequest {
    entry: String,
    #[serde(default)]
    reason: Option<String>,
}

/// `GET /v1/suppressed?contains=&since=&until=`: the entries, or those whose
/// entry or reason contains the text, or those added within the window.
pub(in crate::api) async fn list_suppressed(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(filter): Query<SuppressionFilter>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.find_suppressions(&auth.organization_id, &filter)?
    )))
}

/// `POST /v1/recipients/check` with `{addresses}`: how a send would meet each
/// address, as `message recipients` prints it; nothing is sent.
#[derive(Deserialize)]
pub(in crate::api) struct CheckRecipients {
    addresses: Vec<String>,
}

pub(in crate::api) async fn check_recipients(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<CheckRecipients>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.check_recipients(
        &auth.organization_id,
        &request.addresses
    )?)))
}

/// `GET /v1/suppressed/export` with the same parameters: the entries kept as
/// the text an import reads, offered as `suppressed.txt`, the bytes
/// `message suppressed --export` writes.
pub(in crate::api) async fn export_suppressed(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(filter): Query<SuppressionFilter>,
) -> Result<axum::response::Response, AppError> {
    use axum::{
        http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        response::IntoResponse,
    };
    let text = state.suppression_text(&auth.organization_id, &filter)?;
    Ok((
        [
            (CONTENT_TYPE, "text/plain; charset=utf-8"),
            (
                CONTENT_DISPOSITION,
                "attachment; filename=\"suppressed.txt\"",
            ),
        ],
        text,
    )
        .into_response())
}

/// `GET /v1/suppressed/csv` with the same parameters: the entries kept as
/// the CSV table `message suppressed --csv` writes, offered as
/// `suppressed.csv`.
pub(in crate::api) async fn suppressed_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(filter): Query<SuppressionFilter>,
) -> Result<axum::response::Response, AppError> {
    let entries = state.find_suppressions(&auth.organization_id, &filter)?;
    Ok(super::super::listing::csv_file(
        "suppressed.csv",
        crate::service::suppression_table(&entries),
    ))
}

pub(in crate::api) async fn suppress(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<SuppressRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.suppress(
        &auth.organization_id,
        &request.entry,
        request.reason.as_deref()
    )?)))
}

/// `POST /v1/suppressed/import` with `{text, reason}`: one entry per line of
/// `text`, each optionally followed by its own reason; `reason` optional.
#[derive(Deserialize)]
pub(in crate::api) struct ImportRequest {
    text: String,
    #[serde(default)]
    reason: Option<String>,
}

pub(in crate::api) async fn import_suppressed(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.import_suppressions(
        &auth.organization_id,
        &request.text,
        request.reason.as_deref()
    )?)))
}

/// `DELETE /v1/suppressed/:entry` (URL-encoded): the entry taken off the list.
pub(in crate::api) async fn unsuppress(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(entry): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(
        json!(state.unsuppress(&auth.organization_id, &entry)?),
    ))
}
