//! Git-arm Claude Code bridge (#27). The host copies this binary into each fresh sandbox.
//!
//! `claude-worker` runs one long-lived `claude -p` stream-json session with the same pinned
//! model, turn cap and isolation flags as the Falinks adapter, but ordinary Git-worktree
//! tools. The agent talks to the host by running `"$FALINKS_HOST" <action>` through Bash
//! (`claude-worker host ...`), which this process forwards as JSON lines. Host events are
//! written to the runtime at once: like the Falinks arm, they are presented at the next
//! tool-result boundary or start a new turn.
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const TOOLS: &str = "Bash,Read,Glob,Grep,Edit,Write";
/// Same per-message agentic turn cap as the Falinks adapter's launch profile. Copied because
/// this crate is standalone from the engine; `adapters/claude/tests/arm.rs` checks the drift.
const MAX_TURNS: &str = "12";
/// An idle, unfinished agent with no host events is reminded after this long (both arms).
const NUDGE: Duration = Duration::from_secs(120);
/// Host channel, relative to the worker directory (the runtime's scratch is beneath it).
const SOCKET: &str = "scratch/host.sock";

enum Input {
    Host(Option<Value>),
    Claude(Option<String>),
    Cli(Value, Sender<Value>),
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("host") => host(&args[1..]),
        _ => run(),
    };
    if let Err(error) = outcome {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// `host message TEXT | share | ack ID | check [RESOLVED] | ready`: one request, one reply.
fn host(args: &[String]) -> Result<()> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let request = match args.first().map(String::as_str) {
        Some("message") => json!({"action": "message", "text": args[1..].join(" ")}),
        Some("share") => json!({"action": "share"}),
        Some("ack") => json!({"action": "ack", "milestone": arg(1)}),
        Some("check") if args.len() > 1 => json!({"action": "check", "resolved": arg(1)}),
        Some("check") => json!({"action": "check"}),
        Some("ready") => json!({"action": "ready"}),
        _ => {
            return Err(
                "usage: host message TEXT | share | ack ID | check [RESOLVED] | ready".into(),
            );
        }
    };
    // Relative to the worker directory: absolute run paths can exceed the 104-byte socket limit.
    env::set_current_dir(env::var("FALINKS_HOST_DIR")?)?;
    let mut stream = UnixStream::connect(SOCKET)?;
    writeln!(stream, "{request}")?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    print!("{reply}");
    let reply: Value = serde_json::from_str(&reply)?;
    if reply["type"] == "refused" {
        std::process::exit(2);
    }
    Ok(())
}

