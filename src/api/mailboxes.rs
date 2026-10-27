//! Mailbox routes: the mailboxes Skarbiec declares, the declaration itself,
//! local mail removal, and sync.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    models::DeclareMailboxRequest,
    service::{AppState, MailboxState, WarmUpRequest},
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::num::NonZeroU32;

/// `GET /v1/mailboxes?state=failing|undeclared|unsynchronized`: every
/// mailbox, or those in one state, as `mailbox list --state` lists them.
#[derive(Deserialize)]
pub(super) struct MailboxesQuery {
    state: Option<MailboxState>,
}

pub(super) async fn list_mailboxes(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<MailboxesQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.list_mailboxes(&auth.organization_id, query.state).await?
    )))
}

/// `GET /v1/mailboxes/csv[?state=]`: the same mailboxes as the CSV table
/// `mailbox list --csv` writes, `text/csv` offered as `mailboxes.csv`.
pub(super) async fn mailboxes_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<MailboxesQuery>,
) -> Result<axum::response::Response, AppError> {
    let mailboxes = state.list_mailboxes(&auth.organization_id, query.state).await?;
    Ok(super::listing::csv_file("mailboxes.csv", crate::models::Mailbox::table(&mailboxes)))
}

pub(super) async fn declare_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<DeclareMailboxRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .declare_mailbox(&auth.organization_id, &request.skarbiec_item_id)
            .await?
    )))
}

pub(super) async fn undeclare_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .undeclare_mailbox(&auth.organization_id, parse_uuid(&id)?)
            .await?
    )))
}

pub(super) async fn get_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state
            .get_mailbox(&auth.organization_id, parse_uuid(&id)?)
            .await?
    )))
}

#[derive(Deserialize)]
pub(super) struct DeleteQuery {
    confirm: Option<bool>,
}

pub(super) async fn delete_mailbox(
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

pub(super) async fn sync_mailbox(
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

pub(super) async fn sync_all(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.sync_all(&auth.organization_id).await?)))
}

/// `GET /v1/mailboxes/:id/delivery?since=&until=`: the window to count, each
/// optional, as `message list` takes them.
#[derive(Deserialize)]
pub(super) struct DeliveryWindow {
    since: Option<String>,
    until: Option<String>,
}

/// How the mailbox's sends ended, from what Skrzynka recorded of them.
pub(super) async fn mailbox_delivery(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(window): Query<DeliveryWindow>,
) -> Result<Json<Value>, AppError> {
    let id = parse_uuid(&id)?;
    Ok(Json(json!(state.mailbox_delivery(&auth.organization_id, id, window.since.as_deref(), window.until.as_deref())?)))
}

/// Which table of the delivery report `GET /v1/mailboxes/:id/delivery/:table/csv` answers.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DeliveryTable { Days, Domains }

/// The report's days or recipient domains as the CSV table `mailbox delivery --days-csv|--domains-csv` writes.
pub(super) async fn mailbox_delivery_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path((id, table)): Path<(String, DeliveryTable)>,
    Query(window): Query<DeliveryWindow>,
) -> Result<axum::response::Response, AppError> {
    let report = state.mailbox_delivery(&auth.organization_id, parse_uuid(&id)?, window.since.as_deref(), window.until.as_deref())?;
    Ok(match table {
        DeliveryTable::Days => super::listing::csv_file("delivery-days.csv", report.day_table()),
        DeliveryTable::Domains => super::listing::csv_file("delivery-domains.csv", report.domain_table()),
    })
}

/// `GET /v1/mailboxes/:id/summary?since=&until=`: the mailbox's stored mail,
/// or that received in the window, counted.
pub(super) async fn mailbox_summary(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(window): Query<DeliveryWindow>,
) -> Result<Json<Value>, AppError> {
    let id = parse_uuid(&id)?;
    Ok(Json(json!(state.mailbox_summary(&auth.organization_id, id, window.since.as_deref(), window.until.as_deref())?)))
}

/// `GET /v1/mailboxes/:id/summary/days/csv?since=&until=`: the summary's days as the CSV table `mailbox summary --days-csv` writes.
pub(super) async fn mailbox_summary_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(window): Query<DeliveryWindow>,
) -> Result<axum::response::Response, AppError> {
    let summary = state.mailbox_summary(&auth.organization_id, parse_uuid(&id)?, window.since.as_deref(), window.until.as_deref())?;
    Ok(super::listing::csv_file("summary-days.csv", summary.day_table()))
}

/// `GET /v1/mailboxes/:id/folders`: the provider's folders, read now.
pub(super) async fn mailbox_folders(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.mailbox_folders(&auth.organization_id, parse_uuid(&id)?).await?)))
}

/// `POST /v1/mailboxes/:id/append` with `{folder, content_base64}`: put one
/// whole message into a provider folder, unless it holds one with the same
/// Message-ID; answers `{folder, message_id, appended}`.
#[derive(Deserialize)]
pub(super) struct AppendRequest {
    folder: String,
    content_base64: String,
}

pub(super) async fn append_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<AppendRequest>,
) -> Result<Json<Value>, AppError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    auth.require_role(OrganizationRole::Member)?;
    let source = STANDARD.decode(request.content_base64.trim()).map_err(|error| {
        AppError::invalid(
            "MESSAGE_APPEND_INVALID",
            format!("content_base64 is not base64 (RFC 4648, standard alphabet): {error}"),
        )
    })?;
    Ok(Json(json!(
        state
            .append_message(&auth.organization_id, parse_uuid(&id)?, &request.folder, source)
            .await?
    )))
}

/// `PUT /v1/mailboxes/:id/limit`: `per_day` sets the limit, `null` removes it;
/// `grow_by` with `up_to` warms it up from today, as `mailbox limit --grow-by --up-to`.
#[derive(Deserialize)]
pub(super) struct SendLimitRequest {
    per_day: Option<NonZeroU32>,
    #[serde(default)]
    grow_by: Option<NonZeroU32>,
    #[serde(default)]
    up_to: Option<NonZeroU32>,
}

pub(super) async fn mailbox_send_limit(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.mailbox_send_limit(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

pub(super) async fn set_mailbox_send_limit(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<SendLimitRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    let warm_up = match (request.grow_by, request.up_to) {
        (Some(grow_by), Some(up_to)) => Some(WarmUpRequest { grow_by, up_to }),
        (None, None) => None,
        _ => {
            return Err(AppError::invalid(
                "MAILBOX_LIMIT_INVALID",
                "a warm-up names both grow_by, what the limit grows by each day, and up_to, where it stops",
            ))
        }
    };
    Ok(Json(json!(state.set_mailbox_send_limit(
        &auth.organization_id,
        parse_uuid(&id)?,
        request.per_day,
        warm_up
    )?)))
}

pub(super) async fn mailbox_signature(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.mailbox_signature(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

/// `PUT /v1/mailboxes/:id/signature`: `signature` sets it, `null` removes it.
pub(super) async fn set_mailbox_signature(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<crate::service::SignatureRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(state.set_mailbox_signature(
        &auth.organization_id,
        parse_uuid(&id)?,
        request.signature.as_deref()
    )?)))
}
