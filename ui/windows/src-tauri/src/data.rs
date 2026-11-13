//! Where the app keeps what it remembers (docs/decisions/0019-data-folder.md):
//! one folder, `data` beside the app when the installer made it, else the
//! user's `%APPDATA%\<identifier>`. The UI's preferences and each connection's
//! query pages are small files in it, read when they are needed and written
//! whole through a temporary file, so a crash never leaves half a file.

use std::fs;
use std::path::{Path, PathBuf};

use kemable_core::{DriverError, ErrorCode};
use serde::Serialize;
use serde_json::{Value as Json, json};

/// What an older version kept in `%APPDATA%`, copied once into a new `data` folder.
const CARRIED: &[&str] = &[
    "connections.json",
    "profiles.json",
    "queries.json",
    "history.db",
    "history.db-wal",
    "history.db-shm",
    "window-state.json",
];
/// The UI's preferences: settings, recent files, sizes it remembers.
const PREFS: &str = "ui.json";
/// One file per connection's query pages.
const PAGES: &str = "pages";
/// The WebView's own files (cache, cookies), out of the user's profile too.
const WEBVIEW: &str = "webview";
/// The connections whose pages are kept; the least recently written go first.
const MAX_PAGE_FILES: usize = 40;
/// The page key of `.sql` files opened with no connection (`NO_CONNECTION` in `pages.ts`).
const NO_CONNECTION: &str = "none";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataFolder {
    pub path: PathBuf,
    /// The folder is `data` beside the app, where the installer put it.
    pub beside_app: bool,
}

impl DataFolder {
    /// `data` beside the running app when it is there and this user can write it,
    /// else `fallback`: a development build, or an app copied without its folder.
    pub fn find(fallback: PathBuf) -> Self {
        match beside_app() {
            Some(beside) if writable(&beside) => Self {
                path: beside,
                beside_app: true,
            },
            _ => Self {
                path: fallback,
                beside_app: false,
            },
        }
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    pub fn webview(&self) -> PathBuf {
        self.path.join(WEBVIEW)
    }

    /// The preferences the UI wrote, or None before it ever did.
    pub fn prefs(&self) -> Option<Json> {
        read_json(&self.file(PREFS))
    }

    pub fn write_prefs(&self, prefs: &Json) -> std::io::Result<()> {
        write_atomic(&self.file(PREFS), &serde_json::to_vec(prefs)?)
    }

    /// The pages kept for `key`, as the UI wrote them.
    pub fn pages(&self, key: &str) -> Option<Json> {
        let mut file = read_json(&self.page_file(key))?;
        // Two keys could share a file name; the key inside says whose it is.
        (file.get("key").and_then(Json::as_str) == Some(key)).then(|| file["kept"].take())
    }

    pub fn write_pages(&self, key: &str, kept: Json) -> std::io::Result<()> {
        let file = json!({ "key": key, "kept": kept });
        write_atomic(&self.page_file(key), &serde_json::to_vec(&file)?)?;
        self.forget_old_pages();
        Ok(())
    }

    fn page_file(&self, key: &str) -> PathBuf {
        self.path.join(PAGES).join(format!("{:016x}.json", fnv1a(key)))
    }

    fn forget_old_pages(&self) {
        let Ok(entries) = fs::read_dir(self.path.join(PAGES)) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        if files.len() <= MAX_PAGE_FILES {
            return;
        }
        files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        for (_, path) in files.drain(MAX_PAGE_FILES..) {
            let _ = fs::remove_file(path);
        }
    }

    /// Copies what an older version kept in `old` the first time this folder is
    /// used, so moving into the install folder loses nothing. `old` is left as it was.
    pub fn carry_over(&self, old: &Path) {
        if !self.beside_app || old == self.path || CARRIED.iter().any(|name| self.file(name).exists()) {
            return;
        }
        for name in CARRIED {
            let from = old.join(name);
            if from.is_file() {
                let _ = fs::copy(&from, self.file(name));
            }
        }
    }

    /// The script the window runs before the UI's own: the preferences and the
    /// pages of files opened with no connection, so the first paint has them.
    pub fn startup_script(&self) -> String {
        let startup = json!({
            "prefs": self.prefs(),
            "pagesWithout": self.pages(NO_CONNECTION),
            "folder": self,
        });
        format!("window.__KEMABLE__ = {startup};")
    }
}

fn beside_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let folder = exe.parent()?.join("data");
    folder.is_dir().then_some(folder)
}

/// An installer run for every user may leave the folder read-only for this one.
fn writable(folder: &Path) -> bool {
    let probe = folder.join(".kemable-write-test");
    let written = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    written
}

fn read_json(path: &Path) -> Option<Json> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// A short, stable name for a page key, which may be a long file path.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn io_error(what: &str, error: impl std::fmt::Display) -> DriverError {
    DriverError::new(ErrorCode::Io, format!("{what}: {error}"))
}
