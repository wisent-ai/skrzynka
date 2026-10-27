//! A file a command saves bytes to: it must not exist yet, it is written
//! whole or removed, and every refusal names the path and carries the
//! command's own codes.

use crate::error::AppError;
use std::{
    fs::OpenOptions,
    io::{ErrorKind, Write},
    path::Path,
};

/// Where a save writes, with the codes its refusals carry: `exists` for a
/// path already taken, `invalid` for one that cannot be checked or written.
pub(super) struct NewFile<'a> {
    pub(super) path: &'a Path,
    pub(super) exists: &'static str,
    pub(super) invalid: &'static str,
}

impl NewFile<'_> {
    /// Refuse a path that is already taken, before anything is read.
    pub(super) fn refuse_occupied(&self) -> Result<(), AppError> {
        let occupied = self.path.try_exists().map_err(|error| {
            AppError::invalid(
                self.invalid,
                format!("{} could not be checked: {error}", self.path.display()),
            )
        })?;
        if occupied {
            return Err(self.occupied());
        }
        Ok(())
    }

    /// Create the file and write `bytes` to disk. A file that cannot be
    /// written whole is removed, and a removal that fails is said too.
    pub(super) fn write(&self, bytes: &[u8]) -> Result<(), AppError> {
        let path = self.path;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| match error.kind() {
                ErrorKind::AlreadyExists => self.occupied(),
                _ => AppError::invalid(
                    self.invalid,
                    format!("{} could not be created: {error}", path.display()),
                ),
            })?;
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        if let Err(error) = written {
            drop(file);
            let removal = match std::fs::remove_file(path) {
                Ok(()) => "the partial file was removed".to_string(),
                Err(removal) => format!("the partial file could not be removed: {removal}"),
            };
            return Err(AppError::invalid(
                self.invalid,
                format!(
                    "{} could not be written: {error}; {removal}",
                    path.display()
                ),
            ));
        }
        Ok(())
    }

    fn occupied(&self) -> AppError {
        AppError::invalid(
            self.exists,
            format!(
                "{} already exists; saving never overwrites a file: name a path that does not exist",
                self.path.display()
            ),
        )
    }
}
