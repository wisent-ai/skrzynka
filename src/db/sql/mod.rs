//! Skrzynka's row operations run on stado-database's synchronous client: one
//! SeaORM connection to the fleet database `skrzynka`, resolved through Stado
//! and Skarbiec, with no Postgres client of Skrzynka's own. The client
//! answers `execute`, `query_row`, `prepare(..).query_map` and `transaction`,
//! and leaves a Tokio worker for each wait.
//!
//! Stado and the database bearer belong to the account running Skrzynka. A
//! caller that gives Skrzynka another HOME (the mailbox journeys isolate
//! Skarbiec and onboarding state that way) names the account's own home in
//! `SKRZYNKA_FLEET_HOME`.

mod announcements;

pub use stado_database::params;
pub use stado_database::sync::{Error, OptionalExtension, Result, Row};

use crate::error::AppError;
use std::mem::ManuallyDrop;
use std::ops::Deref;

use super::DATABASE_NAME;

/// stado-database's synchronous client. It owns a Tokio runtime, and Tokio
/// refuses to shut a runtime down from inside another one's async context
/// with a panic that hides why: the CLI and the service hold this client in
/// their own runtime. So the client is opened, and released, on a thread of
/// its own, where its runtime may start and stop.
pub struct Client(ManuallyDrop<stado_database::sync::Client>);

impl Deref for Client {
    type Target = stado_database::sync::Client;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: `self.0` is taken once, here, and never read again.
        let client = unsafe { ManuallyDrop::take(&mut self.0) };
        if let Err(stopped) = std::thread::spawn(move || drop(client)).join() {
            let cause = panic_text(stopped.as_ref());
            tracing::error!(%cause, "the fleet database client's runtime stopped abnormally while it was released");
        }
    }
}

/// What a panicking thread said, as its payload carries it.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "a panic whose payload is not text".to_owned()
}

/// The fleet database `skrzynka`; a refusal names the step that failed and
/// what Stado answered.
pub fn connect() -> std::result::Result<Client, AppError> {
    let unreachable = |error: &dyn std::fmt::Display| {
        tracing::error!(error = %error, "fleet database connection failed");
        AppError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "DATABASE_UNREACHABLE",
            format!("the fleet database {DATABASE_NAME} is unreachable: {error}"),
            true,
        )
    };
    let opened = std::thread::spawn(|| {
        stado_database::FleetDatabase::for_product(DATABASE_NAME, "SKRZYNKA_FLEET_HOME")
            .and_then(|fleet| stado_database::sync::Client::connect(&fleet))
    })
    .join()
    .map_err(|stopped| unreachable(&format!("connecting stopped abnormally: {}", panic_text(stopped.as_ref()))))?;
    opened.map(|client| Client(ManuallyDrop::new(client))).map_err(|error| unreachable(&error))
}
