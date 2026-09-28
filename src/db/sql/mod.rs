//! Skrzynka's row operations against the fleet database. `Database::lock`
//! hands out a `Session` over the one Postgres client; `Session::transaction`
//! a `Tx`. Both answer `execute`, `query_row` and `prepare(..).query_map`.
//!
//! The Postgres client blocks on its own small runtime, which Tokio refuses
//! inside a worker thread, so every call runs through `blocking`: on a worker
//! it leaves the runtime for the call (`block_in_place`, the service runs the
//! multi-threaded runtime); on a blocking-pool thread or outside Tokio it
//! simply runs.

pub mod fleet;
mod row;

use std::cell::RefCell;
use std::fmt;
use std::sync::MutexGuard;

use postgres::error::SqlState;
use postgres::types::ToSql;
use postgres::{Client, Transaction};

pub use row::Row;

/// One bound parameter of a statement.
pub type Value<'a> = &'a (dyn ToSql + Sync);

#[derive(Debug)]
pub enum Error {
    /// A statement that must answer one row answered none.
    NoRows,
    Postgres(postgres::Error),
    /// A stored value does not fit the type Skrzynka reads it as.
    Conversion { column: usize, detail: String },
}

impl Error {
    /// The insert hit a unique constraint: the row already exists.
    pub fn is_unique_violation(&self) -> bool {
        matches!(self, Self::Postgres(error) if error.code() == Some(&SqlState::UNIQUE_VIOLATION))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRows => formatter.write_str("the fleet database answered no row"),
            Self::Postgres(error) => write!(formatter, "the fleet database refused: {error}"),
            Self::Conversion { column, detail } => {
                write!(formatter, "column {column} holds a value Skrzynka cannot read: {detail}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Postgres(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for Error {
    fn from(error: postgres::Error) -> Self {
        Self::Postgres(error)
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Turns "no row" into `None` for statements whose row may be absent.
pub trait OptionalExtension<T> {
    fn optional(self) -> Result<Option<T>>;
}

impl<T> OptionalExtension<T> for Result<T> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(Error::NoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// Parameters of mixed types, built by `params!`.
pub struct Values<'a>(pub Vec<Value<'a>>);

/// What a statement can be bound with: `params![...]` or an array of one type.
pub trait Params {
    fn values(&self) -> Vec<Value<'_>>;
}

impl Params for Values<'_> {
    fn values(&self) -> Vec<Value<'_>> {
        self.0.clone()
    }
}

impl<T: ToSql + Sync, const N: usize> Params for [T; N] {
    fn values(&self) -> Vec<Value<'_>> {
        self.iter().map(|value| value as Value<'_>).collect()
    }
}

/// Binds values of mixed types: `params![id, name, now]`.
macro_rules! params {
    ($($value:expr),* $(,)?) => {
        $crate::db::sql::Values(vec![$(&$value as &(dyn ::postgres::types::ToSql + Sync)),*])
    };
}
pub(crate) use params;

/// Run one blocking database call where Tokio allows it.
pub fn blocking<R>(call: impl FnOnce() -> R) -> R {
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(call)
    } else {
        call()
    }
}

/// The two places a statement runs: the client itself, or a transaction on it.
pub trait Run {
    fn rows(&self, sql: &str, values: &[Value<'_>]) -> Result<Vec<postgres::Row>>;
    fn exec(&self, sql: &str, values: &[Value<'_>]) -> Result<u64>;
}

/// The client, held for as long as one operation needs it.
pub struct Session<'a> {
    client: RefCell<MutexGuard<'a, Client>>,
}

impl<'a> Session<'a> {
    pub fn new(client: MutexGuard<'a, Client>) -> Self {
        Self {
            client: RefCell::new(client),
        }
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        let mut client = self.client.borrow_mut();
        Ok(blocking(|| client.batch_execute(sql))?)
    }

    pub fn transaction(&mut self) -> Result<Tx<'_>> {
        let client: &mut Client = self.client.get_mut();
        let transaction = blocking(|| client.transaction())?;
        Ok(Tx {
            transaction: RefCell::new(Some(transaction)),
        })
    }
}

impl Run for Session<'_> {
    fn rows(&self, sql: &str, values: &[Value<'_>]) -> Result<Vec<postgres::Row>> {
        let mut client = self.client.borrow_mut();
        Ok(blocking(|| client.query(sql, values))?)
    }

    fn exec(&self, sql: &str, values: &[Value<'_>]) -> Result<u64> {
        let mut client = self.client.borrow_mut();
        Ok(blocking(|| client.execute(sql, values))?)
    }
}

/// A transaction: committed by `commit`, rolled back when dropped without it.
pub struct Tx<'a> {
    transaction: RefCell<Option<Transaction<'a>>>,
}

impl Tx<'_> {
    pub fn commit(self) -> Result<()> {
        let transaction = self.transaction.borrow_mut().take();
        match transaction {
            Some(transaction) => Ok(blocking(|| transaction.commit())?),
            None => Ok(()),
        }
    }

    fn with<R>(&self, call: impl FnOnce(&mut Transaction<'_>) -> Result<R>) -> Result<R> {
        let mut held = self.transaction.borrow_mut();
        let transaction = held.as_mut().ok_or(Error::NoRows)?;
        blocking(|| call(transaction))
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if let Some(transaction) = self.transaction.get_mut().take() {
            blocking(move || drop(transaction));
        }
    }
}

impl Run for Tx<'_> {
    fn rows(&self, sql: &str, values: &[Value<'_>]) -> Result<Vec<postgres::Row>> {
        self.with(|transaction| Ok(transaction.query(sql, values)?))
    }

    fn exec(&self, sql: &str, values: &[Value<'_>]) -> Result<u64> {
        self.with(|transaction| Ok(transaction.execute(sql, values)?))
    }
}

macro_rules! statements {
    ($runner:ident) => {
        impl $runner<'_> {
            /// Rows changed.
            pub fn execute(&self, sql: &str, params: impl Params) -> Result<usize> {
                let changed = self.exec(sql, &params.values())?;
                Ok(usize::try_from(changed).unwrap_or(usize::MAX))
            }

            /// The first row the statement answers, mapped; `Error::NoRows` if none.
            pub fn query_row<T, F>(&self, sql: &str, params: impl Params, map: F) -> Result<T>
            where
                F: FnOnce(&Row<'_>) -> Result<T>,
            {
                let rows = self.rows(sql, &params.values())?;
                let first = rows.first().ok_or(Error::NoRows)?;
                map(&Row::new(first))
            }

            pub fn prepare(&self, sql: &str) -> Result<Statement<'_, Self>> {
                Ok(Statement {
                    runner: self,
                    sql: sql.to_owned(),
                })
            }
        }
    };
}
statements!(Session);
statements!(Tx);

/// A statement answering many rows.
pub struct Statement<'r, R> {
    runner: &'r R,
    sql: String,
}

impl<R: Run> Statement<'_, R> {
    pub fn query_map<T, F>(
        &mut self,
        params: impl Params,
        mut map: F,
    ) -> Result<std::vec::IntoIter<Result<T>>>
    where
        F: FnMut(&Row<'_>) -> Result<T>,
    {
        let rows = self.runner.rows(&self.sql, &params.values())?;
        let mapped: Vec<Result<T>> = rows.iter().map(|row| map(&Row::new(row))).collect();
        Ok(mapped.into_iter())
    }
}
