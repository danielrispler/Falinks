//! The answer hook (#461, decision 16): a synchronous hook that lets Daniel answer from the studio card.
//! While the studio watches the asking session, the hook leaves a request in `asks/<session>.json`
//! with a one-time nonce and waits for `asks/<session>.answer.json` echoing it. It returns no decision,
//! so the terminal asks as today, when nobody watches, the watching goes stale, Daniel answers in the
//! terminal, or 120 seconds pass.
//!
//! Files, all in the studio's data directory, written whole (temp file plus rename) with mode 0600:
//! - `watching.json`, by the studio every second: `{"session_id", "pid", "heartbeat_ms"}`.
//! - `asks/<session>.json`, by the hook: `{"nonce", "hook_event_name", "tool_name", "tool_use_id",
//!   "tool_input", "asked_ms"}`.
//! - `asks/<session>.answer.json`, by the studio: `{"nonce", "answers"}`, each answer a label or, for a
//!   multi-select, a list of labels; with no `answers`, Daniel answers in the terminal.
//!
//! Trust boundary: a process that could write the answer file could approve a call, so the hook takes
//! only a file of its own user with mode 0600 that echoes this request's nonce.

use serde_json::{Map, Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// A heartbeat older than this counts as nobody watching (decision 22).
pub const STALE_MS: u64 = 5_000;
/// With no answer by then, the terminal asks (decision 22).
pub const TIMEOUT_MS: u64 = 120_000;

pub fn watching_file(data: &Path) -> PathBuf {
    data.join("watching.json")
}

pub fn ask_file(data: &Path, session: &str) -> PathBuf {
    data.join("asks").join(format!("{session}.json"))
}

pub fn answer_file(data: &Path, session: &str) -> PathBuf {
    data.join("asks").join(format!("{session}.answer.json"))
}

/// Whether a session id may name a file: a plain one.
pub fn plain(session: &str) -> bool {
    let plain = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    !session.is_empty() && session.chars().all(plain)
}

pub fn now_ms() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    since.as_millis() as u64
}

/// Writes `value` to `path` whole, readable by its user only, so a reader never sees half a file.
pub fn write_private(path: &Path, value: &Value) -> io::Result<()> {
    let folder = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)?;
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    let temp = folder.join(format!(".{}.{}.tmp", name.display(), std::process::id()));
    // A new file, so the mode holds even if a crashed writer left its temp file.
    let _ = fs::remove_file(&temp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(value.to_string().as_bytes())?;
    fs::rename(&temp, path)
}

/// Whether the studio watches `session`: the watching file names it, its pid lives and its heartbeat
/// is fresh.
pub fn watched(data: &Path, session: &str) -> bool {
    let Ok(text) = fs::read_to_string(watching_file(data)) else {
        return false;
    };
    let watching: Value = serde_json::from_str(&text).unwrap_or_default();
    let pid = watching["pid"].as_i64().and_then(|p| i32::try_from(p).ok());
    let heartbeat = watching["heartbeat_ms"].as_u64().unwrap_or(0);
    watching["session_id"] == session
        && now_ms().saturating_sub(heartbeat) <= STALE_MS
        // Signal 0 only checks that the process exists and is the user's own.
        && pid.is_some_and(|pid| pid > 0 && unsafe { libc::kill(pid, 0) } == 0)
}

/// Whether the hook answers this call. #478 adds `ExitPlanMode` and `PermissionRequest` here and in
/// `reply`.
fn handled(payload: &Value) -> bool {
    payload["hook_event_name"] == "PreToolUse" && payload["tool_name"] == "AskUserQuestion"
}

/// The hook's output for the studio's `answer` to `payload`, or `None` for no decision.
pub fn reply(payload: &Value, answer: &Value) -> Option<Value> {
    let answers = answer.get("answers")?.as_object()?;
    let mut joined = Map::new();
    for (question, chosen) in answers {
        let label = match chosen {
            Value::String(label) => label.clone(),
            // A multi-select's labels, joined with commas as Claude Code does.
            Value::Array(labels) => {
                let labels: Option<Vec<&str>> = labels.iter().map(Value::as_str).collect();
                labels?.join(", ")
            }
            _ => return None,
        };
        joined.insert(question.clone(), label.into());
    }
    if joined.is_empty() {
        return None;
    }
    // `updatedInput` replaces the whole input, so it keeps the original questions.
    let mut input = payload["tool_input"].as_object()?.clone();
    input.insert("answers".into(), joined.into());
    Some(json!({ "hookSpecificOutput": {
        "hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": input } }))
}

/// Runs the hook for `payload` with the studio's data directory `data`: the output to print, if any.
pub fn run(payload: &Value, data: &Path, timeout: Duration) -> io::Result<Option<Value>> {
    let session = payload["session_id"].as_str().unwrap_or_default();
    if !handled(payload) || !plain(session) || !watched(data, session) {
        return Ok(None);
    }
    let nonce = nonce()?;
    let (ask, answer) = (ask_file(data, session), answer_file(data, session));
    let _ = fs::remove_file(&answer);
    let request = json!({
        "nonce": nonce,
        "hook_event_name": payload["hook_event_name"],
        "tool_name": payload["tool_name"],
        "tool_use_id": payload["tool_use_id"],
        "tool_input": payload["tool_input"],
        "asked_ms": now_ms(),
    });
    write_private(&ask, &request)?;
    let out = wait(payload, data, session, &nonce, timeout);
    // shortcut: one ask per session at a time; a second ask meanwhile replaces the request, so this
    // one only times out. Key the files by tool_use_id if parallel subagent questions show up.
    let mine = fs::read_to_string(&ask).is_ok_and(|text| text.contains(&nonce));
    if mine {
        let _ = fs::remove_file(&ask);
        let _ = fs::remove_file(&answer);
    }
    Ok(out)
}

fn wait(
    payload: &Value,
    data: &Path,
    session: &str,
    nonce: &str,
    timeout: Duration,
) -> Option<Value> {
    let until = Instant::now() + timeout;
    loop {
        if let Some(answer) = take(&answer_file(data, session)) {
            if answer["nonce"] == nonce {
                return reply(payload, &answer);
            }
            eprintln!("studio-answer: an answer with a wrong nonce was refused");
        }
        if !watched(data, session) || Instant::now() >= until {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// The answer file's content, removed as it is read; `None` when there is none, or when it is not the
/// user's own with mode 0600, which is refused.
fn take(path: &Path) -> Option<Value> {
    let meta = fs::symlink_metadata(path).ok()?;
    let text = fs::read_to_string(path);
    let _ = fs::remove_file(path);
    let own = meta.is_file() && meta.uid() == unsafe { libc::getuid() } && meta.mode() & 0o077 == 0;
    if !own {
        eprintln!("studio-answer: an answer file not private to its user was refused");
        return None;
    }
    serde_json::from_str(&text.ok()?).ok()
}

/// A one-time nonce: 16 random bytes in hex.
fn nonce() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
