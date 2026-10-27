//! Message routes: stored messages, their attachments, replies and outbound mail.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    models::{CreateOutboundRequest, CreateReplyRequest},
    service::{AppState, OutboundFilters},
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderValue,
    },
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) async fn get_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_message(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

pub(super) async fn list_replies(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.list_replies(&auth.organization_id, parse_uuid(&id)?,)?
    )))
}

pub(super) async fn create_reply(
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
pub(super) struct OutboundQuery {
    mailbox_id: Option<String>,
    /// `to`, `contains`, `status`, `attachment`, `since`, `until`, `bounced`, `answered` and `oldest_first` narrow and order the listing as `message outbound` does.
    to: Option<String>,
    contains: Option<String>,
    status: Option<String>,
    attachment: Option<String>,
    since: Option<String>,
    until: Option<String>,
    #[serde(default)]
    bounced: bool,
    /// `true` keeps the answered messages, `false` those nothing answers yet.
    answered: Option<bool>,
    #[serde(default)]
    oldest_first: bool,
    limit: Option<u32>,
    offset: Option<u32>,
}

impl OutboundQuery {
    fn filters(&self) -> OutboundFilters {
        OutboundFilters {
            to: self.to.clone(),
            contains: self.contains.clone(),
            status: self.status.clone(),
            attachment: self.attachment.clone(),
            since: self.since.clone(),
            until: self.until.clone(),
            bounced: self.bounced,
            answered: self.answered,
            oldest_first: self.oldest_first,
        }
    }
}

pub(super) async fn list_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<OutboundQuery>,
) -> Result<Json<Value>, AppError> {
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    let filters = query.filters();
    Ok(Json(json!(state.search_outbound(&auth.organization_id, mailbox_id, &filters, query.limit, query.offset)?)))
}

/// `GET /v1/outbound/csv` with the listing's parameters: the messages it
/// keeps as the CSV table `message outbound --csv` writes, offered as
/// `outbound.csv`.
pub(super) async fn outbound_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<OutboundQuery>,
) -> Result<axum::response::Response, AppError> {
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    let messages = state.search_outbound(&auth.organization_id, mailbox_id, &query.filters(), query.limit, query.offset)?;
    Ok(super::listing::csv_file("outbound.csv", crate::service::outbound_table(&messages)))
}

/// `POST /v1/outbound/follow-ups` with the listing's parameters and `{by}`:
/// expect an answer to every sent message the listing keeps, as `message
/// outbound --follow-up` does; role `member`.
#[derive(Deserialize)]
pub(super) struct ExpectAnswers {
    by: String,
}

pub(super) async fn expect_answers(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<OutboundQuery>,
    Json(request): Json<ExpectAnswers>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    let set = state.expect_answers(&auth.organization_id, mailbox_id, &query.filters(), query.limit, query.offset, &request.by)?;
    Ok(Json(json!(set)))
}

/// `POST /v1/outbound/resend` with the listing's parameters and
/// `{idempotency_key}`: every failed message the listing keeps sent again, as
/// `message outbound --resend-failed` does; role `member`.
#[derive(Deserialize)]
pub(super) struct ResendFailed {
    idempotency_key: String,
}

pub(super) async fn resend_failed(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<OutboundQuery>,
    Json(request): Json<ResendFailed>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    Ok(Json(json!(state.resend_failed(&auth.organization_id, mailbox_id, &query.filters(), &request.idempotency_key).await?)))
}

pub(super) async fn list_mailbox_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(query): Query<OutboundQuery>,
) -> Result<Json<Value>, AppError> {
    let mailbox_id = Some(parse_uuid(&id)?);
    Ok(Json(json!(state.search_outbound(&auth.organization_id, mailbox_id, &query.filters(), query.limit, query.offset)?)))
}

pub(super) async fn get_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_outbound(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

pub(super) async fn create_outbound(
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

/// The attachments of one message; a message imported before attachments
/// were recorded is read from its provider once.
pub(super) async fn list_attachments(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state
            .list_attachments(&auth.organization_id, parse_uuid(&id)?)
            .await?
    )))
}

/// One attachment's bytes, read from the provider now, typed as the message
/// types it and offered as a download under the name the message gives it.
pub(super) async fn get_attachment(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path((id, index)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let index = index.parse::<usize>().map_err(|error| {
        AppError::invalid(
            "ATTACHMENT_INDEX_INVALID",
            format!("attachment index {index:?} is not a whole number counted from zero: {error}"),
        )
    })?;
    let content = state
        .read_attachment(&auth.organization_id, parse_uuid(&id)?, index)
        .await?;
    let content_type = HeaderValue::from_str(&content.attachment.content_type).map_err(|error| {
        AppError::internal(format!(
            "attachment {index} type {:?} cannot be sent as a Content-Type header: {error}",
            content.attachment.content_type
        ))
    })?;
    let disposition = content_disposition(content.attachment.filename.as_deref());
    let disposition = HeaderValue::from_str(&disposition).map_err(|error| {
        AppError::internal(format!(
            "attachment {index} disposition {disposition:?} cannot be sent as a header: {error}"
        ))
    })?;
    Ok((
        [(CONTENT_TYPE, content_type), (CONTENT_DISPOSITION, disposition)],
        content.bytes,
    )
        .into_response())
}

/// `GET /v1/messages/:id/source`: the original message as the provider holds
/// it, typed `message/rfc822` and offered as `<id>.eml`.
pub(super) async fn get_source(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let message_id = parse_uuid(&id)?;
    let source = state
        .message_source(&auth.organization_id, message_id)
        .await?;
    let disposition = content_disposition(Some(&format!("{message_id}.eml")));
    let disposition = HeaderValue::from_str(&disposition).map_err(|error| {
        AppError::internal(format!(
            "message {message_id} disposition {disposition:?} cannot be sent as a header: {error}"
        ))
    })?;
    Ok((
        [
            (CONTENT_TYPE, HeaderValue::from_static("message/rfc822")),
            (CONTENT_DISPOSITION, disposition),
        ],
        source,
    )
        .into_response())
}

/// `attachment`, with the file name as RFC 6266 carries it: an ASCII
/// `filename` for old readers and the exact name as UTF-8 in `filename*`.
fn content_disposition(filename: Option<&str>) -> String {
    let Some(name) = filename else {
        return "attachment".to_string();
    };
    let ascii: String = name
        .chars()
        .map(|character| match character {
            ' '..='~' if character != '"' && character != '\\' => character,
            _ => '_',
        })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => char::from(byte).to_string(),
            b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}
