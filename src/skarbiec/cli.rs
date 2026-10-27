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
    /// The Skarbiec executable this resolver runs, so recovery guidance names
    /// the one `--skarbiec-bin` selected; `None` when an explicit credentials
    /// file answers instead of Skarbiec.
    pub(crate) fn status_executable(&self) -> Option<&std::path::Path> {
        match self.local {
            Some(_) => None,
            None => Some(&self.binary),
        }
    }

    /// Observe the selected vault without reading any credential payload.
    pub(crate) async fn credential_source(&self) -> crate::models::CredentialSource {
        let mut source = crate::models::CredentialSource {
            vault: None,
            route: None,
            endpoint: None,
            error: None,
            item_held: None,
            item_error: None,
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
                            .filter(|vault| !vault.trim().is_empty())
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
                                "{} status returned no non-empty vault identity",
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
                super::local::run(path, &["set", item_id, "--type", kind], Some(&bytes))?;
            if !output.status.success() {
                return Err(AppError::dependency(
                    "SKARBIEC_WRITE_FAILED",
                    format!(
                        "the credentials file {} refused item '{item_id}' ({}): {}",
                        path.display(),
                        output.status,
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                    false,
                ));
            }
            return Ok(());
        }
        let mut command = Command::new(&self.binary);
        command
            .args(["set", item_id, "--type", kind])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            AppError::dependency(
                "SKARBIEC_UNAVAILABLE",
                format!(
                    "start {} set for item '{item_id}': {error}",
                    self.binary.display()
                ),
                true,
            )
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::internal("Skarbiec input pipe was unavailable"))?;
        let (written, output) = tokio::join!(
            async {
                let result = stdin.write_all(&bytes).await;
                drop(stdin);
                result
            },
            stado_wait::child_output_async(
                child,
                format!("skarbiec set {item_id} --type {kind}"),
            )
        );
        let output = output.map_err(|error| {
            AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                format!(
                    "wait for Skarbiec set for item '{item_id}': {error}; persisted state is unknown"
                ),
                false,
            )
        })?;
        if !output.status.success() {
            return Err(AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                format!(
                    "Skarbiec rejected the item '{item_id}' ({}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                false,
            ));
        }
        written.map_err(|error| {
            AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                format!(
                    "write Gmail authorization to {} for item '{item_id}': {error}; Skarbiec exited successfully but full input delivery was not confirmed",
                    self.binary.display()
                ),
                false,
            )
        })?;
        Ok(())
    }

    pub(crate) async fn get_item(&self, item_id: &str) -> Result<Value, AppError> {
        let output = self.output(&["get", item_id]).await?;
        if !output.status.success() {
            let vault = self.credential_source().await.describe();
            return Err(invalid_item(format!(
                "Skarbiec item '{item_id}' could not be read from {vault} ({}): {}",
                output.status,
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
                    "Skarbiec refused to delete item '{item_id}' ({}): {}",
                    output.status,
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
    ) -> Result<super::local::Reply, AppError> {
        if let Some(path) = &self.local {
            return super::local::run(path, arguments, None);
        }
        self.run(&self.binary, arguments).await.map(super::local::Reply::from)
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
                // The process may have started: the failure can come from
                // starting it or from collecting its output and exit status.
                AppError::dependency(
                    "SKARBIEC_UNAVAILABLE",
                    format!(
                        "{} {} did not run to an observed exit: {error}",
                        program.display(),
                        arguments.join(" ")
                    ),
                    true,
                )
            })
    }
}
