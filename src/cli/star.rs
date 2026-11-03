//! The GitHub star request: after a person's fifth message sent on a
//! terminal (`message send`, `message reply`, `message draft send`),
//! Skrzynka asks once for a star on its public repository. The request is
//! the `star-request` journey every Wisent product walks, run by the shared
//! onboarding runtime and kept per user in
//! `$XDG_STATE_HOME/skrzynka/onboarding.json` (default
//! `~/.local/state/skrzynka/onboarding.json`); `skrzynka star` reads,
//! dismisses and resets it. The threshold is the runtime's, the operator's
//! number; the repository is this package's `repository`, the catalog's
//! `star_repository`.

use clap::Subcommand;
use wisent_onboarding_client::{JourneyError, ProductStarRequest, StarProduct};

use crate::error::AppError;

#[derive(Subcommand)]
pub(super) enum StarCommand {
    /// Where the request stands: counting, shown or dismissed.
    Status,
    /// Never make the request again, until a reset.
    Dismiss,
    /// Count again from zero; set aside a journey file that holds no state.
    Reset,
}

fn request() -> Result<ProductStarRequest, JourneyError> {
    let repository = env!("CARGO_PKG_REPOSITORY");
    let product = StarProduct::from_package("skrzynka", "Skrzynka", repository)?;
    ProductStarRequest::new(product)
}

/// Count one message a person sent, when a person reads this terminal: the
/// request, on the one that makes it, goes to standard error, so standard
/// output keeps only the command's answer; a request that cannot be kept is
/// said there too, and the command's own answer stands.
pub(super) fn observe() {
    match request() {
        Ok(request) => {
            request.observe_reporting();
        }
        Err(error) => {
            eprintln!(
                "skrzynka: this message is not counted toward the GitHub star request: {error}"
            )
        }
    }
}

/// `skrzynka star status|dismiss|reset`: the status as JSON, or as `--text`
/// lines, as every command answers. Needs no organization or database.
pub(super) fn run(command: &StarCommand) -> Result<(), AppError> {
    let verb = match command {
        StarCommand::Status => "status",
        StarCommand::Dismiss => "dismiss",
        StarCommand::Reset => "reset",
    };
    let answered = request()
        .map_err(|error| error.to_string())
        .and_then(|request| request.command(&[verb.to_string()], true))
        .map_err(|refusal| AppError::invalid("STAR_REQUEST", refusal))?;
    let document: serde_json::Value = serde_json::from_str(&answered).map_err(|error| {
        AppError::internal(format!("the star status is not JSON ({error}): {answered}"))
    })?;
    super::print_json(&document)
}
