//! Shared trusted-host library for the worker-runtime adapters. The engine envelope is
//! independent of the runtime.
pub mod check_binary;
pub mod controlled_host;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
pub struct Notice {
    pub event: String,
    pub context: Value,
}
pub struct EngineOutcome {
    pub result: Value,
    pub notice: Option<Notice>,
}
pub type Engine = Box<dyn FnMut(&Value) -> Result<EngineOutcome>>;
/// Verified pins keyed by `std::env::consts::{OS, ARCH}`. A platform is declared only after
/// its adapter controls were re-run there (review-enforced); every other platform fails closed.
/// Promotion never edits a test: the pin tests derive from the map and check its docs row.
pub type PinMap<T> = &'static [((&'static str, &'static str), T)];

pub fn platform_pin<T: Copy>(pins: PinMap<T>, os: &str, arch: &str) -> Result<T> {
    pins.iter()
        .find(|(platform, _)| *platform == (os, arch))
        .map(|(_, pin)| *pin)
        .ok_or_else(|| {
            format!("unsupported platform {os}/{arch}; publication/scoring disabled").into()
        })
}
/// Verifies a pinned runtime on this platform: pin, canonicalize, hash, then `--version`
/// (with `env`) under a timeout must print exactly `version`. Returns the canonical binary
/// and the digest it verified.
pub fn verify_pinned(
    binary: &Path,
    pins: PinMap<&'static str>,
    version: &str,
    env: &[(&str, &str)],
) -> Result<(PathBuf, &'static str)> {
    let pin = platform_pin(pins, std::env::consts::OS, std::env::consts::ARCH)?;
    let binary = binary.canonicalize()?;
    // Hash first: an unpinned executable is never run, and no other installed CLI is tried.
    require(
        file_hash(&binary)? == pin,
        "unpinned runtime SHA-256; publication/scoring disabled",
    )?;
    let mut command = std::process::Command::new(&binary);
    command.arg("--version").envs(env.iter().copied());
    let output = command_output(command, std::time::Duration::from_secs(20))?;
    require(
        output.status.success() && String::from_utf8(output.stdout)?.trim() == version,
        "unsupported runtime version",
    )?;
    Ok((binary, pin))
}
/// The `docs/platforms.md` columns, as `std::env::consts::{OS, ARCH}`.
pub const PLATFORMS: &[(&str, &str)] = &[
    ("macos", "aarch64"),
    ("macos", "x86_64"),
    ("linux", "x86_64"),
    ("linux", "aarch64"),
];
/// The adapter's row in `docs/platforms.md`, derived from its pins so the two cannot drift.
pub fn platforms_row<T>(component: &str, pins: PinMap<T>) -> String {
    for (platform, _) in pins {
        assert!(
            PLATFORMS.contains(platform),
            "{platform:?} has no docs column"
        );
    }
    let cells: Vec<&str> = PLATFORMS
        .iter()
        .map(
            |platform| match pins.iter().any(|(pinned, _)| pinned == platform) {
                true => "Supported",
                false => "Not yet verified (fails closed)",
            },
        )
        .collect();
    format!("| {component} | {} |", cells.join(" | "))
}

pub fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
/// Streams the file: pinned runtimes are hundreds of megabytes.
pub fn file_hash(path: &Path) -> Result<String> {
    use std::io::BufRead;
    let mut hasher = Sha256::new();
    let mut file = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
    loop {
        let chunk = file.fill_buf()?;
        if chunk.is_empty() {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(chunk);
        let read = chunk.len();
        file.consume(read);
    }
}
/// `bytes` random bytes as lowercase hex: opaque tokens, nonces and identities.
pub fn random_hex(bytes: usize) -> Result<String> {
    let mut random = vec![0u8; bytes];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut random)?;
    Ok(hex(&random))
}

