//! The Skarbiec CLI transport: get and set one item.
//!
//! Skarbiec owns vault selection, including the declared fleet owner on a
//! machine without a local vault. Skrzynka invokes the selected Skarbiec
//! executable once and retains its answer or causal refusal; a failed read
//! never selects another vault or transport. Child waits are announced by
//! `stado_wait`.

use super::{invalid_item, validate_item_id, SkarbiecResolver};
use crate::error::AppError;
use serde_json::Value;
use std::process::Stdio;
use tokio::{io::AsyncWriteExt, process::Command};

impl SkarbiecResolver {
    /// Observe the selected vault without reading any credential payload.
    pub(crate) async fn credential_source(&self) -> crate::models::CredentialSource {
        let mut source = crate::models::CredentialSource {
            vault: None,
            route: None,
            endpoint: None,
            error: None,
        };
        if let Some(path) = &self.local {
            source.vault = Some(path.display().to_string());
            source.route = Some(super::local::CREDENTIALS_FILE_ENV.to_string());
            return source;
        }
        match self.run(&self.binary, &["status"]).await {
            Ok(output) if output.status.success() => {
                match serde_json::from_slice::<Value>(&output.stdout) {
                    Ok(status) => {
                        source.vault = status
                            .get("vault")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        source.route = status
                            .pointer("/source/route")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        source.endpoint = status
                            .pointer("/source/endpoint")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        if source.vault.is_none() {
                            source.error = Some(format!(
                                "{} status returned no vault identity",
                                self.binary.display()
                            ));
                        }
                    }
                    Err(error) => {
                        source.error = Some(format!(
                            "{} status returned invalid JSON: {error}",
                            self.binary.display()
                        ))
                    }
                }
            }
            Ok(output) => {
                source.error = Some(format!(
                    "{} status failed ({}): {}",
                    self.binary.display(),
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
            Err(error) => source.error = Some(error.message),
        }
        source
    }

    pub(super) async fn set_item(
        &self,
        item_id: &str,
        kind: &str,
        payload: &Value,
    ) -> Result<(), AppError> {
        validate_item_id(item_id)?;
        let bytes = serde_json::to_vec(payload).map_err(|error| {
            AppError::internal(format!(
                "encode Gmail credential payload for Skarbiec item '{item_id}': {error}"
            ))
        })?;
        if let Some(path) = &self.local {
            let output =
                super::local::run(path, &["set-json", item_id, "--type", kind], Some(&bytes))?;
            if !output.status.success() {
                return Err(AppError::dependency(
                    "SKARBIEC_WRITE_FAILED",
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                    false,
                ));
            }
            return Ok(());
        }
        let mut command = Command::new(&self.binary);
        command
            .args(["set-json", item_id, "--type", kind])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            AppError::dependency(
                "SKARBIEC_UNAVAILABLE",
                format!(
                    "start {} set-json for item '{item_id}': {error}",
                    self.binary.display()
                ),
                true,
            )
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::internal("Skarbiec input pipe was unavailable"))?;
        stdin.write_all(&bytes).await.map_err(|error| {
            AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                format!(
                    "write Gmail authorization to {} for item '{item_id}': {error}",
                    self.binary.display()
                ),
                false,
            )
        })?;
        drop(stdin);
        let output = stado_wait::child_output_async(
            child,
            format!("skarbiec set-json {item_id} --type {kind}"),
        )
        .await
        .map_err(|error| {
            AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                format!("Skarbiec did not persist the item '{item_id}': {error}"),
                false,
            )
        })?;
        if !output.status.success() {
            return Err(AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                format!(
                    "Skarbiec rejected the item '{item_id}': {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                false,
            ));
        }
        Ok(())
    }

    pub(crate) async fn get_item(&self, item_id: &str) -> Result<Value, AppError> {
        let output = self.output(&["get", item_id]).await?;
        if !output.status.success() {
            return Err(invalid_item(format!(
                "Skarbiec item '{item_id}' could not be read: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        serde_json::from_slice(&output.stdout).map_err(|error| {
            AppError::dependency(
                "SKARBIEC_RESPONSE_INVALID",
                format!("decode Skarbiec item '{item_id}' JSON: {error}"),
                false,
            )
        })
    }

    /// Move one item to Skarbiec's recoverable trash (`skarbiec delete`).
    pub(crate) async fn delete_item(&self, item_id: &str) -> Result<(), AppError> {
        validate_item_id(item_id)?;
        let output = self.output(&["delete", item_id]).await?;
        if !output.status.success() {
            return Err(AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                format!(
                    "Skarbiec refused to delete item '{item_id}': {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                false,
            ));
        }
        self.token_cache.lock().await.remove(item_id);
        Ok(())
    }

    /// Invoke the explicitly configured credentials file or the selected
    /// Skarbiec executable. Skarbiec, not its consumer, selects the vault.
    pub(super) async fn output(
        &self,
        arguments: &[&str],
    ) -> Result<std::process::Output, AppError> {
        if let Some(path) = &self.local {
            return super::local::run(path, arguments, None);
        }
        self.run(&self.binary, arguments).await
    }

    async fn run(
        &self,
        program: &std::path::Path,
        arguments: &[&str],
    ) -> Result<std::process::Output, AppError> {
        let mut command = Command::new(program);
        command.args(arguments);
        command.kill_on_drop(true);
        stado_wait::output_async(&mut command)
            .await
            .map_err(|error| {
                AppError::dependency(
                    "SKARBIEC_UNAVAILABLE",
                    format!("{} could not be started: {error}", program.display()),
                    true,
                )
            })
    }
}
