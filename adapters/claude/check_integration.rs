//! Two pinned Claude Code workers against the production engine (#26): host-scripted
//! phases on a dedicated Go gate fixture, real tool calls, deterministic host barriers
//! and controlled faults. A saved report never authorizes another run.
use falinks::{
    Action, Availability, Check, Client, Engine, Language, Pin, ProposalState, RunOutcome, Stage,
    Toolchain, sha256_file,
};
use falinks_claude::{
    Launch, MODEL, REQUIRED, Runtime, VERSION, engine_host::EngineHost, launch_args,
    probe_controls, probe_names, verify_binary,
};
use falinks_host::{Boundary, Result, file_hash, hash, require, runtime::EngineOutcome};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PRICE: &str = "package gate\n\nfunc Net(gross int) int {\n\treturn gross - Discount(gross)\n}\n\nfunc Discount(gross int) int {\n\treturn 0\n}\n";
const DISCOUNT_DRAFT: &str = "package gate\n\nfunc Net(gross int) int {\n\treturn gross - Discount(gross)\n}\n\nfunc Discount(gross int) int {\n\treturn gross / 20\n}\n";
const DISCOUNT_BROKEN: &str = "package gate\n\nfunc Net(gross int) int {\n\treturn gross - Discount(gross)\n}\n\nfunc Discount(gross int) int {\n\treturn gross /\n}\n";
const DISCOUNT_FINAL: &str = "package gate\n\nfunc Net(gross int) int {\n\treturn gross - Discount(gross)\n}\n\nfunc Discount(gross int) int {\n\treturn gross / 10\n}\n";
const LABEL: &str = "package gate\n\nfunc Label(name string) string {\n\treturn name\n}\n";
const LABEL_DRAFT: &str =
    "package gate\n\nfunc Label(name string) string {\n\treturn \"item \" + name\n}\n";
const LABEL_FINAL: &str =
    "package gate\n\nfunc Label(name string) string {\n\treturn \"item: \" + name\n}\n";
const SUMMARY_DRAFT: &str =
    "package gate\n\nfunc Summary(gross int, name string) string {\n\treturn Label(name)\n}\n";
const SUMMARY_FINAL: &str = "package gate\n\nimport \"strconv\"\n\nfunc Summary(gross int, name string) string {\n\treturn Label(name) + \" costs \" + strconv.Itoa(Net(gross))\n}\n";
const SUMMARY_NET: &str = "package gate\n\nimport \"strconv\"\n\nfunc Summary(gross int, name string) string {\n\treturn Label(name) + \" nets \" + strconv.Itoa(Net(gross))\n}\n";
/// gofmt turns this into `SUMMARY_FINAL`.
const SUMMARY_MESSY: &str = "package gate\n\nimport \"strconv\"\n\nfunc Summary(gross int, name string) string {\n\treturn Label(name)+\" costs \"+strconv.Itoa(Net(gross))\n}\n";
const LABEL_LOUD: &str =
    "package gate\n\nfunc Label(name string) string {\n\treturn \"item: \" + name + \"!\"\n}\n";
const TEST: &str = "package gate\n\nimport (\n\t\"strings\"\n\t\"testing\"\n)\n\nfunc TestNetStaysWithinGross(t *testing.T) {\n\tfor gross := 0; gross <= 1000; gross++ {\n\t\tif n := Net(gross); n < 0 || n > gross {\n\t\t\tt.Fatalf(\"Net(%d) = %d\", gross, n)\n\t\t}\n\t}\n}\n\nfunc TestLabelNamesTheItem(t *testing.T) {\n\tif !strings.Contains(Label(\"lamp\"), \"lamp\") {\n\t\tt.Fatal(Label(\"lamp\"))\n\t}\n}\n";
/// Enrolled source; `summary.go` is absent until agent 1 creates it.
const FILES: &[(&str, Option<&str>)] = &[
    ("go.mod", Some("module example.com/gate\n\ngo 1.22\n")),
    ("price.go", Some(PRICE)),
    ("label.go", Some(LABEL)),
    ("gate_test.go", Some(TEST)),
    ("summary.go", None),
];
const PROTECTED: &[&str] = &["state", "controller", "peer"];
const PHASE: Duration = Duration::from_secs(900);

struct Worker {
    runtime: Option<Runtime>,
    client: Client,
    session: Option<String>,
    /// Highest engine event seq sent to this worker.
    delivered: u64,
    controls: BTreeMap<String, bool>,
    transcripts: Vec<Value>,
}
struct Gate {
    dir: PathBuf,
    binary: PathBuf,
    go: PathBuf,
    gopls: PathBuf,
    engine: Option<Arc<Engine>>,
    workers: [Worker; 2],
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    validator: Option<JoinHandle<Vec<Value>>>,
    validations: Vec<Value>,
    evidence: Value,
}

