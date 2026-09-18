//! The persisted journey state: where it lives, how it is read, started and saved.

use super::{definition::screen_by_id, OnboardingState, JOURNEY_ID, PRODUCT_ID, STATE_SCHEMA};
use crate::error::AppError;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};
use uuid::Uuid;

pub(super) fn load_or_start_state(definition: &Value) -> Result<OnboardingState, AppError> {
    let path = state_path()?;
    if path.exists() {
        return read_state(&path, definition);
    }
    let state = new_state(definition)?;
    save_state(&state)?;
    Ok(state)
}

pub(super) fn read_state(path: &PathBuf, definition: &Value) -> Result<OnboardingState, AppError> {
    let body = fs::read_to_string(path).map_err(|error| {
        AppError::internal(format!(
            "onboarding state could not be read from {}: {error}",
            path.display()
        ))
    })?;
    let state: OnboardingState = serde_json::from_str(&body)
        .map_err(|error| AppError::internal(format!("onboarding state is invalid: {error}")))?;
    if state.schema != STATE_SCHEMA
        || state.product_id != PRODUCT_ID
        || state.journey_id != JOURNEY_ID
    {
        return Err(AppError::internal(
            "stored onboarding state identity mismatch; use --reset to replace it",
        ));
    }
    let definition_version = definition
        .get("journey_version")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::internal("canonical onboarding journey has no version"))?;
    if state.journey_version != definition_version {
        let replacement = new_state(definition)?;
        save_state(&replacement)?;
        return Ok(replacement);
    }
    screen_by_id(definition, &state.current_screen_id)?;
    Ok(state)
}

pub(super) fn new_state(definition: &Value) -> Result<OnboardingState, AppError> {
    Ok(OnboardingState {
        schema: STATE_SCHEMA.to_string(),
        product_id: PRODUCT_ID.to_string(),
        journey_id: JOURNEY_ID.to_string(),
        journey_version: definition
            .get("journey_version")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::internal("canonical onboarding journey has no version"))?
            .to_string(),
        source_revision: definition
            .get("source_revision")
            .and_then(Value::as_str)
            .map(str::to_string),
        attempt_id: Uuid::new_v4(),
        current_screen_id: definition
            .get("entry_screen_id")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::internal("canonical onboarding journey has no entry screen"))?
            .to_string(),
        status: "in_progress".to_string(),
        evidence: BTreeMap::new(),
    })
}

pub(super) fn save_state(state: &OnboardingState) -> Result<(), AppError> {
    let path = state_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("onboarding state path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| {
        AppError::internal(format!(
            "onboarding state directory could not be created: {error}"
        ))
    })?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|error| {
        AppError::internal(format!(
            "onboarding state directory permissions could not be set: {error}"
        ))
    })?;

    let temporary = path.with_extension(format!("json.tmp-{}", Uuid::new_v4()));
    let body = serde_json::to_vec(state).map_err(|error| {
        AppError::internal(format!("onboarding state could not be encoded: {error}"))
    })?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| {
            AppError::internal(format!("onboarding state could not be created: {error}"))
        })?;
    file.write_all(&body)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            AppError::internal(format!("onboarding state could not be saved: {error}"))
        })?;
    fs::rename(&temporary, &path).map_err(|error| {
        AppError::internal(format!("onboarding state could not be replaced: {error}"))
    })?;
    Ok(())
}

/// The state file: under `XDG_STATE_HOME` when set, otherwise under the home directory.
/// With neither set there is no place the journey can be recorded, and that is refused
/// rather than written into the working directory.
pub(super) fn state_path() -> Result<PathBuf, AppError> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("skrzynka/onboarding.json"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        AppError::internal(
            "neither XDG_STATE_HOME nor HOME is set, so the onboarding state has no place to live",
        )
    })?;
    Ok(PathBuf::from(home).join(".local/state/skrzynka/onboarding.json"))
}
