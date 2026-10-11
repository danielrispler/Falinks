//! One Falinks-arm evaluation run (#27): two pinned Claude Code workers on the production
//! engine with natural timing. Task information, milestone triggers, the 30-minute bound,
//! the oracle seam and the result schema match the Git arm (`falinks-eval baseline`).
//! Safety controls come from a fresh `check-integration` gate that the batch runs first.
use falinks::{Action, Engine, ProposalState};
use falinks_claude::{
    Launch, MODEL, Runtime, VERSION,
    arm::{self, Arm, Tools, agent_name},
    launch_args, verify_binary,
};
use falinks_host::{Boundary, Result, file_hash, require, runtime::EngineOutcome};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    env, fs,
    io::Write,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::{self, Command},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// An idle, unfinished agent with no new events is reminded after this long (both arms).
const NUDGE: Duration = Duration::from_secs(120);
const LETTERS: [&str; 2] = ["A", "B"];

struct Worker {
    runtime: Option<Runtime>,
    session: Option<String>,
    source: PathBuf,
    /// Highest engine event seq sent to this worker.
    delivered: u64,
    /// Developments not yet sent to this worker.
    developments: Vec<Value>,
    last_result: Value,
    last_activity: Instant,
    scanned: usize,
    rate_limit: Value,
    usage: Vec<Value>,
    transcripts: Vec<Value>,
}

struct Run {
    dir: PathBuf,
    binary: PathBuf,
    started: Instant,
    engine: Arc<Engine>,
    arm: Rc<RefCell<Arm>>,
    workers: [Worker; 2],
    prompt: [String; 2],
    protected: Vec<PathBuf>,
    timeline: fs::File,
    evidence: Value,
}

fn utc() -> String {
    // `date` formats UTC without a date dependency; evidence only.
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_str(
        &fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
    )?)
}

impl Run {
    fn event(&mut self, kind: &str, values: Value) -> Result<()> {
        let mut row = json!({"elapsed": self.started.elapsed().as_secs_f64(), "kind": kind});
        if let Some(values) = values.as_object() {
            row.as_object_mut().unwrap().extend(values.clone());
        }
        writeln!(self.timeline, "{row}")?;
        Ok(())
    }

    fn launch(&self, agent: usize) -> Result<Launch> {
        let client = self.arm.borrow().client(agent).clone();
        let root = self.engine.root(&client)?.canonicalize()?;
        let mine = self.dir.join(agent_name(agent));
        let mut protected = self.protected.clone();
        protected.push(self.dir.join(agent_name(1 - agent)).join("controller"));
        // Only the agent's own live root is exposed; any other space stays hidden.
        protected.extend(
            (0..2)
                .map(|i| self.dir.join(format!("space{i}")))
                .filter(|s| *s != root),
        );
        Ok(Launch {
            tools: arm::tools(&root),
            source: root,
            scratch: mine.join("scratch"),
            controller: mine.join("controller"),
            worker_tools: self.dir.join("worker-tools"),
            protected,
        })
    }

    /// Launch, or resume against the agent's current root after an applied regrouping.
    fn start(&mut self, agent: usize, text: String) -> Result<()> {
        let launch = self.launch(agent)?;
        let resume = self.workers[agent].session.clone();
        if let Some(session) = &resume {
            Boundary::new(
                &launch.controller.join("context.sqlite"),
                &launch.source,
                "host".into(),
            )?
            .relocate(session, &agent_name(agent))?;
        }
        let arm = Rc::clone(&self.arm);
        let runtime = Runtime::new(
            &self.binary,
            &launch,
            &agent_name(agent),
            resume.as_deref(),
            Box::new(move |envelope| {
                Ok(EngineOutcome {
                    result: arm.borrow_mut().call(agent, envelope)?,
                    notice: None,
                })
            }),
            // Controls were verified by the batch's fresh gate; calls still latch on failure.
            true,
        )?;
        let session = runtime.session.session().to_owned();
        let record = json!({"agent": agent, "resume": resume.is_some(), "source": launch.source,
            "protected": launch.protected, "settings": fs::read_to_string(launch.controller.join("settings.json"))?,
            "args": launch_args(&launch.controller, &session, resume.is_some())});
        self.evidence["launches"]
            .as_array_mut()
            .unwrap()
            .push(record);
        let worker = &mut self.workers[agent];
        worker.session = Some(session);
        worker.source = launch.source;
        worker.runtime = Some(runtime);
        self.send(agent, text, None)?;
        self.event(
            "agent_started",
            json!({"agent": agent, "resume": resume.is_some()}),
        )
    }