/// `sandbox-probe` arguments: the source file the worker may only read, its expected hash,
/// the writable scratch directory and each `label=file` storage the worker may not touch.
pub fn probe_args(
    probe: &Path,
    source: &Path,
    expected_hash: &str,
    scratch: &Path,
    protected: &[(&str, &Path)],
) -> Vec<String> {
    let mut args = [probe, source]
        .map(|p| p.to_string_lossy().into_owned())
        .to_vec();
    args.push(expected_hash.into());
    args.push(scratch.to_string_lossy().into_owned());
    args.extend(
        protected
            .iter()
            .map(|(label, file)| format!("{label}={}", file.display())),
    );
    args
}
/// `sandbox-probe` arguments for the historical fixture layout (`ROOT/{source,scratch,...}`).
pub fn fixture_probe_args(root: &Path, expected: &str) -> Vec<String> {
    let secrets =
        ["controller", "snapshots", "validation"].map(|n| (n, root.join(n).join("secret.txt")));
    let protected: Vec<(&str, &Path)> = secrets.iter().map(|(n, p)| (*n, p.as_path())).collect();
    probe_args(
        &root.join("worker-tools/sandbox-probe"),
        &root.join("source/source.txt"),
        &hash(expected.as_bytes()),
        &root.join("scratch"),
        &protected,
    )
}
/// Every check `sandbox-probe` reports for the given protected storage labels.
pub fn probe_names(protected: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = [
        "read",
        "scratch",
        "source_write",
        "source_delete",
        "source_rename",
        "source_create",
        "source_replace",
        "alias_write",
        "child_write",
    ]
    .map(String::from)
    .to_vec();
    for label in protected {
        names.extend([format!("{label}_read"), format!("{label}_write")]);
    }
    names
}
/// The gate controls a passing probe proves.
pub const PROBE_CONTROLS: &[&str] = &[
    "source_write_denial",
    "storage_protection",
    "scratch_access",
];
/// The controls in a probe's `stdout` for the exact `args` it ran with, or `None` without a
/// `FALINKS_CONTROLS` line. Every check for the protected labels in `args` must pass.
pub fn probe_controls(stdout: &str, args: &[String]) -> Result<Option<Value>> {
    let Some(text) = stdout
        .lines()
        .find_map(|l| l.strip_prefix("FALINKS_CONTROLS="))
    else {
        return Ok(None);
    };
    let mut checks: Value = serde_json::from_str(text)?;
    let protected: Vec<(&str, &str)> = args
        .iter()
        .skip(4)
        .filter_map(|a| a.split_once('='))
        .collect();
    // On Linux the sandbox hides protected storage behind an empty tmpfs, so a probe
    // write there succeeds into the mask. The host decides: a protected write passes
    // only if the real file still exists and never received the probe's bytes.
    for (label, secret) in &protected {
        let unchanged = std::fs::read(secret)? != b"BYPASS";
        if let Some(write) = checks.get_mut(format!("{label}_write")) {
            *write = json!(unchanged);
        }
    }
    let names = probe_names(
        &protected
            .iter()
            .map(|(label, _)| *label)
            .collect::<Vec<_>>(),
    );
    require(
        checks.as_object().is_some_and(|c| {
            c.len() == names.len() && names.iter().all(|n| c.get(n) == Some(&json!(true)))
        }),
        &format!("filesystem controls failed: {checks}"),
    )?;
    Ok(Some(checks))
}

pub struct ControlGate {
    pub controls: BTreeMap<String, bool>,
    pub failed: bool,
    required: &'static [&'static str],
}
impl ControlGate {
    /// A runtime adapter names its own required control set.
    pub fn new(required: &'static [&'static str]) -> Self {
        Self {
            controls: BTreeMap::new(),
            failed: false,
            required,
        }
    }
    pub fn record(&mut self, name: &str, passed: bool) -> Result<()> {
        require(self.required.contains(&name), "unknown safety control")?;
        self.controls.insert(name.into(), passed);
        self.failed |= !passed;
        Ok(())
    }
    /// Refuses once any control has failed: the failure is latched.
    pub fn open(&self) -> Result<()> {
        require(!self.failed, "runtime safety failure; engine calls blocked")
    }
    pub fn require_supported(&self) -> Result<()> {
        require(
            !self.failed
                && self.controls.len() == self.required.len()
                && self.controls.values().all(|x| *x),
            "fresh safety controls required; publication/scoring disabled",
        )
    }
}

