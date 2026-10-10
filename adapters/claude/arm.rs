//! The Falinks evaluation arm (#27), independent of the worker runtime: the production
//! engine on a frozen fixture, the same observable milestone triggers as the Git arm, and
//! the shared `falinks-eval oracle` seam on exact captures. Agents reach it through the
//! `falinks_*` tools; `ack`, `ready` and `check` are evaluation-host tools, the rest map
//! onto the engine via `EngineHost`.
use crate::engine_host::{self, EngineHost};
use falinks::{Check, Client, Engine, Language, Pin, Toolchain, sha256_file};
use falinks_host::{Result, require};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

/// Evaluation-host tools added to the engine's agent operations.
pub const HOST_TOOLS: &[(&str, &str)] = &[
    (
        "ack",
        "{milestone} -> acknowledge a delivered task development before working on it.",
    ),
    (
        "check",
        "{} -> run the trusted visible checks and the independent acceptance check on your workspace's last completed revision; returns pass/fail only. Never publishes.",
    ),
    (
        "ready",
        "{} -> declare your part finished. Accepted only after every development is acknowledged and your workspace has no unpublished work; when both agents are ready the published state is the final candidate.",
    ),
];
/// Operations that change a live workspace; refused after an agent declares ready.
const MUTATIONS: &[&str] = &["edit", "retry", "incorporate", "apply_job"];

pub fn agent_name(agent: usize) -> String {
    format!("agent-{agent}")
}

/// MCP definitions: every engine operation plus the evaluation-host tools.
pub fn tools(workspace: &Path) -> Value {
    let mut tools = engine_host::tools(workspace);
    let list = tools.as_array_mut().expect("tool list");
    for (operation, help) in HOST_TOOLS {
        list.push(json!({"name": format!("falinks_{operation}"),
            "description": format!("Evaluation host {operation}: {help} Call with {{\"workspace\": \"{}\", \"request\": {{...}}}}.", workspace.display()),
            "inputSchema": {"type": "object", "properties": {"workspace": {"type": "string"}, "request": {"type": "object"}},
                "required": ["workspace", "request"], "additionalProperties": false}}));
    }
    tools
}

/// Fixture files the engine cannot enroll (it reserves dot-paths, e.g. `.gitignore`).
/// They stay out of the live root; the host adds their frozen bytes to every candidate.
pub fn reserved(path: &str) -> bool {
    path.split('/').any(|part| part.starts_with('.'))
}

/// The fixture's `language`; anything but Go or Rust fails closed.
pub fn language(name: &str) -> Result<Language> {
    match name {
        "go" => Ok(Language::Go),
        "rust" => Ok(Language::Rust),
        other => Err(format!("unsupported fixture language {other:?}").into()),
    }
}

/// Enrolled source: the fixture files plus optional files agents may create. The engine
/// audits every live file against its enrollment, so the Falinks arm fixes this set up
/// front (`x.go` -> `x_test.go`, `src/x.rs` -> `tests/x.rs`). The Git arm may add any file.
pub fn enrollment(files: &[String], language: Language) -> Vec<String> {
    let files: Vec<&String> = files.iter().filter(|f| !reserved(f)).collect();
    let mut enrolled: BTreeSet<String> = files.iter().map(|f| f.to_string()).collect();
    for file in files {
        let extra = match language {
            Language::Go => file
                .strip_suffix(".go")
                .filter(|stem| !stem.ends_with("_test"))
                .map(|stem| format!("{stem}_test.go")),
            Language::Rust => file
                .strip_prefix("src/")
                .filter(|f| *f != "lib.rs" && *f != "main.rs")
                .map(|f| format!("tests/{f}")),
        };
        enrolled.extend(extra);
    }
    enrolled.into_iter().collect()
}

/// Pinned analysis toolchain and visible checks equal to the Git arm's trusted commands.
pub struct Tools {
    pub go: PathBuf,
    pub gopls: PathBuf,
    /// Directory holding the pinned `cargo`, `rustc` and `rust-analyzer`.
    pub rust: PathBuf,
}
impl Tools {
    fn pin(path: &Path) -> Result<Pin> {
        Ok(Pin {
            sha256: sha256_file(path)?,
            path: path.to_owned(),
        })
    }
    pub fn configure(&self, engine: &Engine, language: Language) -> Result<Value> {
        let (toolchain, checks) = if language == Language::Go {
            let go = self.go.to_string_lossy().into_owned();
            (
                Toolchain {
                    language: Language::Go,
                    executables: vec![Self::pin(&self.gopls)?, Self::pin(&self.go)?],
                    features: vec![],
                },
                vec![Check {
                    name: "go-test".into(),
                    program: go,
                    args: ["test", "./..."].map(String::from).to_vec(),
                }],
            )
        } else {
            let bin = self.rust.to_string_lossy().into_owned();
            // The engine clears the environment; cargo finds the pinned rustc through PATH.
            let cargo = |name: &str, command: &str| Check {
                name: name.into(),
                program: "/usr/bin/env".into(),
                args: vec![
                    format!("PATH={bin}:/usr/bin:/bin"),
                    format!("{bin}/cargo"),
                    command.into(),
                    "--offline".into(),
                    "--locked".into(),
                ],
            };
            (
                Toolchain {
                    language: Language::Rust,
                    executables: ["rust-analyzer", "cargo", "rustc"]
                        .iter()
                        .map(|t| Self::pin(&self.rust.join(t)))
                        .collect::<Result<_>>()?,
                    features: vec![],
                },
                vec![cargo("cargo-check", "check"), cargo("cargo-test", "test")],
            )
        };
        let evidence = json!({"toolchain": toolchain.executables, "checks": checks});
        engine.configure_analysis(vec![toolchain])?;
        engine.configure_checks(checks)?;
        Ok(evidence)
    }
}

