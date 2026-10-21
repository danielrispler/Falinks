//! Pinned Claude Code worker adapter: the #23 boundary ported to stream-json.
//! Runtime-independent pieces (`Boundary`, `ControlGate`, `ControlledHost`) come from `falinks-host`.
use falinks_host::{
    Boundary, ControlGate, Engine, Notice, PinMap, Result, platform_pin, random_hex, require,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::OpenOptionsExt,
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// SHA-256 of the `claude` binary per verified platform.
pub const PINS: PinMap<&str> = &[
    (
        ("macos", "aarch64"),
        "b9ac5374839f14abf3f1f45610637c332b5daee5ce0d1634726cef87aab78e6a",
    ),
    (
        ("linux", "x86_64"),
        "ea0f135784631619a7a409d07322ecec964f9cc4fd377508ed1ba3f141f27912",
    ),
    (
        ("linux", "aarch64"),
        "c487117526509ab6d2ee6013e14f181faf68d6d6afe6bb0d4b2b4a9feca5ecb3",
    ),
];
pub const VERSION: &str = "2.1.304";
pub const MODEL: &str = "claude-opus-5-5";
/// Built-in tools every worker gets; the launch adds its `falinks` tools.
pub const NATIVE: &[&str] = &["Bash", "Glob", "Grep", "Read"];
pub const PLUGINS: &[&str] = &[
    "cc-plugin-agents-md@builtin",
    "cc-plugin-plugin-authoring@builtin",
    "cc-plugin-telemetry@builtin",
];
pub const REQUIRED: &[&str] = &[
    "pin",
    "init",
    "model_identity",
    "disabled_capabilities",
    "mediated_calls",
    "source_write_denial",
    "storage_protection",
    "scratch_access",
    "unknown_change",
    "steering",
    "completion_race",
    "resume_replay",
];
/// System subtypes recorded from the pinned binary in #32; any other shape fails visibly.
const SYSTEM: &[&str] = &[
    "init",
    "hook_started",
    "hook_response",
    "informational",
    "thinking_tokens",
    "task_started",
    "task_notification",
];

pub mod arm;
pub mod engine_host;

/// The exact probe command must have completed through the real Bash tool, with every
/// check passing (`falinks_host::probe_controls`).
pub fn bash_probe_controls(events: &[Value], command: &str) -> Result<Value> {
    let args = shlex::split(command).ok_or("unparsable probe command")?;
    let bash = |b: &Value| b["name"] == "Bash" && b["input"]["command"] == command;
    for (event, _) in tool_results(events, bash).filter(|(_, b)| b["is_error"] != true) {
        let stdout = event["tool_use_result"]["stdout"].as_str().unwrap_or("");
        if let Some(checks) = falinks_host::probe_controls(stdout, &args)? {
            return Ok(checks);
        }
    }
    Err("exact probe command did not complete through the actual Bash tool".into())
}

/// What a live step proved where the worker sandbox cannot start.
#[derive(Debug, PartialEq)]
pub enum WorkerSandboxOutcome {
    /// The probe's Bash error text.
    FailClosed(String),
    FailOpen(String),
    Failed(String),
    /// A model refusal: not evidence either way.
    Inconclusive,
}
impl WorkerSandboxOutcome {
    pub fn name(&self) -> &'static str {
        match self {
            Self::FailClosed(_) => "fail-closed",
            Self::FailOpen(_) => "fail-open",
            Self::Failed(_) => "failed",
            Self::Inconclusive => "inconclusive",
        }
    }
}
/// The runtime emitted `model_refusal_fallback`.
pub fn refused(events: &[Value]) -> bool {
    events
        .iter()
        .any(|e| e["type"] == "system" && e["subtype"] == "model_refusal_fallback")
}
/// The bytes of every file in the fixture's source and protected storage directories;
/// `None` for an unreadable entry, so it never compares equal to an empty file.
pub fn fixture_files(root: &Path) -> Result<BTreeMap<PathBuf, Option<Vec<u8>>>> {
    let mut files = BTreeMap::new();
    for name in ["source", "controller", "snapshots", "validation"] {
        let Ok(entries) = fs::read_dir(root.join(name)) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            files.insert(path.clone(), fs::read(&path).ok());
        }
    }
    Ok(files)
}
/// Classify a turn whose only task was the exact probe command, with fixture files
/// `before` and `after` it. Fail-open evidence outranks a refusal.
pub fn worker_sandbox_unavailable(
    events: &[Value],
    command: &str,
    before: &BTreeMap<PathBuf, Option<Vec<u8>>>,
    after: &BTreeMap<PathBuf, Option<Vec<u8>>>,
) -> WorkerSandboxOutcome {
    let controls = tool_results(events, |_| true).any(|(event, block)| {
        [&block["content"], &event["tool_use_result"]]
            .iter()
            .any(|v| v.to_string().contains("FALINKS_CONTROLS="))
    });
    if controls {
        return WorkerSandboxOutcome::FailOpen(
            "a tool result contains a FALINKS_CONTROLS line".into(),
        );
    }
    if before != after {
        return WorkerSandboxOutcome::FailOpen("a source or protected file changed".into());
    }
    if refused(events) {
        return WorkerSandboxOutcome::Inconclusive;
    }
    let bash = |b: &Value| b["name"] == "Bash" && b["input"]["command"] == command;
    tool_results(events, bash)
        .find(|(_, b)| b["is_error"] == true)
        .map_or_else(
            || {
                WorkerSandboxOutcome::Failed(
                    "the exact probe command returned no Bash error".into(),
                )
            },
            |(_, b)| {
                let content = &b["content"];
                WorkerSandboxOutcome::FailClosed(
                    content
                        .as_str()
                        .map_or_else(|| content.to_string(), String::from),
                )
            },
        )
}

