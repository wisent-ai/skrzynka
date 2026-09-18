//! The Skarbiec CLI transport: get and set one item, with bounded output and a timeout.

use super::{invalid_item, validate_item_id, SkarbiecResolver, MAX_SKARBIEC_RESPONSE_BYTES, SKARBIEC_COMMAND_TIMEOUT_SECONDS};
use crate::error::AppError;
use serde_json::Value;
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

impl SkarbiecResolver {
    pub(super) async fn set_item(&self, item_id: &str, kind: &str, payload: &Value) -> Result<(), AppError> {
        validate_item_id(item_id)?;
        let bytes = serde_json::to_vec(payload)
            .map_err(|_| AppError::internal("Gmail credential payload could not be encoded"))?;
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
        let output = tokio::time::timeout(
            Duration::from_secs(SKARBIEC_COMMAND_TIMEOUT_SECONDS),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            AppError::dependency(
                "SKARBIEC_TIMEOUT",
                "Skarbiec did not finish within 15 seconds",
                true,
            )
        })?
        .map_err(|_| {
            AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                "Skarbiec did not persist Gmail authorization",
                false,
            )
        })?;
        if !output.status.success() {
            return Err(AppError::dependency(
                "SKARBIEC_WRITE_FAILED",
                "Skarbiec rejected the Gmail authorization item",
                false,
            ));
        }
        Ok(())
    }

    pub(super) async fn get_item(&self, item_id: &str) -> Result<Value, AppError> {
        let output = self.output(&["get", item_id]).await?;
        if !output.status.success() {
            return Err(invalid_item(
                "selected Skarbiec item is missing, unreadable, or unavailable",
            ));
        }
        bounded_stdout(&output.stdout)?;
        serde_json::from_slice(&output.stdout).map_err(|_| {
            AppError::dependency(
                "SKARBIEC_RESPONSE_INVALID",
                "Skarbiec returned invalid item JSON",
                false,
            )
        })
    }

    pub(super) async fn output(&self, arguments: &[&str]) -> Result<std::process::Output, AppError> {
        let mut command = Command::new(&self.binary);
        command.args(arguments);
        command.kill_on_drop(true);
        tokio::time::timeout(
            Duration::from_secs(SKARBIEC_COMMAND_TIMEOUT_SECONDS),
            command.output(),
        )
        .await
        .map_err(|_| {
            AppError::dependency(
                "SKARBIEC_TIMEOUT",
                "Skarbiec did not finish within 15 seconds",
                true,
            )
        })?
        .map_err(|_| {
            AppError::dependency(
                "SKARBIEC_UNAVAILABLE",
                "Skarbiec could not be started from the configured path",
                true,
            )
        })
    }
}

pub(super) fn bounded_stdout(stdout: &[u8]) -> Result<(), AppError> {
    if stdout.len() > MAX_SKARBIEC_RESPONSE_BYTES {
        return Err(AppError::dependency(
            "SKARBIEC_RESPONSE_TOO_LARGE",
            "Skarbiec response exceeded the 2 MiB safety limit",
            false,
        ));
    }
    Ok(())
}
