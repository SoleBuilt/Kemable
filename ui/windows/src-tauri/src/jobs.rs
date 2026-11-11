//! Import and Generate Rows as jobs (`docs/decisions/0018-export-import-generate.md`):
//! `INSERT` statements of a batch of rows each, made one at a time and run
//! in one transaction, so a cancel or a failure rolls every row back. Events
//! say how far it got; `cancel_run` stops it.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use paboable_core::catalog::{ColumnInfo, ColumnMeta, TableRef};
use paboable_core::connection::Connection;
use paboable_core::generate;
use paboable_core::import::{
    CsvReader, NewColumn, ROWS_PER_INSERT, ValueShape, create_statement, detect_delimiter, field_literal, infer_shape,
    insert_statement, match_columns, transaction,
};
use paboable_core::sink::ResultSink;
use paboable_core::sql::SqlGrammar;
use paboable_core::text_file;
use paboable_core::value::Value;
use paboable_core::{CallOptions, DriverError, ErrorCode};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::state::AppState;

/// Progress reaches the UI at most this often.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// A CSV file bigger than this is not read: it would sit whole in memory.
const MAX_CSV_BYTES: u64 = 512 * 1024 * 1024;
/// One statement of a job may take this long before it is stopped.
const STATEMENT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// What a job (export, import, generate) reports.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum JobEvent {
    Progress {
        rows: u64,
    },
    /// `path` is the file an export wrote.
    Finished {
        rows: u64,
        path: Option<String>,
    },
    Cancelled,
    Failed {
        error: DriverError,
    },
}

/// A sink for statements whose rows nobody reads.
pub struct Discard;

impl ResultSink for Discard {
    fn begin_set(&self, _columns: Vec<ColumnMeta>) {}
    fn push_rows(&self, _values: &[Value]) -> bool {
        false
    }
    fn end_set(&self) {}
    fn command_done(&self, _rows_affected: Option<u64>) {}
    fn notice(&self, _text: &str) {}
}

/// The first value of the first row, for `SELECT MAX(...)`.
#[derive(Default)]
struct Scalar(Mutex<Option<Value>>);

impl ResultSink for Scalar {
    fn begin_set(&self, _columns: Vec<ColumnMeta>) {}
    fn push_rows(&self, values: &[Value]) -> bool {
        let mut first = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if first.is_none() {
            *first = values.first().cloned();
        }
        false
    }
    fn end_set(&self) {}
    fn command_done(&self, _rows_affected: Option<u64>) {}
    fn notice(&self, _text: &str) {}
}

async fn execute(
    session: &Arc<dyn Connection>,
    statement: &str,
    cancel: &CancellationToken,
) -> Result<(), DriverError> {
    let sink: Arc<dyn ResultSink> = Arc::new(Discard);
    session
        .execute(
            statement,
            sink,
            CallOptions::new(cancel.child_token()).with_timeout(STATEMENT_TIMEOUT),
        )
        .await
        .map(drop)
}

/// Runs `statements` (each with the rows it inserts) in one transaction when
/// `transactional`; resolves to the rows inserted.
async fn run_batches(
    session: &Arc<dyn Connection>,
    grammar: SqlGrammar,
    transactional: bool,
    statements: impl Iterator<Item = Result<(String, u64), DriverError>>,
    cancel: &CancellationToken,
    channel: &Channel<JobEvent>,
) -> Result<u64, DriverError> {
    let (begin, commit, rollback) = transaction(grammar);
    if transactional {
        execute(session, begin, cancel).await?;
    }
    let mut rows = 0u64;
    let mut sent = Instant::now();
    let outcome: Result<(), DriverError> = async {
        for statement in statements {
            if cancel.is_cancelled() {
                return Err(DriverError::cancelled());
            }
            let (sql, count) = statement?;
            execute(session, &sql, cancel).await?;
            rows += count;
            if sent.elapsed() >= PROGRESS_INTERVAL {
                sent = Instant::now();
                let _ = channel.send(JobEvent::Progress { rows });
            }
        }
        Ok(())
    }
    .await;
    match outcome {
        Ok(()) => {
            if transactional {
                execute(session, commit, cancel).await?;
            }
            Ok(rows)
        }
        Err(error) => {
            if transactional {
                // The rollback runs even when the cancel fired, so it gets a token of its own.
                let _ = execute(session, rollback, &CancellationToken::new()).await;
            }
            Err(error)
        }
    }
}

