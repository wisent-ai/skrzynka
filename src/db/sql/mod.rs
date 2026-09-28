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

pub use stado_database::params;
pub use stado_database::sync::{Client, Error, OptionalExtension, Result, Row};

use crate::error::AppError;

use super::DATABASE_NAME;

/// The fleet database `skrzynka`; a refusal names the step that failed and
/// what Stado answered.
pub fn connect() -> std::result::Result<Client, AppError> {
    stado_database::FleetDatabase::for_product(DATABASE_NAME, "SKRZYNKA_FLEET_HOME")
        .and_then(|fleet| Client::connect(&fleet))
        .map_err(|error| {
            tracing::error!(error = %error, "fleet database connection failed");
            AppError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "DATABASE_UNREACHABLE",
                format!("the fleet database {DATABASE_NAME} is unreachable: {error}"),
                true,
            )
        })
}
