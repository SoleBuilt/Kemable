//! Export as a job (`docs/decisions/0018-export-import-generate.md`): the
//! statement streams into a sink that writes each batch of rows to a `.part`
//! file beside the target and forgets it. Finished, the file takes its name;
//! cancelled or failed, it is deleted, so a stopped export leaves nothing on
//! disk and holds no rows in memory.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use kemable_core::catalog::{ColumnMeta, TableRef};
use kemable_core::connection::Connection;
use kemable_core::export::{ExportOptions, ExportWriter};
use kemable_core::sink::ResultSink;
use kemable_core::sql::SqlGrammar;
use kemable_core::value::Value;
use kemable_core::{CallOptions, DriverError, ErrorCode};
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::jobs::JobEvent;
use crate::state::AppState;

/// Progress reaches the UI at most this often.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

struct FileSink {
    writer: Mutex<ExportWriter>,
    file: Mutex<BufWriter<File>>,
    channel: Channel<JobEvent>,
    last_sent: Mutex<Option<Instant>>,
    /// The first write that failed; the driver is told to stop.
    failed: Mutex<Option<std::io::Error>>,
    started: AtomicBool,
}

impl FileSink {
    fn write(&self, bytes: &[u8]) -> bool {
        if self.failed.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
            return false;
        }
        match self
            .file
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write_all(bytes)
        {
            Ok(()) => true,
            Err(error) => {
                *self.failed.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
                false
            }
        }
    }
}

impl ResultSink for FileSink {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        // A table's SELECT makes one set; a second one, if any, is not part of the table.
        if self.started.swap(true, Ordering::Relaxed) {
            return;
        }
        let mut out = Vec::new();
        self.writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .start(columns.into_iter().map(|column| column.name).collect(), &mut out);
        self.write(&out);
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        let mut out = Vec::with_capacity(values.len() * 16);
        let rows = {
            let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            writer.rows(values, &mut out);
            writer.rows_written()
        };
        if !self.write(&out) {
            return false;
        }
        let mut last = self.last_sent.lock().unwrap_or_else(PoisonError::into_inner);
        if last.is_none_or(|sent| sent.elapsed() >= PROGRESS_INTERVAL) {
            *last = Some(Instant::now());
            let _ = self.channel.send(JobEvent::Progress { rows });
        }
        true
    }

    fn end_set(&self) {}
    fn command_done(&self, _rows_affected: Option<u64>) {}
    fn notice(&self, _text: &str) {}
}

/// Where the rows go until the export ends.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(".part");
    path.with_file_name(name)
}

pub struct Export {
    pub id: u64,
    pub session: Arc<dyn Connection>,
    pub statement: String,
    pub grammar: SqlGrammar,
    pub table: TableRef,
    pub options: ExportOptions,
    pub path: PathBuf,
    pub cancel: CancellationToken,
    pub channel: Channel<JobEvent>,
}

/// Runs `export` to its end and says how it ended on its channel.
pub async fn run(state: Arc<AppState>, export: Export) {
    let channel = export.channel.clone();
    let part = part_path(&export.path);
    let event = match write_file(&export, &part).await {
        Ok(rows) => match std::fs::rename(&part, &export.path) {
            Ok(()) => JobEvent::Finished {
                rows,
                path: Some(export.path.to_string_lossy().into_owned()),
            },
            Err(error) => {
                let _ = std::fs::remove_file(&part);
                JobEvent::Failed {
                    error: DriverError::new(
                        ErrorCode::Io,
                        format!("cannot replace {}: {error}", export.path.display()),
                    ),
                }
            }
        },
        Err(error) => {
            // Nothing half written stays on disk.
            let _ = std::fs::remove_file(&part);
            if export.cancel.is_cancelled() || error.code == ErrorCode::Cancelled {
                JobEvent::Cancelled
            } else {
                JobEvent::Failed { error }
            }
        }
    };
    state.remove_run(export.id);
    let _ = channel.send(event);
}

async fn write_file(export: &Export, part: &Path) -> Result<u64, DriverError> {
    let file = File::create(part)
        .map_err(|error| DriverError::new(ErrorCode::Io, format!("cannot write {}: {error}", part.display())))?;
    let sink = Arc::new(FileSink {
        writer: Mutex::new(ExportWriter::new(
            export.options.clone(),
            export.grammar,
            export.table.clone(),
        )),
        file: Mutex::new(BufWriter::with_capacity(1 << 20, file)),
        channel: export.channel.clone(),
        last_sent: Mutex::new(None),
        failed: Mutex::new(None),
        started: AtomicBool::new(false),
    });
    let shared: Arc<dyn ResultSink> = sink.clone();
    export
        .session
        .execute(&export.statement, shared, CallOptions::new(export.cancel.child_token()))
        .await?;
    if let Some(error) = sink.failed.lock().unwrap_or_else(PoisonError::into_inner).take() {
        return Err(DriverError::new(
            ErrorCode::Io,
            format!("cannot write {}: {error}", part.display()),
        ));
    }
    if export.cancel.is_cancelled() {
        return Err(DriverError::cancelled());
    }
    let mut out = Vec::new();
    let rows = {
        let mut writer = sink.writer.lock().unwrap_or_else(PoisonError::into_inner);
        if !sink.started.load(Ordering::Relaxed) {
            writer.start(Vec::new(), &mut out);
        }
        writer.finish(&mut out);
        writer.rows_written()
    };
    let mut file = sink.file.lock().unwrap_or_else(PoisonError::into_inner);
    file.write_all(&out)
        .and_then(|()| file.flush())
        .map_err(|error| DriverError::new(ErrorCode::Io, format!("cannot write {}: {error}", part.display())))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_go_to_a_part_file_beside_the_target() {
        assert_eq!(
            part_path(Path::new("C:/data/khách hàng.csv")),
            PathBuf::from("C:/data/khách hàng.csv.part")
        );
    }
}