/// Says how a job ended, and forgets its cancel.
pub fn report(
    state: &AppState,
    id: u64,
    cancel: &CancellationToken,
    channel: &Channel<JobEvent>,
    outcome: Result<u64, DriverError>,
) {
    let event = match outcome {
        Ok(rows) => JobEvent::Finished { rows, path: None },
        Err(_) if cancel.is_cancelled() => JobEvent::Cancelled,
        Err(error) if error.code == ErrorCode::Cancelled => JobEvent::Cancelled,
        Err(error) => JobEvent::Failed { error },
    };
    state.remove_run(id);
    let _ = channel.send(event);
}

// --- Import ---------------------------------------------------------------------

/// A CSV file's text, read whole: what Import parses.
pub fn read_csv(path: &str) -> Result<String, DriverError> {
    let size = std::fs::metadata(path)
        .map_err(|error| DriverError::new(ErrorCode::Io, format!("cannot read {path}: {error}")))?
        .len();
    if size > MAX_CSV_BYTES {
        return Err(DriverError::unsupported(format!(
            "{path} is {size} bytes; files up to {MAX_CSV_BYTES} import"
        )));
    }
    let bytes = std::fs::read(Path::new(path))
        .map_err(|error| DriverError::new(ErrorCode::Io, format!("cannot read {path}: {error}")))?;
    Ok(text_file::decode(&bytes)?.text)
}

/// The first lines of a CSV file, its separator, and the shape of each field's values.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvPreview {
    pub delimiter: char,
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub shapes: Vec<ValueShape>,
    /// The type a new table's column takes for each field, in the connection's engine.
    pub types: Vec<String>,
    /// Records in the file after the header.
    pub records: u64,
}

/// Reads `text`'s header (or names the fields `column1`, … when it has none),
/// its first 20 rows, and the shapes of its first 1,000.
pub fn preview(text: &str, delimiter: Option<char>, header: bool, grammar: SqlGrammar) -> CsvPreview {
    let delimiter = delimiter.unwrap_or_else(|| detect_delimiter(text));
    let mut reader = CsvReader::new(text, delimiter);
    let first = if header {
        reader.next().unwrap_or_default()
    } else {
        Vec::new()
    };
    let sample: Vec<Vec<String>> = reader.by_ref().take(1000).collect();
    let records = sample.len() as u64 + reader.count() as u64;
    let width = sample.iter().map(Vec::len).chain([first.len()]).max().unwrap_or(0);
    let names = (0..width)
        .map(|index| {
            first
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("column{}", index + 1))
        })
        .collect();
    let shapes: Vec<ValueShape> = (0..width)
        .map(|index| infer_shape(sample.iter().filter_map(|row| row.get(index).map(String::as_str))))
        .collect();
    let types = shapes
        .iter()
        .map(|shape| paboable_core::import::column_type(grammar, *shape).to_owned())
        .collect();
    CsvPreview {
        types,
        delimiter,
        header: names,
        rows: sample.into_iter().take(20).collect(),
        shapes,
        records,
    }
}

/// Where Import puts the rows.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ImportTarget {
    Existing {
        table: TableRef,
    },
    /// A table made first, with the columns (names and types) the dialog shows.
    New {
        table: TableRef,
        columns: Vec<NewColumn>,
    },
}

impl ImportTarget {
    pub fn table(&self) -> &TableRef {
        match self {
            Self::Existing { table } | Self::New { table, .. } => table,
        }
    }
}

/// How Import reads the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvOptions {
    pub delimiter: char,
    pub header: bool,
    pub empty_is_null: bool,
}

/// What an import will do, shown before it runs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    /// For each field, the column it goes to; None for a field left out.
    pub mapping: Vec<Option<String>>,
    pub create: Option<String>,
    /// The first INSERT, with up to five rows.
    pub first_insert: Option<String>,
    pub records: u64,
}

/// The fields and the columns they go to, with each column's type.
pub fn import_columns(
    target: &ImportTarget,
    header: &[String],
    columns: &[ColumnInfo],
) -> Vec<Option<(String, String)>> {
    match target {
        ImportTarget::New { columns, .. } => header
            .iter()
            .enumerate()
            .map(|(index, _)| {
                columns
                    .get(index)
                    .map(|column| (column.name.clone(), column.type_name.clone()))
            })
            .collect(),
        ImportTarget::Existing { .. } => {
            let matched = match_columns(header, columns);
            matched
                .into_iter()
                .map(|found| {
                    found
                        .and_then(|index| columns.get(index))
                        .map(|column| (column.name.clone(), column.type_name.clone()))
                })
                .collect()
        }
    }
}

