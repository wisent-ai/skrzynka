//! Draft routes: messages written over time and sent when ready.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    mail::DraftChange,
    service::{AppState, DraftFilter, DraftSendRequest},
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `GET /v1/drafts?mailbox_id=&scheduled=&contains=&bounced=&suppressed=`, as `message draft list` takes them.
#[derive(Deserialize)]
pub(in crate::api) struct DraftQuery {
    mailbox_id: Option<String>,
    #[serde(default)]
    scheduled: bool,
    contains: Option<String>,
    #[serde(default)]
    bounced: bool,
    #[serde(default)]
    suppressed: bool,
    since: Option<String>,
    until: Option<String>,
    #[serde(default)]
    with_attachments: bool,
    send_by: Option<String>,
}

/// `POST /v1/drafts`: the mailbox (ID or address) and the first fields.
#[derive(Deserialize)]
pub(in crate::api) struct NewDraft {
    mailbox: String,
    #[serde(flatten)]
    change: DraftChange,
}

pub(in crate::api) async fn list_drafts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<DraftQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.list_drafts(&auth.organization_id, &query.filter()?)?
    )))
}

/// `GET /v1/drafts/csv` with the listing's parameters: the drafts it keeps
/// as the CSV table `message draft list --csv` writes, offered as `drafts.csv`.
pub(in crate::api) async fn drafts_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<DraftQuery>,
) -> Result<axum::response::Response, AppError> {
    let drafts = state.list_drafts(&auth.organization_id, &query.filter()?)?;
    Ok(super::listing::csv_file(
        "drafts.csv",
        crate::service::draft_table(&drafts),
    ))
}

impl DraftQuery {
    /// The listing these parameters ask for, as `message draft list` takes it.
    fn filter(self) -> Result<DraftFilter, AppError> {
        Ok(DraftFilter {
            mailbox_id: self.mailbox_id.as_deref().map(parse_uuid).transpose()?,
            scheduled: self.scheduled,
            contains: self.contains,
            bounced: self.bounced,
            suppressed: self.suppressed,
            since: self.since,
            until: self.until,
            with_attachments: self.with_attachments,
            send_by: self.send_by,
        })
    }
}

pub(in crate::api) async fn create_draft(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<NewDraft>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.create_draft(
        &auth.organization_id,
        &request.mailbox,
        request.change
    )?)))
}

/// `POST /v1/drafts/merge` with `{mailbox, template, table}`: one draft per
/// row of the CSV text `table`, as `message draft save --template --merge`
/// keeps them; answers the drafts kept, every row checked first.
#[derive(Deserialize)]
pub(in crate::api) struct MergeRequest {
    mailbox: String,
    template: String,
    table: String,
}

pub(in crate::api) async fn merge_drafts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<MergeRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let drafts = state.merge_drafts(
        &auth.organization_id,
        &request.mailbox,
        &request.template,
        &request.table,
    )?;
    Ok(Json(json!(drafts)))
}

/// `POST /v1/outbound/:id/draft`: a new draft from an originated message.
pub(in crate::api) async fn draft_from_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.draft_from_outbound(
        &auth.organization_id,
        parse_uuid(&id)?
    )?)))
}

/// `POST /v1/drafts/:id/copy` with `{mailbox}` (optional): a new draft
/// holding the same addresses, subject, body and files, as `message draft copy`.
#[derive(Deserialize, Default)]
#[serde(default)]
pub(in crate::api) struct CopyDraft {
    mailbox: Option<String>,
}

pub(in crate::api) async fn copy_draft(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<CopyDraft>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.copy_draft(
        &auth.organization_id,
        parse_uuid(&id)?,
        request.mailbox.as_deref()
    )?)))
}

pub(in crate::api) async fn get_draft(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_draft(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

pub(in crate::api) async fn update_draft(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(change): Json<DraftChange>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.update_draft(
        &auth.organization_id,
        parse_uuid(&id)?,
        change
    )?)))
}

pub(in crate::api) async fn delete_draft(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    state.delete_draft(&auth.organization_id, parse_uuid(&id)?)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /v1/drafts?contains=TEXT&mailbox_id=UUID` (the listing's
/// parameters, `contains` required): remove every draft the listing keeps
/// and answer them, as `message draft delete --contains` does.
pub(in crate::api) async fn delete_drafts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<DraftQuery>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.delete_drafts(&auth.organization_id, &query.filter()?)?
    )))
}

/// `DELETE /v1/drafts/schedule?contains=TEXT&mailbox_id=UUID` (the
/// listing's parameters, `contains` required): take every scheduled draft
/// the listing keeps off its schedule and answer them, as `message draft
/// unschedule --contains` does.
pub(in crate::api) async fn unschedule_drafts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<DraftQuery>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.unschedule_drafts(
        &auth.organization_id,
        &query.filter()?
    )?)))
}

pub(in crate::api) async fn send_draft(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<DraftSendRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .send_draft(&auth.organization_id, parse_uuid(&id)?, request)
            .await?
    )))
}

/// `POST /v1/drafts/send?contains=TEXT&mailbox_id=UUID` (the listing's
/// parameters, `contains` required) with `{idempotency_key, allow_bounced,
/// follow_up}`: send every draft the listing keeps now, each claimed by
/// `<idempotency_key>-<draft id>`, as `message draft send --contains` does.
pub(in crate::api) async fn send_drafts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<DraftQuery>,
    Json(request): Json<DraftSendRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let filter = query.filter()?;
    let DraftSendRequest {
        idempotency_key,
        allow_bounced,
        follow_up,
    } = request;
    let sent = state
        .send_drafts(
            &auth.organization_id,
            &filter,
            &idempotency_key,
            allow_bounced,
            follow_up.as_deref(),
        )
        .await?;
    Ok(Json(json!(sent)))
}

/// `POST /v1/drafts/:id/schedule` with `{send_at, idempotency_key}`.
#[derive(Deserialize)]
pub(in crate::api) struct ScheduleDraft {
    send_at: String,
    idempotency_key: String,
}

pub(in crate::api) async fn schedule_draft(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<ScheduleDraft>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.schedule_draft(
        &auth.organization_id,
        parse_uuid(&id)?,
        &request.send_at,
        &request.idempotency_key
    )?)))
}

/// `DELETE /v1/drafts/:id/schedule`: the draft stays, unscheduled.
pub(in crate::api) async fn unschedule_draft(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.unschedule_draft(
        &auth.organization_id,
        parse_uuid(&id)?
    )?)))
}

/// `POST /v1/drafts/schedule` with `{contains, mailbox_id, send_at, every,
/// idempotency_key}` (`mailbox_id` and `every` optional): every draft
/// mentioning `contains` and not scheduled yet, spread `every` apart, as
/// `message draft schedule --contains --every` schedules them.
#[derive(Deserialize)]
pub(in crate::api) struct ScheduleMany {
    contains: String,
    mailbox_id: Option<String>,
    send_at: String,
    every: Option<String>,
    idempotency_key: String,
}

pub(in crate::api) async fn schedule_drafts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<ScheduleMany>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let filter = DraftFilter {
        mailbox_id: request.mailbox_id.as_deref().map(parse_uuid).transpose()?,
        contains: Some(request.contains),
        ..DraftFilter::default()
    };
    let every = request.every.as_deref();
    Ok(Json(json!(state.schedule_drafts(
        &auth.organization_id,
        &filter,
        &request.send_at,
        every,
        &request.idempotency_key
    )?)))
}
