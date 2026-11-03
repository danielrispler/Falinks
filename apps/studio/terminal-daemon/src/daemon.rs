// The daemon itself, built from the #322 spike (`terminal-spike/src/daemon.rs`). It owns the PTYs, keeps each
// terminal's raw output as scrollback, and serves clients over the socket (protocol in lib.rs).
use crate::{PROTOCOL, decode, encode};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{BufRead, BufReader, Read, Write},
    os::{fd::AsRawFd, unix::net::{UnixListener, UnixStream}},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

// shortcut: raw bytes, cut at a fixed size, replay garbled if the cut lands mid-sequence or after a screen switch;
// upgrade to a headless emulator snapshot if reattach shows it.
pub const SCROLLBACK: usize = 4 << 20;

// Variables that leak in from whatever started the app (Ghostty, tmux, a Claude Code Bash tool) are dropped. Terminal
// ones make programs assume the wrong terminal. Claude Code's mark a nested session (transcripts off) and carry its
// session id and messaging token. Terminals start as login shells, so anything set in Daniel's profile comes back.
const SCRUB: &[&str] = &[
    "TERM_PROGRAM",
    "TERM_SESSION_ID",
    "LC_TERMINAL",
    "ITERM_",
    "GHOSTTY_",
    "KITTY_",
    "WEZTERM_",
    "ALACRITTY_",
    "ZED_",
    "VSCODE_",
    "TMUX",
    "HERDR_",
    "STUDIO_",
    "CLAUDE",
];

struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
}

struct Term {
    pty: Pty,
    argv: Vec<String>,
    cwd: String,
    buf: VecDeque<u8>,
    cols: u16,
    rows: u16,
    exited: Option<String>,
}

struct Conn {
    stream: UnixStream,
    attached: HashSet<String>,
}

#[derive(Default)]
struct State {
    terms: HashMap<String, Term>,
    conns: HashMap<usize, Conn>,
}

type Shared = Arc<Mutex<State>>;

pub fn serve(socket: &Path) -> Result<(), String> {
    // Its own session, so no hangup or Ctrl-C from whatever started it reaches it.
    unsafe {
        libc::setsid();
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // One daemon at a time: the lock is held for the daemon's whole life and freed by the kernel when it dies.
    let lock = std::fs::File::create(socket.with_extension("lock")).map_err(|e| e.to_string())?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(format!("a daemon already serves {}", socket.display()));
    }
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).map_err(|e| e.to_string())?;
    eprintln!("daemon {} (protocol {PROTOCOL}) listening on {}", std::process::id(), socket.display());
    let state = Shared::default();
    for (n, stream) in listener.incoming().enumerate() {
        let (Ok(stream), state) = (stream, state.clone()) else { continue };
        std::thread::spawn(move || serve_conn(n, stream, state));
    }
    drop(lock);
    Ok(())
}

// Writes to every client attached to `id`, and drops clients whose socket is gone (the app quit or crashed).
fn broadcast(conns: &mut HashMap<usize, Conn>, id: &str, msg: &Value) {
    let line = format!("{msg}\n");
    conns.retain(|_, c| !c.attached.contains(id) || c.stream.write_all(line.as_bytes()).is_ok());
}

fn on_output(state: &Shared, id: &str, bytes: &[u8]) {
    let s = &mut *state.lock().unwrap();
    let Some(t) = s.terms.get_mut(id) else { return };
    t.buf.extend(bytes);
    let excess = t.buf.len().saturating_sub(SCROLLBACK);
    t.buf.drain(..excess);
    broadcast(&mut s.conns, id, &json!({ "ev": "output", "id": id, "data": encode(bytes) }));
}

fn on_exit(state: &Shared, id: &str, status: String) {
    eprintln!("{id} exited: {status}");
    let s = &mut *state.lock().unwrap();
    if let Some(t) = s.terms.get_mut(id) {
        t.exited = Some(status.clone());
    }
    broadcast(&mut s.conns, id, &json!({ "ev": "exit", "id": id, "status": status }));
}

