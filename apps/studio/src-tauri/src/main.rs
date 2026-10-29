//! The studio app's shell (#328, module 9). At start it replays the whole event log through the
//! core, then feeds it each new line live. The pane shows the session Daniel prompted last; a session
//! that newly needs Daniel gets a macOS notification while the window is not focused.

use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
    sync::Mutex,
    thread,
    time::Duration,
};
use studio_core::{Output, Session, Studio};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

#[tauri::command]
fn now(studio: State<Mutex<Studio>>) -> Option<Session> {
    studio.lock().unwrap().prompted_last().cloned()
}

fn main() {
    let log = studio_hook::event_log(&std::env::home_dir().expect("a home directory"));
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(Mutex::new(Studio::default()))
        .invoke_handler(tauri::generate_handler![now])
        .setup(move |app| {
            let app = app.handle().clone();
            thread::spawn(move || follow(&app, &log));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run the studio");
}

/// Reads the log from its start, then from where it stopped. A line counts once it ends in a newline,
/// so a line the hook is still writing waits for the next read. The lines present at start are
/// replayed; later ones are live.
// shortcut: polls every 200 ms; switch to file-system events if the delay or the wake-ups matter.
fn follow(app: &AppHandle, log: &Path) {
    let mut read_to = 0;
    let mut live = false;
    loop {
        if let Ok(mut file) = File::open(log)
            && file.seek(SeekFrom::Start(read_to)).is_ok()
        {
            let mut reader = BufReader::new(file);
            let mut line = Vec::new();
            let read_from = read_to;
            let mut needs_you = Vec::new();
            let state = app.state::<Mutex<Studio>>();
            let mut studio = state.lock().unwrap();
            while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 && line.ends_with(b"\n") {
                read_to += line.len() as u64;
                let event = String::from_utf8_lossy(&line);
                if !live {
                    studio.replay(&event);
                } else {
                    for output in studio.apply(&event) {
                        match output {
                            Output::NeedsYou { session_id } => {
                                needs_you.push(studio.session(&session_id).cloned())
                            }
                        }
                    }
                }
                line.clear();
            }
            drop(studio);
            if read_to > read_from {
                let _ = app.emit("now", now(state));
            }
            for session in needs_you {
                notify(app, session);
            }
        }
        live = true;
        thread::sleep(Duration::from_millis(200));
    }
}

/// One notification per notice, only while the window is not focused (#328, module 9). Called
/// without the core's lock, because asking for focus waits on the main thread, which runs `now`.
fn notify(app: &AppHandle, session: Option<Session>) {
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok());
    if focused == Some(true) {
        return;
    }
    let folder = session.map(|s| s.cwd.rsplit('/').next().unwrap_or_default().to_string());
    let _ = app
        .notification()
        .builder()
        .title("Needs you")
        .body(folder.unwrap_or_default())
        .show();
}
