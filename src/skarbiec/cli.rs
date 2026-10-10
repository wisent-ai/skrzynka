//! The Skarbiec CLI transport: get and set one item.
//!
//! A read is answered by the vault this machine resolves to. On the vault
//! owner that is the local `skarbiec`; on any other machine the local
//! `skarbiec` holds no vault and refuses every read, so the read goes
//! through Stado (`stado credentials get <item>`, `stado credentials ls
//! --json`), which reaches the fleet vault over the service directory. The
//! answer is read exactly as Skarbiec's. A refusal carries the vault's own
//! sentence, never a generic "missing, unreadable, or unavailable", and
//! every child process is waited on through `stado_wait`, which says on
//! stderr what is waited for and since when.

use super::{invalid_item, validate_item_id, SkarbiecResolver};
use crate::error::AppError;
use serde_json::Value;
use std::process::Stdio;
use tokio::{io::AsyncWriteExt, process::Command};

/// The program that reads the fleet vault from a machine that holds none.
const STADO: &str = "stado";

impl SkarbiecResolver {
    pub(super) async fn set_item(
        &self,
        item_id: &str,
        kind: &str,
        payload: &Value,
    ) -> Result<(), AppError> {
        validate_item_id(item_id)?;
        let bytes = serde_json::to_vec(payload)
            .map_err(|_| AppError::internal("Gmail credential payload could not be encoded"))?;
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
        let mut child = command.spawn().map_err(|_| {
            AppError::dependency(
                "SKARBIEC_UNAVAILABLE",
                "Skarbiec could not be started from the configured path",
                true,
            )
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::internal("Skarbiec input pipe was unavailable"))?;
        stdin.write_all(&bytes).await.map_err(|_| {
            AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                "Gmail authorization could not be sent to Skarbiec",
                false,
            )
        })?;
        drop(stdin);
        let output = stado_wait::child_output_async(child, format!("skarbiec set-json {item_id} --type {kind}"))
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
        serde_json::from_slice(&output.stdout).map_err(|_| {
            AppError::dependency(
                "SKARBIEC_RESPONSE_INVALID",
                "Skarbiec returned invalid item JSON",
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

    /// One Skarbiec invocation, answered by the credentials file when this
    /// machine names one, else by the local `skarbiec`; a read (`get`, `list`)
    /// the local `skarbiec` refuses because this machine holds no vault is
    /// answered by Stado from the fleet vault, and a refusal from there is
    /// Stado's own beside the local vault's.
    pub(super) async fn output(
        &self,
        arguments: &[&str],
    ) -> Result<std::process::Output, AppError> {
        if let Some(path) = &self.local {
            return super::local::run(path, arguments, None);
        }
        let local = self.run(&self.binary, arguments).await?;
        if local.status.success() || !self.reads_through_stado().await {
            return Ok(local);
        }
        let through_stado: Vec<&str> = match arguments {
            ["get", item] => vec!["credentials", "get", item],
            ["list"] => vec!["credentials", "ls", "--json"],
            _ => return Ok(local),
        };
        let fleet = self.run(std::path::Path::new(STADO), &through_stado).await?;
        if fleet.status.success() {
            return Ok(fleet);
        }
        Ok(std::process::Output {
            status: fleet.status,
            stdout: fleet.stdout,
            stderr: format!(
                "this machine holds no Skarbiec vault ({}) and the fleet vault refused through stado {}: {}",
                String::from_utf8_lossy(&local.stderr).trim(),
                through_stado.join(" "),
                String::from_utf8_lossy(&fleet.stderr).trim()
            )
            .into_bytes(),
        })
    }

    /// Whether this machine holds no vault of its own: the local `skarbiec
    /// status` refuses then, and every read belongs to the fleet vault.
    async fn reads_through_stado(&self) -> bool {
        self.run(&self.binary, &["status"])
            .await
            .is_ok_and(|status| !status.status.success())
    }

    async fn run(
        &self,
        program: &std::path::Path,
        arguments: &[&str],
    ) -> Result<std::process::Output, AppError> {
        let mut command = Command::new(program);
        command.args(arguments);
        command.kill_on_drop(true);
        stado_wait::output_async(&mut command).await.map_err(|error| {
            AppError::dependency(
                "SKARBIEC_UNAVAILABLE",
                format!("{} could not be started: {error}", program.display()),
                true,
            )
        })
    }
}
