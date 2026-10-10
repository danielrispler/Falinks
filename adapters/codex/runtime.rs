use crate::{Boundary, ControlGate, HOST_PIN, PIN, Result, file_hash, require};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub const DISABLED: &[&str] = &[
    "apps",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "computer_use",
    "image_generation",
    "multi_agent",
    "web_search_request",
    "code_mode",
];
pub struct Notice {
    pub event: String,
    pub context: Value,
}
pub struct EngineOutcome {
    pub result: Value,
    pub notice: Option<Notice>,
}
pub type Engine = Box<dyn FnMut(&Value) -> Result<EngineOutcome>>;

pub fn configuration(root: &Path, binary: &Path) -> Vec<String> {
    let paths = [
        (root.to_owned(), "deny"),
        (root.join("source"), "read"),
        (root.join("scratch"), "write"),
        (root.join("worker-tools"), "read"),
        (binary.parent().unwrap().to_owned(), "read"),
    ];
    let filesystem = paths
        .iter()
        .map(|(path, mode)| format!("{}={}", json!(path), json!(mode)))
        .collect::<Vec<_>>()
        .join(",");
    let mut args = vec![
        "-c".into(),
        "analytics.enabled=false".into(),
        "-c".into(),
        "default_permissions=\"falinks\"".into(),
        "-c".into(),
        format!(
            "permissions.falinks={{extends=\":workspace\",filesystem={{{filesystem}}},network={{enabled=false}}}}"
        ),
        "-c".into(),
        format!("sqlite_home={}", json!(root.join("worker-state"))),
        "-c".into(),
        format!("log_dir={}", json!(root.join("worker-logs"))),
        "-c".into(),
        "mcp_servers={}".into(),
        "-c".into(),
        "web_search=\"disabled\"".into(),
    ];
    for feature in DISABLED {
        args.extend(["--disable".into(), (*feature).into()]);
    }
    args
}