/// The content blocks of one stream-json message.
pub fn blocks(event: &Value) -> impl Iterator<Item = &Value> {
    event["message"]["content"].as_array().into_iter().flatten()
}
/// Each `tool_result` block, with the user event carrying it, that answers an assistant
/// `tool_use` block matching `called`.
pub fn tool_results(
    events: &[Value],
    called: impl Fn(&Value) -> bool,
) -> impl Iterator<Item = (&Value, &Value)> {
    let ids: Vec<&Value> = events
        .iter()
        .filter(|e| e["type"] == "assistant")
        .flat_map(blocks)
        .filter(|b| b["type"] == "tool_use" && called(b))
        .map(|b| &b["id"])
        .collect();
    events
        .iter()
        .filter(|e| e["type"] == "user")
        .flat_map(|e| blocks(e).map(move |b| (e, b)))
        .filter(move |(_, b)| b["type"] == "tool_result" && ids.contains(&&b["tool_use_id"]))
}

/// The Go toolchain (`GOROOT/bin/go`) and `gopls` (`FALINKS_GOPLS`, else `GOPATH/bin/gopls`).
pub fn go_tools() -> Result<(PathBuf, PathBuf)> {
    let go_env = |key: &str| -> Result<PathBuf> {
        let out = Command::new("go").args(["env", key]).output()?;
        Ok(PathBuf::from(String::from_utf8(out.stdout)?.trim()))
    };
    let gopls = match std::env::var_os("FALINKS_GOPLS") {
        Some(path) => PathBuf::from(path),
        None => go_env("GOPATH")?.join("bin/gopls"),
    };
    Ok((go_env("GOROOT")?.join("bin/go"), gopls))
}

pub fn pin(os: &str, arch: &str) -> Result<&'static str> {
    platform_pin(PINS, os, arch)
}

/// The canonical binary and the SHA-256 it was verified against.
pub fn verify_binary(binary: &Path) -> Result<(PathBuf, &'static str)> {
    falinks_host::verify_pinned(
        binary,
        PINS,
        &format!("{VERSION} (Claude Code)"),
        &[("DISABLE_AUTOUPDATER", "1")],
    )
}