pub struct Arm {
    pub engine: Arc<Engine>,
    hosts: [EngineHost; 2],
    clients: [Client; 2],
    fixture: Value,
    eval: PathBuf,
    /// Host-only evidence directory (oracle checks, retained candidates).
    private: PathBuf,
    denied: Vec<PathBuf>,
    milestones: Vec<Value>,
    delivered: usize,
    awaiting: BTreeSet<usize>,
    drafts: [usize; 2],
    floor: [usize; 2],
    ready: [bool; 2],
    checks: usize,
    /// Developments for the runner to deliver to both agents, oldest first.
    pub outbox: Vec<Value>,
    /// Timeline rows for the runner's `events.jsonl`.
    pub log: Vec<Value>,
    pub validation_attempts: Vec<Value>,
    pub rejected: u64,
    pub retries: u64,
    /// Set once both agents are ready: final oracle on the exact published state.
    pub outcome: Option<Value>,
    /// Agents whose live root moved after an applied regrouping.
    pub relocated: BTreeSet<usize>,
}

impl Arm {
    #[allow(clippy::too_many_arguments)] // One construction site in the runner and one in tests.
    pub fn new(
        engine: Arc<Engine>,
        clients: [Client; 2],
        formatter: PathBuf,
        fixture: Value,
        eval: PathBuf,
        private: PathBuf,
        denied: Vec<PathBuf>,
    ) -> Self {
        let host = |agent: usize| {
            EngineHost::new(
                Arc::clone(&engine),
                clients[agent].clone(),
                &agent_name(agent),
                formatter.clone(),
            )
        };
        Self {
            hosts: [host(0), host(1)],
            milestones: fixture["milestones"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            engine,
            clients,
            fixture,
            eval,
            private,
            denied,
            delivered: 0,
            awaiting: BTreeSet::new(),
            drafts: [0; 2],
            floor: [0; 2],
            ready: [false; 2],
            checks: 0,
            outbox: vec![],
            log: vec![],
            validation_attempts: vec![],
            rejected: 0,
            retries: 0,
            outcome: None,
            relocated: BTreeSet::new(),
        }
    }

    pub fn is_ready(&self, agent: usize) -> bool {
        self.ready[agent]
    }

    pub fn client(&self, agent: usize) -> &Client {
        &self.clients[agent]
    }

    /// One authenticated tool envelope from `agent`. Agent mistakes are refusals; an
    /// engine error or incident is an error that latches the worker's gate.
    pub fn call(&mut self, agent: usize, envelope: &Value) -> Result<Value> {
        let operation = envelope["operation"].as_str().unwrap_or_default();
        let request = &envelope["request"];
        let root = self.engine.root(&self.clients[agent])?;
        if envelope["identity"]["workspace"].as_str() != root.to_str() {
            // An applied split/join moved this agent; the runner resumes it in its new root.
            self.relocated.insert(agent);
            return Ok(refuse(
                "your workspace moved after an agreed regrouping; the host is restarting you there",
            ));
        }
        if self.ready[agent] && MUTATIONS.contains(&operation) {
            return Ok(refuse("you declared ready; no further edits"));
        }
        let result = match operation {
            "ack" => self.ack(agent, request),
            "ready" => self.ready(agent)?,
            "check" => self.check(agent)?,
            _ => self.hosts[agent].call(envelope)?,
        };
        if result["accepted"] == false {
            self.rejected += 1;
        }
        if operation == "retry" {
            self.retries += 1;
        }
        // Mutation results are accepted exactly when the engine applied them.
        if MUTATIONS.contains(&operation) && result["accepted"] == true {
            self.drafts[agent] += 1;
            self.trigger();
        }
        Ok(result)
    }

    /// Same observable triggers as the Git arm: a new nonempty draft from each agent
    /// since its last acknowledgment, regardless of compile status.
    fn trigger(&mut self) {
        if self.awaiting.is_empty()
            && self.delivered < self.milestones.len()
            && (0..2).all(|a| self.drafts[a] > self.floor[a])
        {
            let milestone = self.milestones[self.delivered].clone();
            self.delivered += 1;
            self.awaiting = BTreeSet::from([0, 1]);
            self.ready = [false; 2];
            self.log
                .push(json!({"kind": "development", "milestone": milestone}));
            let mut development = json!({"type": "development"});
            development
                .as_object_mut()
                .unwrap()
                .extend(milestone.as_object().unwrap().clone());
            self.outbox.push(development);
        }
    }

    fn ack(&mut self, agent: usize, request: &Value) -> Value {
        let expected = self
            .delivered
            .checked_sub(1)
            .map(|i| &self.milestones[i]["id"]);
        if expected != Some(&request["milestone"]) || !self.awaiting.remove(&agent) {
            return refuse("no such unacknowledged development");
        }
        self.floor[agent] = self.drafts[agent];
        self.log
            .push(json!({"kind": "ack", "agent": agent, "milestone": request["milestone"]}));
        json!({"accepted": true})
    }

    fn files(capture: &falinks::Capture) -> BTreeMap<&String, &Option<falinks::File>> {
        capture.files.iter().map(|(p, v)| (p, &v.file)).collect()
    }

    fn ready(&mut self, agent: usize) -> Result<Value> {
        if self.delivered < self.milestones.len() || self.awaiting.contains(&agent) {
            return Ok(refuse(
                "not every task development has been delivered and acknowledged",
            ));
        }
        let published = self.engine.published()?;
        let mine = self.engine.capture_for(&self.clients[agent])?;
        if Self::files(&published) != Self::files(&mine) {
            return Ok(refuse(
                "your workspace has unpublished work: offer it and wait for publication",
            ));
        }
        self.ready[agent] = true;
        self.log.push(json!({"kind": "ready", "agent": agent}));
        if self.ready == [true; 2] {
            let checks = self.oracle(&published, "final")?;
            let passed = checks["passed"] == true;
            self.log.push(json!({"kind": "final_validation", "revision": published.revision, "passed": passed}));
            self.outcome = Some(
                json!({"outcome": if passed { "success" } else { "task_failure" },
                "final_snapshot": checks["candidate"], "published_revision": published.revision,
                "oracle": checks["oracle"]}),
            );
        }
        Ok(json!({"accepted": true}))
    }

    fn check(&mut self, agent: usize) -> Result<Value> {
        let capture = self.engine.capture_for(&self.clients[agent])?;
        let checks = self.oracle(&capture, &agent_name(agent))?;
        self.log.push(json!({"kind": "validation", "agent": agent, "revision": capture.revision, "passed": checks["passed"]}));
        Ok(json!({"accepted": true, "revision": capture.revision, "passed": checks["passed"]}))
    }

    /// Commit the capture's exact bytes and run the shared oracle seam on them. Details stay
    /// host-only; agents get pass/fail, as in the Git arm.
    fn oracle(&mut self, capture: &falinks::Capture, label: &str) -> Result<Value> {
        self.checks += 1;
        let number = self.checks;
        let repo = self.private.join(format!("candidate-{number}"));
        fs::create_dir(&repo)?;
        let mut files: BTreeMap<String, Vec<u8>> = Self::files(capture)
            .into_iter()
            .filter_map(|(path, file)| Some((path.clone(), file.as_ref()?.bytes.clone())))
            .collect();
        for (path, text) in self.fixture["files"].as_object().into_iter().flatten() {
            if reserved(path) {
                files.insert(path.clone(), text.as_str().unwrap_or_default().into());
            }
        }
        for (path, bytes) in files {
            let target = repo.join(path);
            fs::create_dir_all(target.parent().ok_or("bad path")?)?;
            fs::write(target, bytes)?;
        }
        for args in [
            &["init", "--quiet", "--initial-branch=main"][..],
            &["add", "--all"],
            &["commit", "--quiet", "-m", "Captured candidate"],
        ] {
            let status = Command::new("git")
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "user.name=Falinks",
                    "-c",
                ])
                .arg("user.email=falinks@example.invalid")
                .args(args)
                .current_dir(&repo)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .status()?;
            require(status.success(), "cannot commit captured candidate")?;
        }
        let evidence = self.private.join(format!("check-{number}"));
        let mut command = Command::new(&self.eval);
        command
            .args(["oracle", self.fixture["name"].as_str().unwrap_or_default()])
            .arg("--repo")
            .arg(&repo)
            .arg("--evidence")
            .arg(&evidence);
        for root in &self.denied {
            command.arg("--deny-root").arg(root);
        }
        let output = command.output()?;
        let line: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| format!("oracle failed: {}", String::from_utf8_lossy(&output.stderr)))?;
        let details: Value =
            serde_json::from_str(&fs::read_to_string(evidence.join("checks.json"))?)?;
        let attempt = json!({"number": number, "candidate": line["candidate"], "passed": line["passed"],
            "conflict": null, "revision": capture.revision, "space": capture.space, "requested_by": label});
        self.validation_attempts.push(attempt);
        Ok(
            json!({"candidate": line["candidate"], "passed": line["passed"], "oracle": details["oracle"]}),
        )
    }
}

fn refuse(reason: &str) -> Value {
    json!({"accepted": false, "reason": reason})
}