fn name(agent: usize) -> String {
    format!("agent-{agent}")
}
fn space(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("space{index}"))
}
fn json_string(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

impl Gate {
    fn engine(&self) -> &Arc<Engine> {
        self.engine.as_ref().expect("engine open")
    }
    fn client(&self, agent: usize) -> &Client {
        &self.workers[agent].client
    }
    fn open(dir: &Path, go: &Path, gopls: &Path) -> Result<(Engine, [Client; 2])> {
        let enrolled: Vec<&str> = FILES.iter().map(|(p, _)| *p).collect();
        let engine = Engine::open(&space(dir, 0), &dir.join("state"), &enrolled)?;
        let pin = |path: &Path| -> Result<Pin> {
            Ok(Pin {
                sha256: sha256_file(path)?,
                path: path.to_owned(),
            })
        };
        engine.configure_analysis(vec![Toolchain {
            language: Language::Go,
            executables: vec![pin(gopls)?, pin(go)?],
            features: Vec::new(),
        }])?;
        let go = go.to_string_lossy().into_owned();
        engine.configure_checks(vec![
            Check {
                name: "vet".into(),
                program: go.clone(),
                args: ["vet", "./..."].map(String::from).to_vec(),
            },
            Check {
                name: "test".into(),
                program: go,
                args: ["test", "./..."].map(String::from).to_vec(),
            },
        ])?;
        let clients = [
            engine.authenticate(0, &engine.credentials()[0])?,
            engine.authenticate(1, &engine.credentials()[1])?,
        ];
        Ok((engine, clients))
    }
    fn new(binary: &Path, probe: &Path, tool: &Path) -> Result<Self> {
        let binary = verify_binary(binary)?;
        let dir = tempfile::Builder::new()
            .prefix("falinks-integration-")
            .tempdir_in("/private/tmp")?
            .keep()
            .canonicalize()?;
        for path in ["space0", "space1", "worker-tools"] {
            fs::create_dir(dir.join(path))?;
        }
        for agent in 0..2 {
            for part in ["scratch", "controller"] {
                fs::create_dir_all(dir.join(name(agent)).join(part))?;
            }
            fs::write(
                dir.join(name(agent)).join("controller/marker.txt"),
                b"marker\n",
            )?;
        }
        for (path, bytes) in FILES {
            if let Some(bytes) = bytes {
                fs::write(space(&dir, 0).join(path), bytes)?;
            }
        }
        fs::copy(probe, dir.join("worker-tools/sandbox-probe"))?;
        fs::copy(tool, dir.join("worker-tools/falinks-claude-tool"))?;
        let go_env = |key: &str| -> Result<PathBuf> {
            let out = Command::new("go").args(["env", key]).output()?;
            Ok(PathBuf::from(String::from_utf8(out.stdout)?.trim()))
        };
        let go = go_env("GOROOT")?.join("bin/go");
        let gopls = match env::var_os("FALINKS_GOPLS") {
            Some(path) => PathBuf::from(path),
            None => go_env("GOPATH")?.join("bin/gopls"),
        };
        let (engine, clients) = Self::open(&dir, &go, &gopls)?;
        fs::write(dir.join("state/marker.txt"), b"marker\n")?;
        let [c0, c1] = clients;
        let worker = |client| Worker {
            runtime: None,
            client,
            session: None,
            delivered: 0,
            controls: BTreeMap::new(),
            transcripts: vec![],
        };
        let mut gate = Self {
            evidence: json!({"root":dir,"binary":binary,"binary_sha256":file_hash(&binary)?,
                "claude_code_version":VERSION,"model":MODEL,
                "probe_sha256":file_hash(probe)?,"tool_sha256":file_hash(tool)?,
                "go":go,"go_sha256":file_hash(&go)?,"gopls":gopls,"gopls_sha256":file_hash(&gopls)?,
                "fixture":FILES,"checks":["go vet ./...","go test ./..."],"phases":{},"launches":[]}),
            dir,
            binary,
            go,
            gopls,
            engine: Some(Arc::new(engine)),
            workers: [worker(c0), worker(c1)],
            stop: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
            validator: None,
            validations: vec![],
        };
        gate.start_validator();
        Ok(gate)
    }

    /// The host worker processes queued runs while agents keep drafting.
    fn start_validator(&mut self) {
        let (engine, stop, pause) = (
            Arc::clone(self.engine()),
            Arc::clone(&self.stop),
            Arc::clone(&self.pause),
        );
        self.stop.store(false, Ordering::SeqCst);
        self.validator = Some(thread::spawn(move || {
            let mut log = vec![];
            while !stop.load(Ordering::SeqCst) {
                if !pause.load(Ordering::SeqCst) {
                    match engine.validate() {
                        Ok(Some(run)) => log.push(json!({"run":run.id,"outcome":run.outcome})),
                        Ok(None) => {}
                        Err(error) => log.push(json!({"error":error.to_string()})),
                    }
                }
                thread::sleep(Duration::from_millis(200));
            }
            log
        }));
    }
    fn stop_validator(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.validator.take()
            && let Ok(log) = handle.join()
        {
            self.validations.extend(log);
        }
    }

    fn launch(&self, agent: usize) -> Result<Launch> {
        let root = self.engine().root(self.client(agent))?.canonicalize()?;
        let mine = self.dir.join(name(agent));
        let mut protected = vec![
            self.dir.join("state"),
            self.dir.join(name(1 - agent)).join("controller"),
        ];
        // Only the agent's own live root is exposed; any other space stays hidden.
        protected.extend((0..2).map(|i| space(&self.dir, i)).filter(|s| *s != root));
        let alias = mine.join("scratch/alias");
        let _ = fs::remove_file(&alias);
        symlink(root.join("price.go"), &alias)?;
        Ok(Launch {
            tools: falinks_claude::engine_host::tools(&root),
            source: root,
            scratch: mine.join("scratch"),
            controller: mine.join("controller"),
            worker_tools: self.dir.join("worker-tools"),
            protected,
        })
    }
    /// Launch or resume a worker against the agent's current workspace.
    fn start(&mut self, agent: usize) -> Result<()> {
        let launch = self.launch(agent)?;
        let engine = Arc::clone(self.engine());
        let client = engine.authenticate(agent, &engine.credentials()[agent])?;
        let resume = self.workers[agent].session.clone();
        if let Some(session) = &resume {
            // A host-applied regrouping may have moved this agent's workspace.
            Boundary::new(
                &launch.controller.join("context.sqlite"),
                &launch.source,
                "host".into(),
            )?
            .relocate(session, &name(agent))?;
        }
        let gofmt = self.go.with_file_name("gofmt");
        let host = EngineHost::new(engine, client.clone(), &name(agent), gofmt);
        let runtime = Runtime::new(
            &self.binary,
            &launch,
            &name(agent),
            resume.as_deref(),
            Box::new(move |envelope| {
                Ok(EngineOutcome {
                    result: host.call(envelope)?,
                    notice: None,
                })
            }),
            true,
        )?;
        let worker = &mut self.workers[agent];
        worker.session = Some(runtime.session.session().to_owned());
        worker.client = client;
        worker.runtime = Some(runtime);
        let record = json!({"agent":agent,"resume":resume.is_some(),"source":launch.source,
            "protected":launch.protected,"settings":fs::read_to_string(launch.controller.join("settings.json"))?,
            "args":launch_args(&launch.controller, worker.session.as_deref().unwrap_or(""), resume.is_some())});
        if let Some(launches) = self.evidence["launches"].as_array_mut() {
            launches.push(record);
        }
        Ok(())
    }
    /// Close the worker, archive its transcript and tell the engine it disconnected.
    fn stop_worker(&mut self, agent: usize) -> Result<()> {
        if let Some(mut runtime) = self.workers[agent].runtime.take() {
            runtime.close();
            let session = &runtime.session;
            let worker = &mut self.workers[agent];
            for (control, passed) in &session.gate.controls {
                let entry = worker.controls.entry(control.clone()).or_insert(true);
                *entry &= *passed;
            }
            worker
                .transcripts
                .push(json!({"session":session.session(),"events":session.events,
                "deliveries":session.deliveries,"permission_denials":session.denials,
                "gate_failed":session.gate.failed,"project":session.project}));
            if let Some(engine) = &self.engine {
                engine.disconnect(&self.workers[agent].client)?;
            }
        }
        Ok(())
    }
    fn runtime(&mut self, agent: usize) -> Result<&mut Runtime> {
        Ok(self.workers[agent]
            .runtime
            .as_mut()
            .ok_or("worker not running")?)
    }
    fn control(&mut self, agent: usize, control: &str) -> Result<()> {
        self.workers[agent].controls.insert(control.into(), true);
        self.runtime(agent)?.session.gate.record(control, true)
    }

    fn header(&self, agent: usize) -> Result<String> {
        let root = self.engine().root(self.client(agent))?;
        Ok(format!(
            "[Falinks host] You are {name} in a Falinks coordination test on throwaway files. Your live workspace is {root} (read it with Read, Grep or Glob). You cannot write it directly: change source only with falinks tools, each called as {{\"workspace\": \"{root}\", \"request\": {{...}}}}. Do exactly the numbered steps below, one tool call at a time, then end your turn with a one-line summary. Host notices are information; act on them only when a step says so.",
            name = name(agent),
            root = root.display()
        ))
    }
    /// Events the worker has not been sent yet, oldest first.
    fn undelivered(&self, agent: usize) -> Result<Vec<falinks::Event>> {
        self.engine()
            .events(self.client(agent), self.workers[agent].delivered)
    }
    /// Send a host prompt, replaying undelivered events and pending obligations first.
    fn prompt(&mut self, agent: usize, steps: &str) -> Result<()> {
        let events = self.undelivered(agent)?;
        let pending = self.engine().pending(self.client(agent))?;
        let deferred: Vec<Value> = pending
            .deferred
            .iter()
            .map(|(event, note)| json!({"event":event,"note":note}))
            .collect();
        let text = format!(
            "{}\nEngine events since your last notice: {}\nYour deferred events: {}\nYour pending obligations: {}\n\n{steps}",
            self.header(agent)?,
            json_string(&events),
            json_string(&deferred),
            json_string(&pending.obligations),
        );
        if let Some(last) = events.last() {
            self.workers[agent].delivered = last.seq;
        }
        self.runtime(agent)?.send(text, None)
    }
    /// Steering: a worker in an active turn is sent new events at once; the runtime
    /// presents them at its next tool-result boundary. Idle workers get them with
    /// their next prompt. Neither clears anything in the engine.
    fn notices(&mut self) -> Result<()> {
        for agent in 0..2 {
            let in_turn = self.workers[agent]
                .runtime
                .as_ref()
                .is_some_and(|r| r.session.in_turn());
            if !in_turn {
                continue;
            }
            let events = self.undelivered(agent)?;
            let Some(last) = events.last() else {
                continue;
            };
            let pending = self.engine().pending(self.client(agent))?;
            let text = format!(
                "Host notice {}: engine events {}. Your pending obligations: {}. Continue your current steps.",
                last.id,
                json_string(&events),
                json_string(&pending.obligations)
            );
            self.workers[agent].delivered = last.seq;
            let id = last.id.clone();
            self.runtime(agent)?.send(text, Some(&id))?;
        }
        Ok(())
    }
    /// Pump both workers until every one is idle and `hook` reports it is done.
    fn drive(&mut self, hook: impl FnMut(&mut Self) -> Result<bool>) -> Result<()> {
        self.pump(&[0, 1], true, hook)
    }
    /// Step only `agents` until `until` holds (and, when `idle`, every worker is idle).
    /// A worker left out is a host barrier: its mediated calls wait for their replies.
    fn pump(
        &mut self,
        agents: &[usize],
        idle: bool,
        mut until: impl FnMut(&mut Self) -> Result<bool>,
    ) -> Result<()> {
        let deadline = Instant::now() + PHASE;
        loop {
            self.notices()?;
            for &agent in agents {
                if let Some(runtime) = self.workers[agent].runtime.as_mut() {
                    runtime.step(Duration::from_millis(20))?;
                    if runtime.session.gate.failed {
                        // Any failed control blocks publication at once.
                        self.pause.store(true, Ordering::SeqCst);
                    }
                }
            }
            let done = until(self)?;
            let quiet = !idle
                || self
                    .workers
                    .iter()
                    .all(|w| w.runtime.as_ref().is_none_or(|r| r.session.idle()));
            if done && quiet {
                for worker in self.workers.iter().filter_map(|w| w.runtime.as_ref()) {
                    let result = &worker.session.last_result;
                    require(
                        result.is_null() || result["subtype"] == "success",
                        &format!("turn did not complete: {result}"),
                    )?;
                }
                return Ok(());
            }
            require(Instant::now() < deadline, "phase deadline exceeded")?;
        }
    }
    fn run(&mut self, prompts: &[(usize, String)]) -> Result<()> {
        for (agent, steps) in prompts {
            self.prompt(*agent, steps)?;
        }
        self.drive(|_| Ok(true))
    }
    /// Mediated calls of one operation in the worker's current process.
    fn calls(&self, agent: usize, operation: &str) -> Vec<Value> {
        self.workers[agent]
            .runtime
            .as_ref()
            .map(|r| {
                r.session
                    .events
                    .iter()
                    .filter(|e| e["host_call"]["operation"] == operation)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    fn accepted(&self, agent: usize, operation: &str) -> Vec<Value> {
        self.calls(agent, operation)
            .into_iter()
            .filter(|c| c["result"]["accepted"] == true)
            .collect()
    }
    fn probe(&self, agent: usize) -> Result<String> {
        let root = self.engine().root(self.client(agent))?;
        let mine = self.dir.join(name(agent));
        let args = [
            self.dir
                .join("worker-tools/sandbox-probe")
                .to_string_lossy()
                .into_owned(),
            root.join("price.go").to_string_lossy().into_owned(),
            hash(&fs::read(root.join("price.go"))?),
            mine.join("scratch").to_string_lossy().into_owned(),
            format!("state={}", self.dir.join("state/marker.txt").display()),
            format!(
                "controller={}",
                mine.join("controller/marker.txt").display()
            ),
            format!(
                "peer={}",
                self.dir
                    .join(name(1 - agent))
                    .join("controller/marker.txt")
                    .display()
            ),
        ];
        Ok(shlex::try_join(args.iter().map(String::as_str))?)
    }
    /// The probe ran through the real Bash tool with every filesystem control passing.
    fn sandbox(&mut self, agent: usize, command: &str) -> Result<Value> {
        let checks = probe_controls(
            &self.runtime(agent)?.session.events,
            command,
            &probe_names(PROTECTED),
        )?;
        for control in [
            "source_write_denial",
            "storage_protection",
            "scratch_access",
        ] {
            self.control(agent, control)?;
        }
        Ok(checks)
    }
    fn phase(&mut self, name: &str, result: Value) {
        println!("PASS: {name}");
        self.evidence["phases"][name] = result;
    }
    fn read_live(&self, agent: usize, path: &str) -> Result<String> {
        Ok(fs::read_to_string(
            self.engine().root(self.client(agent))?.join(path),
        )?)
    }
}

/// A notice names the last engine event of its batch.
fn event_seq(delivery: &Value) -> Option<u64> {
    delivery["event"]
        .as_str()
        .and_then(|id| id.rsplit(':').next()?.parse().ok())
}
/// What to do when an edit is refused for a pending obligation.
fn unreviewed(retry: &str) -> String {
    format!(
        " If the result is Unreviewed: call falinks_pending, call falinks_capture with request {{}}, then for each pending obligation falinks_review with request {{\"scope\": SCOPE, \"revision\": REVISION, \"decision\": \"Keep\", \"note\": \"reread the current workspace\"}} using that capture's revision, then repeat this edit with id \"{retry}\" and a fresh version."
    )
}
fn edit_step(id: &str, path: &str, content: &str, version: &str) -> String {
    format!(
        "falinks_edit with request {{\"id\": \"{id}\", \"expected\": {{\"{path}\": {version}}}, \"output\": {{\"{path}\": {{\"content\": {}}}}}}}",
        json!(content)
    )
}

fn scenario(gate: &mut Gate) -> Result<()> {
    // Phase 1: both workers at once: sandbox and storage controls, scope registration.
    let scopes = [
        json!({"id":"discount","task":"discount","nodes":["price.go#Discount"]}),
        json!({"id":"label","task":"label","nodes":["label.go#Label"]}),
    ];
    let mut probes = vec![];
    for (agent, scope) in scopes.iter().enumerate() {
        let probe = gate.probe(agent)?;
        gate.start(agent)?;
        let steps = format!(
            "1. Bash: run exactly `{probe}` (do not change it) and keep its FALINKS_CONTROLS line.\n2. Call the Read tool on {ledger} even if you expect it to fail, and report the tool result.\n3. falinks_register with request {scope}.\n4. falinks_capture with request {{}}.",
            ledger = gate.dir.join("state/ledger.sqlite").display()
        );
        gate.prompt(agent, &steps)?;
        probes.push(probe);
    }
    gate.drive(|_| Ok(true))?;
    let mut controls = vec![];
    for (agent, probe) in probes.iter().enumerate() {
        controls.push(gate.sandbox(agent, probe)?);
        let ledger = json!(gate.dir.join("state/ledger.sqlite"));
        require(
            gate.runtime(agent)?
                .session
                .denials
                .iter()
                .any(|d| d["tool_name"] == "Read" && d["tool_input"]["file_path"] == ledger),
            "native Read of engine storage was not denied",
        )?;
        require(
            gate.accepted(agent, "register").len() == 1
                && !gate.accepted(agent, "capture").is_empty(),
            "registration or capture missing",
        )?;
        gate.control(agent, "mediated_calls")?;
        // Every turn's init matched the exact tool list; nothing outside it was attempted.
        gate.control(agent, "disabled_capabilities")?;
    }
    gate.phase("controls", json!({"sandbox":controls}));

    // Phase 2: unfinished but compiling drafts in different files of the shared workspace.
    let version = |gate: &Gate, path: &str| -> Result<String> {
        Ok(gate.engine().capture()?.files[path].version.to_string())
    };
    let steps = format!(
        "1. falinks_capture with request {{\"paths\": [\"price.go\"]}}.\n2. {} — use the price.go version step 1 returned instead of VERSION.",
        edit_step("discount-draft", "price.go", DISCOUNT_DRAFT, "VERSION")
    );
    gate.run(&[(0, steps)])?;
    let steps = format!(
        "1. falinks_capture with request {{\"paths\": [\"label.go\"]}}.\n2. {} — use the label.go version step 1 returned instead of VERSION.\n3. falinks_capture with request {{}}.\n4. falinks_offer with request {{\"id\": \"label-early\", \"task\": \"label\", \"revision\": REVISION, \"scope\": [\"label\"], \"text\": \"unfinished label draft\"}} — REVISION is the revision step 3 returned.",
        edit_step("label-draft", "label.go", LABEL_DRAFT, "VERSION")
    );
    gate.run(&[(1, steps)])?;
    require(
        gate.read_live(0, "price.go")? == DISCOUNT_DRAFT
            && gate.read_live(1, "label.go")? == LABEL_DRAFT,
        "independent drafts did not both survive",
    )?;
    let early = gate
        .engine()
        .checkpoint("1/label-early")?
        .ok_or("early offer missing")?;
    let candidate = gate.engine().candidate(early.capture.revision)?;
    require(
        early.members == BTreeSet::from([0, 1]) && candidate.missing == BTreeSet::from([0]),
        "the team candidate must hold agent 0's unoffered draft",
    )?;
    require(
        gate.engine().runs()?.is_empty(),
        "an incomplete team was checked",
    )?;
    gate.phase(
        "drafts",
        json!({"early_offer":early,"missing":candidate.missing}),
    );

    // Phase 3: engine-recommended split, agreed by both real agents; agent 1 resumes elsewhere.
    gate.engine().add_workspace(&space(&gate.dir, 1))?;
    let proposal = gate.engine().reconsider()?;
    require(
        proposal.action == Action::Split,
        &format!("expected a split: {}", proposal.reasoning),
    )?;
    let respond = format!(
        "1. falinks_pending with request {{}}.\n2. falinks_respond with request {{\"id\": \"{}\", \"context\": \"{}\", \"response\": \"Agree\"}} — this agrees to the Regrouping proposal in your events.",
        proposal.id, proposal.context
    );
    gate.run(&[(0, respond.clone()), (1, respond)])?;
    let applied = gate
        .engine()
        .proposal(&proposal.id)?
        .ok_or("proposal missing")?;
    require(
        matches!(applied.state, ProposalState::Applied { .. })
            && gate.engine().placement()? == [0, 1],
        &format!("split not applied: {:?}", applied.state),
    )?;
    gate.stop_worker(1)?;
    gate.start(1)?;
    // Unfinished work moved with its author; readiness did not move with it.
    let moved = gate.engine().capture_for(gate.client(1))?;
    let after = gate.engine().candidate(moved.revision)?;
    require(
        gate.read_live(0, "label.go")? == LABEL
            && gate.read_live(1, "label.go")? == LABEL_DRAFT
            && gate.engine().checkpoint("1/label-early")?.map(|c| c.state)
                == Some(Availability::Available)
            && after.missing == BTreeSet::from([1]),
        "split lost a draft or transferred readiness",
    )?;
    gate.phase(
        "split",
        json!({"proposal":applied,"space1_candidate_missing":after.missing}),
    );

    // Phase 4: agent 1 offers its finished label; checks run on that exact candidate
    // while agent 0 keeps drafting in its own workspace.
    gate.pause.store(true, Ordering::SeqCst);
    let probe = gate.probe(1)?;
    let steps = format!(
        "1. Bash: run exactly `{probe}` (do not change it).\n2. falinks_capture with request {{\"paths\": [\"label.go\"]}}.\n3. {} — use the label.go version step 2 returned.\n4. falinks_capture with request {{}}.\n5. falinks_offer with request {{\"id\": \"label\", \"task\": \"label\", \"revision\": REVISION, \"scope\": [\"label\"], \"text\": \"label finished\"}} — REVISION from step 4.",
        edit_step("label-final", "label.go", LABEL_FINAL, "VERSION")
    );
    gate.run(&[(1, steps)])?;
    gate.sandbox(1, &probe)?;
    let held = gate.engine().runs()?;
    require(held.len() == 1, "complete coverage did not queue one run")?;
    let steps = format!(
        "1. falinks_capture with request {{\"paths\": [\"price.go\"]}}.\n2. {} — use the price.go version step 1 returned.",
        edit_step("discount-final", "price.go", DISCOUNT_FINAL, "VERSION")
    );
    gate.run(&[(0, steps)])?;
    gate.pause.store(false, Ordering::SeqCst);
    let id = held[0].id.clone();
    gate.drive(|gate| {
        Ok(gate
            .engine()
            .run(&id)
            .ok()
            .flatten()
            .is_some_and(|r| r.outcome.is_some()))
    })?;
    let run = gate.engine().run(&id)?.ok_or("run missing")?;
    let published = gate.engine().published()?;
    require(
        matches!(run.outcome, Some(RunOutcome::Published { .. }))
            && published.revision == run.binding.revision
            && published.files["price.go"]
                .file
                .as_ref()
                .map(|f| f.bytes.as_slice())
                == Some(PRICE.as_bytes())
            && gate.read_live(0, "price.go")? == DISCOUNT_FINAL,
        &format!(
            "exact candidate was not the one published: {:?}",
            run.outcome
        ),
    )?;
    gate.phase("checks_while_drafting", json!({"run":run}));

    // Phase 5: agent 1's next task uses agent 0's work: it registers, proposes a join,
    // and both agree. Agent 1 resumes in the shared workspace.
    let steps = "1. falinks_register with request {\"id\": \"summary\", \"task\": \"summary\", \"nodes\": [\"summary.go\"], \"depends\": [\"price.go#Net\", \"label.go#Label\"]}.\n2. falinks_propose with request {\"action\": \"Join\", \"text\": \"Summary uses Net from agent-0's discount work\", \"failing\": false}.";
    gate.run(&[(1, steps.into())])?;
    let proposal = gate
        .accepted(1, "propose")
        .last()
        .map(|c| c["result"]["proposal"].clone())
        .ok_or("join proposal missing")?;
    require(
        proposal["action"] == "Join",
        &format!("expected a join: {proposal}"),
    )?;
    let respond = format!(
        "1. falinks_respond with request {{\"id\": {}, \"context\": {}, \"response\": \"Agree\"}} — this agrees to the Regrouping proposal in your events.",
        proposal["id"], proposal["context"]
    );
    gate.run(&[(0, respond.clone()), (1, respond)])?;
    require(gate.engine().placement()? == [0, 0], "join not applied")?;
    gate.stop_worker(1)?;
    gate.start(1)?;
    gate.phase(
        "join",
        json!({"proposal":gate.engine().proposal(proposal["id"].as_str().unwrap_or(""))?}),
    );

    // Phase 6: steering. While agent 1's turn is active, agent 0 installs an incomplete
    // draft that agent 1's registered work depends on.
    // The join's transition raised obligations; agent 1 reviews them before drafting.
    let review = "1. falinks_pending with request {}.\n2. falinks_capture with request {}.\n3. For each pending obligation, falinks_review with request {\"scope\": SCOPE, \"revision\": REVISION, \"decision\": \"Keep\", \"note\": \"reread the current workspace\"} using that capture's revision. If none is pending, do nothing.";
    gate.run(&[(0, review.into()), (1, review.into())])?;
    let summary_version = version(gate, "summary.go")?;
    let steps = format!(
        "1. {}\n2. falinks_capture with request {{\"paths\": [\"price.go\"]}} and quote the body of Discount from its price.go content.\n3. Read {root}/price.go and quote the body of Discount exactly as it is now.\n4. falinks_capture with request {{\"paths\": [\"summary.go\"]}}.\n5. {} — use the summary.go version step 4 returned. Attempt this edit before any falinks_review.{}",
        edit_step(
            "summary-draft",
            "summary.go",
            SUMMARY_DRAFT,
            &summary_version
        ),
        edit_step("summary-final", "summary.go", SUMMARY_FINAL, "VERSION"),
        unreviewed("summary-final-2"),
        root = gate.engine().root(gate.client(1))?.display()
    );
    let before = gate.accepted(1, "edit").len();
    let phase_edits = gate.calls(1, "edit").len();
    gate.prompt(1, &steps)?;
    // Host barrier: once agent 1's draft is in, only agent 0 runs until its incomplete
    // draft commits. Agent 1's next mediated call waits, inside the same turn.
    gate.pump(&[1], false, |gate| {
        Ok(gate.accepted(1, "edit").len() > before)
    })?;
    let broken = format!(
        "1. falinks_capture with request {{\"paths\": [\"price.go\"]}}.\n2. {} — this is an intentionally unfinished draft that does not compile yet; use the price.go version step 1 returned.{}",
        edit_step("discount-broken", "price.go", DISCOUNT_BROKEN, "VERSION"),
        unreviewed("discount-broken-2")
    );
    gate.prompt(0, &broken)?;
    gate.pump(&[0], false, |gate| {
        Ok(gate.read_live(0, "price.go")? == DISCOUNT_BROKEN
            && gate.workers[0]
                .runtime
                .as_ref()
                .is_some_and(|r| r.session.idle()))
    })?;
    require(
        gate.runtime(1)?.session.in_turn(),
        "agent 1's turn ended before the peer edit",
    )?;
    gate.drive(|_| Ok(true))?;
    let obligation = gate
        .engine()
        .events(gate.client(1), 0)?
        .into_iter()
        .rev()
        .find(|e| matches!(&e.body, falinks::Body::Obligation { agent: 1, .. }))
        .ok_or("no obligation reached agent 1")?;
    let runtime = gate.runtime(1)?;
    let delivery = runtime
        .session
        .deliveries
        .iter()
        .find(|d| event_seq(d) >= Some(obligation.seq))
        .cloned()
        .ok_or("obligation notice not sent mid-turn")?;
    // A native Read (not a capture) returned the peer's incomplete draft.
    let reads: Vec<Value> = runtime
        .session
        .events
        .iter()
        .filter(|e| e["type"] == "assistant")
        .flat_map(|e| {
            e["message"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|b| b["type"] == "tool_use" && b["name"] == "Read")
        .map(|b| b["id"].clone())
        .collect();
    let saw_draft = runtime
        .session
        .events
        .iter()
        .filter(|e| e["type"] == "user" && e["isReplay"] != true)
        .flat_map(|e| {
            e["message"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .any(|b| {
            b["type"] == "tool_result"
                && reads.contains(&b["tool_use_id"])
                && b.to_string().contains("return gross /\\n")
        });
    let reviews = gate.accepted(1, "review");
    let edits = gate.calls(1, "edit").split_off(phase_edits);
    let refused = edits
        .iter()
        .position(|c| c["result"]["outcome"]["Unreviewed"].is_object());
    let applied = edits.iter().position(|c| {
        c["result"]["accepted"] == true
            && c["host_call"]["request"]["output"]["summary.go"]["content"] == SUMMARY_FINAL
    });
    require(
        delivery["sent_in_turn"] == true
            && delivery["presented_span"] == delivery["sent_span"]
            && saw_draft
            && refused.is_some()
            && applied > refused,
        "steering, live incomplete draft or review-before-write failed",
    )?;
    gate.control(1, "steering")?;
    gate.phase(
        "steering",
        json!({"delivery":delivery,"edits":edits,"reviews":reviews,"saw_incomplete_peer_draft":saw_draft}),
    );

    // Whole-operation stale rejection: two outputs bound to a superseded price.go version.
    let first = gate.workers[1].transcripts[0]["events"]
        .as_array()
        .and_then(|events| {
            events
                .iter()
                .find(|e| e["host_call"]["operation"] == "capture")
        })
        .map(|e| e["result"]["files"]["price.go"]["version"].clone())
        .ok_or("phase 1 capture missing")?;
    let current = gate.engine().capture()?;
    let stale = json!({"id":"both-stale","expected":{"price.go":first,
        "label.go":current.files["label.go"].version,"summary.go":current.files["summary.go"].version},
        "output":{"label.go":{"content":"package gate\n\nfunc Label(name string) string {\n\treturn \"x\"\n}\n"},
            "summary.go":{"content":"package gate\n"}}});
    gate.run(&[(
        1,
        format!("1. falinks_edit with exactly this request: {stale}"),
    )])?;
    let record = gate
        .engine()
        .request(gate.client(1), "both-stale")?
        .ok_or("stale request not retained")?;
    require(
        matches!(&record.outcome, Some(falinks::Outcome::Stale { conflicts }) if conflicts.contains_key("price.go"))
            && record.request.output.len() == 2
            && gate.read_live(1, "summary.go")? == SUMMARY_FINAL
            && gate.read_live(1, "label.go")? == LABEL_FINAL,
        "stale request applied output or lost its proposal",
    )?;
    gate.phase("stale", json!({"record":record}));

    // Phase 7: finish, review, offer the exact team candidate; acceptance commits but the
    // host faults before acknowledging it, then restarts the engine and both workers.
    // Steering for agent 0 too: it declares a dependency on Summary, the host holds it
    // at its edit call, and agent 1's Summary change reaches it inside that turn.
    let steps = format!(
        "1. falinks_register with request {{\"id\": \"summary-check\", \"task\": \"discount\", \"nodes\": [\"price.go#Net\"], \"depends\": [\"summary.go#Summary\"]}}.\n2. falinks_pending with request {{}}.\n3. falinks_capture with request {{}}.\n4. For each pending obligation, falinks_review with request {{\"scope\": SCOPE, \"revision\": REVISION, \"decision\": \"Keep\", \"note\": \"reread the current workspace\"}} using step 3's revision. If none is pending, do nothing.\n5. falinks_capture with request {{\"paths\": [\"price.go\"]}}.\n6. {} — use the price.go version step 5 returned.{}",
        edit_step("discount-done", "price.go", DISCOUNT_FINAL, "VERSION"),
        unreviewed("discount-done-2")
    );
    // Only calls from this phase count; the process may hold earlier captures.
    let before = gate.accepted(0, "capture").len();
    let (edits_before, reviews_before) = (
        gate.calls(0, "edit").len(),
        gate.accepted(0, "review").len(),
    );
    gate.prompt(0, &steps)?;
    gate.pump(&[0], false, |gate| {
        Ok(gate.accepted(0, "capture")[before..]
            .iter()
            .any(|c| c["host_call"]["request"]["paths"] == json!(["price.go"])))
    })?;
    let since = gate
        .engine()
        .events(gate.client(0), 0)?
        .last()
        .map_or(0, |e| e.seq);
    let summary = gate.engine().capture()?.files["summary.go"].version;
    let steps = format!(
        "1. {}{}",
        edit_step(
            "summary-net",
            "summary.go",
            SUMMARY_NET,
            &summary.to_string()
        ),
        unreviewed("summary-net-2")
    );
    gate.prompt(1, &steps)?;
    gate.pump(&[1], false, |gate| {
        Ok(gate.read_live(1, "summary.go")? == SUMMARY_NET
            && gate.workers[1]
                .runtime
                .as_ref()
                .is_some_and(|r| r.session.idle()))
    })?;
    require(
        gate.runtime(0)?.session.in_turn(),
        "agent 0's turn ended before the peer edit",
    )?;
    gate.drive(|_| Ok(true))?;
    require(
        gate.read_live(0, "price.go")? == DISCOUNT_FINAL,
        "final discount not applied",
    )?;
    let obligation = gate
        .engine()
        .events(gate.client(0), since)?
        .into_iter()
        .find(|e| matches!(&e.body, falinks::Body::Obligation { agent: 0, .. }))
        .ok_or("no obligation reached agent 0")?;
    let delivery = gate
        .runtime(0)?
        .session
        .deliveries
        .iter()
        .find(|d| event_seq(d) >= Some(obligation.seq))
        .cloned()
        .ok_or("obligation notice not sent to agent 0")?;
    let edits = gate.calls(0, "edit").split_off(edits_before);
    let refused = edits
        .iter()
        .position(|c| c["result"]["outcome"]["Unreviewed"].is_object());
    let applied = edits.iter().position(|c| {
        c["host_call"]["request"]["output"]["price.go"]["content"] == DISCOUNT_FINAL
            && c["result"]["accepted"] == true
    });
    require(
        delivery["sent_in_turn"] == true
            && delivery["presented_span"] == delivery["sent_span"]
            && refused.is_some()
            && applied > refused
            && gate.accepted(0, "review").len() > reviews_before,
        "agent 0 steering or review-before-write failed",
    )?;
    gate.control(0, "steering")?;
    gate.evidence["phases"]["steering"]["agent0"] = json!({"delivery":delivery,"edits":edits});

    // Exact two-member checks while drafting continues: agent 0 asks for feedback on the
    // team revision; agent 1 drafts past it before the held run is processed.
    gate.pause.store(true, Ordering::SeqCst);
    let team = gate.engine().capture()?;
    gate.run(&[(
        0,
        format!(
            "1. falinks_capture with request {{}}.\n2. falinks_feedback with request {{\"id\": \"team-early\", \"revision\": {}}}.",
            team.revision
        ),
    )])?;
    let early = gate
        .engine()
        .runs()?
        .into_iter()
        .find(|r| r.binding.revision == team.revision && r.outcome.is_none())
        .ok_or("feedback run not queued")?;
    let summary = gate.engine().capture()?.files["summary.go"].version;
    gate.run(&[(
        1,
        format!(
            "1. {}{}",
            edit_step(
                "summary-more",
                "summary.go",
                SUMMARY_FINAL,
                &summary.to_string()
            ),
            unreviewed("summary-more-2")
        ),
    )])?;
    let drafted = gate.engine().capture()?.revision;
    gate.pause.store(false, Ordering::SeqCst);
    let id = early.id.clone();
    gate.drive(|gate| {
        Ok(gate
            .engine()
            .run(&id)
            .ok()
            .flatten()
            .is_some_and(|r| r.outcome.is_some()))
    })?;
    let checked = gate.engine().run(&id)?.ok_or("feedback run missing")?;
    require(
        checked.outcome == Some(RunOutcome::Passed)
            && checked.binding.members == BTreeSet::from([0, 1])
            && checked.binding.tree == team.tree
            && drafted > team.revision,
        &format!(
            "two-member feedback did not check the exact held revision: {:?}",
            checked.outcome
        ),
    )?;
    gate.phase(
        "team_checks_while_drafting",
        json!({"run":checked,"drafted_revision":drafted}),
    );
    let review = "1. falinks_pending with request {}.\n2. falinks_capture with request {}.\n3. For each pending obligation, falinks_review with request {\"scope\": SCOPE, \"revision\": REVISION, \"decision\": \"Keep\", \"note\": \"reread the current workspace\"} using that capture's revision. If none is pending, do nothing.";
    gate.run(&[(0, review.into()), (1, review.into())])?;
    for agent in 0..2 {
        require(
            gate.engine()
                .pending(gate.client(agent))?
                .obligations
                .is_empty(),
            "an obligation is still pending before offers",
        )?;
    }
    gate.pause.store(true, Ordering::SeqCst);
    let revision = gate.engine().capture()?.revision;
    let offer = |scope: &str| {
        format!(
            "1. falinks_capture with request {{}}.\n2. falinks_offer with request {{\"id\": \"team\", \"task\": \"gate\", \"revision\": {revision}, \"scope\": [\"{scope}\"], \"text\": \"team candidate\"}}."
        )
    };
    gate.run(&[(0, offer("discount")), (1, offer("summary"))])?;
    let runs = gate.engine().runs()?;
    let run = runs
        .iter()
        .find(|r| r.binding.revision == revision && r.outcome.is_none())
        .ok_or("team offers did not queue a run")?
        .id
        .clone();
    // The host stops: workers disconnect, the validator stops and the engine is released.
    for agent in 0..2 {
        gate.stop_worker(agent)?;
    }
    gate.stop_validator();
    let engine = gate.engine.take().ok_or("engine missing")?;
    require(Arc::strong_count(&engine) == 1, "engine still shared")?;
    drop(engine);
    // A host process validates and dies right after the acceptance commit, before its
    // mirror update, notification or response.
    let crash = Command::new(env::current_exe()?)
        .arg("--crash-after-accept")
        .args([&gate.dir, &gate.go, &gate.gopls])
        .status()?;
    let objects = gate.dir.join("state/objects.git");
    let mirror = || -> Result<String> {
        let out = Command::new("/usr/bin/git")
            .arg("--git-dir")
            .arg(&objects)
            .args(["show-ref", "--verify", "--hash", "refs/falinks/published"])
            .output()?;
        Ok(String::from_utf8(out.stdout)?.trim().to_owned())
    };
    let stale_mirror = mirror()?;
    require(crash.code() == Some(73), "crash barrier missed")?;
    let (engine, clients) = Gate::open(&gate.dir, &gate.go, &gate.gopls)?;
    gate.engine = Some(Arc::new(engine));
    let [c0, c1] = clients;
    gate.workers[0].client = c0;
    gate.workers[1].client = c1;
    gate.pause.store(false, Ordering::SeqCst);
    gate.start_validator();
    let published = gate.engine().published()?;
    require(
        published.revision == revision
            && stale_mirror != published.tree
            && mirror()? == published.tree
            && matches!(
                gate.engine().run(&run)?.and_then(|r| r.outcome),
                Some(RunOutcome::Published { .. })
            ),
        "committed acceptance or its mirror was not recovered",
    )?;
    let mut probes = vec![];
    for (agent, scope) in [(0, "discount"), (1, "summary")] {
        gate.start(agent)?;
        let probe = gate.probe(agent)?;
        let steps = format!(
            "The host restarted. 1. Bash: run exactly `{probe}` (do not change it).\n2. falinks_checkpoint with request {{\"id\": \"{agent}/team\"}}.\n3. falinks_offer with exactly the same request as your earlier team offer: {{\"id\": \"team\", \"task\": \"gate\", \"revision\": {revision}, \"scope\": [\"{scope}\"], \"text\": \"team candidate\"}}."
        );
        gate.prompt(agent, &steps)?;
        probes.push(probe);
    }
    gate.drive(|_| Ok(true))?;
    for (agent, probe) in probes.iter().enumerate() {
        gate.sandbox(agent, probe)?;
        let offers = gate.accepted(agent, "offer");
        let seen = gate.accepted(agent, "checkpoint");
        require(
            offers.len() == 1
                && offers[0]["result"]["checkpoint"]["state"]["Accepted"].is_object()
                && seen
                    .iter()
                    .any(|c| c["result"]["checkpoint"]["state"]["Accepted"].is_object()),
            "resumed worker did not observe the recorded acceptance",
        )?;
        gate.control(agent, "resume_replay")?;
    }
    gate.phase(
        "accepted_recovery",
        json!({"run":gate.engine().run(&run)?,"published_revision":published.revision,
            "mirror_after_crash":stale_mirror,"mirror_after_restart":published.tree}),
    );

    // Phase 8: durable deferral and restored gates. Agent 0 changes Label; agent 1 defers
    // its first unhandled event; its worker restarts; the resumed worker gets the deferral
    // and obligation replayed, and its related write stays refused until review.
    let label = gate.engine().capture()?.files["label.go"].version;
    gate.run(&[(
        0,
        format!(
            "1. {}{}",
            edit_step("label-loud", "label.go", LABEL_LOUD, &label.to_string()),
            unreviewed("label-loud-2")
        ),
    )])?;
    require(
        gate.read_live(0, "label.go")? == LABEL_LOUD,
        "label change missing",
    )?;
    let first = gate
        .engine()
        .pending(gate.client(1))?
        .unhandled
        .first()
        .ok_or("agent 1 has no unhandled event")?
        .seq;
    gate.run(&[(
        1,
        format!("1. falinks_handle with request {{\"seq\": {first}, \"deferred\": \"revisit after restart\"}}."),
    )])?;
    let before_restart = gate.engine().pending(gate.client(1))?;
    require(
        before_restart.deferred.iter().any(|(e, _)| e.seq == first)
            && !before_restart.obligations.is_empty(),
        "deferral or obligation not recorded before the restart",
    )?;
    gate.stop_worker(1)?;
    gate.start(1)?;
    let summary = gate.engine().capture()?.files["summary.go"].version;
    let phase_edits = gate.calls(1, "edit").len();
    gate.run(&[(
        1,
        format!(
            "The host restarted your worker; your deferred events and obligations are above.\n1. {} Attempt this edit before any falinks_review.{}",
            edit_step("summary-messy", "summary.go", SUMMARY_MESSY, &summary.to_string()),
            unreviewed("summary-messy-2")
        ),
    )])?;
    let edits = gate.calls(1, "edit").split_off(phase_edits);
    let refused = edits
        .iter()
        .position(|c| c["result"]["outcome"]["Unreviewed"].is_object());
    let applied = edits.iter().position(|c| c["result"]["accepted"] == true);
    require(
        refused.is_some() && applied > refused && gate.read_live(1, "summary.go")? == SUMMARY_MESSY,
        "restored review gate did not hold the resumed worker's write",
    )?;
    // The resumed worker formats its draft with the host-enrolled job.
    let revision = gate.engine().capture()?.revision;
    gate.run(&[(
        1,
        format!(
            "1. falinks_capture with request {{}}.\n2. falinks_job with request {{\"id\": \"fmt\", \"revision\": {revision}, \"paths\": [\"summary.go\"]}}.\n3. falinks_apply_job with request {{\"id\": \"fmt\"}}."
        ),
    )])?;
    let job = gate
        .engine()
        .request(gate.client(1), "job:fmt")?
        .ok_or("job application missing")?;
    require(
        gate.read_live(1, "summary.go")? == SUMMARY_FINAL
            && job.agent == 1
            && matches!(job.outcome, Some(falinks::Outcome::Applied { .. })),
        "captured job did not apply attributed output",
    )?;
    gate.phase(
        "deferral_and_jobs",
        json!({"deferred_seq":first,"edits":edits,"job":job}),
    );

    // Phase 9: completion race. A message sent after `result` starts a new turn and
    // leaves the engine event unhandled.
    let post = |text: &str| {
        format!(
            "1. falinks_post with request {{\"task\": \"gate\", \"scope\": [], \"work\": \"None\", \"text\": \"{text}\", \"reply_to\": null}}."
        )
    };
    // One pair at a time: a post reaches the idle peer only as the race notice; a prompt
    // to the peer would replay it first.
    let mut deliveries = vec![];
    for (poster, agent) in [(1, 0), (0, 1)] {
        gate.run(&[(poster, post(&format!("{} is done", name(poster))))])?;
        let events = gate.undelivered(agent)?;
        let last = events.last().ok_or("no race event")?.clone();
        gate.workers[agent].delivered = last.seq;
        gate.runtime(agent)?.send(
            format!(
                "Host notice {}: engine events {}. Reply with one line acknowledging it and make no tool calls.",
                last.id,
                json_string(&events)
            ),
            Some(&last.id),
        )?;
        gate.drive(|_| Ok(true))?;
        let delivery = gate
            .runtime(agent)?
            .session
            .deliveries
            .iter()
            .find(|d| d["event"] == last.id)
            .cloned()
            .ok_or("race notice missing")?;
        let unhandled = gate.engine().pending(gate.client(agent))?.unhandled;
        require(
            delivery["sent_in_turn"] == false
                && delivery["presented_span"].as_u64() > delivery["sent_span"].as_u64()
                && unhandled.iter().any(|e| e.seq == last.seq),
            "later-turn presentation changed engine state",
        )?;
        gate.control(agent, "completion_race")?;
        deliveries.push(delivery);
    }
    gate.phase("completion_race", json!({"deliveries":deliveries}));

    // Phase 10: unknown-source host injection. The engine halts, evidence stays, and the
    // worker's capability is revoked, which blocks publication and scoring.
    for agent in 0..2 {
        require(
            !gate.runtime(agent)?.session.gate.failed,
            "a control failed before the injection",
        )?;
    }
    let injected = "package gate\n\n// UNEXPECTED_HOST_WRITE\n";
    fs::write(space(&gate.dir, 0).join("label.go"), injected)?;
    let current = gate.engine().capture()?.files["label.go"].version;
    gate.run(&[(
        0,
        format!(
            "1. {}",
            edit_step(
                "after-injection",
                "label.go",
                LABEL_FINAL,
                &current.to_string()
            )
        ),
    )])?;
    let incident = gate.engine().incident()?.ok_or("injection not detected")?;
    // The halt reaches agent 1's next mediated call too.
    gate.run(&[(1, "1. falinks_capture with request {}.".into())])?;
    let published = gate.engine().published()?;
    // Publication stays refused: a host-queued check run cannot publish.
    let feedback = gate
        .engine()
        .feedback(gate.client(1), "after-halt", published.revision);
    let validated = gate.engine().validate();
    require(
        gate.engine().published()? == published
            && !matches!(validated, Ok(Some(ref run)) if matches!(run.outcome, Some(RunOutcome::Published { .. }))),
        "publication proceeded after the incident",
    )?;
    for agent in 0..2 {
        require(
            gate.runtime(agent)?.session.gate.failed
                && gate.runtime(agent)?.require_supported().is_err(),
            "unknown change did not revoke a worker's capability",
        )?;
        gate.workers[agent]
            .controls
            .insert("unknown_change".into(), true);
    }
    require(
        fs::read_to_string(space(&gate.dir, 0).join("label.go"))? == injected,
        "unknown change evidence was overwritten",
    )?;
    gate.evidence["phases"]["after_halt"] = json!({"feedback":format!("{:?}", feedback.map(|r| r.id)),
        "validate":format!("{:?}", validated.map(|r| r.map(|r| r.outcome)))});
    gate.phase(
        "unknown_change",
        json!({"incident":incident.reason,"capability_revoked":true}),
    );
    Ok(())
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("--crash-after-accept") && args.len() == 4 {
        let paths: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
        let crashed = Gate::open(&paths[0], &paths[1], &paths[2]).and_then(|(engine, _)| {
            engine.validate_observed(|stage| {
                if stage == Stage::Accepted {
                    process::exit(73);
                }
                Ok(())
            })
        });
        eprintln!("crash barrier missed: {:?}", crashed.err());
        process::exit(1);
    }
    let arguments = (|| -> Result<(PathBuf, PathBuf, PathBuf, PathBuf)> {
        if args == ["--help"] {
            println!(
                "Usage: check-integration --binary PINNED_CLAUDE --output REPORT.json [--probe SANDBOX_PROBE] [--tool FALINKS_CLAUDE_TOOL]"
            );
            process::exit(0);
        }
        require(args.len() % 2 == 0, "expected option/value pairs")?;
        let (mut binary, mut output, mut probe, mut tool) = (None, None, None, None);
        for pair in args.as_chunks::<2>().0 {
            let value = Some(PathBuf::from(&pair[1]));
            match pair[0].as_str() {
                "--binary" => binary = value,
                "--output" => output = value,
                "--probe" => probe = value,
                "--tool" => tool = value,
                _ => return Err("unknown option".into()),
            }
        }
        let sibling = |name| env::current_exe().map(|exe| exe.with_file_name(name));
        Ok((
            binary.ok_or("--binary required")?,
            output.ok_or("--output required")?,
            probe.map_or_else(|| sibling("sandbox-probe"), Ok)?,
            tool.map_or_else(|| sibling("falinks-claude-tool"), Ok)?,
        ))
    })();
    let (binary, output, probe, tool) = match arguments {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    };
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut report = json!({"implementation":"rust","runtime":"claude-code","started_at_unix_seconds":started,
        "publication_scoring_supported":false});
    let mut gate = match Gate::new(&binary, &probe, &tool) {
        Ok(gate) => Some(gate),
        Err(error) => {
            report["failure"] = json!(error.to_string());
            None
        }
    };
    let mut passed = false;
    if let Some(gate) = gate.as_mut() {
        let result = scenario(gate);
        for agent in 0..2 {
            let _ = gate.stop_worker(agent);
        }
        gate.stop_validator();
        let controls: Vec<Value> = gate.workers.iter().map(|w| json!(w.controls)).collect();
        let complete = gate
            .workers
            .iter()
            .all(|w| REQUIRED.iter().all(|c| w.controls.get(*c) == Some(&true)));
        match result {
            Ok(()) if complete => passed = true,
            Ok(()) => report["failure"] = json!("a required control was not verified"),
            Err(error) => report["failure"] = json!(error.to_string()),
        }
        let mut evidence = gate.evidence.clone();
        evidence["controls"] = json!(controls);
        evidence["validations"] = json!(gate.validations);
        evidence["history"] = gate
            .engine
            .as_ref()
            .and_then(|e| e.history().ok())
            .map_or(Value::Null, |h| json!(h));
        evidence["runs"] = gate
            .engine
            .as_ref()
            .and_then(|e| e.runs().ok())
            .map_or(Value::Null, |r| json!(r));
        evidence["transcripts"] = json!(
            gate.workers
                .iter()
                .map(|w| &w.transcripts)
                .collect::<Vec<_>>()
        );
        // The CLI stores these sacrificial sessions under ~/.claude/projects; remove only those.
        let mut removed = vec![];
        for worker in &gate.workers {
            for transcript in &worker.transcripts {
                if let Some(project) = transcript["project"].as_str()
                    && project.contains("falinks-integration-")
                {
                    removed.push(
                        json!({"project":project,"removed":fs::remove_dir_all(project).is_ok()}),
                    );
                }
            }
        }
        evidence["removed_runtime_projects"] = json!(removed);
        report["evidence"] = evidence;
    }
    report["passed"] = json!(passed);
    report["scope"] = json!(
        "Finite host-scripted gate on one machine and binary. The final deliberate unknown-change injection leaves the disposable engine halted. A saved report never authorizes another run; scoring needs a fresh passing gate."
    );
    let written = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| {
            fs::write(
                &output,
                serde_json::to_string_pretty(&report).unwrap() + "\n",
            )
        });
    if let Err(error) = written {
        eprintln!("{error}");
        process::exit(1);
    }
    println!(
        "{}",
        json!({"passed":passed,"publication_scoring_supported":false,"failure":report["failure"]})
    );
    if !passed {
        process::exit(1);
    }
}
