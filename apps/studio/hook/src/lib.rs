//! The event log (#328, module 2): one append-only JSON Lines file, the only source of session state.

use std::path::{Path, PathBuf};

/// The log in the studio's data directory, which is Tauri's app data directory for the identifier
/// in `src-tauri/tauri.conf.json`. The hook writes it; the app reads it.
pub fn event_log(home: &Path) -> PathBuf {
    home.join("Library/Application Support/dev.falinks.studio/events.jsonl")
}