fn serve_conn(n: usize, stream: UnixStream, state: Shared) {
    let mut out = stream.try_clone().unwrap();
    let hello = json!({ "ev": "hello", "protocol": PROTOCOL, "daemon_pid": std::process::id() });
    if writeln!(out, "{hello}").is_err() {
        return;
    }
    let conn = Conn { stream: stream.try_clone().unwrap(), attached: HashSet::new() };
    state.lock().unwrap().conns.insert(n, conn);
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let req: Value = serde_json::from_str(&line).unwrap_or_default();
        let (op, id) = (req["op"].as_str().unwrap_or(""), req["id"].as_str().unwrap_or("").to_string());
        let size = |d: (u64, u64)| (req["cols"].as_u64().unwrap_or(d.0) as u16, req["rows"].as_u64().unwrap_or(d.1) as u16);
        if op == "list" {
            // Outside the lock: `ps` is slow, and output must keep flowing meanwhile.
            let msg = list(&state);
            let _ = writeln!(out, "{msg}");
            continue;
        }
        // Every write happens under this lock, so a replay and the live output after it never interleave.
        let s = &mut *state.lock().unwrap();
        match op {
            "spawn" if s.terms.contains_key(&id) => {
                let _ = writeln!(out, "{}", json!({ "ev": "error", "id": id, "message": "the id is taken" }));
            }
            "spawn" => {
                let argv: Vec<String> = serde_json::from_value(req["argv"].clone()).unwrap_or_default();
                let cwd = req["cwd"].as_str().unwrap_or("/").to_string();
                let (cols, rows) = size((80, 24));
                let (st1, st2, id1, id2) = (state.clone(), state.clone(), id.clone(), id.clone());
                let started = spawn(&argv, &cwd, &id, cols, rows, move |b| on_output(&st1, &id1, b), move |x| {
                    on_exit(&st2, &id2, x)
                });
                let reply = match started {
                    Ok(pty) => {
                        s.terms.insert(id.clone(), Term { pty, argv, cwd, buf: VecDeque::new(), cols, rows, exited: None });
                        json!({ "ev": "spawned", "id": id })
                    }
                    Err(message) => json!({ "ev": "error", "id": id, "message": message }),
                };
                let _ = writeln!(out, "{reply}");
            }
            "attach" => match s.terms.get_mut(&id) {
                Some(t) => {
                    let msg = json!({ "ev": "replay", "id": id, "data": encode(t.buf.make_contiguous()),
                                      "cols": t.cols, "rows": t.rows, "exited": t.exited });
                    let _ = writeln!(out, "{msg}");
                    if let Some(c) = s.conns.get_mut(&n) {
                        c.attached.insert(id);
                    }
                }
                None => {
                    let _ = writeln!(out, "{}", json!({ "ev": "error", "id": id, "message": "no such terminal" }));
                }
            },
            "input" => {
                if let Some(t) = s.terms.get_mut(&id) {
                    let _ = t.pty.writer.write_all(&decode(&req["data"]));
                }
            }
            "resize" => {
                if let Some(t) = s.terms.get_mut(&id) {
                    (t.cols, t.rows) = size((t.cols as u64, t.rows as u64));
                    let _ = t.pty.master.resize(PtySize { rows: t.rows, cols: t.cols, pixel_width: 0, pixel_height: 0 });
                }
            }
            "kill" => {
                if let Some(t) = s.terms.get_mut(&id) {
                    kill(t);
                }
            }
            "forget" => {
                if let Some(mut t) = s.terms.remove(&id) {
                    kill(&mut t);
                }
            }
            _ => {}
        }
    }
    state.lock().unwrap().conns.remove(&n);
}