/// What one worker launch may see and call. Controls come only from here, never from a saved session.
#[derive(Clone, Debug)]
pub struct Launch {
    /// The live root the worker reads; the engine-facing workspace.
    pub source: PathBuf,
    /// The worker's cwd and only writable directory.
    pub scratch: PathBuf,
    /// Host channel, token, settings and context: neither readable nor writable by the worker.
    pub controller: PathBuf,
    /// Host-provided executables: readable, write-denied.
    pub worker_tools: PathBuf,
    /// Other storage the worker may neither read nor write.
    pub protected: Vec<PathBuf>,
    /// MCP definitions of the `falinks` tools.
    pub tools: Value,
}
impl Launch {
    /// The #33 fixture layout under `root`, with the historical edit/review/offer tools.
    pub fn fixture(root: &Path) -> Self {
        let source = root.join("source");
        Self {
            tools: falinks_host::tools(&source.to_string_lossy()),
            source,
            scratch: root.join("scratch"),
            controller: root.join("controller"),
            worker_tools: root.join("worker-tools"),
            protected: ["snapshots", "validation"].map(|n| root.join(n)).to_vec(),
        }
    }
    /// `falinks_<operation>` names offered by this launch.
    pub fn operations(&self) -> Vec<String> {
        falinks_host::operations(&self.tools)
    }
    /// The exact runtime tool list `init` must report.
    pub fn allowed(&self) -> Vec<String> {
        let mut tools: Vec<String> = NATIVE.iter().map(|t| t.to_string()).collect();
        tools.extend(
            self.operations()
                .iter()
                .map(|o| format!("mcp__falinks__falinks_{o}")),
        );
        tools
    }
}

/// Every turn's `system/init` must match the pinned launch profile exactly, except that
/// its cwd may be beneath scratch: Bash keeps its cwd across turns while it stays there (#110).
pub fn verify_init(init: &Value, scratch: &Path, session: &str, tools: &[String]) -> Result<()> {
    let strings = |key: &str, field: Option<&str>| -> BTreeSet<String> {
        init[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| field.map_or(x, |f| &x[f]).as_str().unwrap_or("").to_owned())
            .collect()
    };
    let set = |items: &[&str]| items.iter().map(|x| x.to_string()).collect::<BTreeSet<_>>();
    let tools: BTreeSet<String> = tools.iter().cloned().collect();
    require(
        init["claude_code_version"] == VERSION
            && init["model"] == MODEL
            && init["permissionMode"] == "dontAsk"
            && init["session_id"] == session
            && init["cwd"]
                .as_str()
                .is_some_and(|cwd| beneath(Path::new(cwd), scratch))
            && init["tools"].as_array().map(Vec::len) == Some(tools.len())
            && strings("tools", None) == tools
            && init["mcp_servers"]
                == json!([{"name":"falinks","source":"dynamic","status":"connected"}])
            && init["skills"] == json!([])
            && init["slash_commands"] == json!([])
            && init["plugins"].as_array().map(Vec::len) == Some(PLUGINS.len())
            && strings("plugins", Some("source")) == set(PLUGINS),
        &format!("unsupported runtime init; publication/scoring disabled: {init}"),
    )
}

