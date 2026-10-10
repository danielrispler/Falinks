//! Pinned Claude Code worker adapter: the #23 boundary ported to stream-json.
//! Runtime-independent pieces (`Boundary`, `ControlGate`, `ControlledHost`) come from `falinks-host`.
use falinks_host::{
    Boundary, ControlGate, Result, file_hash, require,
    runtime::{Engine, Notice},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::OpenOptionsExt,
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

pub const PIN: &str = "6eab8333fe2121553100d8f40bfada384a3e989b94f947e18ba6677a6fcb41ea";
pub const VERSION: &str = "2.1.287";
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

pub fn verify_binary(binary: &Path) -> Result<PathBuf> {
    let binary = binary.canonicalize()?;
    // Hash first: an unpinned executable is never run, and no other installed CLI is tried.
    require(
        cfg!(target_os = "macos") && file_hash(&binary)? == PIN,
        "unsupported platform or claude SHA-256; publication/scoring disabled",
    )?;
    let mut command = Command::new(&binary);
    command.arg("--version").env("DISABLE_AUTOUPDATER", "1");
    let version = falinks_host::command_output(command, Duration::from_secs(20))?;
    require(
        version.status.success()
            && String::from_utf8(version.stdout)?.trim() == format!("{VERSION} (Claude Code)"),
        "unsupported claude version",
    )?;
    Ok(binary)
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
            tools: falinks_host::runtime::tools(&source.to_string_lossy()),
            source,
            scratch: root.join("scratch"),
            controller: root.join("controller"),
            worker_tools: root.join("worker-tools"),
            protected: ["snapshots", "validation"].map(|n| root.join(n)).to_vec(),
        }
    }
    /// `falinks_<operation>` names offered by this launch.
    pub fn operations(&self) -> Vec<String> {
        self.tools
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str()?.strip_prefix("falinks_"))
            .map(String::from)
            .collect()
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

/// Every turn's `system/init` must match the pinned launch profile exactly.
pub fn verify_init(init: &Value, cwd: &Path, session: &str, tools: &[String]) -> Result<()> {
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
            && init["cwd"].as_str().map(Path::new) == Some(cwd)
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

fn random_hex(bytes: usize) -> Result<String> {
    let mut random = vec![0u8; bytes];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    Ok(random.iter().map(|x| format!("{x:02x}")).collect())
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
    sent: u64,
    presented: u64,
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
        )?;
        boundary.operations = launch.operations();
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
            sent: 0,
            presented: 0,
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
    /// Between `init` and `result`: a message sent now is presented within this turn.
    pub fn in_turn(&self) -> bool {
        self.in_turn
    }
    /// The runtime has presented every host message and finished its turn.
    pub fn idle(&self) -> bool {
        !self.in_turn && self.presented >= self.sent
    }
    /// Record a host message written to the runtime. Sending never implies handling.
    pub fn sent(&mut self, text: &str, event: Option<&str>) {
        self.sent += 1;
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
                let blocks = line["message"]["content"].as_array().into_iter().flatten();
                for block in blocks.filter(|b| b["type"] == "tool_use") {
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
                    self.presented += 1;
                    let text = &line["message"]["content"];
                    if let Some(delivery) = self
                        .deliveries
                        .iter_mut()
                        .find(|d| d["text"] == *text && d["presented_span"].is_null())
                    {
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
        require(
            !self.gate.failed,
            "runtime safety failure; engine calls blocked",
        )?;
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
        let envelope = self.boundary.authenticate(&json!({"threadId":self.session,
            "turnId":prompt,"callId":id,"tool":tool,"arguments":request["arguments"]}))?;
        let outcome = match (self.engine)(&envelope) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.gate.failed = true;
                return Err(error);
            }
        };
        self.boundary.record_result(&envelope, &outcome.result)?;
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
        "--tools",
        "Bash,Read,Glob,Grep",
        "--permission-mode",
        "dontAsk",
        "--max-turns",
        "12",
    ]
    .map(String::from)
    .to_vec();
    // Resume restores conversation only; every control is re-supplied by this launch.
    args.extend([
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

pub struct Runtime {
    pub session: Session,
    binary: PathBuf,
    control_host: bool,
    child: Child,
    input: Option<ChildStdin>,
    inputs: Receiver<Input>,
    socket: PathBuf,
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
        let binary = verify_binary(binary)?;
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
        fs::write(
            controller.join("settings.json"),
            settings(launch, &tool)?.to_string(),
        )?;
        fs::write(controller.join("tools.json"), launch.tools.to_string())?;
        let server = json!({"mcpServers":{"falinks":{"command":tool,"args":["mcp",controller]}}});
        fs::write(controller.join("mcp.json"), server.to_string())?;
        let (sender, inputs) = mpsc::channel();
        let socket = controller.join("host.sock");
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        let host = sender.clone();
        thread::spawn(move || {
            for stream in listener.incoming().map_while(|s| s.ok()) {
                let host = host.clone();
                thread::spawn(move || serve(stream, &host));
            }
        });
        let mut command = Command::new(&binary);
        command
            .args(launch_args(&controller, &id, resume.is_some()))
            .current_dir(&launch.scratch)
            .env("DISABLE_AUTOUPDATER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(controller.join("claude.stderr"))?,
            )
            .process_group(0);
        // Keep an enclosing Claude Code session's identity out of the worker.
        for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("CLAUDE")) {
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
            binary,
            control_host,
            child,
            input,
            inputs,
            socket,
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
        // shortcut: fixed bound for short fixture turns; production integration sets per-task budgets.
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
        let deadline = Instant::now() + Duration::from_secs(20);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            // SAFETY: the child was started in its own process group; this targets its descendants only.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.wait();
        }
        let _ = fs::remove_file(&self.socket);
    }
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
