//! The main window: made here rather than from the config, so its WebView keeps
//! its files in the data folder and the UI starts with what it remembers; and
//! put back where, and as big as, it was closed (docs/decisions/0019-data-folder.md).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{App, PhysicalPosition, PhysicalSize, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::data::{self, DataFolder};

const LABEL: &str = "main";
const PLACE_FILE: &str = "window-state.json";

/// Where the window was. The names are those `tauri-plugin-window-state` wrote,
/// so a file an earlier version kept still reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
    /// The place before the last move: maximizing moves the window before it
    /// says it is maximized, so the place to restore to is the one before.
    #[serde(default)]
    pub prev_x: i32,
    #[serde(default)]
    pub prev_y: i32,
}

impl Place {
    /// The place to restore to: before the maximizing move when it was maximized.
    pub fn restore_at(&self) -> (i32, i32) {
        if self.maximized {
            (self.prev_x, self.prev_y)
        } else {
            (self.x, self.y)
        }
    }
}

/// Makes the main window, hidden, puts it back in its place, then shows it.
/// It shows even when its place cannot be read, so it is never left hidden.
pub fn open(app: &App, folder: &DataFolder, webview: Option<PathBuf>, import_local_storage: bool) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == LABEL)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let mut builder = WebviewWindowBuilder::from_config(app.handle(), &config)?
        .initialization_script(folder.startup_script(import_local_storage));
    if let Some(webview) = webview {
        builder = builder.data_directory(webview);
    }
    let window = builder.build()?;
    let path = folder.file(PLACE_FILE);
    let place = read_place(&path);
    if let Some(place) = place {
        let _ = put_back(&window, &place);
    }
    track(&window, place.unwrap_or_else(|| current_place(&window)), path);
    window.show()?;
    window.set_focus()
}

fn read_place(path: &Path) -> Option<Place> {
    let file: std::collections::HashMap<String, Place> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    file.get(LABEL)
        .copied()
        .filter(|place| place.width > 0 && place.height > 0)
}

fn write_place(path: &Path, place: &Place) {
    if let Ok(bytes) = serde_json::to_vec_pretty(&serde_json::json!({ LABEL: place })) {
        let _ = data::write_atomic(path, &bytes);
    }
}

/// A place on a screen no longer there is left to Windows; the size still comes back.
fn put_back(window: &WebviewWindow, place: &Place) -> tauri::Result<()> {
    let (x, y) = place.restore_at();
    let size = PhysicalSize::new(place.width, place.height);
    if window
        .available_monitors()?
        .iter()
        .any(|monitor| on_screen(monitor.position(), monitor.size(), x, y, size))
    {
        window.set_position(PhysicalPosition::new(x, y))?;
    }
    window.set_size(size)?;
    if place.maximized {
        window.maximize()?;
    }
    Ok(())
}

/// Whether any corner of the window falls on the screen.
pub fn on_screen(
    screen: &PhysicalPosition<i32>,
    screen_size: &PhysicalSize<u32>,
    x: i32,
    y: i32,
    size: PhysicalSize<u32>,
) -> bool {
    let (left, top) = (screen.x, screen.y);
    let (right, bottom) = (
        left.saturating_add_unsigned(screen_size.width),
        top.saturating_add_unsigned(screen_size.height),
    );
    let (far_x, far_y) = (
        x.saturating_add_unsigned(size.width),
        y.saturating_add_unsigned(size.height),
    );
    [(x, y), (far_x, y), (x, far_y), (far_x, far_y)]
        .into_iter()
        .any(|(corner_x, corner_y)| (left..right).contains(&corner_x) && (top..bottom).contains(&corner_y))
}

fn current_place(window: &WebviewWindow) -> Place {
    let position = window.outer_position().unwrap_or_default();
    let size = window.inner_size().unwrap_or_default();
    Place {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        prev_x: position.x,
        prev_y: position.y,
        maximized: false,
    }
}

/// Follows moves and resizes while the window is neither minimized nor
/// maximized, and writes the place when the window is asked to close.
fn track(window: &WebviewWindow, start: Place, path: PathBuf) {
    let place = Arc::new(Mutex::new(start));
    let watched = window.clone();
    window.on_window_event(move |event| {
        let mut place = place.lock().unwrap_or_else(PoisonError::into_inner);
        match event {
            WindowEvent::Moved(position) if !watched.is_minimized().unwrap_or_default() => {
                place.prev_x = place.x;
                place.prev_y = place.y;
                place.x = position.x;
                place.y = position.y;
            }
            WindowEvent::Resized(size)
                if size.width > 0
                    && size.height > 0
                    && !watched.is_minimized().unwrap_or_default()
                    && !watched.is_maximized().unwrap_or_default() =>
            {
                place.width = size.width;
                place.height = size.height;
            }
            WindowEvent::CloseRequested { .. } => {
                place.maximized = watched.is_maximized().unwrap_or_default();
                write_place(&path, &place);
            }
            _ => {}
        }
    });
}
