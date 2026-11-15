//! Update in place (docs/decisions/0028-updates.md): the UI asks once at launch,
//! or from Settings, whether GitHub Releases has a newer version; this side
//! fetches `latest.json`, downloads the installer it names with its progress
//! coalesced, checks its signature and hands it to Windows. When and whether to
//! ask, Later and Skip are the UI's.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use kemable_core::{DriverError, ErrorCode};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Error, Update, UpdaterExt};

use crate::window::KeptPlace;

/// How long the check, and the download between two chunks, may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Download progress reaches the UI at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(50);

/// The update the last check found, which Update installs.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

/// A newer version than this one, as `latest.json` announces it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// What changed, as the release's notes say it (Markdown, shown as text).
    pub notes: String,
    /// When it was published, as `latest.json` writes it (RFC 3339).
    pub date: Option<String>,
    /// A critical fix: the app is not used until it is installed.
    pub critical: bool,
}

/// How much of the installer has arrived.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// Whether a release marks itself critical: `"critical": true` beside the
/// version in `latest.json`, which the release workflow writes from the
/// notes' front matter.
pub fn is_critical(raw: &serde_json::Value) -> bool {
    raw.get("critical").and_then(serde_json::Value::as_bool) == Some(true)
}

fn info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone().unwrap_or_default().trim().to_owned(),
        date: update
            .raw_json
            .get("pub_date")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        critical: is_critical(&update.raw_json),
    }
}

fn update_error(error: &Error) -> DriverError {
    let code = match error {
        Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) | Error::MissingSignedVersion => {
            ErrorCode::UpdateSignature
        }
        Error::Reqwest(inner) if inner.is_timeout() => ErrorCode::Timeout,
        Error::Reqwest(_) | Error::Network(_) | Error::ReleaseNotFound => ErrorCode::Connect,
        Error::Io(_) => ErrorCode::Io,
        _ => ErrorCode::Internal,
    };
    DriverError::new(code, error.to_string())
}

/// Asks GitHub Releases for a newer version. Nothing newer is `None`; a
/// network that is down, or a release the app cannot read, is an error the UI
/// keeps quiet at launch.
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateInfo>, DriverError> {
    let updater = app
        .updater_builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|error| update_error(&error))?;
    let found = updater.check().await.map_err(|error| update_error(&error))?;
    let reply = found.as_ref().map(info);
    *pending.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = found;
    Ok(reply)
}

/// Downloads the update the last check found, reporting its progress on
/// `channel`, checks its signature, and runs its installer, passive. The
/// installer ends the app, keeps `data`, and starts the new version; this
/// command only returns when something failed. The UI has written what it
/// keeps before it asks.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
    channel: Channel<UpdateProgress>,
) -> Result<(), DriverError> {
    let update = pending
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or_else(|| DriverError::not_found("no update was found to install"))?;
    let mut downloaded = 0u64;
    let mut last_sent: Option<Instant> = None;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if last_sent.is_none_or(|sent| sent.elapsed() >= PROGRESS_EVERY) {
                    last_sent = Some(Instant::now());
                    let _ = channel.send(UpdateProgress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(|error| update_error(&error))?;
    let _ = channel.send(UpdateProgress {
        downloaded,
        total: Some(downloaded),
    });
    // The installer ends the process without closing the window, so its place is written now.
    if let Some(place) = app.try_state::<KeptPlace>() {
        place.keep();
    }
    update.install(bytes).map_err(|error| update_error(&error))
}

#[cfg(test)]
mod tests {
    use super::is_critical;

    #[test]
    fn only_a_true_critical_field_blocks_the_app() {
        assert!(is_critical(
            &serde_json::json!({ "version": "0.2.0", "critical": true })
        ));
        assert!(!is_critical(
            &serde_json::json!({ "version": "0.2.0", "critical": false })
        ));
        assert!(!is_critical(
            &serde_json::json!({ "version": "0.2.0", "critical": "true" })
        ));
        assert!(!is_critical(&serde_json::json!({ "version": "0.2.0" })));
    }
}