/// `path` is `root` or beneath it once symlinks resolve; a missing path is not.
fn beneath(path: &Path, root: &Path) -> bool {
    match (fs::canonicalize(path), fs::canonicalize(root)) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

/// Host-chosen runtime session identity (RFC 4122 version 4).
pub fn session_id() -> Result<String> {
    let mut h = random_hex(16)?.into_bytes();
    h[12] = b'4';
    h[16] = b"89ab"[(h[16] % 4) as usize];
    let h = String::from_utf8(h)?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

/// Process-free translation of stream-json lines, hook stamps and mediated calls
/// into the runtime-independent `Boundary` and `ControlGate`.
pub struct Session {
    pub boundary: Boundary,
    pub gate: ControlGate,
    pub events: Vec<Value>,
    pub deliveries: Vec<Value>,
    pub denials: Vec<Value>,
    pub last_result: Value,
    /// `~/.claude/projects/<cwd>/` reported by the runtime for this worker.
    pub project: Option<PathBuf>,
    connection: String,
    session: String,
    token: String,
    cwd: PathBuf,
    tools: Vec<String>,
    engine: Engine,
    spans: u64,
    in_turn: bool,
    /// Host messages sent but not yet presented by a replay.
    unpresented: Vec<String>,
    hooks: BTreeMap<String, Value>,
    ended: BTreeSet<String>,
}
impl Session {
    pub fn new(
        launch: &Launch,
        agent: &str,
        session: &str,
        token: &str,
        engine: Engine,
        resume: bool,
    ) -> Result<Self> {
        let connection = random_hex(16)?;
        let mut boundary = Boundary::new(
            &launch.controller.join("context.sqlite"),
            &launch.source,
            connection.clone(),
            launch.operations(),
        )?;
        boundary.bind(session, agent, resume)?;
        Ok(Self {
            boundary,
            gate: ControlGate::new(REQUIRED),
            events: vec![],
            deliveries: vec![],
            denials: vec![],
            last_result: Value::Null,
            project: None,
            connection,
            session: session.into(),
            token: token.into(),
            cwd: launch.scratch.clone(),
            tools: launch.allowed(),
            engine,
            spans: 0,
            in_turn: false,
            unpresented: vec![],
            hooks: BTreeMap::new(),
            ended: BTreeSet::new(),
        })
    }
    pub fn connection(&self) -> &str {
        &self.connection
    }
    pub fn session(&self) -> &str {
        &self.session
    }
    /// This process's evidence: events, deliveries, denials and controls.
    pub fn transcript(&self) -> Value {
        json!({"session":self.session,"events":self.events,"deliveries":self.deliveries,
            "permission_denials":self.denials,"gate_failed":self.gate.failed,
            "controls":self.gate.controls,"project":self.project})
    }
    /// Between `init` and `result`: a message sent now is presented within this turn.
    pub fn in_turn(&self) -> bool {
        self.in_turn
    }
    /// The runtime has presented every host message and finished its turn.
    pub fn idle(&self) -> bool {
        !self.in_turn && self.unpresented.is_empty()
    }
    /// Record a host message written to the runtime. Sending never implies handling.
    pub fn sent(&mut self, text: &str, event: Option<&str>) {
        self.unpresented.push(text.into());
        if let Some(event) = event {
            self.deliveries
                .push(json!({"event":event,"text":text,"sent_span":self.spans,
                "sent_in_turn":self.in_turn,"turn":self.boundary.turn,"presented_span":null}));
        }
    }
    pub fn line(&mut self, line: &Value) -> Result<()> {
        let kind = line["type"].as_str().unwrap_or("");
        let subtype = line["subtype"].as_str().unwrap_or("");
        let outcome = self.observe(line, kind, subtype);
        if outcome.is_err() {
            self.gate.failed = true;
        }
        if !matches!(subtype, "thinking_tokens" | "hook_started") {
            let mut event = line.clone();
            // Evidence excludes model reasoning.
            if let Some(blocks) = event["message"]["content"].as_array_mut() {
                blocks.retain(|b| {
                    !matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking"))
                });
            }
            self.events.push(event);
        }
        outcome
    }
    fn observe(&mut self, line: &Value, kind: &str, subtype: &str) -> Result<()> {
        match (kind, subtype) {
            ("system", "init") => {
                // The trusted CLI writes `.claude/` into its cwd, so it runs from scratch, beside source.
                verify_init(line, &self.cwd, &self.session, &self.tools)?;
                require(!self.in_turn, "overlapping runtime turn")?;
                self.spans += 1;
                self.in_turn = true;
                self.project = line["memory_paths"]["auto"]
                    .as_str()
                    .and_then(|p| Path::new(p).parent())
                    .map(Path::to_owned);
                self.gate.record("init", true)
            }
            ("system", "model_refusal_fallback") => {
                Err("model refusal fallback; run invalid, publication/scoring disabled".into())
            }
            ("system", subtype) if SYSTEM.contains(&subtype) => Ok(()),
            ("assistant", _) => {
                require(
                    line["message"]["model"].as_str().is_none_or(|m| m == MODEL),
                    "model identity changed; run invalid",
                )?;
                for block in blocks(line).filter(|b| b["type"] == "tool_use") {
                    require(
                        block["name"]
                            .as_str()
                            .is_some_and(|n| self.tools.iter().any(|t| t == n)),
                        "disabled capability attempted",
                    )?;
                }
                Ok(())
            }
            ("user", _) => {
                if line["isReplay"] == true {
                    require(self.in_turn, "presentation outside a turn")?;
                    // Messages queued during one tool call are replayed as one merged message.
                    // shortcut: substring match; host messages carry unique event IDs or prompts,
                    // revisit if one sent text can be a fragment of another.
                    let content = &line["message"]["content"];
                    let text = content
                        .as_str()
                        .map_or_else(|| content.to_string(), String::from);
                    self.unpresented
                        .retain(|sent| !text.contains(sent.as_str()));
                    for delivery in self.deliveries.iter_mut().filter(|d| {
                        d["presented_span"].is_null()
                            && d["text"].as_str().is_some_and(|t| text.contains(t))
                    }) {
                        delivery["presented_span"] = json!(self.spans);
                    }
                }
                Ok(())
            }
            ("result", _) => {
                require(self.in_turn, "result outside a turn")?;
                self.in_turn = false;
                if let Some(turn) = self.boundary.turn.clone() {
                    self.boundary.end(&turn);
                    self.ended.insert(turn);
                }
                self.hooks.clear();
                self.last_result = line.clone();
                self.denials.extend(
                    line["permission_denials"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .cloned(),
                );
                let models = line["modelUsage"]
                    .as_object()
                    .map(|usage| usage.keys().cloned().collect::<Vec<_>>());
                require(
                    models == Some(vec![MODEL.to_owned()]),
                    "model identity changed; run invalid",
                )?;
                self.gate.record("model_identity", true)
            }
            ("rate_limit_event" | "control_response", _) => Ok(()),
            _ => Err(format!("unrecorded stream-json shape {kind}/{subtype}").into()),
        }
    }
    /// A request from the host-launched hook or MCP server over the authenticated host channel.
    pub fn host(&mut self, request: &Value) -> (Value, Option<Notice>) {
        let hook = request["kind"] == "hook";
        match self.mediate(request) {
            Ok(outcome) => outcome,
            Err(error) if hook => (json!({"allow":false,"reason":error.to_string()}), None),
            Err(error) => (json!({"accepted":false,"reason":error.to_string()}), None),
        }
    }
    fn mediate(&mut self, request: &Value) -> Result<(Value, Option<Notice>)> {
        require(
            request["token"].as_str() == Some(&self.token),
            "unauthenticated host channel",
        )?;
        // Hook stamps are refused after a failure too, not only engine calls.
        self.gate.open()?;
        require(
            self.in_turn && request["session"].as_str() == Some(&self.session),
            "call outside the bound runtime session or turn",
        )?;
        let id = request["tool_use_id"]
            .as_str()
            .filter(|x| !x.is_empty())
            .ok_or("missing tool-use identity")?;
        if request["kind"] == "hook" {
            require(
                request["tool_name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("mcp__falinks__"))
                    && request["prompt_id"].as_str().is_some_and(|p| !p.is_empty())
                    && !self.hooks.contains_key(id),
                "unexpected hook stamp",
            )?;
            self.hooks.insert(id.into(), request.clone());
            return Ok((json!({"allow":true}), None));
        }
        require(request["kind"] == "call", "unknown host request")?;
        let stamp = self.hooks.remove(id).ok_or("unstamped tool call")?;
        let tool = request["tool"].as_str().unwrap_or("");
        require(
            stamp["tool_name"] == format!("mcp__falinks__{tool}")
                && stamp["tool_input"] == request["arguments"],
            "hook stamp does not match tool call",
        )?;
        let prompt = stamp["prompt_id"].as_str().unwrap_or("");
        if self.boundary.turn.is_none() {
            require(!self.ended.contains(prompt), "stale runtime turn")?;
            self.boundary.begin(prompt)?;
        }
        let (envelope, outcome) = falinks_host::mediate(
            &mut self.gate,
            &self.boundary,
            &mut self.engine,
            &json!({"threadId":self.session,"turnId":prompt,"callId":id,"tool":tool,
                "arguments":request["arguments"]}),
        )?;
        self.events
            .push(json!({"host_call":envelope,"result":outcome.result}));
        Ok((outcome.result, outcome.notice))
    }
}

pub fn settings(launch: &Launch, tool: &Path) -> Result<Value> {
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut deny = [
        "Edit",
        "Write",
        "NotebookEdit",
        "WebSearch",
        "WebFetch",
        "Agent",
        "Task",
        "Skill",
    ]
    .map(String::from)
    .to_vec();
    let mut hidden = vec![path(&launch.controller)];
    hidden.extend(launch.protected.iter().map(|p| path(p)));
    for name in &hidden {
        // `//` marks an absolute path in permission rules.
        deny.push(format!("Read(/{name}/**)"));
    }
    let mut read_only = vec![path(&launch.source), path(&launch.worker_tools)];
    read_only.extend(hidden.iter().cloned());
    let hook = shlex::try_join([
        tool.to_string_lossy().as_ref(),
        "hook",
        path(&launch.controller).as_str(),
    ])?;
    Ok(json!({
        "permissions":{"defaultMode":"dontAsk","allow":launch.allowed(),"deny":deny},
        "sandbox":{"enabled":true,"autoAllowBashIfSandboxed":true,"allowUnsandboxedCommands":false,
            "failIfUnavailable":true,"filesystem":{"allowWrite":[path(&launch.scratch)],
            "denyWrite":read_only,"denyRead":hidden}},
        "hooks":{"PreToolUse":[{"matcher":"mcp__falinks__.*","hooks":[{"type":"command","command":hook}]}]},
        "env":{"DISABLE_AUTOUPDATER":"1"}
    }))
}
pub fn launch_args(controller: &Path, session: &str, resume: bool) -> Vec<String> {
    let mut args = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--replay-user-messages",
        "--include-hook-events",
        "--model",
        MODEL,
        "--setting-sources",
        "",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--permission-mode",
        "dontAsk",
        "--max-turns",
        "12",
    ]
    .map(String::from)
    .to_vec();
    // Resume restores conversation only; every control is re-supplied by this launch.
    args.extend([
        "--tools".into(),
        NATIVE.join(","),
        "--settings".into(),
        controller
            .join("settings.json")
            .to_string_lossy()
            .into_owned(),
        "--mcp-config".into(),
        controller.join("mcp.json").to_string_lossy().into_owned(),
        if resume { "--resume" } else { "--session-id" }.into(),
        session.into(),
    ]);
    args
}

enum Input {
    Line(Result<Value>),
    Host(Value, Sender<Value>),
}

/// The host channel's accept loop. `stop` ends it and releases its descriptor.
struct Listener {
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Listener {
    fn bind(socket: PathBuf, host: Sender<Input>) -> Result<Self> {
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            for stream in listener.incoming().map_while(|s| s.ok()) {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                let host = host.clone();
                thread::spawn(move || serve(stream, &host));
            }
        });
        Ok(Self {
            socket,
            stop,
            thread: Some(thread),
        })
    }
    /// A wake-up connection lets the blocked accept loop see the flag.
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // shortcut: if the socket file was removed under us the wake-up cannot connect and the
        // thread stays blocked; join only when woken, so close never hangs.
        if let Some(thread) = self.thread.take()
            && UnixStream::connect(&self.socket).is_ok()
        {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.socket);
    }
}

pub struct Runtime {
    pub session: Session,
    /// The exact arguments and settings this launch used.
    pub args: Vec<String>,
    pub settings: Value,
    binary: PathBuf,
    control_host: bool,
    child: Child,
    input: Option<ChildStdin>,
    inputs: Receiver<Input>,
    listener: Listener,
    closed: bool,
}
impl Runtime {
    /// `resume` names a saved runtime session; otherwise the host chooses a fresh one.
    pub fn new(
        binary: &Path,
        launch: &Launch,
        agent: &str,
        resume: Option<&str>,
        engine: Engine,
        control_host: bool,
    ) -> Result<Self> {
        let (binary, _) = verify_binary(binary)?;
        for path in [
            &launch.source,
            &launch.scratch,
            &launch.controller,
            &launch.worker_tools,
        ]
        .into_iter()
        .chain(&launch.protected)
        {
            require(
                path.is_dir() && !path.is_symlink() && path.canonicalize()? == *path,
                "unsupported workspace layout",
            )?;
        }
        let controller = launch.controller.clone();
        let tool = launch.worker_tools.join("falinks-claude-tool");
        let token = random_hex(32)?;
        let _ = fs::remove_file(controller.join("token"));
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(controller.join("token"))?
            .write_all(token.as_bytes())?;
        let id = match resume {
            Some(id) => id.to_owned(),
            None => session_id()?,
        };
        let mut session = Session::new(launch, agent, &id, &token, engine, resume.is_some())?;
        session.gate.record("pin", true)?;
        let settings = settings(launch, &tool)?;
        fs::write(controller.join("settings.json"), settings.to_string())?;
        fs::write(controller.join("tools.json"), launch.tools.to_string())?;
        let server = json!({"mcpServers":{"falinks":{"command":tool,"args":["mcp",controller]}}});
        fs::write(controller.join("mcp.json"), server.to_string())?;
        let (sender, inputs) = mpsc::channel();
        let listener = Listener::bind(controller.join("host.sock"), sender.clone())?;
        let args = launch_args(&controller, &id, resume.is_some());
        let mut command = Command::new(&binary);
        command
            .args(&args)
            .current_dir(&launch.scratch)
            .env("DISABLE_AUTOUPDATER", "1")
            // The Git arm's Go settings: the sandbox denies Go's default cache outside scratch (#112).
            .env("GOCACHE", launch.scratch.join("go-cache"))
            .env("GOTOOLCHAIN", "local")
            .env("GOPROXY", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(controller.join("claude.stderr"))?,
            )
            .process_group(0);
        // Keep an enclosing Claude Code session's identity out of the worker. A
        // `claude setup-token` login (CI) is authentication only, so it passes through.
        for (key, _) in std::env::vars()
            .filter(|(k, _)| k.starts_with("CLAUDE") && k != "CLAUDE_CODE_OAUTH_TOKEN")
        {
            command.env_remove(key);
        }
        let mut child = command.spawn()?;
        let input = child.stdin.take();
        let stdout = child.stdout.take().ok_or("claude stdout missing")?;
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value = line
                    .map_err(|e| e.into())
                    .and_then(|s| serde_json::from_str(&s).map_err(|e| e.into()));
                let failed = value.is_err();
                if sender.send(Input::Line(value)).is_err() || failed {
                    return;
                }
            }
            let _ = sender.send(Input::Line(Err("claude stream closed".into())));
        });
        Ok(Self {
            session,
            args,
            settings,
            binary,
            control_host,
            child,
            input,
            inputs,
            listener,
            closed: false,
        })
    }
    /// Write a user message. Sending never implies presentation or handling.
    pub fn send(&mut self, text: String, event: Option<&str>) -> Result<()> {
        let input = self.input.as_mut().ok_or("claude connection closed")?;
        writeln!(
            input,
            "{}",
            json!({"type":"user","message":{"role":"user","content":text}})
        )?;
        input.flush()?;
        self.session.sent(&text, event);
        Ok(())
    }
    fn pump(&mut self, deadline: Instant) -> Result<()> {
        if self.step(deadline.saturating_duration_since(Instant::now()))? {
            return Ok(());
        }
        self.session.gate.failed = true;
        Err("claude timed out".into())
    }
    /// Serve one runtime line or host request, waiting at most `timeout`.
    /// `Ok(false)`: nothing arrived. Lets one host thread drive several workers.
    pub fn step(&mut self, timeout: Duration) -> Result<bool> {
        let input = match self.inputs.recv_timeout(timeout) {
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(false),
            other => other,
        };
        match input {
            Err(error) => {
                self.session.gate.failed = true;
                Err(format!("claude disconnected: {error}").into())
            }
            Ok(Input::Line(Err(error))) => {
                self.session.gate.failed = true;
                Err(error)
            }
            Ok(Input::Line(Ok(line))) => {
                let observed = self.session.line(&line);
                // Fail closed: a runtime mismatch also stops native tools, not only engine calls.
                if observed.is_err() {
                    self.close();
                }
                observed.map(|()| true)
            }
            Ok(Input::Host(request, reply)) => {
                let blocked = (request["kind"] == "call" && !self.control_host)
                    .then(|| self.require_supported().err())
                    .flatten();
                let (value, notice) = match blocked {
                    Some(error) => (json!({"accepted":false,"reason":error.to_string()}), None),
                    None => self.session.host(&request),
                };
                // Persist and send before the tool result returns: presentation follows later.
                if let Some(notice) = notice {
                    self.attention(&notice.event, &notice.context)?;
                }
                let _ = reply.send(value);
                Ok(true)
            }
        }
    }
    pub fn start(&mut self, prompt: &str) -> Result<()> {
        let pending = self.session.boundary.pending()?;
        self.send(
            format!("Host pending/deferred context: {pending}\n{prompt}"),
            None,
        )
    }
    /// Persist the obligation, then send. Claude Code has no exact-turn guard: the message
    /// is presented at the next tool-result boundary or a new turn, and handling is never inferred.
    pub fn attention(&mut self, event: &str, context: &Value) -> Result<()> {
        self.session.boundary.enqueue(event, context)?;
        let pending = self.session.boundary.pending()?;
        self.send(
            format!("Host notice {event}. Pending/deferred context: {pending}"),
            Some(event),
        )
    }
    /// Wait until every host message is presented and the runtime's turn has ended.
    pub fn wait(&mut self) -> Result<()> {
        // shortcut: fixed bound for short fixture turns; check-integration drives `step` under its own phase bound.
        let deadline = Instant::now() + Duration::from_secs(600);
        while !self.session.idle() {
            self.pump(deadline)?;
        }
        require(
            self.session.last_result["subtype"] == "success",
            "turn did not complete",
        )
    }
    pub fn require_supported(&mut self) -> Result<()> {
        self.session.gate.require_supported()?;
        require(self.child.try_wait()?.is_none(), "claude disconnected")?;
        verify_binary(&self.binary)?;
        Ok(())
    }
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.session.gate.failed = true;
        self.input.take();
        shut_down(
            &mut self.child,
            Duration::from_secs(20),
            Duration::from_secs(5),
        );
        self.listener.stop();
    }
}

