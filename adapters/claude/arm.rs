//! The Falinks evaluation arm (#27), independent of the worker runtime: the production
//! engine on a frozen fixture, the same observable milestone triggers as the Git arm, and
//! the shared `falinks-eval oracle` seam on exact captures. Agents reach it through the
//! `falinks_*` tools; `ack`, `ready` and `check` are evaluation-host tools, the rest map
//! onto the engine via `EngineHost`.
use crate::engine_host::{self, EngineHost};
use falinks::{Check, Client, Engine, Language, Outcome, Pin, Toolchain};
use falinks_host::{Result, require, tool};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

/// Evaluation-host tools added to the engine's agent operations, with the JSON type of each
/// request key.
pub const HOST_TOOLS: &[(&str, &str, &str)] = &[
    (
        "ack",
        r#"{"milestone":"string"}"#,
        "{milestone} -> acknowledge a delivered task development before working on it.",
    ),
    (
        "check",
        "{}",
        "{} -> run the trusted visible checks and the independent acceptance check on your workspace's last completed revision; returns pass/fail only. Never publishes.",
    ),
    (
        "ready",
        "{}",
        "{} -> declare your part finished. Accepted only after every development is acknowledged and your workspace has no unpublished work; when both agents are ready the published state is the final candidate.",
    ),
];
/// Operations that change a live workspace; refused after an agent declares ready.
pub const MUTATIONS: &[&str] = &["edit", "retry", "incorporate", "apply_job"];
/// Mutations that take output forms.
pub const EDITS: [&str; 3] = ["edit", "retry", "incorporate"];
/// Output forms of `EDITS` (#193 decision 1); a deletion has none. `evaluation/src/timing.rs`
/// and `report.rs` copy this and `EDIT_COUNTS`; `tests/arm.rs` checks they agree.
pub const FORMS: [&str; 3] = ["content", "replace", "from"];
/// Counted per form: the ledger's outcomes, then calls refused before they reached the ledger
/// (a malformed request or, for `from`, a scratch reference the host refused) and resends.
pub const EDIT_COUNTS: [&str; 7] = [
    "accepted",
    "stale",
    "unreviewed",
    "rejected",
    "interrupted",
    "host_refused",
    "resends",
];
const LETTERS: [&str; 2] = ["A", "B"];

pub fn agent_name(agent: usize) -> String {
    format!("agent-{agent}")
}

/// The output forms an edit request uses, in `FORMS` order.
pub fn forms(request: &Value) -> Vec<&'static str> {
    let outputs: Vec<&Value> = request["output"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .collect();
    FORMS
        .into_iter()
        .filter(|form| outputs.iter().any(|o| o.get(form).is_some()))
        .collect()
}

/// The lines of `path` in `capture` that have a letter or digit, counted; blank and
/// brace-only lines name nobody's text.
fn lines(capture: Option<&falinks::Capture>, path: &str) -> BTreeMap<String, usize> {
    let mut lines = BTreeMap::new();
    let file = capture.and_then(|c| c.files.get(path)?.file.as_ref());
    let text = file.map_or_else(String::new, |f| String::from_utf8_lossy(&f.bytes).into());
    for line in text
        .lines()
        .filter(|l| l.chars().any(char::is_alphanumeric))
    {
        *lines.entry(line.to_string()).or_insert(0) += 1;
    }
    lines
}

/// Lines of `a` beyond their count in `b`.
fn minus(a: &BTreeMap<String, usize>, b: &BTreeMap<String, usize>) -> BTreeMap<String, usize> {
    a.iter()
        .filter(|(line, n)| b.get(*line).is_none_or(|m| m < n))
        .map(|(line, n)| (line.clone(), n - b.get(line).unwrap_or(&0)))
        .collect()
}

/// One edit, retry or incorporation as it reached the host.
struct EditCall {
    agent: usize,
    forms: Vec<&'static str>,
    accepted: bool,
    /// Set when it was refused before reaching the ledger.
    host_refused: Option<String>,
}