/// One MCP tool definition: `falinks_<operation>` taking `{workspace, request}`, the
/// argument shape `Boundary::authenticate` accepts.
pub fn tool(operation: &str, description: String) -> Value {
    json!({"name":format!("falinks_{operation}"),"description":description,
        "inputSchema":{"type":"object","properties":{"workspace":{"type":"string"},"request":{"type":"object"}},
            "required":["workspace","request"],"additionalProperties":false}})
}
/// The historical edit/review/offer tools of the fixture host.
pub fn tools(workspace: &str) -> Value {
    json!(["edit", "review", "offer"].map(|operation| tool(
        operation,
        format!("Submit an engine {operation}. Workspace: {workspace}")
    )))
}
/// The `<operation>` of every `falinks_<operation>` in an MCP tool list.
pub fn operations(tools: &Value) -> Vec<String> {
    tools
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str()?.strip_prefix("falinks_"))
        .map(String::from)
        .collect()
}

pub struct Boundary {
    db: Connection,
    pub workspace: String,
    session: String,
    pub thread: Option<String>,
    agent: Option<String>,
    pub turn: Option<String>,
    /// Registered `falinks_<operation>` tools; anything else is refused.
    pub operations: Vec<String>,
}
impl Boundary {
    pub fn new(database: &Path, workspace: &Path, session: String) -> Result<Self> {
        let db = Connection::open(database)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS binding(thread TEXT PRIMARY KEY,agent TEXT,workspace TEXT); CREATE TABLE IF NOT EXISTS context(thread TEXT,event TEXT,payload TEXT,status TEXT,PRIMARY KEY(thread,event));")?;
        let workspace = workspace.canonicalize()?.to_string_lossy().into_owned();
        Ok(Self {
            db,
            operations: operations(&tools(&workspace)),
            workspace,
            session,
            thread: None,
            agent: None,
            turn: None,
        })
    }
    pub fn bind(&mut self, thread: &str, agent: &str, resume: bool) -> Result<()> {
        let row: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT agent,workspace FROM binding WHERE thread=?",
                [thread],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let expected = (agent.to_owned(), self.workspace.clone());
        require(
            !resume || row.as_ref() == Some(&expected),
            "resume identity/workspace mismatch",
        )?;
        require(
            row.is_none() || row.as_ref() == Some(&expected),
            "thread already bound",
        )?;
        self.db.execute(
            "INSERT OR IGNORE INTO binding VALUES(?,?,?)",
            params![thread, agent, self.workspace],
        )?;
        self.thread = Some(thread.into());
        self.agent = Some(agent.into());
        self.turn = None;
        Ok(())
    }
    /// Host-applied regrouping moved this agent: rebind its thread to this boundary's
    /// workspace so resume accepts the new root. Never an agent capability.
    pub fn relocate(&self, thread: &str, agent: &str) -> Result<()> {
        let changed = self.db.execute(
            "UPDATE binding SET workspace=? WHERE thread=? AND agent=?",
            params![self.workspace, thread, agent],
        )?;
        require(changed == 1, "no binding to relocate")
    }
    pub fn begin(&mut self, turn: &str) -> Result<()> {
        require(
            self.thread.is_some() && self.turn.is_none() && !turn.is_empty(),
            "unbound thread or overlapping turn",
        )?;
        self.turn = Some(turn.into());
        Ok(())
    }
    pub fn end(&mut self, turn: &str) {
        if self.turn.as_deref() == Some(turn) {
            self.turn = None;
        }
    }
    pub fn pending(&self) -> Result<Value> {
        let mut statement=self.db.prepare("SELECT event,payload,status FROM context WHERE thread=? AND status!='reconsidered' ORDER BY rowid")?;
        let rows = statement.query_map([self.thread.as_deref()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (event, payload, status) = row?;
            events.push(json!({"event":event,"context":serde_json::from_str::<Value>(&payload)?,"status":status}));
        }
        Ok(json!(events))
    }
    pub fn enqueue(&self, event: &str, context: &Value) -> Result<()> {
        require(self.thread.is_some(), "unbound event thread")?;
        let payload = serde_json::to_string(context)?;
        let row: Option<String> = self
            .db
            .query_row(
                "SELECT payload FROM context WHERE thread=? AND event=?",
                params![self.thread, event],
                |r| r.get(0),
            )
            .optional()?;
        require(
            row.is_none() || row.as_deref() == Some(&payload),
            "event identity reused with different context",
        )?;
        self.db.execute(
            "INSERT OR IGNORE INTO context VALUES(?,?,?,'pending')",
            params![self.thread, event, payload],
        )?;
        Ok(())
    }
    pub fn authenticate(&self, call: &Value) -> Result<Value> {
        require(
            self.turn.is_some()
                && call["threadId"].as_str() == self.thread.as_deref()
                && call["turnId"].as_str() == self.turn.as_deref()
                && call["callId"].as_str().is_some_and(|x| !x.is_empty()),
            "inactive runtime identity",
        )?;
        let operation = call["tool"]
            .as_str()
            .and_then(|tool| tool.strip_prefix("falinks_"))
            .filter(|operation| self.operations.iter().any(|o| o == operation))
            .ok_or("unregistered tool")?;
        let arguments = call["arguments"]
            .as_object()
            .ok_or("expected workspace and engine request")?;
        require(
            arguments.len() == 2
                && arguments.contains_key("workspace")
                && arguments.contains_key("request"),
            "expected workspace and engine request",
        )?;
        let request = arguments["request"]
            .as_object()
            .ok_or("expected engine request object")?;
        require(
            arguments["workspace"].as_str() == Some(&self.workspace)
                && ![
                    "author",
                    "identity",
                    "agent",
                    "session",
                    "thread",
                    "turn",
                    "workspace",
                ]
                .iter()
                .any(|key| request.contains_key(*key)),
            "unauthorized workspace or caller-supplied identity",
        )?;
        Ok(
            json!({"identity":{"agent":self.agent,"session":self.session,"thread":self.thread,"turn":self.turn,"workspace":self.workspace},"operation":operation,"request":request}),
        )
    }
    pub fn record_result(&self, envelope: &Value, result: &Value) -> Result<()> {
        let request = &envelope["request"];
        if envelope["operation"] == "review"
            && result["accepted"] == true
            && ["defer", "keep", "revise", "drop"]
                .contains(&request["action"].as_str().unwrap_or(""))
        {
            let status = if request["action"] == "defer" {
                "deferred"
            } else {
                "reconsidered"
            };
            self.db.execute(
                "UPDATE context SET status=? WHERE thread=? AND event=?",
                params![status, self.thread, request["event"].as_str()],
            )?;
        }
        Ok(())
    }
}

/// The one mediation sequence, a security invariant every runtime shares: refuse once the
/// gate has failed, authenticate, call the engine, latch the gate on an engine error,
/// record the result. Runtimes run only their transport checks before it.
pub fn mediate(
    gate: &mut ControlGate,
    boundary: &Boundary,
    engine: &mut Engine,
    call: &Value,
) -> Result<(Value, EngineOutcome)> {
    gate.open()?;
    let envelope = boundary.authenticate(call)?;
    let outcome = engine(&envelope).inspect_err(|_| gate.failed = true)?;
    boundary.record_result(&envelope, &outcome.result)?;
    Ok((envelope, outcome))
}

pub fn command_output(
    mut command: std::process::Command,
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    use std::{
        io::Read,
        os::unix::process::CommandExt,
        process::Stdio,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let (send, receive) = mpsc::channel();
    let streams: [(bool, Box<dyn Read + Send>); 2] = [
        (true, Box::new(child.stdout.take().unwrap())),
        (false, Box::new(child.stderr.take().unwrap())),
    ];
    for (stdout, mut stream) in streams {
        let send = send.clone();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stream.read_to_end(&mut bytes).map(|_| bytes);
            let _ = send.send((stdout, result));
        });
    }
    drop(send);
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            // SAFETY: only the separately created group belonging to this child is killed.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            return Err("controlled command deadline exceeded".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let mut stdout = vec![];
    let mut stderr = vec![];
    for _ in 0..2 {
        let (is_stdout, bytes) = match receive.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => result,
            Err(error) => {
                // SAFETY: kill only descendants in this command's dedicated process group.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                return Err(error.into());
            }
        };
        if is_stdout {
            stdout = bytes?;
        } else {
            stderr = bytes?;
        }
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}
