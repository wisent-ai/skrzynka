//! One row the fleet database answered, read by column position.

use std::fmt::Display;

use postgres::row::RowIndex;
use postgres::types::FromSql;

use super::Result;

pub struct Row<'a>(&'a postgres::Row);

impl<'a> Row<'a> {
    pub(super) fn new(row: &'a postgres::Row) -> Self {
        Self(row)
    }

    /// One column as `T`; a NULL reads as `None` when `T` is an `Option`.
    pub fn get<I, T>(&self, index: I) -> Result<T>
    where
        I: RowIndex + Display,
        T: FromSql<'a>,
    {
        Ok(self.0.try_get(index)?)
    }
}
