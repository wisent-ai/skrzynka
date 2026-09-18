//! The canonical first-use journey definition and the screen lookups over it.

use super::{DEFINITION, FIRST_SUCCESS_FACT, JOURNEY_ID, MAX_JOURNEY_SCREENS, MIN_JOURNEY_SCREENS, PRODUCT_ID};
use crate::error::AppError;
use serde_json::Value;
use std::collections::HashSet;

pub(super) fn canonical_definition() -> Result<Value, AppError> {
    let definition: Value = serde_json::from_str(DEFINITION).map_err(|error| {
        AppError::internal(format!("canonical onboarding journey is invalid: {error}"))
    })?;
    if definition.get("schema_version").and_then(Value::as_u64) != Some(1)
        || definition.get("product_id").and_then(Value::as_str) != Some(PRODUCT_ID)
        || definition.get("journey_id").and_then(Value::as_str) != Some(JOURNEY_ID)
        || definition.get("first_success_fact").and_then(Value::as_str) != Some(FIRST_SUCCESS_FACT)
    {
        return Err(AppError::internal(
            "canonical onboarding journey identity mismatch",
        ));
    }

    let entry = definition
        .get("entry_screen_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::internal("canonical onboarding journey has no entry screen"))?;
    let screens = definition
        .get("screens")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::internal("canonical onboarding journey has no screens"))?;
    if !(MIN_JOURNEY_SCREENS..=MAX_JOURNEY_SCREENS).contains(&screens.len()) {
        return Err(AppError::internal(
            "canonical onboarding journey must have three to five screens",
        ));
    }

    let mut ids = HashSet::new();
    for screen in screens {
        let id = screen
            .get("screen_id")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::internal("canonical onboarding screen has no id"))?;
        if !ids.insert(id) {
            return Err(AppError::internal(format!(
                "duplicate canonical onboarding screen id: {id}"
            )));
        }
        let presentation = screen
            .get("presentation")
            .and_then(Value::as_object)
            .ok_or_else(|| AppError::internal("canonical onboarding screen has no presentation"))?;
        if presentation.get("title").and_then(Value::as_str).is_none()
            || presentation.get("body").and_then(Value::as_str).is_none()
        {
            return Err(AppError::internal(
                "canonical onboarding screen has incomplete presentation",
            ));
        }
    }
    if !ids.contains(entry) {
        return Err(AppError::internal(
            "canonical onboarding entry screen does not exist",
        ));
    }
    for screen in screens {
        for transition in screen
            .get("transitions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let next = transition
                .get("next_screen_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::internal("canonical onboarding transition has no target")
                })?;
            if !ids.contains(next) {
                return Err(AppError::internal(format!(
                    "canonical onboarding transition target does not exist: {next}"
                )));
            }
        }
    }
    Ok(definition)
}

pub(super) fn screen_by_id<'a>(definition: &'a Value, id: &str) -> Result<&'a Value, AppError> {
    definition
        .get("screens")
        .and_then(Value::as_array)
        .and_then(|screens| {
            screens
                .iter()
                .find(|screen| screen.get("screen_id").and_then(Value::as_str) == Some(id))
        })
        .ok_or_else(|| {
            AppError::internal(format!("canonical onboarding screen is unavailable: {id}"))
        })
}

pub(super) fn next_screen_id(screen: &Value) -> Result<Option<String>, AppError> {
    let Some(transitions) = screen.get("transitions").and_then(Value::as_array) else {
        return Ok(None);
    };
    transitions
        .iter()
        .max_by_key(|transition| {
            transition
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or_default()
        })
        .map(|transition| {
            transition
                .get("next_screen_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| AppError::internal("canonical onboarding transition has no target"))
        })
        .transpose()
}

pub(super) fn completion_fact(screen: &Value) -> Result<Option<&str>, AppError> {
    let Some(evidence) = screen
        .get("completion_evidence")
        .filter(|value| !value.is_null())
    else {
        return Ok(None);
    };
    if evidence.get("kind").and_then(Value::as_str) != Some("fact")
        || evidence.get("operator").and_then(Value::as_str) != Some("eq")
        || evidence.get("value") != Some(&Value::Bool(true))
    {
        return Err(AppError::internal(
            "canonical onboarding evidence rule is unsupported",
        ));
    }
    evidence
        .get("fact")
        .and_then(Value::as_str)
        .map(Some)
        .ok_or_else(|| AppError::internal("canonical onboarding evidence rule has no fact"))
}
