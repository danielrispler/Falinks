//! Git-arm Claude Code bridge (#27). The host copies this binary into each fresh sandbox.
//!
//! `claude-worker` runs one long-lived `claude -p` stream-json session with the same pinned
//! model, turn cap and isolation flags as the Falinks adapter, but ordinary Git-worktree
//! tools. The agent talks to the host by running `"$FALINKS_HOST" <action>` through Bash
//! (`claude-worker <action> ...`), which this process forwards as JSON lines. Host events are
//! written to the runtime at once: like the Falinks arm, they are presented at the next
//! tool-result boundary or start a new turn.
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
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
    // The bridge itself takes no arguments; with any, this is the agent's `"$FALINKS_HOST" ACTION`.
    let outcome = if args.is_empty() { run() } else { host(&args) };
    if let Err(error) = outcome {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// `message TEXT | share | ack ID | check [RESOLVED] | ready`: one request, one reply.
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
            return Err("usage: message TEXT | share | ack ID | check [RESOLVED] | ready".into());
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
    // A team of one (#178 C1): no peer worktree, messages or diffs.
    if start["peer_workspace"].is_null() {
        return format!(
            "You are agent {agent} in a one-agent software task using Git worktrees. Your worktree is {workspace}; \
             your current directory is its scratch/ subdirectory. The integration worktree is {integration}.\n\
             Talk to the host only by running these Bash commands, each of which prints the host's JSON reply:\n\
             \"$FALINKS_HOST\" share | \"$FALINKS_HOST\" ack MILESTONE_ID | \
             \"$FALINKS_HOST\" check [RESOLVED_MERGE_COMMIT] | \"$FALINKS_HOST\" ready\n\
             These send the requests the workflow below describes; ready names your last shared commit for you and \
             is accepted only after you have acknowledged all {developments} task development(s). Do not commit \
             yourself: share asks the host to commit your draft. Task developments arrive as messages starting \
             with \"Host event\". When you have nothing to do until the next host event, end your turn; the next \
             event starts a new turn. Do not run long sleeps or polling loops.\n\n\
             Task: {task}\nYour allocation: {allocation}\n\nWorkflow:\n{workflow}",
            agent = start["agent"].as_str().unwrap_or("?"),
            workspace = start["workspace"].as_str().unwrap_or("?"),
            integration = start["integration_workspace"].as_str().unwrap_or("?"),
            developments = start["development_count"],
            task = start["task"],
            allocation = start["allocation"],
            workflow = start["workflow"].as_str().unwrap_or(""),
        );
    }
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

/// Copy of `falinks_claude::nested_agent` (#178 C7); this crate is standalone from the adapter.
/// shortcut: an obfuscated launch (`$(echo cl)aude`, an alias, a script) evades it, as #178 states.
fn nested_agent(command: &str, runtime: &str) -> bool {
    let runtime_name = Path::new(runtime).file_name();
    command
        .split(['\n', ';', '&', '|', '(', ')', '`', '{', '}'])
        .filter_map(|part| {
            part.split_whitespace()
                .map(|w| w.trim_matches(['"', '\'']))
                .find(|w| {
                    !w.contains('=') && !["env", "exec", "nohup", "command", "time"].contains(w)
                })
        })
        .any(|program| {
            let name = Path::new(program).file_name();
            ["claude", "codex", "node", "npx"]
                .iter()
                .any(|n| name == Some(n.as_ref()))
                || (!runtime.is_empty() && (program == runtime || name == runtime_name))
        })
}

/// Claude Code's `~/.claude/projects/` folder name for a working directory (as `batch.rs`).
fn project_folder(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Copy of `falinks_claude::other_effort` (#205 E1); this crate is standalone from the adapter.
/// shortcut: entries written after the last turn's `result` stay unchecked; check again after
/// shutdown if a run ever ends on one.
fn other_effort(file: &Path, effort: &str) -> Option<Value> {
    fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["type"] == "assistant")
        .map(|entry| entry["effort"].clone())
        .find(|recorded| recorded != effort)
}

struct Bridge {
    model: String,
    effort: String,
    runtime: String,
    session: String,
    /// The runtime home's session folder for scratch; a session there not ours is a nested agent.
    project: Option<PathBuf>,
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

    /// Reports a nested agent to the host, then fails the run (#178 C7).
    fn nested(&self, detail: Value) -> Result<()> {
        emit(json!({"action": "nested_agent", "detail": detail}))?;
        Err(format!("nested agent {detail}; run invalid").into())
    }

    /// Fail closed on any runtime identity change, like the Falinks adapter's model control.
    fn line(&mut self, line: &str) -> Result<()> {
        let mut event: Value = serde_json::from_str(line)?;
        drop_thinking(&mut event);
        writeln!(self.transcript, "{event}")?;
        let model = self.model.as_str();
        match (event["type"].as_str(), event["subtype"].as_str()) {
            (Some("system"), Some("init")) => {
                if !init_matches(&event, model, &self.version, TOOLS) {
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
            (Some("assistant"), _) => {
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    if let Some(command) = block["input"]["command"].as_str()
                        && block["type"] == "tool_use"
                        && block["name"] == "Bash"
                        && nested_agent(command, &self.runtime)
                    {
                        return self.nested(json!({"command": command}));
                    }
                }
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
                let file = self
                    .project
                    .as_ref()
                    .map(|p| p.join(format!("{}.jsonl", self.session)));
                if let Some(effort) = file.and_then(|f| other_effort(&f, &self.effort)) {
                    let pinned = &self.effort;
                    return Err(
                        format!("session effort {effort} is not {pinned:?}; run invalid").into(),
                    );
                }
                let foreign = self
                    .project
                    .iter()
                    .flat_map(|p| fs::read_dir(p).into_iter().flatten().flatten())
                    .filter_map(|e| e.file_name().to_str().map(String::from))
                    .filter_map(|n| n.strip_suffix(".jsonl").map(String::from))
                    .find(|id| *id != self.session);
                if let Some(session) = foreign {
                    return self.nested(json!({"session": session}));
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
    let effort = start["model"]["reasoning"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let worker = cwd.join("scratch/worker");
    let session = uuid()?;
    let runtime_binary = start["runtime_binary"]
        .as_str()
        .unwrap_or_default()
        .to_string();
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
            "--effort",
            &effort,
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
            &session,
        ])
        .current_dir(&scratch)
        .env("DISABLE_AUTOUPDATER", "1")
        // Bash's temp folder defaults to `/tmp/claude-<uid>`, which the host sandbox denies.
        .env("CLAUDE_CODE_TMPDIR", &scratch)
        // zsh heredocs write `$TMPPREFIX*` files (default `/tmp/zsh`), also denied.
        .env("TMPPREFIX", scratch.join("zsh"))
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

    let project = start["runtime_home"].as_str().map(|home| {
        Path::new(home)
            .join(".claude/projects")
            .join(project_folder(&scratch))
    });
    let mut bridge = Bridge {
        model,
        effort,
        runtime: runtime_binary,
        session,
        project,
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

/// Whether a `system/init` event shows the pinned model, version, `dontAsk` and exactly `tools`
/// (a `--tools` value). The runtime lists tools sorted, not in `--tools` order.
fn init_matches(event: &Value, model: &str, version: &str, tools: &str) -> bool {
    let mut listed: Vec<&str> = event["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut expected: Vec<&str> = tools.split(',').collect();
    listed.sort();
    expected.sort();
    event["model"] == model
        && event["claude_code_version"] == version
        && event["permissionMode"] == "dontAsk"
        && listed == expected
}

/// Evidence excludes model reasoning. Touches only an object `message` with a `content` array:
/// `IndexMut` panics on a string `message` and inserts a missing one (#253).
fn drop_thinking(event: &mut Value) {
    if let Some(blocks) = event
        .get_mut("message")
        .and_then(|m| m.get_mut("content"))
        .and_then(Value::as_array_mut)
    {
        blocks.retain(|b| !matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The init a real 2.1.287 runtime emitted in the #26 integration (Falinks arm's tools).
    fn recorded_init() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../adapters/claude/evidence/2026-10-10-integration.json"
        );
        let evidence: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        evidence["evidence"]["transcripts"][0][0]["events"][0].clone()
    }

    #[test]
    fn drop_thinking_keeps_odd_events_and_strips_reasoning() {
        for raw in [
            json!({"type": "system", "message": "hello"}),
            json!({"type": "result", "subtype": "success"}),
        ] {
            let mut event = raw.clone();
            drop_thinking(&mut event);
            assert_eq!(event, raw);
        }
        let mut event = json!({"type": "assistant", "message": {"content": [
            {"type": "thinking", "thinking": "x"},
            {"type": "redacted_thinking", "data": "y"},
            {"type": "text", "text": "z"},
        ]}});
        drop_thinking(&mut event);
        assert_eq!(
            event["message"]["content"],
            json!([{"type": "text", "text": "z"}])
        );
    }

    #[test]
    fn recorded_real_init_passes_the_init_check() {
        let init = recorded_init();
        assert_eq!(init["subtype"], "init");
        let mut tools: Vec<&str> = init["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        // Real shape: sorted, whatever the `--tools` order.
        assert!(tools.is_sorted(), "recorded tools not sorted: {tools:?}");
        tools.reverse();
        let flag = tools.join(",");
        assert!(init_matches(&init, "claude-opus-5-5", "2.1.287", &flag));
        assert!(!init_matches(&init, "claude-opus-5-5", "2.1.287", TOOLS));
        assert!(!init_matches(&init, "claude-sonnet-5-5", "2.1.287", &flag));
    }

    #[test]
    fn nested_agent_launches_match_the_adapter_rule() {
        for command in [
            "claude -p hi",
            "cd .. && node -e 1",
            "FOO=1 npx tsx",
            "/r/claude-x -p",
        ] {
            assert!(nested_agent(command, "/r/claude-x"), "{command}");
        }
        for command in ["go test ./...", "cat node_modules.txt", "echo claude"] {
            assert!(!nested_agent(command, "/r/claude-x"), "{command}");
        }
        // The adapter's copy must stay identical (#178 C7).
        let adapter = include_str!("../../../adapters/claude/lib.rs");
        let body = |source: &str| {
            let start = source.find("    let runtime_name").unwrap();
            source[start..start + source[start..].find("\n}\n").unwrap()].to_string()
        };
        assert_eq!(body(adapter), body(include_str!("claude-worker.rs")));
    }
}
