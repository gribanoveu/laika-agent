//! The IPC boundary: thin `#[tauri::command]` functions, and the adapters that
//! turn the core's reports into Tauri events.
//!
//! Nothing here decides anything. A command validates what came across the
//! wire, calls one service, and flattens the error to a string — the one place
//! stringly-typed errors are the right answer.

pub mod chat;
pub mod chat_events;
pub mod settings;
pub mod chat_history;
pub mod workspace_events;