pub fn verify_binary(binary: &Path) -> Result<PathBuf> {
    let binary = binary.canonicalize()?;
    require(
        cfg!(target_os = "macos") && file_hash(&binary)? == PIN,
        "unsupported platform or binary SHA-256; publication/scoring disabled",
    )?;
    require(
        file_hash(&binary.with_file_name("codex-code-mode-host"))? == HOST_PIN,
        "missing or mismatched pinned code-mode host",
    )?;
    let mut command = Command::new(&binary);
    command.arg("--version");
    let version = crate::command_output(command, Duration::from_secs(10))?;
    require(
        version.status.success()
            && String::from_utf8(version.stdout)?.trim() == "codex-cli 0.160.0",
        "unsupported version",
    )?;
    Ok(binary)
}
fn verify_schema(binary: &Path) -> Result<Value> {
    let expected: BTreeMap<String, String> =
        serde_json::from_str(include_str!("protocol-pin.json"))?;
    let dir = tempfile::tempdir()?;
    let mut command = Command::new(binary);
    command
        .args([
            "app-server",
            "generate-json-schema",
            "--experimental",
            "--out",
        ])
        .arg(dir.path());
    let out = crate::command_output(command, Duration::from_secs(30))?;
    require(out.status.success(), "schema generation failed")?;
    let actual = expected
        .keys()
        .map(|name| Ok((name.clone(), file_hash(&dir.path().join(name))?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    require(actual == expected, "protocol schema mismatch")?;
    Ok(json!(actual))
}
pub fn tools(workspace: &str) -> Value {
    json!(["edit","review","offer"].map(|operation|json!({"name":format!("falinks_{operation}"),"description":format!("Submit an engine {operation}. Workspace: {workspace}"),"inputSchema":{"type":"object","properties":{"workspace":{"type":"string"},"request":{"type":"object"}},"required":["workspace","request"],"additionalProperties":false}})))
}

pub struct Runtime {
    pub boundary: Boundary,
    pub gate: ControlGate,
    pub events: Vec<Value>,
    pub deliveries: Vec<Value>,
    pub schema: Value,
    root: PathBuf,
    binary: PathBuf,
    engine: Engine,
    control_host: bool,
    child: Child,
    input: Option<ChildStdin>,
    messages: Receiver<Result<Value>>,
    sequence: u64,
    responses: BTreeMap<u64, Value>,
    closed: bool,
}
impl Runtime {
    pub fn new(binary: &Path, root: &Path, engine: Engine, control_host: bool) -> Result<Self> {
        let binary = verify_binary(binary)?;
        let root = root.canonicalize()?;
        for name in [
            "source",
            "scratch",
            "controller",
            "snapshots",
            "validation",
            "worker-home",
            "worker-tools",
        ] {
            let path = root.join(name);
            require(
                path.is_dir() && !path.is_symlink() && path.canonicalize()? == path,
                "unsupported workspace layout",
            )?;
        }
        let schema = verify_schema(&binary)?;
        let mut random = [0u8; 16];
        fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let boundary = Boundary::new(
            &root.join("controller/context.sqlite"),
            &root.join("source"),
            crate::hash(&random),
        )?;
        let mut child = Command::new(&binary)
            .args(["app-server", "--stdio"])
            .args(configuration(&root, &binary))
            .env("CODEX_HOME", root.join("worker-home"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(root.join("controller/server.stderr"))?,
            )
            .process_group(0)
            .spawn()?;
        let input = child.stdin.take();
        let stdout = child.stdout.take().ok_or("app-server stdout missing")?;
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value = line
                    .map_err(|e| e.into())
                    .and_then(|s| serde_json::from_str(&s).map_err(|e| e.into()));
                let failed = value.is_err();
                if sender.send(value).is_err() || failed {
                    break;
                }
            }
        });
        let mut runtime = Self {
            boundary,
            gate: ControlGate::default(),
            events: vec![],
            deliveries: vec![],
            schema,
            root,
            binary,
            engine,
            control_host,
            child,
            input,
            messages,
            sequence: 0,
            responses: BTreeMap::new(),
            closed: false,
        };
        runtime.gate.record("schema", true)?;
        runtime.request("initialize",json!({"clientInfo":{"name":"falinks_rust_adapter","version":"1"},"capabilities":{"experimentalApi":true}}))?;
        runtime.send(json!({"method":"initialized","params":{}}))?;
        Ok(runtime)
    }
    fn send(&mut self, item: Value) -> Result<()> {
        let input = self.input.as_mut().ok_or("app-server connection closed")?;
        writeln!(input, "{item}")?;
        input.flush()?;
        Ok(())
    }
    fn pump(&mut self, deadline: Instant) -> Result<()> {
        let value = match self
            .messages
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(value) => value,
            Err(error) => {
                self.gate.failed = true;
                return Err(format!("app-server disconnected or timed out: {error}").into());
            }
        };
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                self.gate.failed = true;
                return Err(error);
            }
        };
        self.handle(value)
    }
    fn handle(&mut self, item: Value) -> Result<()> {
        let method = item["method"].as_str().unwrap_or("");
        let params = &item["params"];
        if method.is_empty() {
            let id = item["id"]
                .as_u64()
                .ok_or("unknown runtime response identity")?;
            self.responses.insert(id, item);
        } else if !item["id"].is_null() {
            if method == "item/tool/call" {
                let outcome = (|| -> Result<EngineOutcome> {
                    require(
                        !self.gate.failed,
                        "runtime safety failure; engine calls blocked",
                    )?;
                    if !self.control_host {
                        self.require_supported()?;
                    }
                    let envelope = self.boundary.authenticate(params)?;
                    let outcome = match (self.engine)(&envelope) {
                        Ok(result) => result,
                        Err(error) => {
                            self.gate.failed = true;
                            return Err(error);
                        }
                    };
                    self.boundary.record_result(&envelope, &outcome.result)?;
                    Ok(outcome)
                })();
                let result = match outcome {
                    Ok(outcome) => {
                        if let Some(notice) = outcome.notice {
                            self.attention(&notice.event, &notice.context)?;
                        }
                        outcome.result
                    }
                    Err(error) => json!({"accepted":false,"reason":error.to_string()}),
                };
                self.send(json!({"id":item["id"],"result":{"contentItems":[{"type":"inputText","text":result.to_string()}],"success":result["accepted"]==true}}))?;
                self.events
                    .push(json!({"method":method,"params":params,"result":result}));
            } else {
                let response = match method {
                    "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                        json!({"id":item["id"],"result":{"decision":"decline"}})
                    }
                    "item/permissions/requestApproval" => {
                        json!({"id":item["id"],"result":{"permissions":{},"scope":"turn"}})
                    }
                    _ => {
                        json!({"id":item["id"],"error":{"code":-32601,"message":"Host capability disabled"}})
                    }
                };
                self.send(response)?;
            }
        } else if method == "turn/started"
            && params["threadId"].as_str() == self.boundary.thread.as_deref()
        {
            if self.boundary.turn.is_none() {
                self.boundary.begin(
                    params["turn"]["id"]
                        .as_str()
                        .ok_or("missing turn identity")?,
                )?;
            }
        } else if method == "turn/completed"
            && params["threadId"].as_str() == self.boundary.thread.as_deref()
        {
            self.boundary.end(
                params["turn"]["id"]
                    .as_str()
                    .ok_or("missing turn identity")?,
            );
            self.events.push(item);
        } else if method == "item/completed" {
            match params["item"]["type"].as_str().unwrap_or("") {
                "commandExecution" | "fileChange" | "dynamicToolCall" | "agentMessage" => {
                    self.events.push(item)
                }
                "mcpToolCall" | "webSearch" | "imageGeneration" | "collabAgentToolCall" => {
                    self.gate.failed = true;
                    return Err("disabled capability attempted".into());
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.sequence += 1;
        let id = self.sequence;
        self.send(json!({"id":id,"method":method,"params":params}))?;
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.responses.contains_key(&id) {
            self.pump(deadline)?;
            if Instant::now() >= deadline {
                self.gate.failed = true;
                return Err("runtime request timeout".into());
            }
        }
        let reply = self.responses.remove(&id).unwrap();
        if !reply["error"].is_null() {
            self.events.push(json!({"method":"host/rpc-error","params":{"method":method,"error":reply["error"]}}));
            return Err(format!("{method}: {}", reply["error"]).into());
        }
        Ok(reply["result"].clone())
    }
    pub fn register(&mut self, agent: &str, thread: Option<&str>) -> Result<Value> {
        let mut params = json!({"cwd":self.root.join("source"),"runtimeWorkspaceRoots":[self.root],"permissions":"falinks","approvalPolicy":"never"});
        if let Some(thread) = thread {
            params["threadId"] = json!(thread);
            params["excludeTurns"] = json!(true);
        } else {
            params["dynamicTools"] = tools(&self.boundary.workspace);
            params["ephemeral"] = json!(false);
        }
        let result = self.request(
            if thread.is_some() {
                "thread/resume"
            } else {
                "thread/start"
            },
            params,
        )?;
        let matches = result["activePermissionProfile"]["id"] == "falinks"
            && result["approvalPolicy"] == "never"
            && result["cwd"] == json!(self.root.join("source"))
            && result["runtimeWorkspaceRoots"] == json!([self.root]);
        if !matches {
            self.gate.failed = true;
            return Err("effective runtime profile/workspace mismatch".into());
        }
        self.boundary.bind(
            result["thread"]["id"]
                .as_str()
                .ok_or("missing thread identity")?,
            agent,
            thread.is_some(),
        )?;
        Ok(result)
    }
    pub fn start(&mut self, prompt: &str) -> Result<String> {
        let result=self.request("turn/start",json!({"threadId":self.boundary.thread,"permissions":"falinks","approvalPolicy":"never","input":[{"type":"text","text":format!("Host pending/deferred context: {}\n{prompt}",self.boundary.pending()?)}]}))?;
        let turn = result["turn"]["id"]
            .as_str()
            .ok_or("missing turn identity")?
            .to_owned();
        if self.boundary.turn.is_none() {
            self.boundary.begin(&turn)?;
        }
        Ok(turn)
    }
    pub fn attention(&mut self, event: &str, context: &Value) -> Result<&'static str> {
        self.boundary.enqueue(event, context)?;
        let Some(turn) = self.boundary.turn.clone() else {
            self.deliveries
                .push(json!({"event":event,"delivery":"next-boundary"}));
            return Ok("next-boundary");
        };
        let response=self.request("turn/steer",json!({"threadId":self.boundary.thread,"expectedTurnId":turn,"input":[{"type":"text","text":format!("Host pending/deferred context: {}",self.boundary.pending()?)}]}));
        let delivery = match response {
            Ok(result) => {
                require(
                    result["turnId"] == turn,
                    "steering accepted for a different turn",
                )?;
                "steered"
            }
            Err(error) => {
                let message = error.to_string().to_lowercase();
                if message.starts_with("turn/steer:")
                    && ["no active turn", "does not match", "not active"]
                        .iter()
                        .any(|text| message.contains(text))
                {
                    "next-boundary"
                } else {
                    return Err(error);
                }
            }
        };
        self.deliveries
            .push(json!({"event":event,"expectedTurnId":turn,"delivery":delivery}));
        Ok(delivery)
    }
    pub fn wait(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(180);
        while self.boundary.turn.is_some() {
            if Instant::now() >= deadline {
                self.gate.failed = true;
                return Err("bounded turn deadline exceeded".into());
            }
            self.pump(deadline)?;
        }
        let completed = self
            .events
            .iter()
            .rev()
            .find(|event| event["method"] == "turn/completed")
            .ok_or("missing completion")?;
        require(
            completed["params"]["turn"]["status"] == "completed",
            "turn did not complete",
        )
    }
    pub fn require_supported(&mut self) -> Result<()> {
        self.gate.require_supported()?;
        require(self.child.try_wait()?.is_none(), "app-server disconnected")?;
        verify_binary(&self.binary)?;
        Ok(())
    }
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.gate.failed = true;
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            // SAFETY: the child was started in its own process group; this targets its descendants only.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGTERM);
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            if matches!(self.child.try_wait(), Ok(None)) {
                // SAFETY: escalate only within the same dedicated child process group.
                unsafe {
                    libc::kill(-(self.child.id() as i32), libc::SIGKILL);
                }
            }
            let _ = self.child.wait();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.close();
    }
}