/// The INSERT statements for `records`, `ROWS_PER_INSERT` rows each, made as they are needed.
pub fn insert_batches<'a>(
    grammar: SqlGrammar,
    table: &'a TableRef,
    targets: &'a [Option<(String, String)>],
    records: impl Iterator<Item = Vec<String>> + 'a,
    empty_is_null: bool,
    rows_per_insert: usize,
) -> impl Iterator<Item = Result<(String, u64), DriverError>> + 'a {
    let names: Vec<String> = targets.iter().flatten().map(|(name, _)| name.clone()).collect();
    let mut records = records.peekable();
    std::iter::from_fn(move || {
        records.peek()?;
        let batch: Vec<Vec<String>> = records
            .by_ref()
            .take(rows_per_insert)
            .map(|record| {
                targets
                    .iter()
                    .enumerate()
                    .filter_map(|(index, target)| {
                        target.as_ref().map(|(_, type_name)| {
                            field_literal(
                                grammar,
                                type_name,
                                record.get(index).map_or("", String::as_str),
                                empty_is_null,
                            )
                        })
                    })
                    .collect()
            })
            .collect();
        let count = batch.len() as u64;
        Some(Ok((insert_statement(grammar, table, &names, &batch), count)))
    })
}

pub struct ImportJob {
    pub id: u64,
    pub session: Arc<dyn Connection>,
    pub grammar: SqlGrammar,
    pub transactional: bool,
    pub text: String,
    pub options: CsvOptions,
    pub target: ImportTarget,
    pub columns: Vec<ColumnInfo>,
    pub cancel: CancellationToken,
    pub channel: Channel<JobEvent>,
}

pub async fn run_import(state: Arc<AppState>, job: ImportJob) {
    let outcome = import(&job).await;
    report(&state, job.id, &job.cancel, &job.channel, outcome);
}

async fn import(job: &ImportJob) -> Result<u64, DriverError> {
    let mut reader = CsvReader::new(&job.text, job.options.delimiter);
    let header = if job.options.header {
        reader.next().unwrap_or_default()
    } else {
        let width = CsvReader::new(&job.text, job.options.delimiter)
            .next()
            .map_or(0, |row| row.len());
        (0..width).map(|index| format!("column{}", index + 1)).collect()
    };
    let targets = import_columns(&job.target, &header, &job.columns);
    if targets.iter().all(Option::is_none) {
        return Err(DriverError::config(
            "no field of the file matches a column of the table",
        ));
    }
    let table = job.target.table().clone();
    let create = match &job.target {
        ImportTarget::New { table, columns } => Some(Ok((create_statement(job.grammar, table, columns)?, 0))),
        ImportTarget::Existing { .. } => None,
    };
    let inserts = insert_batches(
        job.grammar,
        &table,
        &targets,
        reader,
        job.options.empty_is_null,
        ROWS_PER_INSERT,
    );
    run_batches(
        &job.session,
        job.grammar,
        job.transactional,
        create.into_iter().chain(inserts),
        &job.cancel,
        &job.channel,
    )
    .await
}

// --- Generate Rows -------------------------------------------------------------------

/// What Generate Rows will do, shown before it runs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratePlan {
    /// The columns the rows fill; the server fills the others.
    pub columns: Vec<String>,
    /// The integer key the rows count up, from `key_start`.
    pub key: Option<String>,
    pub key_start: i64,
    /// The first INSERT, with up to five rows.
    pub preview: String,
}

