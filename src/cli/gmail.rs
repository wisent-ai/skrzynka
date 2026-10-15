//! The Gmail subcommands: the browser authorization and the app-password prompt.

use super::{loopback_callback, print_json};
use crate::{
    db::Database, error::AppError, gmail::StartGmailOAuthRequest, service::AppState,
    skarbiec::SkarbiecResolver,
};
use std::{
    io::{self, Read},
    net::SocketAddr,
};

pub(super) async fn authorize_gmail(
    database: Database,
    resolver: SkarbiecResolver,
    organization: &str,
    skarbiec_item: String,
    bind: SocketAddr,
) -> Result<(), AppError> {
    let callback_base_url = loopback_callback(bind)?;
    let state = AppState::new(database, resolver, Some(&callback_base_url))?;
    let listener = tokio::net::TcpListener::bind(bind).await.map_err(|error| {
        AppError::internal(format!(
            "binding loopback OAuth callback at {bind} failed: {error}"
        ))
    })?;
    let flow = state
        .start_gmail_oauth(
            organization,
            StartGmailOAuthRequest {
                skarbiec_item_id: skarbiec_item,
            },
        )
        .await?;
    print_json(&flow)?;

    let callback_state = state.clone();
    let mut server =
        tokio::spawn(
            async move { axum::serve(listener, crate::api::router(callback_state)).await },
        );
    let completion = async {
        tokio::select! {
            settled = state.gmail_oauth_settled(organization, flow.flow_id) => settled,
            diagnosis = crate::gmail::diagnose_authorization(&flow.authorization_url) => {
                if let Err(error) = &diagnosis {
                    eprintln!("{error}; callback authorization remains pending");
                }
                if matches!(diagnosis.as_ref(), Ok(Some(refusal)) if refusal == "redirect_uri_mismatch") {
                    if let Some((client_id, redirect_uri)) =
                        crate::gmail::authorization_operands(&flow.authorization_url)
                    {
                        return Err(crate::gmail::redirect_not_registered(
                            &client_id,
                            &redirect_uri,
                        ));
                    }
                }
                state.gmail_oauth_settled(organization, flow.flow_id).await
            }
        }
    };
    let status = tokio::select! {
        status = completion => status,
        result = &mut server => Err(AppError::internal(match result {
            Ok(Ok(())) => format!(
                "loopback OAuth callback server at {bind} stopped before authorization settled"
            ),
            Ok(Err(error)) => format!(
                "serving loopback OAuth callback at {bind} failed: {error}"
            ),
            Err(error) => format!(
                "loopback OAuth callback task at {bind} failed: {error}"
            ),
        })),
    };
    server.abort();
    let status = status?;
    if status.status == "completed" {
        print_json(&status)?;
        return Ok(());
    }
    // A settled flow that is not completed either carries the error that
    // ended it or says nothing; the latter is reported by the state it
    // settled in, not by a sentence written here on its behalf.
    let (message, retryable) = match status.error.as_ref() {
        Some(error) => (
            format!("{}: {}", error.code, error.message),
            error.retryable,
        ),
        None => (
            format!(
                "the authorization flow settled as {} without stating an error",
                status.status
            ),
            false,
        ),
    };
    Err(AppError::dependency(
        "GMAIL_OAUTH_FAILED",
        message,
        retryable,
    ))
}

pub(super) fn read_gmail_app_password() -> Result<String, AppError> {
    let mut input = Vec::new();
    io::stdin()
        .lock()
        .read_to_end(&mut input)
        .map_err(|cause| {
            AppError::invalid(
                "GMAIL_APP_PASSWORD_INPUT_INVALID",
                format!("Google app-specific password could not be read from stdin: {cause}"),
            )
        })?;
    let input = String::from_utf8(input).map_err(|cause| {
        AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            format!(
                "Google app-specific password supplied through stdin must be valid UTF-8: {}",
                cause.utf8_error()
            ),
        )
    })?;
    let password = input
        .trim_end_matches(|character| character == '\r' || character == '\n')
        .to_string();
    if password.is_empty() {
        return Err(AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            "Google app-specific password supplied through stdin must not be empty",
        ));
    }
    Ok(password)
}
