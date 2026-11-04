//! The shell's terminal daemon client (#466). The window connects with `terminal_connect`, which
//! starts the daemon from beside the app's own executable when none is running (a Dock launch has
//! no useful `PATH`), then sends the daemon's requests with `terminal_send` and gets every daemon
//! event as a `terminal` event, `{"ev":"lost"}` once the daemon is gone. The protocol is in
//! `terminal-daemon/src/lib.rs`.

use serde_json::{Value, json};
use std::{
    fs::File,
    io::{self, BufRead, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::Duration,
};
use studio_terminal_daemon::{Client, PROTOCOL, default_socket};
use tauri::{AppHandle, Emitter, Manager, State};

/// The connection's writing half; `None` before the first connect and after the daemon is lost.
#[derive(Default)]
pub struct Daemon(Mutex<Option<UnixStream>>);

/// Connects to the daemon, once per app run until it is lost, and returns `{repo}`: the
/// configured repository, where the pane's shell starts.
#[tauri::command(async)]
pub fn terminal_connect(app: AppHandle, daemon: State<Daemon>) -> Result<Value, String> {
    let mut writer = daemon.0.lock().unwrap();
    if writer.is_none() {
        let (reader, stream) = connect()?.split();
        *writer = Some(stream);
        thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                if let Ok(event) = serde_json::from_str::<Value>(&line) {
                    let _ = app.emit("terminal", event);
                }
            }
            app.state::<Daemon>().0.lock().unwrap().take();
            let _ = app.emit("terminal", json!({ "ev": "lost" }));
        });
    }
    Ok(json!({ "repo": repo() }))
}

/// Sends one request, such as `{"op":"input","id":…,"data":…}`, to the daemon.
#[tauri::command]
pub fn terminal_send(msg: Value, daemon: State<Daemon>) -> Result<(), String> {
    let mut writer = daemon.0.lock().unwrap();
    let stream = writer
        .as_mut()
        .ok_or("not connected to the terminal daemon")?;
    writeln!(stream, "{msg}").map_err(|e| e.to_string())
}

/// The picker's places: the configured repository's main checkout and its tasks, read live from git
/// (#461, decision 8).
#[tauri::command(async)]
pub fn places() -> Result<studio_workbench::Places, String> {
    studio_workbench::places(Path::new(&repo()))
}

/// Creates task `name` in the configured repository (#468) and returns its place.
#[tauri::command(async)]
pub fn create_task(name: String) -> Result<studio_workbench::Place, String> {
    studio_workbench::create_task(Path::new(&repo()), &name)
}

/// Removes the task's worktree at `path`, or, unless `confirmed`, returns what that would lose.
#[tauri::command(async)]
pub fn remove_task(
    path: String,
    confirmed: bool,
) -> Result<Option<studio_workbench::Losses>, String> {
    studio_workbench::remove_task(Path::new(&repo()), &path, confirmed)
}

/// The command a session's terminal runs: `claude` with the studio plugin of this checkout, whose
/// hooks run its `studio-hook`, or `claude --resume <resume>` for an ended session.
#[tauri::command]
pub fn claude_argv(resume: Option<String>) -> Vec<String> {
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).with_file_name("plugin");
    studio_workbench::claude(&plugin, resume.as_deref())
}

/// Opens a link Daniel clicked in a terminal: an `http(s)` URL, or a file path, absolute or
/// relative to the configured repository, with any `:line:column` dropped.
// shortcut: opens files in their default app and loses the line; open in an editor at the line if
// that matters.
#[tauri::command]
pub fn open_link(target: String) -> Result<(), String> {
    let target = if target.starts_with("https://") || target.starts_with("http://") {
        target
    } else {
        let path = target.split(':').next().unwrap_or_default();
        let path = match path.strip_prefix("~/") {
            Some(rest) => std::env::home_dir().unwrap_or_default().join(rest),
            None => Path::new(&repo()).join(path),
        };
        if !path.exists() {
            return Err(format!("no such file: {}", path.display()));
        }
        path.display().to_string()
    };
    // Never an option: a URL starts with its scheme and a path is absolute.
    let opened = Command::new("/usr/bin/open").arg(&target).status();
    match opened {
        Ok(status) if status.success() => Ok(()),
        _ => Err(format!("could not open {target}")),
    }
}

/// Connects, starting the daemon first when nothing listens on its socket.
fn connect() -> Result<Client, String> {
    let socket = default_socket();
    match Client::connect(&socket, PROTOCOL) {
        Ok(client) => return Ok(client),
        Err(e)
            if !matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(format!("the terminal daemon: {e}"));
        }
        Err(_) => {}
    }
    let log = socket.with_extension("log");
    start(&log).map_err(|e| format!("start the terminal daemon: {e}"))?;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if let Ok(client) = Client::connect(&socket, PROTOCOL) {
            return Ok(client);
        }
    }
    Err(format!(
        "the terminal daemon did not start; see {}",
        log.display()
    ))
}

/// Starts `studio-terminal-daemon serve` from beside this executable, logging to `log`. The daemon
/// calls `setsid` itself, so it outlives the app.
fn start(log: &Path) -> io::Result<()> {
    let daemon = std::env::current_exe()?.with_file_name("studio-terminal-daemon");
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log = File::options().create(true).append(true).open(log)?;
    let mut child = Command::new(&daemon)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", daemon.display())))?;
    // Reaps it if it ever exits while the app runs.
    thread::spawn(move || child.wait());
    Ok(())
}

/// The configured repository: `STUDIO_REPO`, else the main checkout of the repository the app was
/// built in (#461, decision 3), else the home folder.
fn repo() -> String {
    if let Ok(repo) = std::env::var("STUDIO_REPO") {
        return repo;
    }
    let common = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    let main = match common {
        Ok(out) if out.status.success() => {
            let dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
            dir.parent().map(Path::to_path_buf)
        }
        _ => None,
    };
    main.or_else(std::env::home_dir)
        .unwrap_or_default()
        .display()
        .to_string()
}
