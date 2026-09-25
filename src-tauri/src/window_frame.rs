//! The window's size and position on macOS, kept in points.
//!
//! tauri-plugin-window-state keeps them in pixels, and tao turns pixels back
//! into points with the scale of the display the window was *created* on — the
//! main one. A window left on a 1× monitor beside a 2× built-in one came back
//! at half its size and half its offset, every launch, and below the minimum
//! size too: macOS holds a programmatic resize to no minimum. Points are one
//! space across every display on macOS, so they come back as they were.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{LogicalPosition, LogicalSize, Manager, Runtime, WebviewWindow, Window};

const FILE: &str = "window-frame.json";

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn path<R: Runtime, M: Manager<R>>(app: &M) -> tauri::Result<PathBuf> {
    Ok(app.path().app_config_dir()?.join(FILE))
}

/// Writes the frame, unless the window is not showing its own: minimized,
/// maximized and fullscreen are the plugin's to bring back.
pub fn save<R: Runtime>(window: &Window<R>) -> Result<(), String> {
    if window.is_minimized().unwrap_or(true)
        || window.is_maximized().unwrap_or(true)
        || window.is_fullscreen().unwrap_or(true)
    {
        return Ok(());
    }
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let position: LogicalPosition<f64> = window.outer_position().map_err(|e| e.to_string())?.to_logical(scale);
    let size: LogicalSize<f64> = window.inner_size().map_err(|e| e.to_string())?.to_logical(scale);
    let frame = Frame { x: position.x, y: position.y, width: size.width, height: size.height };
    let path = path(window).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec(&frame).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Puts the window where it was. Nothing saved, or a file that does not read:
/// the config's size stays. A position on no display still attached is left
/// to the system; the size is kept.
pub fn restore<R: Runtime>(window: &WebviewWindow<R>) {
    if window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return;
    }
    let Some(frame) = path(window)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<Frame>(&bytes).ok())
    else {
        return;
    };
    let on_screen = window.available_monitors().unwrap_or_default().iter().any(|m| {
        let origin: LogicalPosition<f64> = m.position().to_logical(m.scale_factor());
        let size: LogicalSize<f64> = m.size().to_logical(m.scale_factor());
        frame.x < origin.x + size.width
            && frame.x + frame.width > origin.x
            && frame.y < origin.y + size.height
            && frame.y + frame.height > origin.y
    });
    if on_screen {
        let _ = window.set_position(LogicalPosition::new(frame.x, frame.y));
    }
    let _ = window.set_size(LogicalSize::new(frame.width, frame.height));
}

