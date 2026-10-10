//! Trusted Codex host boundary. The engine envelope is independent of the runtime.
pub mod controlled_host;
pub mod runtime;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error, path::Path};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
pub const REQUIRED: &[&str] = &[
    "schema",
    "profile",
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
pub const PIN: &str = "112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b";
pub const HOST_PIN: &str = "679eedaea70529aa1cffc9bc0a0788c186412663544fa76c09d63b57f383a65a";

pub fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn file_hash(path: &Path) -> Result<String> {
    Ok(hash(&std::fs::read(path)?))
}

#[derive(Default)]
pub struct ControlGate {
    pub controls: BTreeMap<String, bool>,
    pub failed: bool,
}
impl ControlGate {
    pub fn record(&mut self, name: &str, passed: bool) -> Result<()> {
        require(REQUIRED.contains(&name), "unknown safety control")?;
        self.controls.insert(name.into(), passed);
        self.failed |= !passed;
        Ok(())
    }
    pub fn require_supported(&self) -> Result<()> {
        require(
            !self.failed
                && self.controls.len() == REQUIRED.len()
                && self.controls.values().all(|x| *x),
            "fresh safety controls required; publication/scoring disabled",
        )
    }
}

pub struct Boundary {
    db: Connection,
    pub workspace: String,
    session: String,
    pub thread: Option<String>,
    agent: Option<String>,
    pub turn: Option<String>,
}
impl Boundary {
    pub fn new(database: &Path, workspace: &Path, session: String) -> Result<Self> {
        let db = Connection::open(database)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS binding(thread TEXT PRIMARY KEY,agent TEXT,workspace TEXT); CREATE TABLE IF NOT EXISTS context(thread TEXT,event TEXT,payload TEXT,status TEXT,PRIMARY KEY(thread,event));")?;
        Ok(Self {
            db,
            workspace: workspace.canonicalize()?.to_string_lossy().into_owned(),
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
        let operation = match call["tool"].as_str() {
            Some("falinks_edit") => "edit",
            Some("falinks_review") => "review",
            Some("falinks_offer") => "offer",
            _ => return Err("unregistered tool".into()),
        };
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
        let (is_stdout, bytes) = receive.recv_timeout(Duration::from_secs(5))?;
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
