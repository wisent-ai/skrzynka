//! The stored form of a `Date` header a message states but that does not
//! read as an instant: the text column `sent_at_unreadable`, holding the
//! header and why it was not read as one JSON object. NULL when the message's
//! `Date` read as an instant or the message states none.

use super::super::sql::{self, Row};
use serde::{de::DeserializeOwned, Serialize};

const COLUMN: &str = "sent_at_unreadable";

/// The column's value for `record`, the message's unreadable `Date` if any.
pub(super) fn stored<T: Serialize>(record: Option<&T>) -> sql::Result<Option<String>> {
    match record {
        Some(record) => serde_json::to_string(record).map(Some).map_err(|error| {
            sql::Error::Conversion(format!("{COLUMN} could not be written as JSON: {error}"))
        }),
        None => Ok(None),
    }
}

/// The unreadable `Date` a stored row holds; a value that is not such a
/// record is refused naming the column and the text it holds.
pub(super) fn read<T: DeserializeOwned>(row: &Row<'_>) -> sql::Result<Option<T>> {
    match row.get::<_, Option<String>>(COLUMN)? {
        Some(text) => serde_json::from_str(&text).map(Some).map_err(|error| {
            sql::Error::Conversion(format!(
                "{COLUMN} holds {text:?}, which is not an unreadable Date record: {error}"
            ))
        }),
        None => Ok(None),
    }
}
