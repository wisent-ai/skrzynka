//! The first-use journey: three to five canonical screens, walked once and recorded.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{self, Write},
};
use uuid::Uuid;

mod definition;
mod state;

use definition::{canonical_definition, completion_fact, next_screen_id, screen_by_id};
use state::{load_or_start_state, new_state, read_state, save_state, state_path};

const PRODUCT_ID: &str = "skrzynka";
const JOURNEY_ID: &str = "first-use";
const STATE_SCHEMA: &str = "skrzynka.onboarding-state.v1";
const FIRST_SUCCESS_FACT: &str = "mailbox_import_persisted";
const DEFINITION: &str = include_str!("first_use.json");
/// A canonical first-use journey has three to five screens.
const MIN_JOURNEY_SCREENS: usize = 3;
const MAX_JOURNEY_SCREENS: usize = 5;

#[derive(Deserialize, Serialize)]
struct OnboardingState {
    schema: String,
    product_id: String,
    journey_id: String,
    journey_version: String,
    source_revision: Option<String>,
    attempt_id: Uuid,
    current_screen_id: String,
    status: String,
    evidence: BTreeMap<String, bool>,
}

pub fn run(reset: bool) -> Result<(), AppError> {
    let definition = canonical_definition()?;
    let mut state = if reset {
        let state = new_state(&definition)?;
        save_state(&state)?;
        println!(
            "Skrzynka first-use journey reset: recorded progress and evidence discarded; showing it again now."
        );
        println!();
        state
    } else {
        load_or_start_state(&definition)?
    };

    if state.status == "completed" {
        println!(
            "Skrzynka first-use journey is already complete: {FIRST_SUCCESS_FACT} was recorded."
        );
        return Ok(());
    }

    loop {
        let screen = screen_by_id(&definition, &state.current_screen_id)?;
        render(screen)?;

        if let Some(fact) = completion_fact(screen)? {
            if state.evidence.get(fact) == Some(&true) {
                state.status = "completed".to_string();
                save_state(&state)?;
                println!();
                println!("First-use complete: Skrzynka recorded {fact} at the successful command.");
            } else {
                println!();
                println!("{{\"onboarding\":\"awaiting_first_success\",\"fact\":\"{fact}\"}}");
            }
            return Ok(());
        }

        wait_for_enter()?;
        state.current_screen_id = next_screen_id(screen)?
            .ok_or_else(|| AppError::internal("canonical onboarding screen has no next screen"))?;
        save_state(&state)?;
        println!();
    }
}

pub fn record_mailbox_import_completed() -> Result<(), AppError> {
    let path = state_path()?;
    if !path.exists() {
        return Ok(());
    }

    let definition = canonical_definition()?;
    let mut state = read_state(&path, &definition)?;
    if state.status != "completed" {
        state.evidence.insert(FIRST_SUCCESS_FACT.to_string(), true);
        save_state(&state)?;
    }
    Ok(())
}

fn render(screen: &Value) -> Result<(), AppError> {
    let presentation = screen
        .get("presentation")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::internal("canonical onboarding screen has no presentation"))?;
    let title = presentation
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::internal("canonical onboarding screen has no title"))?;
    let body = presentation
        .get("body")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::internal("canonical onboarding screen has no body"))?;
    println!("== {title} ==\n{body}");
    Ok(())
}

fn wait_for_enter() -> Result<(), AppError> {
    print!("Press Enter to continue.");
    io::stdout()
        .flush()
        .map_err(|error| AppError::internal(format!("stdout could not be flushed: {error}")))?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|error| AppError::internal(format!("stdin could not be read: {error}")))?;
    Ok(())
}
