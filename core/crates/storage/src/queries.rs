//! Saved queries: named SQL kept in `queries.json` beside the connections,
//! written the same way (whole, through a temporary file, never over a file
//! that cannot be read). A query belongs to one saved connection, or to all
//! when `connection_id` is `None`.

use std::path::{Path, PathBuf};

use paboable_core::{DriverError, ErrorCode, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedQuery {
    /// Empty for a query not saved yet; `save` gives it one.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub sql: String,
    /// The saved connection it belongs to; `None` shows it for every connection.
    #[serde(default)]
    pub connection_id: Option<String>,
    /// Milliseconds since the Unix epoch.
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct QueriesFile {
    version: u32,
    queries: Vec<SavedQuery>,
}

pub struct SavedQueries {
    path: PathBuf,
}

fn io_error(what: &str, path: &Path, error: impl std::fmt::Display) -> DriverError {
    DriverError::new(ErrorCode::Io, format!("{what} {}: {error}", path.display()))
}

impl SavedQueries {
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            path: folder.join("queries.json"),
        }
    }

    fn read(&self) -> Result<QueriesFile> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|error| io_error("cannot understand", &self.path, error))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(QueriesFile {
                version: 1,
                queries: Vec::new(),
            }),
            Err(error) => Err(io_error("cannot read", &self.path, error)),
        }
    }

    fn write(&self, file: &QueriesFile) -> Result<()> {
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| io_error("cannot create", folder, error))?;
        }
        let text = serde_json::to_vec_pretty(file).map_err(|error| DriverError::internal(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(|error| io_error("cannot write", &temporary, error))?;
        std::fs::rename(&temporary, &self.path).map_err(|error| io_error("cannot replace", &self.path, error))
    }

    /// The queries `connection_id` shows: its own and the global ones, by name.
    pub fn list(&self, connection_id: Option<&str>) -> Result<Vec<SavedQuery>> {
        let mut queries: Vec<SavedQuery> = self
            .read()?
            .queries
            .into_iter()
            .filter(|query| query.connection_id.is_none() || query.connection_id.as_deref() == connection_id)
            .collect();
        queries.sort_by_key(|query| query.name.to_lowercase());
        Ok(queries)
    }

    pub fn save(&self, mut query: SavedQuery, now: i64) -> Result<SavedQuery> {
        if query.name.trim().is_empty() {
            return Err(DriverError::config("a saved query needs a name"));
        }
        if query.sql.trim().is_empty() {
            return Err(DriverError::config("a saved query needs some SQL"));
        }
        let mut file = self.read()?;
        if query.id.is_empty() {
            query.id = uuid::Uuid::new_v4().to_string();
        }
        query.updated_at = now;
        match file.queries.iter_mut().find(|saved| saved.id == query.id) {
            Some(saved) => *saved = query.clone(),
            None => file.queries.push(query.clone()),
        }
        file.version = 1;
        self.write(&file)?;
        Ok(query)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut file = self.read()?;
        file.queries.retain(|query| query.id != id);
        self.write(&file)
    }
}