    fn stop(&mut self, agent: usize) {
        let worker = &mut self.workers[agent];
        if let Some(mut runtime) = worker.runtime.take() {
            runtime.close();
            let session = &runtime.session;
            worker.transcripts.push(
                json!({"session": session.session(), "events": session.events,
                "deliveries": session.deliveries, "permission_denials": session.denials,
                "gate_failed": session.gate.failed, "controls": session.gate.controls,
                "project": session.project}),
            );
        }
    }

    fn send(&mut self, agent: usize, text: String, event: Option<&str>) -> Result<()> {
        let worker = &mut self.workers[agent];
        worker.last_activity = Instant::now();
        worker
            .runtime
            .as_mut()
            .ok_or("worker not running")?
            .send(text, event)
    }

    /// Engine events and developments not yet sent, with pending obligations. A worker in a
    /// turn gets them at once (presented at its next tool-result boundary); an idle one gets
    /// them as a new turn. Neither clears anything in the engine.
    fn deliver(&mut self, agent: usize) -> Result<()> {
        let Some(runtime) = self.workers[agent].runtime.as_ref() else {
            return Ok(());
        };
        let (in_turn, idle) = (runtime.session.in_turn(), runtime.session.idle());
        if !in_turn && !idle {
            return Ok(()); // A message is already on its way to a new turn.
        }
        let client = self.arm.borrow().client(agent).clone();
        let events = self.engine.events(&client, self.workers[agent].delivered)?;
        // A ready agent is woken only by a new development, which reopens its work.
        if self.arm.borrow().is_ready(agent) && self.workers[agent].developments.is_empty() {
            return Ok(());
        }
        let developments = std::mem::take(&mut self.workers[agent].developments);
        if events.is_empty() && developments.is_empty() {
            return Ok(());
        }
        let pending = self.engine.pending(&client)?;
        let id = events.last().map(|e| e.id.clone()).unwrap_or_else(|| {
            format!(
                "development:{}",
                developments
                    .last()
                    .map_or("", |d| d["id"].as_str().unwrap_or(""))
            )
        });
        let text = format!(
            "Host notice {id}. Engine events: {}. Task developments: {}. Your pending obligations: {}.",
            serde_json::to_string(&events)?,
            serde_json::to_string(&developments)?,
            serde_json::to_string(&pending.obligations)?
        );
        if let Some(last) = events.last() {
            self.workers[agent].delivered = last.seq;
        }
        self.send(agent, text, Some(&id))
    }

    /// One pass over runtime lines: usage, rate limits, capped turns and idle reminders.
    fn observe(&mut self, agent: usize) -> Result<Option<&'static str>> {
        let ready = self.arm.borrow().is_ready(agent);
        let worker = &mut self.workers[agent];
        let Some(runtime) = worker.runtime.as_ref() else {
            return Ok(None);
        };
        let events = &runtime.session.events;
        for event in &events[worker.scanned.min(events.len())..] {
            if event["type"] == "rate_limit_event" {
                worker.rate_limit = event["rate_limit_info"].clone();
                if worker.rate_limit["status"] == "rejected" {
                    return Ok(Some("usage_limit"));
                }
            }
            if event["type"] == "result" {
                // Token counts only: they do not establish subscription cost.
                worker.usage.push(
                    json!({"turn": worker.usage.len() + 1, "subtype": event["subtype"],
                    "usage": event["usage"], "num_turns": event["num_turns"],
                    "duration_ms": event["duration_ms"], "rate_limit": worker.rate_limit}),
                );
            }
        }
        worker.scanned = events.len();
        let result = runtime.session.last_result.clone();
        let idle = runtime.session.idle();
        if result != worker.last_result {
            worker.last_result = result.clone();
            worker.last_activity = Instant::now();
            match result["subtype"].as_str() {
                Some("success") => {}
                Some("error_max_turns") if !ready => self.send(
                    agent,
                    "Host: your turn reached the per-message tool-call cap. Continue your task."
                        .into(),
                    None,
                )?,
                Some("error_max_turns") => {}
                other => return Err(format!("runtime turn failed: {other:?}").into()),
            }
        } else if idle && !ready && worker.last_activity.elapsed() >= NUDGE {
            self.send(agent, "Host: no new events for two minutes. If your part is unfinished, continue it; otherwise follow the workflow (offer, check, ready).".into(), None)?;
        }
        Ok(None)
    }
}

