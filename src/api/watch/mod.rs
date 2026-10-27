//! `GET /v1/messages/watch?mailbox=&after=`: every message stored from now
//! on, or after a stored-order number, as a server-sent event per message
//! (`event: message`, `id:` its stored order, `data:` `{stored_order,
//! message}`), as `skrzynka message watch` prints them. A client that
//! reconnects with `Last-Event-ID` resumes after the last message it got; a
//! watch that fails ends with one `event: refused` naming the code and cause
//! (docs/messages/watch.md).

use crate::{auth::AuthContext, error::AppError, service::AppState};
use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

#[derive(Deserialize)]
pub(super) struct WatchQuery {
    mailbox: Option<String>,
    after: Option<i64>,
}

type Events = Sse<UnboundedReceiverStream<Result<Event, Infallible>>>;

/// Where the watch starts: `after`, else the `Last-Event-ID` a reconnecting
/// client sends, else what is stored now. An unreadable header is refused.
fn starting_point(query: &WatchQuery, headers: &HeaderMap) -> Result<Option<i64>, AppError> {
    if query.after.is_some() {
        return Ok(query.after);
    }
    let Some(header) = headers.get("last-event-id") else { return Ok(None) };
    let refused = |cause: String| {
        AppError::new(
            StatusCode::BAD_REQUEST,
            "WATCH_RESUME_INVALID",
            format!("Last-Event-ID must be the stored_order of a watched message; {cause}"),
            false,
        )
    };
    let text = header.to_str().map_err(|error| refused(format!("it is not text: {error}")))?;
    text.trim().parse::<i64>().map(Some).map_err(|error| refused(format!("{text:?} is not a whole number: {error}")))
}

pub(super) async fn watch_messages(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<WatchQuery>,
    headers: HeaderMap,
) -> Result<Events, AppError> {
    let after = starting_point(&query, &headers)?;
    let mut watch = state.watch_messages(&auth.organization_id, query.mailbox.as_deref(), after)?;
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let batch = tokio::select! {
                batch = watch.next() => batch,
                () = sender.closed() => return,
            };
            let events = match batch {
                Ok(batch) => batch
                    .iter()
                    .map(|each| Event::default().event("message").id(each.stored_order.to_string()).json_data(each))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| AppError::internal(format!("a watched message could not be written as JSON: {error}"))),
                Err(error) => Err(error),
            };
            match events {
                Ok(events) => {
                    for event in events {
                        if sender.send(Ok(event)).is_err() {
                            return;
                        }
                    }
                }
                Err(error) => {
                    let refusal = json!({"code": error.code, "message": error.message, "retryable": error.retryable});
                    let _client_may_be_gone = sender.send(Ok(Event::default().event("refused").data(refusal.to_string())));
                    return;
                }
            }
        }
    });
    Ok(Sse::new(UnboundedReceiverStream::new(receiver)))
}