/// MCP definitions: every engine operation plus the evaluation-host tools.
pub fn tools() -> Value {
    let mut tools = engine_host::tools();
    let list = tools.as_array_mut().expect("tool list");
    for (operation, types, help) in HOST_TOOLS {
        list.push(tool(
            operation,
            types,
            format!("Evaluation host {operation}: {help}"),
        ));
    }
    tools
}

/// The opening prompt: task information equal to the Git arm's, plus the initial protocol
/// state from `engine_host::opening`.
pub fn prompt(
    agent: usize,
    root: &Path,
    fixture: &Value,
    workflow: &str,
    optional: &[String],
    opening: &Value,
) -> String {
    let task: serde_json::Map<String, Value> = fixture
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| *k != "files" && *k != "milestones")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let developments = fixture["milestones"].as_array().map_or(0, Vec::len);
    format!(
        "[Falinks host] You are {name} (agent {letter} in the task allocation), one of two agents implementing a small software task together. \
         Your live workspace is {root}; read it with Read, Grep or Glob. It shows your peer's unfinished edits as they happen. \
         You cannot write it directly: change source only with falinks tools, each taking its request as its whole input. \
         Your current directory is a private scratch directory where you may copy files and run builds or tests with Bash. \
         Engine events, host notices and task developments arrive as messages starting with \"Host notice\". \
         When you have nothing to do until the next event, end your turn; the next event starts a new turn. Do not run long sleeps or polling loops. \
         There are {developments} task development(s): acknowledge each with falinks_ack before working on it. \
         falinks_check gives pass/fail for the trusted checks on your workspace. When your part is finished and published, call falinks_ready. \
         Besides the existing files you may create only: {optional}.\n\n\
         Initial protocol state, from a capture given to you (read file contents with Read or falinks_capture): {opening}\n\n\
         Task: {task}\nYour allocation: {allocation}\n\nWorkflow:\n{workflow}",
        name = agent_name(agent),
        letter = LETTERS[agent],
        root = root.display(),
        optional = optional.join(", "),
        task = Value::Object(task),
        allocation = fixture["allocation"][LETTERS[agent]],
    )
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
    pub fn configure(&self, engine: &Engine, language: Language) -> Result<Value> {
        let (toolchain, checks) = if language == Language::Go {
            let go = self.go.to_string_lossy().into_owned();
            (
                Toolchain {
                    language: Language::Go,
                    executables: vec![Pin::from_path(&self.gopls)?, Pin::from_path(&self.go)?],
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
                        .map(|t| Pin::from_path(self.rust.join(t)))
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

/// The configured `denied_roots`, resolved, plus the evaluation checkout's Git directories: the
/// one list behind the worker sandbox, engine checks and the host `check`. A malformed,
/// placeholder or misspelled entry fails setup rather than leaving its root readable.
pub fn denied_roots(config: &Value) -> Result<Vec<PathBuf>> {
    let roots = config["denied_roots"]
        .as_array()
        .ok_or("denied_roots must be a list of paths")?;
    let mut roots = roots
        .iter()
        .map(|root| {
            let root = root.as_str().ok_or("denied_roots must be paths")?;
            Path::new(root)
                .canonicalize()
                .map_err(|error| format!("denied root {root}: {error}").into())
        })
        .collect::<Result<Vec<_>>>()?;
    let evaluation = config["evaluation_root"]
        .as_str()
        .ok_or("evaluation_root must be a path")?;
    roots.extend(checkout_roots(Path::new(evaluation))?);
    Ok(roots)
}

/// The checkout holding `evaluation`, its Git common directory (which holds oracle bytes) and
/// every worktree of it, as the Git arm's `protected_roots()` derives them. A worktree whose
/// directory is gone has nothing left to read.
fn checkout_roots(evaluation: &Path) -> Result<Vec<PathBuf>> {
    let git = |args: &[&str]| -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(evaluation)
            .args(args)
            .output()?;
        require(
            output.status.success(),
            "evaluation_root is not in a Git checkout",
        )?;
        Ok(String::from_utf8(output.stdout)?)
    };
    let checkout = evaluation
        .parent()
        .ok_or("evaluation_root has no checkout")?;
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let mut roots = vec![
        checkout.canonicalize()?,
        Path::new(common.trim()).canonicalize()?,
    ];
    for line in git(&["worktree", "list", "--porcelain"])?.lines() {
        if let Some(path) = line.strip_prefix("worktree ").map(Path::new)
            && path.exists()
        {
            roots.push(path.canonicalize()?);
        }
    }
    Ok(roots)
}

/// The worker sandbox's protected roots: the run's controller and every denied root, which the
/// worker launch accepts only as directories.
pub fn worker_protected(run: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut protected = vec![run.join("controller")];
    protected.extend(roots.iter().filter(|root| root.is_dir()).cloned());
    protected
}

/// Roots no check may read, as the Git arm's checks cannot: the whole run directory (the slot
/// and output stay readable beneath it) and every configured root.
pub fn deny_check_reads(engine: &Engine, run: &Path, roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut denied = vec![run.canonicalize()?];
    denied.extend_from_slice(roots);
    engine.configure_denied_roots(denied.clone())?;
    Ok(denied)
}

/// Every entry beneath `root` except the directories leading down to `keep`, listed at each
/// call so entries created since setup are included.
fn around(root: &Path, keep: &Path) -> Result<Vec<PathBuf>> {
    let (mut out, mut dir) = (vec![], root.to_path_buf());
    while dir != keep {
        let mut next = None;
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if keep.starts_with(&path) {
                next = Some(path);
            } else {
                out.push(path);
            }
        }
        dir = next.ok_or("the oracle directory is missing from its denied root")?;
    }
    Ok(out)
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
    /// Every edit, retry and incorporation in call order, and its forms by (agent, request id).
    edit_calls: Vec<EditCall>,
    forms: BTreeMap<(usize, String), Vec<&'static str>>,
    /// Set once both agents are ready: final oracle on the exact published state.
    pub outcome: Option<Value>,
    /// Agents whose live root moved after an applied regrouping.
    pub relocated: BTreeSet<usize>,
    /// Mediated calls so far: only a call can apply a regrouping and move a root.
    pub calls: u64,
    /// Set once the clock stops: the usage drain refuses every call (#179).
    pub draining: bool,
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
            edit_calls: vec![],
            forms: BTreeMap::new(),
            outcome: None,
            relocated: BTreeSet::new(),
            calls: 0,
            draining: false,
        }
    }

    pub fn is_ready(&self, agent: usize) -> bool {
        self.ready[agent]
    }

    pub fn client(&self, agent: usize) -> &Client {
        &self.clients[agent]
    }

    /// `answer`, with a check's oracle run inline.
    pub fn call(&mut self, agent: usize, envelope: &Value) -> Result<Value> {
        match self.answer(agent, envelope)? {
            Answer::Now { result, .. } => Ok(result),
            Answer::Check(oracle) => self.checked(agent, oracle.run()),
        }
    }

    /// One authenticated tool envelope from `agent`. Agent mistakes are refusals; an
    /// engine error or incident is an error that latches the worker's gate. A `check` returns
    /// its oracle run, which the caller may run on another thread and then pass to `checked`.
    pub fn answer(&mut self, agent: usize, envelope: &Value) -> Result<Answer> {
        if self.draining {
            return Ok(Answer::now(refuse(
                "the run is finished; no further requests",
            )));
        }
        self.calls += 1;
        let operation = envelope["operation"].as_str().unwrap_or_default();
        let request = &envelope["request"];
        let root = self.engine.root(&self.clients[agent])?;
        if envelope["identity"]["workspace"].as_str() != root.to_str() {
            // An applied split/join moved this agent; the runner resumes it in its new root.
            self.relocated.insert(agent);
            return Ok(Answer::now(refuse(
                "your workspace moved after an agreed regrouping; the host is restarting you there",
            )));
        }
        if self.ready[agent] && MUTATIONS.contains(&operation) {
            return Ok(Answer::now(refuse("you declared ready; no further edits")));
        }
        let mut stages = None;
        let result = match operation {
            "ack" | "ready" | "check" => {
                let required: &[&str] = if operation == "ack" {
                    &["milestone"]
                } else {
                    &[]
                };
                match (
                    operation,
                    engine_host::require_keys(request, operation, required, &[]),
                ) {
                    (_, Err(error)) => refuse(&error.to_string()),
                    ("ack", _) => self.ack(agent, request),
                    ("ready", _) => self.ready(agent)?,
                    _ => return Ok(Answer::Check(self.check(agent)?)),
                }
            }
            _ => {
                let (result, report) = self.hosts[agent].call_timed(envelope)?;
                stages = report;
                result
            }
        };
        if result["accepted"] == false {
            self.rejected += 1;
        }
        if operation == "retry" {
            self.retries += 1;
        }
        if EDITS.contains(&operation) {
            let forms = forms(request);
            let host_refused = result.get("outcome").is_none();
            // A call refused before the ledger uses no ID: a later request may reuse it.
            if let Some(id) = request["id"].as_str().filter(|_| !host_refused) {
                self.forms
                    .entry((agent, id.into()))
                    .or_insert(forms.clone());
            }
            self.edit_calls.push(EditCall {
                agent,
                forms,
                accepted: result["accepted"] == true,
                host_refused: host_refused
                    .then(|| result["reason"].as_str().unwrap_or_default().into()),
            });
        }
        // Mutation results are accepted exactly when the engine applied them.
        if MUTATIONS.contains(&operation) && result["accepted"] == true {
            self.drafts[agent] += 1;
            self.trigger();
        }
        Ok(Answer::Now { result, stages })
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

    /// Per output form, from the ledger and the calls: each `EDIT_COUNTS` entry, `Rejected` and
    /// host-refusal reasons; and accepted edits that bring back lines a peer's earlier accepted
    /// edit removed from that file, listed for a manual check (#193 decision 9). A resend is an
    /// edit sent after the same agent's previous one was refused.
    pub fn edits(&self) -> Result<Value> {
        let mut forms: BTreeMap<&'static str, Value> = FORMS
            .iter()
            .map(|form| {
                let mut counts: serde_json::Map<String, Value> = EDIT_COUNTS
                    .iter()
                    .map(|k| (k.to_string(), json!(0)))
                    .collect();
                counts.insert("rejected_reasons".into(), json!([]));
                counts.insert("host_refused_reasons".into(), json!([]));
                (*form, Value::Object(counts))
            })
            .collect();
        let mut add = |used: &[&'static str], count: &str, reason: Option<&str>| {
            for form in used {
                let entry = forms.get_mut(form).unwrap();
                entry[count] = json!(entry[count].as_u64().unwrap() + 1);
                if let Some(reason) = reason {
                    let key = format!("{count}_reasons");
                    entry[key.as_str()]
                        .as_array_mut()
                        .unwrap()
                        .push(json!(reason));
                }
            }
        };
        let mut refused = [false; 2];
        for call in &self.edit_calls {
            if std::mem::replace(&mut refused[call.agent], !call.accepted) {
                add(&call.forms, "resends", None);
            }
            if let Some(reason) = &call.host_refused {
                add(&call.forms, "host_refused", Some(reason));
            }
        }
        let history = self.engine.history()?;
        let mut reverted = vec![];
        for (n, record) in history.iter().enumerate() {
            let key = (record.agent, record.request.id.clone());
            let Some(used) = self.forms.get(&key) else {
                continue; // a job's output, not an edit
            };
            let (count, reason) = match &record.outcome {
                Some(Outcome::Applied { .. }) => ("accepted", None),
                Some(Outcome::Stale { .. }) => ("stale", None),
                Some(Outcome::Unreviewed { .. }) => ("unreviewed", None),
                Some(Outcome::Rejected { reason }) => ("rejected", Some(reason.as_str())),
                // No recorded outcome: the operation never finished.
                Some(Outcome::Interrupted { .. }) | None => ("interrupted", None),
            };
            add(used, count, reason);
            if count != "accepted" {
                continue;
            }
            for path in record.request.output.keys() {
                let added = minus(
                    &lines(record.proposed.as_ref(), path),
                    &lines(Some(&record.before), path),
                );
                for peer in &history[..n] {
                    if peer.agent == record.agent
                        || !matches!(peer.outcome, Some(Outcome::Applied { .. }))
                        || !peer.request.output.contains_key(path)
                    {
                        continue;
                    }
                    let removed = minus(
                        &lines(Some(&peer.before), path),
                        &lines(peer.proposed.as_ref(), path),
                    );
                    let back: Vec<&String> =
                        added.keys().filter(|l| removed.contains_key(*l)).collect();
                    if !back.is_empty() {
                        reverted.push(json!({"agent": record.agent, "id": record.request.id,
                            "path": path, "peer_id": peer.request.id, "lines": back}));
                    }
                }
            }
        }
        Ok(json!({"forms": forms, "reverted_peer_text": reverted}))
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
        let mine = self.engine.host_capture(&self.clients[agent])?;
        if Self::files(&published) != Self::files(&mine) {
            return Ok(refuse(
                "your workspace has unpublished work: offer it and wait for publication",
            ));
        }
        self.ready[agent] = true;
        self.log.push(json!({"kind": "ready", "agent": agent}));
        if self.ready == [true; 2] {
            let checks = self.oracle(&published, "final")?.run()?;
            self.validation_attempts.push(checks["attempt"].clone());
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

    fn check(&mut self, agent: usize) -> Result<Oracle> {
        let capture = self.engine.host_capture(&self.clients[agent])?;
        self.oracle(&capture, &agent_name(agent))
    }

    /// Records a `check`'s oracle run and returns the agent's answer. A failed run is an error.
    pub fn checked(&mut self, agent: usize, checks: Result<Value>) -> Result<Value> {
        let checks = checks?;
        let attempt = &checks["attempt"];
        self.validation_attempts.push(attempt.clone());
        self.log.push(json!({"kind": "validation", "agent": agent, "revision": attempt["revision"], "passed": checks["passed"]}));
        Ok(json!({"accepted": true, "revision": attempt["revision"], "passed": checks["passed"]}))
    }

    /// Commit the capture's exact bytes and prepare the shared oracle seam on them.
    fn oracle(&mut self, capture: &falinks::Capture, label: &str) -> Result<Oracle> {
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
            // The oracle runner splits its own directory; everything around it stays denied.
            let roots = if self.private.starts_with(root) {
                around(root, &self.private)?
            } else {
                vec![root.clone()]
            };
            for root in roots {
                command.arg("--deny-root").arg(root);
            }
        }
        let attempt = json!({"number": number, "conflict": null, "revision": capture.revision,
            "space": capture.space, "requested_by": label});
        Ok(Oracle {
            command,
            evidence,
            attempt,
        })
    }
}

/// `Arm::answer`'s reply: a result now, with an edit's stage report, or a check to run.
pub enum Answer {
    Now {
        result: Value,
        stages: Option<Value>,
    },
    Check(Oracle),
}
impl Answer {
    fn now(result: Value) -> Self {
        Self::Now {
            result,
            stages: None,
        }
    }
}

/// A prepared run of the shared oracle seam; `run` needs no arm state, so any thread may run it.
pub struct Oracle {
    command: Command,
    evidence: PathBuf,
    attempt: Value,
}
impl Oracle {
    /// Details stay host-only; agents get pass/fail, as in the Git arm.
    pub fn run(mut self) -> Result<Value> {
        let output = self.command.output()?;
        let line: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| format!("oracle failed: {}", String::from_utf8_lossy(&output.stderr)))?;
        let details: Value =
            serde_json::from_str(&fs::read_to_string(self.evidence.join("checks.json"))?)?;
        self.attempt["candidate"] = line["candidate"].clone();
        self.attempt["passed"] = line["passed"].clone();
        Ok(
            json!({"candidate": line["candidate"], "passed": line["passed"],
            "oracle": details["oracle"], "attempt": self.attempt}),
        )
    }
}

fn refuse(reason: &str) -> Value {
    json!({"accepted": false, "reason": reason})
}