fn prompt(
    agent: usize,
    root: &Path,
    fixture: &Value,
    workflow: &str,
    optional: &[String],
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
         You cannot write it directly: change source only with falinks tools, each called as {{\"workspace\": \"{root}\", \"request\": {{...}}}}. \
         Your current directory is a private scratch directory where you may copy files and run builds or tests with Bash. \
         Engine events, host notices and task developments arrive as messages starting with \"Host notice\". \
         When you have nothing to do until the next event, end your turn; the next event starts a new turn. Do not run long sleeps or polling loops. \
         There are {developments} task development(s): acknowledge each with falinks_ack before working on it. \
         falinks_check gives pass/fail for the trusted checks on your workspace. When your part is finished and published, call falinks_ready. \
         Besides the existing files you may create only: {optional}.\n\n\
         Task: {task}\nYour allocation: {allocation}\n\nWorkflow:\n{workflow}",
        name = agent_name(agent),
        letter = LETTERS[agent],
        root = root.display(),
        optional = optional.join(", "),
        task = Value::Object(task),
        allocation = fixture["allocation"][LETTERS[agent]],
    )
}

struct Options {
    fixture: String,
    config: PathBuf,
    output: PathBuf,
    pair: String,
    order: i64,
    scored: bool,
}

fn options() -> Result<Options> {
    let args: Vec<String> = env::args().skip(1).collect();
    let fixture = args.first().cloned().ok_or(
        "usage: falinks-arm FIXTURE --config FILE --output DIR [--pair NAME] [--order N] [--scored true]",
    )?;
    let get = |key: &str| {
        args.iter()
            .position(|a| a == key)
            .and_then(|i| args.get(i + 1).cloned())
    };
    Ok(Options {
        fixture,
        config: get("--config").ok_or("--config required")?.into(),
        output: get("--output").ok_or("--output required")?.into(),
        pair: get("--pair").unwrap_or("unscored".into()),
        order: get("--order").unwrap_or("0".into()).parse()?,
        scored: get("--scored").as_deref() == Some("true"),
    })
}

fn main() {
    let outcome = options().and_then(run);
    match outcome {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}

fn run(options: Options) -> Result<i32> {
    let started = Instant::now();
    let config = read(&options.config)?;
    fs::create_dir(&options.output)?; // Never reuse a run directory or previous contexts.
    let dir = options.output.canonicalize()?;
    let private = dir.join("controller");
    fs::DirBuilder::new().mode(0o700).create(&private)?;
    let mut evidence = json!({
        "schema_version": 1, "run_id": dir.file_name().unwrap().to_string_lossy(), "arm": "falinks",
        "fixture": options.fixture, "configuration": config, "order": options.order, "pair": options.pair,
        "scored": options.scored, "outcome": "infrastructure_failure",
        "usage": {"status": "unavailable"}, "usage_limit_interruptions": [], "rejected_operations": 0,
        "retries": 0, "discarded_or_rewritten_work": {"status": "unavailable",
            "reason": "overwritten operations are retained in controller/history.json, not quantified"},
        "validation_attempts": [], "recommendations": 0, "transitions": 0, "launches": [],
        "timing": {"start_utc": utc(), "endpoint": "clean preparation through exact published candidate passing the independent oracle"},
        "tool_differences": config["tool_differences"],
    });
    let timeline = fs::File::create(private.join("events.jsonl"))?;
    let mut run = match setup(&options, &config, &dir, started, timeline, &mut evidence) {
        Ok(run) => run,
        Err(error) => {
            evidence["failure_reason"] = error.to_string().into();
            evidence["timing"]["elapsed_seconds"] = started.elapsed().as_secs_f64().into();
            evidence["timing"]["stop_utc"] = utc().into();
            fs::write(
                private.join("result.json"),
                serde_json::to_string_pretty(&evidence)?,
            )?;
            return Err(error);
        }
    };
    let stop = Arc::new(AtomicBool::new(false));
    let validator = {
        let (engine, stop) = (Arc::clone(&run.engine), Arc::clone(&stop));
        // The host processes queued check runs while agents keep drafting.
        thread::spawn(move || {
            let mut log = vec![];
            while !stop.load(Ordering::SeqCst) {
                match engine.validate() {
                    Ok(Some(run)) => log.push(json!({"run": run.id, "outcome": run.outcome})),
                    Ok(None) => thread::sleep(Duration::from_millis(200)),
                    Err(error) => {
                        log.push(json!({"error": error.to_string()}));
                        thread::sleep(Duration::from_millis(200));
                    }
                }
            }
            log
        })
    };
    let timeout = Duration::from_secs_f64(
        config["timeout_seconds"]
            .as_f64()
            .unwrap_or(1800.0)
            .clamp(1.0, 1800.0),
    );
    let result = drive(&mut run, started + timeout);
    stop.store(true, Ordering::SeqCst);
    let validations = validator.join().unwrap_or_default();
    let (outcome, reason) = match result {
        Ok(outcome) => (outcome, None),
        Err(error) => {
            // Unsafe publication or an unknown source change pauses the whole evaluation.
            let incident = run.engine.incident().ok().flatten();
            let outcome = if incident.is_some() {
                "safety_failure"
            } else {
                "infrastructure_failure"
            };
            (outcome, Some(error.to_string()))
        }
    };
    if run.evidence["timing"].get("elapsed_seconds").is_none() {
        run.evidence["timing"]["elapsed_seconds"] = started.elapsed().as_secs_f64().into();
        run.evidence["timing"]["stop_utc"] = utc().into();
    }
    let elapsed = run.evidence["timing"]["elapsed_seconds"]
        .as_f64()
        .unwrap_or(0.0);
    run.evidence["outcome"] = outcome.into();
    if let Some(reason) = &reason {
        run.event("failure", json!({"reason": reason}))?;
        run.evidence["failure_reason"] = reason.clone().into();
    }
    for agent in 0..2 {
        run.stop(agent);
    }
    let arm = run.arm.borrow();
    let proposals = run.engine.proposals()?;
    let changes = |p: &&falinks::RegroupingProposal| p.action != Action::Keep;
    let recommendations = proposals
        .iter()
        .filter(changes)
        .filter(|p| p.proposer.is_none())
        .count();
    let transitions = proposals
        .iter()
        .filter(changes)
        .filter(|p| matches!(p.state, ProposalState::Applied { .. }))
        .count();
    let usage: serde_json::Map<String, Value> = (0..2)
        .map(|a| (agent_name(a), json!(run.workers[a].usage)))
        .collect();
    let reported = usage
        .values()
        .filter(|u| u.as_array().is_some_and(|u| !u.is_empty()))
        .count();
    let mut evidence = run.evidence.clone();
    if let Some(final_outcome) = &arm.outcome {
        evidence["final_snapshot"] = final_outcome["final_snapshot"].clone();
        evidence["oracle"] = final_outcome["oracle"].clone();
        evidence["published_revision"] = final_outcome["published_revision"].clone();
    }
    evidence["usage"] = json!({"status": match reported { 2 => "available", 1 => "partial", _ => "unavailable" },
        "agents": usage, "format": "per-turn result reports; not cumulative totals or cost"});
    if outcome == "usage_limit" {
        let limits: Vec<Value> = (0..2)
            .map(|a| json!({"agent": a, "detail": run.workers[a].rate_limit}))
            .collect();
        evidence["usage_limit_interruptions"] = json!(limits);
    }
    evidence["rejected_operations"] = arm.rejected.into();
    evidence["retries"] = arm.retries.into();
    evidence["validation_attempts"] = json!(arm.validation_attempts);
    evidence["recommendations"] = recommendations.into();
    evidence["transitions"] = transitions.into();
    evidence["proposals"] = json!(proposals);
    evidence["engine_runs"] = json!(run.engine.runs()?);
    evidence["validator"] = json!(validations);
    evidence["timing"]["cleanup_elapsed_seconds"] =
        (started.elapsed().as_secs_f64() - elapsed).into();
    evidence["harness_sha256"] = file_hash(&env::current_exe()?)?.into();
    for row in &arm.log {
        writeln!(run.timeline, "{row}")?;
    }
    fs::write(
        private.join("history.json"),
        serde_json::to_string(&run.engine.history()?)?,
    )?;
    let transcripts: Vec<Value> = run.workers.iter().map(|w| json!(w.transcripts)).collect();
    fs::write(
        private.join("transcripts.json"),
        serde_json::to_string(&transcripts)?,
    )?;
    fs::write(
        private.join("result.json"),
        serde_json::to_string_pretty(&evidence)?,
    )?;
    println!(
        "{}",
        json!({"outcome": outcome, "evidence": private.join("result.json")})
    );
    Ok(if outcome == "success" { 0 } else { 1 })
}

fn setup(
    options: &Options,
    config: &Value,
    dir: &Path,
    started: Instant,
    timeline: fs::File,
    evidence: &mut Value,
) -> Result<Run> {
    require(
        config["model"] == MODEL && config["runtime_version"] == VERSION,
        "run configuration must name the adapter's pinned model and runtime version",
    )?;
    let binary = verify_binary(Path::new(
        config["runtime_binary"]
            .as_str()
            .ok_or("runtime_binary must be a path")?,
    ))?;
    let hash = file_hash(&binary)?;
    require(
        config["runtime_sha256"] == hash.as_str(),
        "runtime binary hash differs from frozen run configuration",
    )?;
    evidence["runtime_verified_sha256"] = hash.into();
    let evaluation = PathBuf::from(
        config["evaluation_root"]
            .as_str()
            .ok_or("evaluation_root must be a path")?,
    );
    let eval = PathBuf::from(
        config["eval_binary"]
            .as_str()
            .ok_or("eval_binary must be a path")?,
    );
    let manifest = read(&evaluation.join("manifest.json"))?;
    let fixture_path = evaluation.join(format!("fixtures/{}.json", options.fixture));
    // The public task bytes must be the frozen ones the Git arm also uses.
    require(
        manifest["hashes"][format!("fixtures/{}.json", options.fixture)]
            == file_hash(&fixture_path)?.as_str(),
        "fixture differs from the frozen manifest",
    )?;
    let fixture = read(&fixture_path)?;
    evidence["freeze"] = manifest.clone();
    evidence["fixture_version"] = fixture["version"].clone();

    let space0 = dir.join("space0");
    let prepared = Command::new(&eval)
        .args(["prepare", &options.fixture, "--output"])
        .arg(&space0)
        .output()?;
    require(
        prepared.status.success(),
        &format!(
            "prepare failed: {}",
            String::from_utf8_lossy(&prepared.stderr)
        ),
    )?;
    let prepared: Value = serde_json::from_slice(&prepared.stdout)?;
    evidence["initial_snapshot"] = prepared["initial_snapshot"].clone();
    let files: Vec<String> = fixture["files"]
        .as_object()
        .ok_or("fixture files")?
        .keys()
        .cloned()
        .collect();
    for file in files.iter().filter(|f| arm::reserved(f)) {
        // Held by the host and added to every candidate; the engine reserves dot-paths.
        fs::remove_file(space0.join(file))?;
    }
    let language = arm::language(fixture["language"].as_str().unwrap_or_default())?;
    let enrolled = arm::enrollment(&files, language);
    let optional: Vec<String> = enrolled
        .iter()
        .filter(|f| !files.contains(f))
        .cloned()
        .collect();
    let engine = Engine::open(
        &space0,
        &dir.join("controller/state"),
        &enrolled.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    let go_env = |key: &str| -> Result<PathBuf> {
        let out = Command::new("go").args(["env", key]).output()?;
        Ok(PathBuf::from(String::from_utf8(out.stdout)?.trim()))
    };
    let rust = Command::new("rustup").args(["which", "cargo"]).output()?;
    let tools = Tools {
        go: go_env("GOROOT")?.join("bin/go"),
        gopls: match env::var_os("FALINKS_GOPLS") {
            Some(path) => PathBuf::from(path),
            None => go_env("GOPATH")?.join("bin/gopls"),
        },
        rust: PathBuf::from(String::from_utf8(rust.stdout)?.trim())
            .parent()
            .ok_or("rust toolchain")?
            .to_owned(),
    };
    evidence["toolchain"] = tools.configure(&engine, language)?;
    evidence["enrolled"] = json!(enrolled);
    fs::create_dir(dir.join("space1"))?;
    engine.add_workspace(&dir.join("space1"))?;
    // Lets the engine recommend at later boundaries; agents decide through agreement.
    let initial = engine.reconsider()?;
    let clients = [0, 1].map(|a| engine.authenticate(a, &engine.credentials()[a]));
    let [c0, c1] = clients;
    let clients = [c0?, c1?];
    let engine = Arc::new(engine);

    let tool = env::current_exe()?.with_file_name("falinks-claude-tool");
    fs::create_dir(dir.join("worker-tools"))?;
    fs::copy(&tool, dir.join("worker-tools/falinks-claude-tool"))?;
    evidence["tool_sha256"] = file_hash(&tool)?.into();
    for agent in 0..2 {
        for part in ["scratch", "controller"] {
            fs::create_dir_all(dir.join(agent_name(agent)).join(part))?;
        }
    }
    let oracle = dir.join("controller/oracle");
    fs::create_dir(&oracle)?;
    let mut protected = vec![dir.join("controller")];
    let mut denied = vec![dir.join("controller/state")];
    for root in config["denied_roots"].as_array().into_iter().flatten() {
        let root = PathBuf::from(root.as_str().ok_or("denied_roots must be paths")?);
        denied.push(root.clone());
        if let Ok(root) = root.canonicalize()
            && root.is_dir()
        {
            protected.push(root);
        }
    }
    let arm = Arm::new(
        Arc::clone(&engine),
        clients,
        tools.go.with_file_name("gofmt"),
        fixture.clone(),
        eval,
        oracle,
        denied,
    );
    let workflow = fs::read_to_string(evaluation.join("instructions/falinks.md"))?;
    let root = engine.root(arm.client(0))?;
    let prompt = [0, 1].map(|a| prompt(a, &root, &fixture, &workflow, &optional));
    let worker = || Worker {
        runtime: None,
        session: None,
        source: PathBuf::new(),
        delivered: 0,
        developments: vec![],
        last_result: Value::Null,
        last_activity: Instant::now(),
        scanned: 0,
        rate_limit: Value::Null,
        usage: vec![],
        transcripts: vec![],
    };
    let mut run = Run {
        dir: dir.to_owned(),
        binary,
        started,
        engine,
        arm: Rc::new(RefCell::new(arm)),
        workers: [worker(), worker()],
        prompt,
        protected,
        timeline,
        evidence: evidence.clone(),
    };
    run.event("initial_recommendation", json!({"proposal": initial}))?;
    for agent in 0..2 {
        let text = run.prompt[agent].clone();
        run.start(agent, text)?;
    }
    *evidence = run.evidence.clone();
    Ok(run)
}

/// Pump both workers until both are ready (final oracle decided), a limit or a failure.
fn drive(run: &mut Run, deadline: Instant) -> Result<&'static str> {
    loop {
        if let Some(outcome) = run.arm.borrow().outcome.clone() {
            run.evidence["timing"]["elapsed_seconds"] = run.started.elapsed().as_secs_f64().into();
            run.evidence["timing"]["stop_utc"] = utc().into();
            return Ok(if outcome["outcome"] == "success" {
                "success"
            } else {
                "task_failure"
            });
        }
        if Instant::now() >= deadline {
            return Ok("timeout");
        }
        let developments = std::mem::take(&mut run.arm.borrow_mut().outbox);
        for development in developments {
            run.event("development", json!({"milestone": development["id"]}))?;
            for worker in &mut run.workers {
                worker.developments.push(development.clone());
            }
        }
        for agent in 0..2 {
            let moved = run.arm.borrow_mut().relocated.remove(&agent);
            let client = run.arm.borrow().client(agent).clone();
            let root = run.engine.root(&client)?.canonicalize()?;
            if moved || root != run.workers[agent].source {
                run.stop(agent);
                run.event("relocated", json!({"agent": agent, "root": root}))?;
                let text = format!(
                    "Host: the agreed regrouping moved your live workspace to {}. Use that path as \"workspace\" in every falinks call from now on and continue your task.",
                    root.display()
                );
                run.start(agent, text)?;
            }
            run.deliver(agent)?;
            if let Some(runtime) = run.workers[agent].runtime.as_mut() {
                runtime.step(Duration::from_millis(20))?;
                require(
                    !runtime.session.gate.failed,
                    "runtime safety control failed; run invalid",
                )?;
            }
            if let Some(limit) = run.observe(agent)? {
                run.event("usage_limit", json!({"agent": agent}))?;
                return Ok(limit);
            }
        }
        if let Some(incident) = run.engine.incident()? {
            return Err(format!("engine incident: {}", incident.reason).into());
        }
    }
}