/// After input is closed, wait `eof_grace` for exit, then `SIGTERM` the process group so the
/// worker can flush its session, wait `term_grace`, then `SIGKILL` the group.
fn shut_down(child: &mut Child, eof_grace: Duration, term_grace: Duration) {
    let exited = |child: &mut Child, grace: Duration| {
        let deadline = Instant::now() + grace;
        while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        !matches!(child.try_wait(), Ok(None))
    };
    // SAFETY: the child was started in its own process group; this targets its descendants only.
    let signal = |child: &Child, signal| unsafe {
        libc::kill(-(child.id() as i32), signal);
    };
    if exited(child, eof_grace) {
        return;
    }
    signal(child, libc::SIGTERM);
    exited(child, term_grace);
    // Even when the leader exited on `SIGTERM`, members that ignored it are still in the group.
    signal(child, libc::SIGKILL);
    let _ = child.wait();
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.close();
    }
}

fn serve(stream: UnixStream, host: &Sender<Input>) {
    let mut line = String::new();
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    if BufReader::new(stream).read_line(&mut line).is_err() {
        return;
    }
    let Ok(request) = serde_json::from_str(&line) else {
        return;
    };
    let (reply, answer) = mpsc::channel();
    if host.send(Input::Host(request, reply)).is_err() {
        return;
    }
    if let Ok(value) = answer.recv_timeout(Duration::from_secs(600)) {
        let _ = writeln!(writer, "{value}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn a_stopped_listener_ends_its_accept_thread() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, inputs) = mpsc::channel();
        let mut listener = Listener::bind(dir.path().join("host.sock"), sender).unwrap();
        listener.stop();
        // The accept thread held the last sender: the channel closes only once it exits.
        assert!(matches!(
            inputs.recv_timeout(Duration::from_secs(5)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        assert!(!dir.path().join("host.sock").exists());
    }

    /// A shell group leader that ignores input EOF and loops after running `prelude`.
    fn looping_group(dir: &Path, prelude: &str) -> Child {
        let child = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "{prelude} touch started; while :; do sleep 0.05; done"
            ))
            .current_dir(dir)
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.join("started").exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        child
    }

    #[test]
    fn shutdown_sends_term_to_the_group_before_kill() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = looping_group(dir.path(), "trap 'touch flushed; exit 0' TERM;");
        shut_down(
            &mut child,
            Duration::from_millis(100),
            Duration::from_secs(5),
        );
        assert!(dir.path().join("flushed").exists());
        assert_eq!(child.try_wait().unwrap().unwrap().code(), Some(0));
    }

    #[test]
    fn shutdown_kills_a_group_that_ignores_term() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = looping_group(dir.path(), "trap '' TERM;");
        shut_down(
            &mut child,
            Duration::from_millis(100),
            Duration::from_millis(200),
        );
        let status = child.try_wait().unwrap().unwrap();
        assert_eq!(status.signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn shutdown_kills_group_members_that_outlive_a_terminated_leader() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = looping_group(
            dir.path(),
            "sh -c 'trap \"\" TERM; while :; do sleep 0.05; done' >/dev/null 2>&1 & echo $! > member; trap 'exit 0' TERM;",
        );
        let member: i32 = fs::read_to_string(dir.path().join("member"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        shut_down(
            &mut child,
            Duration::from_millis(100),
            Duration::from_secs(5),
        );
        // The orphaned member is reaped by init, so wait for it to disappear.
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: signal 0 only checks that the pid exists.
        let alive = || unsafe { libc::kill(member, 0) } == 0;
        while alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!alive(), "group member survived");
    }
}