fn spawn(
    argv: &[String],
    cwd: &str,
    id: &str,
    cols: u16,
    rows: u16,
    mut on_output: impl FnMut(&[u8]) + Send + 'static,
    on_exit: impl FnOnce(String) + Send + 'static,
) -> Result<Pty, String> {
    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| e.to_string())?;
    // Everything runs under a login shell, so the profile sets `PATH` even when the studio was started from the Dock
    // with launchd's bare one.
    let shell = std::env::var("SHELL").unwrap_or("/bin/zsh".into());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l");
    if !argv.is_empty() {
        cmd.args(["-c", "exec \"$@\"", "studio"]);
        cmd.args(argv);
    }
    cmd.cwd(cwd);
    for (k, _) in std::env::vars() {
        if SCRUB.iter().any(|p| k.starts_with(p)) {
            cmd.env_remove(k);
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // launchd gives a Dock-started app no locale; terminal apps set one, and Claude Code needs UTF-8.
    if ["LANG", "LC_ALL", "LC_CTYPE"].iter().all(|k| std::env::var_os(k).is_none()) {
        cmd.env("LANG", "en_US.UTF-8");
    }
    cmd.env("STUDIO_TERMINAL_ID", id);
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let (killer, pid) = (child.clone_killer(), child.process_id());
    std::thread::spawn(move || {
        let mut buf = [0u8; 16384];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            on_output(&buf[..n]);
        }
    });
    // A separate waiter, because a background job can keep the PTY open after the shell itself exits.
    std::thread::spawn(move || {
        let status = match child.wait() {
            Ok(s) => s.signal().map(|sig| format!("signal {sig}")).unwrap_or(format!("code {}", s.exit_code())),
            Err(err) => err.to_string(),
        };
        on_exit(status);
    });
    Ok(Pty { master: pair.master, writer, killer, pid })
}

// The terminal's shell leads its own session (portable-pty calls setsid), so the session id is the shell's pid, and
// it stays reserved while any process of the session lives, even after the shell exits.
fn kill(t: &mut Term) {
    let _ = t.pty.killer.kill();
    let Some(sid) = t.pty.pid else { return };
    let signal_all = move |sig| {
        for p in session(&processes(), sid) {
            unsafe { libc::kill(p.pid as i32, sig) };
        }
    };
    signal_all(libc::SIGHUP);
    // A grace period for shells to save history, then whatever ignored the hangup.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        signal_all(libc::SIGKILL);
    });
}

struct Proc {
    pid: u32,
    rss_kb: u64,
    command: String,
}

// Every process on the machine, from one `ps`.
fn processes() -> Vec<Proc> {
    let out = std::process::Command::new("ps").args(["-A", "-o", "pid=,rss=,comm="]).output();
    let out = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    out.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (pid, rss_kb) = (f.next()?.parse().ok()?, f.next()?.parse().ok()?);
            let command = f.collect::<Vec<_>>().join(" ").rsplit('/').next()?.to_string();
            Some(Proc { pid, rss_kb, command })
        })
        .collect()
}

// The live processes of session `sid`.
fn session(all: &[Proc], sid: u32) -> Vec<&Proc> {
    all.iter().filter(|p| unsafe { libc::getsid(p.pid as i32) } == sid as i32).collect()
}

fn list(state: &Shared) -> Value {
    // Read under the lock; `ps` runs after it, so output keeps flowing meanwhile.
    let terms: Vec<(Value, Option<u32>, Option<u32>)> = {
        let s = state.lock().unwrap();
        s.terms
            .iter()
            .map(|(id, t)| {
                let row = json!({ "id": id, "argv": t.argv, "cwd": t.cwd, "pid": t.pty.pid, "exited": t.exited,
                                  "cols": t.cols, "rows": t.rows, "scrollback_bytes": t.buf.len() });
                (row, t.pty.pid, t.pty.master.process_group_leader().map(|p| p as u32))
            })
            .collect()
    };
    let all = processes();
    let me = std::process::id();
    let terms: Vec<Value> = terms
        .into_iter()
        .map(|(mut row, pid, foreground)| {
            let procs = pid.map(|sid| session(&all, sid)).unwrap_or_default();
            // The program in the terminal's foreground, such as `zsh` or `claude`.
            row["foreground"] = json!(foreground.and_then(|f| all.iter().find(|p| p.pid == f)).map(|p| &p.command));
            row["rss_kb"] = json!(procs.iter().map(|p| p.rss_kb).sum::<u64>());
            row["processes"] =
                procs.iter().map(|p| json!({ "pid": p.pid, "command": p.command, "rss_kb": p.rss_kb })).collect();
            row
        })
        .collect();
    let daemon_rss_kb = all.iter().find(|p| p.pid == me).map_or(0, |p| p.rss_kb);
    json!({ "ev": "list", "daemon_pid": me, "daemon_rss_kb": daemon_rss_kb, "terms": terms })
}