/// The first free number of `key` in `table`: one past its largest.
pub async fn key_start(
    session: &Arc<dyn Connection>,
    grammar: SqlGrammar,
    table: &TableRef,
    key: &str,
) -> Result<i64, DriverError> {
    let sql = format!(
        "SELECT MAX({}) FROM {}",
        paboable_core::sql::quote_name(grammar, key),
        paboable_core::sql::table_name(grammar, table)
    );
    let sink = Arc::new(Scalar::default());
    let shared: Arc<dyn ResultSink> = sink.clone();
    session
        .execute(
            &sql,
            shared,
            CallOptions::default().with_timeout(Duration::from_secs(60)),
        )
        .await?;
    let max = sink.0.lock().unwrap_or_else(PoisonError::into_inner).take();
    Ok(match max {
        Some(Value::Int(value)) => value.saturating_add(1),
        Some(Value::UInt(value)) => i64::try_from(value).unwrap_or(i64::MAX).saturating_add(1),
        Some(Value::Decimal(text)) => text
            .split('.')
            .next()
            .and_then(|whole| whole.parse::<i64>().ok())
            .map_or(1, |value| value + 1),
        _ => 1,
    })
}

pub fn plan(grammar: SqlGrammar, table: &TableRef, columns: &[ColumnInfo], key_start: i64, seed: u64) -> GeneratePlan {
    let filled = generate::filled_columns(columns);
    let key = generate::counted_key(columns).map(|column| column.name.clone());
    let names: Vec<String> = filled.iter().map(|column| column.name.clone()).collect();
    let rows = generate::rows(grammar, &filled, key.as_deref(), key_start, seed, 0, 5);
    GeneratePlan {
        preview: insert_statement(grammar, table, &names, &rows),
        columns: names,
        key,
        key_start,
    }
}

pub struct GenerateJob {
    pub id: u64,
    pub session: Arc<dyn Connection>,
    pub grammar: SqlGrammar,
    pub transactional: bool,
    pub table: TableRef,
    pub columns: Vec<ColumnInfo>,
    pub count: u64,
    pub seed: u64,
    pub key_start: i64,
    pub cancel: CancellationToken,
    pub channel: Channel<JobEvent>,
}

pub async fn run_generate(state: Arc<AppState>, job: GenerateJob) {
    let outcome = generate_rows(&job).await;
    report(&state, job.id, &job.cancel, &job.channel, outcome);
}

async fn generate_rows(job: &GenerateJob) -> Result<u64, DriverError> {
    let filled = generate::filled_columns(&job.columns);
    if filled.is_empty() {
        return Err(DriverError::config("the server fills every column of this table"));
    }
    let key = generate::counted_key(&job.columns).map(|column| column.name.clone());
    let names: Vec<String> = filled.iter().map(|column| column.name.clone()).collect();
    let step = ROWS_PER_INSERT as u64;
    let batches = (0..job.count.div_ceil(step)).map(|batch| {
        let first = batch * step;
        let count = step.min(job.count - first);
        let rows = generate::rows(
            job.grammar,
            &filled,
            key.as_deref(),
            job.key_start,
            job.seed,
            first,
            count,
        );
        Ok((insert_statement(job.grammar, &job.table, &names, &rows), count))
    });
    run_batches(
        &job.session,
        job.grammar,
        job.transactional,
        batches,
        &job.cancel,
        &job.channel,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_names_headerless_fields_and_counts_the_records() {
        let text = "1;An;2026-01-02\n2;Bình;2026-01-03\n3;Chi;\n";
        let read = preview(text, None, false, SqlGrammar::Postgres);
        assert_eq!(read.delimiter, ';');
        assert_eq!(read.header, ["column1", "column2", "column3"]);
        assert_eq!(read.records, 3);
        assert_eq!(read.shapes, [ValueShape::Integer, ValueShape::Text, ValueShape::Date]);
        assert_eq!(read.types, ["bigint", "text", "date"]);
    }

    #[test]
    fn inserts_come_in_batches_with_only_the_matched_fields() {
        let table = TableRef {
            schema: None,
            name: "t".into(),
        };
        let targets = vec![
            Some(("a".to_owned(), "integer".to_owned())),
            None,
            Some(("c".to_owned(), "text".to_owned())),
        ];
        let records = (1..=5).map(|n| vec![n.to_string(), "skip".to_owned(), String::new()]);
        let batches: Vec<(String, u64)> = insert_batches(SqlGrammar::Postgres, &table, &targets, records, true, 2)
            .map(Result::unwrap)
            .collect();
        assert_eq!(batches.iter().map(|(_, count)| *count).collect::<Vec<_>>(), [2, 2, 1]);
        assert_eq!(
            batches[0].0,
            "INSERT INTO \"t\" (\"a\", \"c\") VALUES\n(1, NULL),\n(2, NULL)"
        );
    }
}
