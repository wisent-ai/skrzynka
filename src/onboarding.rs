use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};
use uuid::Uuid;

const PRODUCT_ID: &str = "skrzynka";
const JOURNEY_ID: &str = "first-use";
const STATE_SCHEMA: &str = "skrzynka.onboarding-state.v1";
const FIRST_SUCCESS_FACT: &str = "mailbox_import_persisted";
const DEFINITION: &str = include_str!("onboarding_first_use.json");

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
        println!("Skrzynka first-use journey is already complete: {FIRST_SUCCESS_FACT} was recorded.");
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
        state.current_screen_id = next_screen_id(screen)?.ok_or_else(|| {
            AppError::internal("canonical onboarding screen has no next screen")
        })?;
        save_state(&state)?;
        println!();
    }
}

pub fn record_mailbox_import_completed() -> Result<(), AppError> {
    let path = state_path();
    if !path.exists() {
        return Ok(());
    }

    let definition = canonical_definition()?;
    let mut state = read_state(&path, &definition)?;
    if state.status != "completed" {
        state
            .evidence
            .insert(FIRST_SUCCESS_FACT.to_string(), true);
        save_state(&state)?;
    }
    Ok(())
}

fn canonical_definition() -> Result<Value, AppError> {
    let definition: Value = serde_json::from_str(DEFINITION)
        .map_err(|error| AppError::internal(format!("canonical onboarding journey is invalid: {error}")))?;
    if definition.get("schema_version").and_then(Value::as_u64) != Some(1)
        || definition.get("product_id").and_then(Value::as_str) != Some(PRODUCT_ID)
        || definition.get("journey_id").and_then(Value::as_str) != Some(JOURNEY_ID)
        || definition.get("first_success_fact").and_then(Value::as_str)
            != Some(FIRST_SUCCESS_FACT)
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
    if !(3..=5).contains(&screens.len()) {
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

fn load_or_start_state(definition: &Value) -> Result<OnboardingState, AppError> {
    let path = state_path();
    if path.exists() {
        return read_state(&path, definition);
    }
    let state = new_state(definition)?;
    save_state(&state)?;
    Ok(state)
}

fn read_state(path: &PathBuf, definition: &Value) -> Result<OnboardingState, AppError> {
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

fn new_state(definition: &Value) -> Result<OnboardingState, AppError> {
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

fn save_state(state: &OnboardingState) -> Result<(), AppError> {
    let path = state_path();
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
    let body = serde_json::to_vec(state)
        .map_err(|error| AppError::internal(format!("onboarding state could not be encoded: {error}")))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| AppError::internal(format!("onboarding state could not be created: {error}")))?;
    file.write_all(&body)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|error| AppError::internal(format!("onboarding state could not be saved: {error}")))?;
    fs::rename(&temporary, &path)
        .map_err(|error| AppError::internal(format!("onboarding state could not be replaced: {error}")))?;
    Ok(())
}

fn screen_by_id<'a>(definition: &'a Value, id: &str) -> Result<&'a Value, AppError> {
    definition
        .get("screens")
        .and_then(Value::as_array)
        .and_then(|screens| {
            screens
                .iter()
                .find(|screen| screen.get("screen_id").and_then(Value::as_str) == Some(id))
        })
        .ok_or_else(|| AppError::internal(format!("canonical onboarding screen is unavailable: {id}")))
}

fn next_screen_id(screen: &Value) -> Result<Option<String>, AppError> {
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
                .ok_or_else(|| {
                    AppError::internal("canonical onboarding transition has no target")
                })
        })
        .transpose()
}

fn completion_fact(screen: &Value) -> Result<Option<&str>, AppError> {
    let Some(evidence) = screen.get("completion_evidence").filter(|value| !value.is_null()) else {
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

fn state_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("skrzynka/onboarding.json");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/skrzynka/onboarding.json")
}
