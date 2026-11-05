//! Answering from the card (#461, decision 16): the shell's side of the answer hook's files
//! (`studio_hook::answer`). The window calls `watch` every second while a session's view shows, which
//! keeps the watching file's heartbeat fresh and returns that session's open request; `answer` writes
//! the reply that echoes the request's nonce.

use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use studio_hook::answer;

fn data() -> Result<PathBuf, String> {
    let log = studio_hook::event_log(&std::env::home_dir().ok_or("no home directory")?);
    Ok(log.parent().ok_or("no data directory")?.to_owned())
}

/// The window shows `session`'s view, or none (`None`, also while the window is hidden). Returns the
/// session's open request, if any.
#[tauri::command]
pub fn watch(session: Option<String>) -> Result<Option<Value>, String> {
    let data = data()?;
    let watching = answer::watching_file(&data);
    let Some(session) = session.filter(|s| answer::plain(s)) else {
        let _ = fs::remove_file(watching);
        return Ok(None);
    };
    let heartbeat = json!({ "session_id": session, "pid": std::process::id(), "heartbeat_ms": answer::now_ms() });
    answer::write_private(&watching, &heartbeat).map_err(|e| e.to_string())?;
    let ask = fs::read_to_string(answer::ask_file(&data, &session)).ok();
    Ok(ask.and_then(|text| serde_json::from_str(&text).ok()))
}

/// Daniel answered `session`'s request `nonce` from its card: `answers` maps each question to a label,
/// or a multi-select's to its labels; `None` answers in the terminal.
#[tauri::command]
pub fn answer(session: String, nonce: String, answers: Option<Value>) -> Result<(), String> {
    if !answer::plain(&session) {
        return Err("not a session id".into());
    }
    let mut reply = json!({ "nonce": nonce });
    if let Some(answers) = answers {
        reply["answers"] = answers;
    }
    answer::write_private(&answer::answer_file(&data()?, &session), &reply)
        .map_err(|e| e.to_string())
}