fn uuid() -> Result<String> {
    let mut b = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

fn emit(value: Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{value}")?;
    stdout.flush()?;
    Ok(())
}

fn prompt(start: &Value) -> String {
    format!(
        "You are agent {agent} in a two-agent software task using Git worktrees. Your worktree is {workspace}; \
         your current directory is its scratch/ subdirectory, which is never shared. Your peer's worktree \
         (read only) is {peer}. The shared integration worktree is {integration}.\n\
         Talk to the host only by running these Bash commands, each of which prints the host's JSON reply:\n\
         \"$FALINKS_HOST\" message \"TEXT\" | \"$FALINKS_HOST\" share | \"$FALINKS_HOST\" ack MILESTONE_ID | \
         \"$FALINKS_HOST\" check [RESOLVED_MERGE_COMMIT] | \"$FALINKS_HOST\" ready\n\
         These send the requests the workflow below describes; ready names your last shared commit for you and \
         is accepted only after you have acknowledged all {developments} task development(s). Do not commit \
         yourself: share asks the host to commit your draft. Peer messages, peer diffs and task developments \
         arrive as messages starting with \"Host event\". When you have nothing to do until the next host \
         event, end your turn; the next event starts a new turn. Do not run long sleeps or polling loops.\n\n\
         Task: {task}\nYour allocation: {allocation}\n\nWorkflow:\n{workflow}",
        agent = start["agent"].as_str().unwrap_or("?"),
        workspace = start["workspace"].as_str().unwrap_or("?"),
        peer = start["peer_workspace"].as_str().unwrap_or("?"),
        integration = start["integration_workspace"].as_str().unwrap_or("?"),
        developments = start["development_count"],
        task = start["task"],
        allocation = start["allocation"],
        workflow = start["workflow"].as_str().unwrap_or(""),
    )
}

struct Bridge {
    model: String,
    version: String,
    input: ChildStdin,
    transcript: fs::File,
    busy: bool,
    ready: bool,
    turn: u64,
    last: Instant,
    rate_limit: Value,
}

impl Bridge {
    fn send(&mut self, text: &str) -> Result<()> {
        writeln!(
            self.input,
            "{}",
            json!({"type": "user", "message": {"role": "user", "content": text}})
        )?;
        self.input.flush()?;
        self.busy = true;
        self.last = Instant::now();
        Ok(())
    }

    /// Fail closed on any runtime identity change, like the Falinks adapter's model control.
    fn line(&mut self, line: &str) -> Result<()> {
        let mut event: Value = serde_json::from_str(line)?;
        if let Some(blocks) = event["message"]["content"].as_array_mut() {
            // Evidence excludes model reasoning.
            blocks
                .retain(|b| !matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking")));
        }
        writeln!(self.transcript, "{event}")?;
        let model = self.model.as_str();
        match (event["type"].as_str(), event["subtype"].as_str()) {
            (Some("system"), Some("init")) => {
                let tools: Vec<&str> = event["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                if event["model"] != model
                    || event["claude_code_version"] != self.version.as_str()
                    || event["permissionMode"] != "dontAsk"
                    || tools != TOOLS.split(',').collect::<Vec<_>>()
                {
                    return Err(format!("unsupported runtime init; run invalid: {event}").into());
                }
            }
            (Some("system"), Some("model_refusal_fallback")) => {
                return Err("model refusal fallback; run invalid".into());
            }
            (Some("assistant"), _)
                if event["message"]["model"]
                    .as_str()
                    .is_some_and(|m| m != model) =>
            {
                return Err("model identity changed; run invalid".into());
            }
            (Some("rate_limit_event"), _) => {
                self.rate_limit = event["rate_limit_info"].clone();
                if self.rate_limit["status"] == "rejected" {
                    // Pause the batch; never switch provider.
                    emit(json!({"action": "limit", "detail": self.rate_limit}))?;
                }
            }
            (Some("result"), subtype) => {
                self.busy = false;
                self.last = Instant::now();
                self.turn += 1;
                let models: Vec<&String> = event["modelUsage"]
                    .as_object()
                    .map(|m| m.keys().collect())
                    .unwrap_or_default();
                if models.iter().any(|m| *m != model) {
                    return Err("model identity changed; run invalid".into());
                }
                // Token counts only: they do not establish subscription cost.
                emit(
                    json!({"action": "usage", "usage": {"turn": self.turn, "subtype": subtype,
                    "usage": event["usage"], "num_turns": event["num_turns"],
                    "duration_ms": event["duration_ms"], "rate_limit": self.rate_limit}}),
                )?;
                match subtype {
                    Some("success") => {}
                    Some("error_max_turns") if !self.ready => {
                        self.send("Host: your turn reached the per-message tool-call cap. Continue your task.")?
                    }
                    Some("error_max_turns") => {}
                    other => return Err(format!("runtime turn failed: {other:?}").into()),
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn run() -> Result<()> {
    let mut first = String::new();
    io::stdin().lock().read_line(&mut first)?;
    let start: Value = serde_json::from_str(&first)?;
    let developments = start["development_count"].as_u64().unwrap_or(0);
    let cwd = env::current_dir()?;
    let scratch = cwd.join("scratch");

    let (tx, inputs): (Sender<Input>, Receiver<Input>) = channel();

    let listener = UnixListener::bind(SOCKET)?;
    let cli = tx.clone();
    thread::spawn(move || {
        for stream in listener.incoming().map_while(|s| s.ok()) {
            let cli = cli.clone();
            thread::spawn(move || {
                let mut line = String::new();
                let Ok(mut writer) = stream.try_clone() else {
                    return;
                };
                if BufReader::new(stream).read_line(&mut line).is_err() {
                    return;
                }
                let request = serde_json::from_str(&line).unwrap_or(json!({"action": "invalid"}));
                let (reply, answer) = channel();
                if cli.send(Input::Cli(request, reply)).is_ok()
                    && let Ok(value) = answer.recv()
                {
                    let _ = writeln!(writer, "{value}");
                }
            });
        }
    });
    let host = tx.clone();
    thread::spawn(move || {
        for line in io::stdin().lines().map_while(|l| l.ok()) {
            let event = serde_json::from_str(&line).ok();
            if event.is_none() || host.send(Input::Host(event)).is_err() {
                break;
            }
        }
        let _ = host.send(Input::Host(None));
    });

    let model = start["model"]["model"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let worker = cwd.join("scratch/worker");
    let settings = json!({
        "permissions": {"defaultMode": "dontAsk", "allow": TOOLS.split(',').collect::<Vec<_>>(),
            "deny": ["NotebookEdit", "WebSearch", "WebFetch", "Agent", "Task", "Skill"]},
        // The host's sandbox-exec profile confines the whole process tree.
        "sandbox": {"enabled": false},
        "env": {"DISABLE_AUTOUPDATER": "1"},
    });
    let mut command = Command::new(
        start["runtime_binary"]
            .as_str()
            .ok_or("runtime_binary missing")?,
    );
    command
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--replay-user-messages",
            "--model",
            &model,
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--disable-slash-commands",
            "--tools",
            TOOLS,
            "--permission-mode",
            "dontAsk",
            "--max-turns",
            MAX_TURNS,
            "--settings",
            &settings.to_string(),
            "--session-id",
            &uuid()?,
        ])
        .current_dir(&scratch)
        .env("DISABLE_AUTOUPDATER", "1")
        .env("FALINKS_HOST", &worker)
        .env("FALINKS_HOST_DIR", &cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(home) = start["runtime_home"].as_str() {
        // Login state lives in the runtime home; build caches stay in scratch via GOCACHE etc.
        command.env("HOME", home);
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or("claude stdout missing")?;
    let runtime = tx;
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if runtime.send(Input::Claude(Some(line))).is_err() {
                return;
            }
        }
        let _ = runtime.send(Input::Claude(None));
    });

    let mut bridge = Bridge {
        model,
        version: start["runtime_version"].as_str().unwrap_or_default().into(),
        input: child.stdin.take().ok_or("claude stdin missing")?,
        transcript: fs::File::create(scratch.join("claude-events.jsonl"))?,
        busy: false,
        ready: false,
        turn: 0,
        last: Instant::now(),
        rate_limit: Value::Null,
    };
    bridge.send(&prompt(&start))?;

    // One forwarded request at a time; the host answers each in order.
    let mut queue: Vec<(Value, Sender<Value>)> = vec![];
    let mut outstanding: Option<(Value, Sender<Value>)> = None;
    let (mut acked, mut shared): (u64, Option<String>) = (0, None);
    loop {
        if outstanding.is_none() && !queue.is_empty() {
            let (mut request, reply) = queue.remove(0);
            let refusal = match request["action"].as_str() {
                Some("ready") if acked < developments => Some(format!(
                    "acknowledge all {developments} development(s) first"
                )),
                Some("ready") if shared.is_none() => Some("share a draft first".into()),
                Some("ready") if bridge.ready => Some("already ready".into()),
                Some("message" | "share" | "ack" | "check" | "ready") => None,
                _ => Some("unknown host command".into()),
            };
            match refusal {
                Some(reason) => {
                    let _ = reply.send(json!({"type": "refused", "reason": reason}));
                }
                None => {
                    if request["action"] == "ready" {
                        request["commit"] = shared.clone().into();
                    }
                    emit(request.clone())?;
                    outstanding = Some((request, reply));
                }
            }
            continue;
        }
        let input = match inputs.recv_timeout(Duration::from_secs(1)) {
            Ok(input) => input,
            Err(RecvTimeoutError::Timeout) => {
                if !bridge.busy && !bridge.ready && bridge.last.elapsed() >= NUDGE {
                    bridge.send("Host: no new events for two minutes. If your part is unfinished, continue it; otherwise follow the workflow (share, check, ready).")?;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        match input {
            Input::Claude(Some(line)) => bridge.line(&line)?,
            Input::Claude(None) => return Err("claude stream closed".into()),
            Input::Host(None) => return Ok(()),
            Input::Cli(request, reply) => queue.push((request, reply)),
            Input::Host(Some(event)) => {
                let kind = event["type"].as_str().unwrap_or_default();
                if kind == "ok" && event["action"] == "usage" {
                    continue; // Telemetry receipts must not wake the model.
                }
                let answers = outstanding.as_ref().is_some_and(|(request, _)| {
                    let action = request["action"].as_str().unwrap_or_default();
                    match action {
                        _ if kind == "refused" => event["action"] == action,
                        "share" => kind == "shared",
                        "check" => kind == "check_result",
                        _ => kind == "ok" && event["action"] == action,
                    }
                });
                if answers {
                    let (request, reply) = outstanding.take().unwrap();
                    match request["action"].as_str() {
                        _ if kind == "refused" => {}
                        Some("ack") => acked += 1,
                        Some("share") => shared = event["commit"].as_str().map(String::from),
                        Some("ready") => bridge.ready = true,
                        _ => {}
                    }
                    let _ = reply.send(event);
                } else if !bridge.ready
                    && matches!(kind, "peer_message" | "peer_diff" | "development")
                {
                    bridge.send(&format!("Host event: {event}"))?;
                }
            }
        }
    }
}
