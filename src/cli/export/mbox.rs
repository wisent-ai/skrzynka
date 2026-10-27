//! One mbox file of an export's originals, in the mboxrd form mail programs
//! import (Apple Mail, Thunderbird, mutt): each message after a `From `
//! line, its lines ending in LF, every line that reads `From ` after any
//! number of `>` given one more `>`, and a blank line after it.

use crate::error::AppError;
use chrono::DateTime;

/// The bytes of one message as an mbox entry, its `From ` line dated with
/// `received_at` (the RFC 3339 instant Skrzynka stored).
pub(super) fn entry(source: &[u8], received_at: &str, message_id: uuid::Uuid) -> Result<Vec<u8>, AppError> {
    let received = DateTime::parse_from_rfc3339(received_at).map_err(|error| {
        AppError::internal(format!(
            "message {message_id} stores receive instant {received_at:?}, which is not RFC 3339: {error}"
        ))
    })?;
    let mut written = format!("From MAILER-DAEMON {}\n", received.format("%a %b %e %H:%M:%S %Y")).into_bytes();
    for line in source.split(|byte| *byte == b'\n') {
        let line = match line.strip_suffix(b"\r") {
            Some(bare) => bare,
            None => line,
        };
        let quotes = line.iter().take_while(|byte| **byte == b'>').count();
        if line[quotes..].starts_with(b"From ") {
            written.push(b'>');
        }
        written.extend_from_slice(line);
        written.push(b'\n');
    }
    written.push(b'\n');
    Ok(written)
}
