//! Credentials for a machine without Skarbiec: one owner-only JSON file named
//! by `SKRZYNKA_CREDENTIALS_FILE`, mapping item id to the same
//! `skarbiec.item.v2` object Skarbiec stores (`kind`, `fields`, `context`)
//! plus its `tags`. The resolver sends it the four Skarbiec verbs it uses —
//! `get`, `list`, `set-json`, `retag` — and `version`, and reads the answer
//! exactly as it reads Skarbiec's, so nothing above the transport changes.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use serde_json::{json, Map, Value};

use crate::error::AppError;

pub const CREDENTIALS_FILE_ENV: &str = "SKRZYNKA_CREDENTIALS_FILE";

/// The file this machine keeps its credentials in, when it names one.
pub fn configured() -> Option<PathBuf> {
    std::env::var_os(CREDENTIALS_FILE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn answer(status: i32, stdout: Vec<u8>, stderr: String) -> Output {
    Output {
        status: ExitStatus::from_raw(status << 8),
        stdout,
        stderr: stderr.into_bytes(),
    }
}

fn refused(detail: String) -> Output {
    answer(1, Vec::new(), detail)
}

fn unavailable(path: &Path, detail: impl std::fmt::Display) -> AppError {
    AppError::dependency(
        "SKARBIEC_UNAVAILABLE",
        format!("{CREDENTIALS_FILE_ENV}={}: {detail}", path.display()),
        false,
    )
}

fn encoded(value: &impl serde::Serialize) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec(value)
        .map_err(|_| AppError::internal("local credentials could not be encoded"))
}

/// Every item in the file; an absent file holds none.
fn read(path: &Path) -> Result<Map<String, Value>, AppError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(unavailable(path, error)),
    };
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(unavailable(
            path,
            format!("must be readable by its owner only (chmod 600); it is {mode:o}"),
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|error| unavailable(path, error))?;
    match serde_json::from_str(&text) {
        Ok(Value::Object(items)) => Ok(items),
        Ok(_) => Err(unavailable(
            path,
            "must be a JSON object of item id -> item",
        )),
        Err(error) => Err(unavailable(path, format!("is not readable JSON: {error}"))),
    }
}

fn write(path: &Path, items: &Map<String, Value>) -> Result<(), AppError> {
    let text = serde_json::to_string_pretty(items)
        .map_err(|_| AppError::internal("local credentials could not be encoded"))?;
    let staged = path.with_extension("tmp");
    std::fs::write(&staged, text).map_err(|error| unavailable(path, error))?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| unavailable(path, error))?;
    std::fs::rename(&staged, path).map_err(|error| unavailable(path, error))
}

fn tags_of(item: &Value) -> Value {
    match item.get("tags") {
        Some(tags) => tags.clone(),
        None => json!([]),
    }
}

/// Answer one Skarbiec invocation from the file at `path`.
pub fn run(path: &Path, arguments: &[&str], stdin: Option<&[u8]>) -> Result<Output, AppError> {
    let mut items = read(path)?;
    Ok(match arguments {
        ["version"] => answer(0, b"local credentials file\n".to_vec(), String::new()),
        ["get", id] => match items.get(*id) {
            Some(item) => answer(0, encoded(item)?, String::new()),
            None => refused(format!("item not found: {id}")),
        },
        ["list"] => {
            let listed: Vec<Value> = items
                .iter()
                .map(|(id, item)| {
                    json!({
                        "id": id,
                        "kind": item.get("kind"),
                        "state": "active",
                        "tags": tags_of(item),
                    })
                })
                .collect();
            answer(0, encoded(&listed)?, String::new())
        }
        ["retag", id, "--tags", joined] => {
            match items.get_mut(*id).and_then(Value::as_object_mut) {
                Some(item) => {
                    let tags: Vec<&str> = joined.split(',').filter(|tag| !tag.is_empty()).collect();
                    item.insert("tags".into(), json!(tags));
                    write(path, &items)?;
                    answer(0, Vec::new(), String::new())
                }
                None => refused(format!("item not found: {id}")),
            }
        }
        ["set-json", id, "--type", kind] => {
            let parsed = stdin.map(serde_json::from_slice::<Value>);
            let Some(Ok(Value::Object(mut item))) = parsed else {
                return Ok(refused(
                    "set-json needs one JSON object on standard input".into(),
                ));
            };
            item.insert("kind".into(), json!(kind));
            if let Some(existing) = items.get(*id) {
                item.insert("tags".into(), tags_of(existing));
            }
            items.insert((*id).to_string(), Value::Object(item));
            write(path, &items)?;
            answer(0, Vec::new(), String::new())
        }
        other => refused(format!(
            "{CREDENTIALS_FILE_ENV} answers get, list, set-json, retag and version; not {other:?}"
        )),
    })
}
