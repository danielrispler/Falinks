//! The studio app's shell (#328, module 9). At start it replays the whole event log through the
//! core, then feeds it each new line live and hands the core's card requests to the card writer. The
//! pane shows the session Daniel prompted last and its cards.

use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
    sync::Mutex,
    thread,
    time::Duration,
};
use studio_core::{CardRequest, Output, Session, Studio};
use studio_understanding::{Card, CardWriter, FakeWriter};
use tauri::{AppHandle, Emitter, Manager, State};

/// The written cards, oldest first, each with the step it explains.
// shortcut: kept in memory, so a restart loses them; the card store (#345) keeps them on disk.
type Cards = Mutex<Vec<(CardRequest, Card)>>;

#[tauri::command]
fn now(studio: State<Mutex<Studio>>) -> Option<Session> {
    studio.lock().unwrap().prompted_last().cloned()
}

/// The cards of the session the pane follows.
#[tauri::command]
fn cards(studio: State<Mutex<Studio>>, cards: State<Cards>) -> Vec<(CardRequest, Card)> {
    let Some(followed) = studio.lock().unwrap().prompted_last().map(|s| s.id.clone()) else {
        return Vec::new();
    };
    cards
        .lock()
        .unwrap()
        .iter()
        .filter(|(request, _)| request.session_id == followed)
        .cloned()
        .collect()
}

fn main() {
    let log = studio_hook::event_log(&std::env::home_dir().expect("a home directory"));
    tauri::Builder::default()
        .manage(Mutex::new(Studio::default()))
        .manage(Cards::default())
        .invoke_handler(tauri::generate_handler![now, cards])
        .setup(move |app| {
            let app = app.handle().clone();
            thread::spawn(move || follow(&app, &log));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run the studio");
}

/// Reads the log from its start, then from where it stopped. A line counts once it ends in a newline,
/// so a line the hook is still writing waits for the next read. The lines there at start are
/// replayed, so steps that ran while the app was down get no cards.
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
            let mut outputs = Vec::new();
            let read_from = read_to;
            let state = app.state::<Mutex<Studio>>();
            let mut studio = state.lock().unwrap();
            while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 && line.ends_with(b"\n") {
                read_to += line.len() as u64;
                let text = String::from_utf8_lossy(&line);
                if live {
                    outputs.extend(studio.apply(&text));
                } else {
                    studio.replay(&text);
                }
                line.clear();
            }
            drop(studio);
            for output in outputs {
                match output {
                    // shortcut: writes on this thread, which suits the instant fake; the real writer
                    // (#342) takes seconds and needs its own thread.
                    Output::Card(request) => {
                        if let Some(card) = FakeWriter.write(&request) {
                            app.state::<Cards>().lock().unwrap().push((request, card));
                        }
                    }
                }
            }
            if read_to > read_from {
                let _ = app.emit("now", now(state));
                let _ = app.emit("cards", cards(app.state(), app.state()));
            }
        }
        live = true;
        thread::sleep(Duration::from_millis(200));
    }
}
