//! `GET /v1/attachments`: every file the stored mail carries, found as
//! `message attachment find` finds them.

use super::super::parse_uuid;
use crate::{
    auth::AuthContext,
    error::AppError,
    service::{AppState, FileFilter},
};
use axum::{
    extract::{Extension, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `mailbox_id`, `name`, `type`, `over`, `sender`, `since`, `until`, `tag`
/// and `largest_first`, each optional.
#[derive(Deserialize)]
pub(in crate::api) struct FileQuery {
    mailbox_id: Option<String>,
    name: Option<String>,
    #[serde(rename = "type")]
    content_type: Option<String>,
    over: Option<u64>,
    sender: Option<String>,
    since: Option<String>,
    until: Option<String>,
    tag: Option<String>,
    #[serde(default)]
    largest_first: bool,
}

pub(in crate::api) async fn find_files(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.find_files(&auth.organization_id, &query.filter()?)?)))
}

/// `GET /v1/attachments/csv` with the same parameters: the files found as
/// the CSV table `message attachment find --csv` writes, offered as
/// `attachments.csv`.
pub(in crate::api) async fn files_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<FileQuery>,
) -> Result<axum::response::Response, AppError> {
    let files = state.find_files(&auth.organization_id, &query.filter()?)?;
    Ok(super::csv_file("attachments.csv", crate::service::file_table(&files)))
}

impl FileQuery {
    /// The search these parameters ask for, as `message attachment find` takes it.
    fn filter(self) -> Result<FileFilter, AppError> {
        Ok(FileFilter {
            mailbox_id: self.mailbox_id.as_deref().map(parse_uuid).transpose()?,
            name: self.name,
            content_type: self.content_type,
            over: self.over,
            sender: self.sender,
            since: self.since,
            until: self.until,
            tag: self.tag,
            largest_first: self.largest_first,
        })
    }
}
