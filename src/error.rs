use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl AppError {
    pub fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message, false)
    }

    pub fn not_found(resource: &'static str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("{resource} was not found"),
            false,
        )
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message, false)
    }

    pub fn dependency(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, code, message, retryable)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            message,
            false,
        )
    }

    /// This error, plus the failure of recording it when that write was
    /// refused too. A send that failed and whose `failed` status could not be
    /// stored still reads as `state`; dropping the write's error left the
    /// caller with the send's refusal and a record that says otherwise.
    pub fn with_unrecorded<T>(self, recorded: Result<T, AppError>, state: &str) -> Self {
        match recorded {
            Ok(_) => self,
            Err(write) => Self {
                message: format!(
                    "{}; recording that failure was refused too ({write}), so the record still reads {state}",
                    self.message
                ),
                ..self
            },
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
    retryable: bool,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code,
                message: self.message,
                retryable: self.retryable,
            },
        };
        (status, Json(body)).into_response()
    }
}

impl From<crate::db::sql::Error> for AppError {
    fn from(error: crate::db::sql::Error) -> Self {
        tracing::error!(error = %error, "database operation failed");
        Self::internal(format!("fleet database operation failed: {error}"))
    }
}

impl From<uuid::Error> for AppError {
    fn from(_: uuid::Error) -> Self {
        Self::invalid("INVALID_IDENTIFIER", "identifier is not a valid UUID")
    }
}
