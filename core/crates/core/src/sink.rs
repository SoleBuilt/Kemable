use crate::catalog::ColumnMeta;
use crate::value::Value;

/// Where a driver writes what a statement returns, as it reads it.
///
/// A statement that returns rows calls `begin_set`, then `push_rows` any
/// number of times, then `end_set`. A statement that returns no rows calls
/// `command_done`. The sink is shared with blocking threads, so it takes
/// `&self` and does its own locking.
pub trait ResultSink: Send + Sync {
    fn begin_set(&self, columns: Vec<ColumnMeta>);

    /// `values` holds whole rows, row after row, each as many values as the
    /// set has columns. Returns `false` once the sink holds every row it will
    /// take, and the driver stops reading.
    fn push_rows(&self, values: &[Value]) -> bool;

    fn end_set(&self);

    fn command_done(&self, rows_affected: Option<u64>);

    /// A message the server sent beside the result, such as a PostgreSQL
    /// NOTICE or an MSSQL PRINT.
    fn notice(&self, text: &str);
}

/// How many values a driver gathers before it calls `push_rows`: big enough
/// that the sink's lock is rare, small enough that the first rows show at once.
pub const PUSH_BATCH_ROWS: usize = 512;

/// What a statement left behind once it ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecOutcome {
    /// Whether the driver stopped because the sink would take no more rows.
    pub limited: bool,
}
