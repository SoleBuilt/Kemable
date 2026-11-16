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
/// The WebView's own files (cache, cookies), in this Windows user's own local folder.
const WEBVIEW: &str = "webview";
/// The connections whose pages are kept; the least recently written go first,
/// except pages with text not saved anywhere else, which are never dropped.
const MAX_PAGE_FILES: usize = 40;
/// The page key of `.sql` files opened with no connection (`NO_CONNECTION` in `pages.ts`).
const NO_CONNECTION: &str = "none";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataFolder {
    pub path: PathBuf,
    /// The folder is `data` beside the app, where the installer put it.
    pub beside_app: bool,
    /// This Windows user's own `%LOCALAPPDATA%\<identifier>`, never shared with
    /// another account, even when `path` is an install for everyone.
    #[serde(skip)]
    pub local: PathBuf,
    /// This Windows user's own `%APPDATA%\<identifier>`: what decides where a secret
    /// may go stays here, out of reach of the PC's other users
    /// (docs/decisions/0033-bound-passwords.md).
    #[serde(skip)]
    pub own: PathBuf,
}

impl DataFolder {
    /// `data` beside the running app when it is there and this user can write it,
    /// else `fallback`: a development build, or an app copied without its folder.
    /// `local` is the user's own local folder.
    pub fn find(fallback: PathBuf, local: PathBuf) -> Self {
        match beside_app() {
            Some(beside) if writable(&beside) => Self {
                path: beside,
                beside_app: true,
                local,
                own: fallback,
            },
            _ => Self {
                path: fallback.clone(),
                beside_app: false,
                local,
                own: fallback,
            },
        }
    }

    /// The SSH host keys Kemable trusts: each Windows user's own, never in a
    /// folder the PC's other users may change (docs/decisions/0033-bound-passwords.md).
    pub fn known_hosts(&self) -> PathBuf {
        self.own.join("known_hosts")
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// The WebView's folder is each Windows user's own: two accounts cannot open one
    /// WebView folder at once, and another account must not plant what this one's
    /// WebView loads.
    pub fn webview(&self) -> PathBuf {
        self.local.join(WEBVIEW)
    }

    /// The preferences the UI wrote, or None before it ever did. A file that
    /// cannot be understood is set aside, never written over, and the error says where.
    pub fn prefs(&self) -> Result<Option<Json>, DriverError> {
        read_or_set_aside(&self.file(PREFS))
    }

    pub fn write_prefs(&self, prefs: &Json) -> std::io::Result<()> {
        write_atomic(&self.file(PREFS), &serde_json::to_vec(prefs)?)
    }

    /// The pages kept for `key`, as the UI wrote them. A file that cannot be
    /// understood is set aside, never written over, and the error says where.
    pub fn pages(&self, key: &str) -> Result<Option<Json>, DriverError> {
        let Some(mut file) = read_or_set_aside(&self.page_file(key))? else {
            return Ok(None);
        };
        // Two keys could share a file name; the key inside says whose it is.
        Ok((file.get("key").and_then(Json::as_str) == Some(key)).then(|| file["kept"].take()))
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

    /// Past the limit, drops the oldest connections' pages that hold no text kept
    /// nowhere else; pages with such text stay, whatever their number.
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
        files.sort_by_key(|(modified, _)| *modified);
        let mut over = files.len() - MAX_PAGE_FILES;
        for (_, path) in files {
            if over == 0 {
                break;
            }
            // A file that cannot be read is not judged: it stays.
            let unsaved = read_json(&path).is_none_or(|file| holds_unsaved_text(&file["kept"]));
            if !unsaved && fs::remove_file(&path).is_ok() {
                over -= 1;
            }
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
        let prefs = self.prefs();
        let pages = self.pages(NO_CONNECTION);
        // What was set aside is said once the window shows (ui `setAsideAtStart`).
        let set_aside: Vec<&str> = [prefs.as_ref().err(), pages.as_ref().err()]
            .into_iter()
            .flatten()
            .map(|error| error.message.as_str())
            .collect();
        let startup = json!({
            "prefs": prefs.as_ref().ok().cloned().flatten(),
            "pagesWithout": pages.as_ref().ok().cloned().flatten(),
            "folder": self,
            "setAside": set_aside,
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

/// Reads a kept file. One that is there but is not JSON is renamed beside itself
/// (`<name>.unreadable-<ms>`), so the next write cannot replace what it held, and
/// the error names both.
fn read_or_set_aside(path: &Path) -> Result<Option<Json>, DriverError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(DriverError::new(
                ErrorCode::Unreadable,
                format!("cannot read {}: {error}", path.display()),
            ));
        }
    };
    let error = match serde_json::from_slice(&bytes) {
        Ok(json) => return Ok(Some(json)),
        Err(error) => error,
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let aside = path.with_file_name(format!("{name}.unreadable-{stamp}"));
    let kept = match fs::rename(path, &aside) {
        Ok(()) => format!("kept as {}", aside.display()),
        Err(rename) => format!("could not be set aside: {rename}"),
    };
    Err(DriverError::new(
        ErrorCode::Unreadable,
        format!("cannot understand {}: {error}; {kept}", path.display()),
    ))
}

/// Whether kept pages hold text found nowhere else: a query page with text, or a
/// `.sql` file's page with changes not in its file.
fn holds_unsaved_text(kept: &Json) -> bool {
    kept["pages"].as_array().is_some_and(|pages| {
        pages.iter().any(|page| match page.get("file") {
            Some(file) if !file.is_null() => file["dirty"].as_bool().unwrap_or(true),
            _ => page["text"].as_str().is_some_and(|text| !text.trim().is_empty()),
        })
    })
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
