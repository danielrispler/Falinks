//! Two concurrent worker processes in Git worktrees, mediated by JSON lines on stdin/stdout.
use crate::{
    Result, Sandbox, bail, check, command, fixture, frozen, git, git_env, initial, kill_group,
    protected_roots, read, resolve, root, sha256, strings, utc_now, write,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

const AGENTS: [&str; 2] = ["A", "B"];

fn peer(agent: &str) -> &'static str {
    if agent == "A" { "B" } else { "A" }
}

struct Run {
    started: Instant,
    deadline: Instant,
    events: PathBuf,
    evidence: Value,
    children: BTreeMap<&'static str, (Child, ChildStdin)>,
}

impl Run {
    fn event(&self, kind: &str, values: Value) -> Result<()> {
        let mut row = json!({"elapsed": self.started.elapsed().as_secs_f64(), "kind": kind});
        row.as_object_mut()
            .unwrap()
            .extend(values.as_object().unwrap().clone());
        let mut stream = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events)?;
        writeln!(stream, "{row}")?;
        Ok(())
    }

    fn send(&mut self, agent: &str, payload: Value) -> Result<()> {
        let (_, stdin) = self.children.get_mut(agent).unwrap();
        writeln!(stdin, "{payload}")?;
        stdin.flush()?;
        Ok(())
    }

    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    fn check_timeout(&self) -> Duration {
        self.remaining()
            .clamp(Duration::from_secs(1), Duration::from_secs(120))
    }

    fn record_check(&mut self, private: &Path, number: usize, result: &Value) -> Result<()> {
        write(private.join(format!("check-{number}.json")), result)?;
        self.evidence["validation_attempts"].as_array_mut().unwrap().push(json!({
            "number": number, "candidate": result["candidate"], "passed": result["passed"], "conflict": result["conflict"],
        }));
        Ok(())
    }
}

pub fn baseline(
    name: &str,
    config_path: &Path,
    output: &Path,
    pair: &str,
    order: i64,
    scored: bool,
) -> Result<i32> {
    let started = Instant::now();
    let config = read(config_path)?;
    let fixture = fixture(name)?;
    let manifest = frozen()?;
    let destination = resolve(output);
    fs::create_dir(&destination)?; // Never reset/reuse a run directory or previous contexts.
    let destination = resolve(&destination);
    let private = destination.join("controller");
    fs::DirBuilder::new().mode(0o700).create(&private)?;
    let public = destination.join("agents");
    fs::create_dir(&public)?;
    let evidence = json!({
        "schema_version": 1, "run_id": destination.file_name().unwrap().to_string_lossy(), "arm": "git",
        "fixture": name, "fixture_version": fixture["version"], "freeze": manifest, "configuration": config,
        "order": order, "pair": pair, "scored": scored, "outcome": "infrastructure_failure",
        "usage": {"status": "unavailable", "reason": "worker has not reported usage"},
        "usage_limit_interruptions": [], "rejected_operations": 0, "retries": 0,
        "discarded_or_rewritten_work": {"status": "unavailable"}, "validation_attempts": [],
        "recommendations": 0, "transitions": 0,
        "timing": {"start_utc": utc_now(), "endpoint": "clean preparation through exact candidate passing checks"},
        "tool_differences": read(root().join("config.json"))?["tool_differences"],
    });
    let timeout = config["timeout_seconds"].as_f64().unwrap_or(0.0);
    let mut run = Run {
        started,
        deadline: started + Duration::from_secs_f64(timeout.clamp(0.0, 1800.0)),
        events: private.join("events.jsonl"),
        evidence,
        children: BTreeMap::new(),
    };
    if let Err(error) = execute(&mut run, &config, &fixture, &manifest, &public, &private) {
        run.evidence["failure_reason"] = error.to_string().into();
        let _ = run.event("failure", json!({"reason": error.to_string()}));
    }
    for (child, _) in run.children.values_mut() {
        if child.try_wait()?.is_none() {
            kill_group(child.id(), libc::SIGTERM);
        }
    }
    let grace = Instant::now() + Duration::from_secs(3);
    for (child, _) in run.children.values_mut() {
        while child.try_wait()?.is_none() && Instant::now() < grace {
            thread::sleep(Duration::from_millis(10));
        }
        kill_group(child.id(), libc::SIGKILL);
        child.wait()?;
    }
    let timing = &mut run.evidence["timing"];
    if timing.get("elapsed_seconds").is_none() {
        timing["elapsed_seconds"] = started.elapsed().as_secs_f64().into();
        timing["stop_utc"] = utc_now().into();
    }
    timing["cleanup_elapsed_seconds"] =
        (started.elapsed().as_secs_f64() - timing["elapsed_seconds"].as_f64().unwrap()).into();
    run.evidence["harness_sha256"] = sha256(&fs::read(env::current_exe()?)?).into();
    write(private.join("result.json"), &run.evidence)?;
    let outcome = run.evidence["outcome"].clone();
    println!(
        "{}",
        json!({"outcome": outcome, "evidence": private.join("result.json")})
    );
    Ok(if outcome == "success" { 0 } else { 1 })
}

fn execute(
    run: &mut Run,
    config: &Value,
    fixture: &Value,
    manifest: &Value,
    public: &Path,
    private: &Path,
) -> Result<()> {
    for key in [
        "model",
        "model_version",
        "reasoning",
        "runtime_version",
        "runtime_sha256",
        "worker_command",
        "timeout_seconds",
    ] {
        if matches!(&config[key], Value::Null | Value::Bool(false))
            || config[key] == ""
            || config[key] == 0
        {
            bail!("Missing frozen run configuration: {key}");
        }
    }
    if config.to_string().contains("\"REQUIRED:") {
        bail!("Replace run configuration placeholders before launch");
    }
    let binary = resolve(Path::new(
        config["runtime_binary"]
            .as_str()
            .ok_or("runtime_binary must be a path")?,
    ));
    let actual_hash = sha256(&fs::read(&binary)?);
    if config["runtime_sha256"] != actual_hash.as_str() {
        bail!("Runtime binary hash differs from frozen run configuration");
    }
    run.evidence["runtime_verified_sha256"] = actual_hash.into();
    let timeout = config["timeout_seconds"].as_f64().unwrap_or(0.0);
    let worker_command: Vec<String> = match config["worker_command"].as_array() {
        Some(argv) if timeout > 0.0 && timeout <= 1800.0 => argv
            .iter()
            .map(|w| w.as_str().map(String::from))
            .collect::<Option<_>>()
            .ok_or("Worker command must be argv of strings")?,
        _ => bail!("Worker command must be argv; maximum run duration is 1800 seconds"),
    };
    let name = fixture["name"].as_str().unwrap();
    let milestones = fixture["milestones"].as_array().unwrap().clone();

    let repo = public.join("repo");
    let sha = initial(&repo, fixture)?;
    if manifest["initial_commits"][name] != sha.as_str() {
        bail!("Initial commit differs from frozen fixture");
    }
    run.evidence["initial_snapshot"] = sha.clone().into();
    let at = |p: &Path| p.to_string_lossy().into_owned();
    for agent in AGENTS {
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                agent,
                &at(&public.join(agent)),
                &sha,
            ],
            None,
        )?;
        fs::create_dir(public.join(agent).join("scratch"))?;
    }
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "integration",
            &at(&public.join("integration")),
            &sha,
        ],
        None,
    )?;
    // The controller Git repository and all exact checks are outside worker access.
    let retained = private.join("retained");
    initial(&retained, fixture)?;
    let mut denied = protected_roots()?;
    denied.push(private.to_path_buf());
    for path in config["denied_roots"].as_array().into_iter().flatten() {
        denied.push(resolve(Path::new(
            path.as_str().ok_or("denied_roots must be paths")?,
        )));
    }
    let public_guard = Sandbox {
        writable: vec![public.to_path_buf()],
        denied: denied.clone(),
        network: false,
    };

    let (tx, incoming): (_, Receiver<(&'static str, Option<String>)>) = channel();
    let worker_script = resolve(Path::new(
        config["worker_script"]
            .as_str()
            .ok_or("worker_script must be a path")?,
    ));
    for agent in AGENTS {
        let scratch = public.join(agent).join("scratch");
        let mut env: Vec<(String, String)> = env::vars()
            .filter(|(k, _)| {
                [
                    "PATH",
                    "CARGO_HOME",
                    "RUSTUP_HOME",
                    "DEVELOPER_DIR",
                    "SDKROOT",
                ]
                .contains(&k.as_str())
            })
            .collect();
        env.extend([
            ("HOME".to_string(), at(&scratch)),
            ("TMPDIR".into(), at(&scratch)),
            ("FALINKS_AGENT".into(), agent.into()),
            ("GOCACHE".into(), at(&scratch.join("go-cache"))),
            ("GOTOOLCHAIN".into(), "local".into()),
            ("GOPROXY".into(), "off".into()),
        ]);
        // The checkout is denied to workers, so each gets its own copy of the worker program.
        let worker_copy = scratch.join("worker");
        fs::copy(&worker_script, &worker_copy)?;
        run.evidence["worker_sha256"] = sha256(&fs::read(&worker_script)?).into();
        let argv: Vec<String> = worker_command
            .iter()
            .map(|w| w.replace("{worker}", &at(&worker_copy)))
            .collect();
        if let Some(auth) = config["auth_file"].as_str() {
            let codex_home = scratch.join("codex");
            fs::create_dir(&codex_home)?;
            fs::copy(auth, codex_home.join("auth.json"))?;
            env.push(("CODEX_HOME".into(), at(&codex_home)));
        }
        let stderr = fs::File::create(private.join(format!("{agent}.stderr")))?;
        let mut writable = vec![
            public.join(agent),
            repo.join(".git"),
            public.join("integration"),
        ];
        // Runtime login/session state (e.g. Claude Code's `~/.claude`); the batch archives it per run.
        for path in config["runtime_writable"].as_array().into_iter().flatten() {
            writable.push(PathBuf::from(
                path.as_str().ok_or("runtime_writable must be paths")?,
            ));
        }
        let sandbox = Sandbox {
            writable,
            denied: denied.clone(),
            network: true,
        };
        let mut argv = sandbox.wrap(argv)?;
        if let Some(home) = config["runtime_home"].as_str() {
            // `~/.claude/projects` is denied (other sessions' transcripts); the runtime may still
            // keep this worker's own session, whose cwd is its scratch. Later rules win.
            let own = Path::new(home)
                .join(".claude/projects")
                .join(crate::batch::project_folder(&scratch));
            argv[2].push_str(&format!(
                "\n(allow file-read* file-write* (subpath {}))",
                serde_json::to_string(&own.to_string_lossy())?
            ));
        }
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(public.join(agent))
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .process_group(0)
            .spawn()?;
        let stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take().unwrap();
        run.children.insert(agent, (child, stdin));
        let tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let _ = tx.send((agent, Some(line)));
            }
            let _ = tx.send((agent, None));
        });
        let task: Map<String, Value> = fixture
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| *k != "files" && *k != "milestones")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        run.send(agent, json!({
            "type": "start", "agent": agent, "workspace": public.join(agent),
            "peer_workspace": public.join(peer(agent)), "integration_workspace": public.join("integration"),
            "task": task, "development_count": milestones.len(), "allocation": fixture["allocation"][agent],
            "workflow": fs::read_to_string(root().join("instructions/git.md"))?,
            "runtime_binary": config["runtime_binary"], "runtime_version": config["runtime_version"],
            "runtime_home": config["runtime_home"],
            "model": {"model": config["model"], "model_version": config["model_version"], "reasoning": config["reasoning"]},
        }))?;
        run.event("agent_started", json!({"agent": agent}))?;
    }
    drop(tx);

    let mut shares: BTreeMap<&str, Option<String>> = AGENTS.iter().map(|a| (*a, None)).collect();
    let mut sequence: BTreeMap<&str, usize> = AGENTS.iter().map(|a| (*a, 0)).collect();
    let mut milestone_floor = sequence.clone();
    let mut milestone_index: usize = 0;
    let mut awaiting_ack: BTreeSet<&str> = BTreeSet::new();
    let mut ready: BTreeSet<&str> = BTreeSet::new();
    let mut check_number = 0;
    let mut final_resolution: Option<String> = None;
    let public_git = |work: &Path, args: &[&str]| git(work, args, Some(&public_guard));

    while ready.len() < 2 {
        let remaining = run.remaining();
        if remaining.is_zero() {
            run.evidence["outcome"] = "timeout".into();
            bail!("30-minute run budget expired");
        }
        let (agent, line) = match incoming.recv_timeout(remaining.min(Duration::from_millis(500))) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => continue,
        };
        let Some(line) = line else {
            if !ready.contains(agent) {
                bail!("{agent} exited before final readiness");
            }
            continue;
        };
        let request: Value = serde_json::from_str(&line)?;
        run.event(
            "worker_request",
            json!({"agent": agent, "request": request}),
        )?;
        let action = request["action"].as_str().unwrap_or_default();
        let peer = peer(agent);
        // Agent protocol mistakes are refusals, as in the Falinks arm, never infrastructure failures.
        let refuse = |run: &mut Run, reason: &str| -> Result<()> {
            let count = run.evidence["rejected_operations"].as_u64().unwrap_or(0) + 1;
            run.evidence["rejected_operations"] = count.into();
            run.event(
                "refused",
                json!({"agent": agent, "action": action, "reason": reason}),
            )?;
            run.send(
                agent,
                json!({"type": "refused", "action": action, "reason": reason}),
            )
        };
        match action {
            "message" => {
                run.send(
                    peer,
                    json!({"type": "peer_message", "sender": agent, "text": request["text"]}),
                )?;
                run.send(agent, json!({"type": "ok", "action": action}))?;
            }
            "ack" => {
                let expected = milestone_index
                    .checked_sub(1)
                    .map(|i| milestones[i]["id"].as_str().unwrap());
                if request["milestone"].as_str() != expected || !awaiting_ack.remove(agent) {
                    refuse(run, "no such unacknowledged development")?;
                    continue;
                }
                milestone_floor.insert(agent, sequence[agent]);
                run.send(agent, json!({"type": "ok", "action": action}))?;
            }
            "usage" => {
                let reports = run
                    .evidence
                    .as_object_mut()
                    .unwrap()
                    .entry("worker_usage")
                    .or_insert(json!({}));
                reports
                    .as_object_mut()
                    .unwrap()
                    .entry(agent)
                    .or_insert(json!([]))
                    .as_array_mut()
                    .unwrap()
                    .push(request["usage"].clone());
                let status = if reports.as_object().unwrap().len() == 2 {
                    "available"
                } else {
                    "partial"
                };
                let agents = reports.clone();
                run.evidence["usage"] = json!({"status": status, "agents": agents, "format": "per-turn reports; not cumulative totals"});
                run.send(agent, json!({"type": "ok", "action": action}))?;
            }
            "limit" => {
                run.evidence["usage_limit_interruptions"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"agent": agent, "detail": request["detail"]}));
                run.evidence["outcome"] = "usage_limit".into();
                bail!("Subscription/usage interruption; batch paused, no provider switch");
            }
            "ready" => {
                if milestone_index != milestones.len() || awaiting_ack.contains(agent) {
                    refuse(run, "not every development is delivered and acknowledged")?;
                    continue;
                }
                if shares[agent].is_none() || request["commit"].as_str() != shares[agent].as_deref()
                {
                    refuse(run, "ready must name your last shared commit")?;
                    continue;
                }
                ready.insert(agent);
                run.send(agent, json!({"type": "ok", "action": action}))?;
            }
            "check" => {
                // Either worker can ask for feedback on an unfinished shared combination.
                let result = match combine(
                    public,
                    private,
                    &retained,
                    fixture,
                    &shares,
                    request["resolved"].as_str(),
                    run.check_timeout(),
                    &denied,
                ) {
                    Ok(result) => result,
                    // A bad resolved commit is the agent's mistake.
                    Err(error) if request["resolved"].is_string() => {
                        refuse(run, &format!("invalid resolved commit: {error}"))?;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if result["passed"] == true {
                    final_resolution = result["candidate"].as_str().map(String::from);
                }
                check_number += 1;
                run.record_check(private, check_number, &result)?;
                run.send(
                    agent,
                    json!({"type": "check_result", "candidate": result["candidate"],
                                       "passed": result["passed"], "conflict": result["conflict"]}),
                )?;
                run.event("validation", json!({"agent": agent, "candidate": result["candidate"], "passed": result["passed"]}))?;
            }
            "share" => {
                if ready.contains(agent) {
                    refuse(run, "you declared ready; no further shares")?;
                    continue;
                }
                let work = public.join(agent);
                public_git(&work, &["add", "--all", ".", ":(exclude)scratch"])?;
                final_resolution = None;
                if public_git(&work, &["diff", "--cached", "--binary"])?.is_empty() {
                    refuse(run, "share must contain new unfinished work")?;
                    continue;
                }
                public_git(
                    &work,
                    &[
                        "commit",
                        "-m",
                        &format!("{agent} draft {}", sequence[agent] + 1),
                    ],
                )?;
                let commit = public_git(&work, &["rev-parse", "HEAD"])?;
                shares.insert(agent, Some(commit.clone()));
                *sequence.get_mut(agent).unwrap() += 1;
                let patch = public_git(&work, &["diff", "--binary", &sha, &commit])?;
                run.event(
                    "draft_shared",
                    json!({"agent": agent, "commit": commit, "diff": patch}),
                )?;
                run.send(agent, json!({"type": "shared", "commit": commit}))?;
                run.send(
                    peer,
                    json!({"type": "peer_diff", "sender": agent, "commit": commit, "diff": patch}),
                )?;
                if awaiting_ack.is_empty()
                    && milestone_index < milestones.len()
                    && AGENTS.iter().all(|a| sequence[a] > milestone_floor[a])
                {
                    let milestone = milestones[milestone_index].clone();
                    milestone_index += 1;
                    awaiting_ack = AGENTS.into();
                    ready.clear();
                    run.event("development", json!({"milestone": milestone}))?;
                    let mut development = json!({"type": "development"});
                    development
                        .as_object_mut()
                        .unwrap()
                        .extend(milestone.as_object().unwrap().clone());
                    for member in AGENTS {
                        run.send(member, development.clone())?;
                    }
                }
            }
            _ => refuse(run, "unknown action")?,
        }
    }
    let result = combine(
        public,
        private,
        &retained,
        fixture,
        &shares,
        final_resolution.as_deref(),
        run.check_timeout(),
        &denied,
    )?;
    check_number += 1;
    run.record_check(private, check_number, &result)?;
    let passed = result["passed"] == true;
    run.evidence["final_snapshot"] = result["candidate"].clone();
    run.evidence["oracle"] = result["oracle"].clone();
    run.evidence["outcome"] = if passed { "success" } else { "task_failure" }.into();
    run.event(
        "final_validation",
        json!({"candidate": result["candidate"], "passed": passed}),
    )?;
    run.evidence["timing"]["elapsed_seconds"] = run.started.elapsed().as_secs_f64().into();
    run.evidence["timing"]["stop_utc"] = utc_now().into();
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Mirrors the single host call site; a struct adds nothing.
fn combine(
    public: &Path,
    private: &Path,
    retained: &Path,
    fixture: &Value,
    shares: &BTreeMap<&str, Option<String>>,
    resolved: Option<&str>,
    timeout: Duration,
    denied: &[PathBuf],
) -> Result<Value> {
    let integration = public.join("integration");
    let guard = Sandbox {
        writable: vec![public.to_path_buf()],
        denied: denied.to_vec(),
        network: false,
    };
    let (Some(a), Some(b)) = (&shares["A"], &shares["B"]) else {
        return Ok(
            json!({"candidate": null, "passed": false, "conflict": "Both drafts are needed"}),
        );
    };
    // Agents may resolve conflicts with ordinary Git in the integration worktree.
    let candidate = match resolved.filter(|r| !r.is_empty()) {
        Some(resolved) => {
            if resolved.len() != 40 || !resolved.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
            {
                bail!("Resolved candidate must be an exact 40-character Git commit ID");
            }
            for commit in [a, b] {
                git(
                    &integration,
                    &["merge-base", "--is-ancestor", commit, resolved],
                    Some(&guard),
                )?;
            }
            git(
                &integration,
                &["rev-parse", &format!("{resolved}^{{commit}}")],
                Some(&guard),
            )?
        }
        None => {
            git(&integration, &["reset", "--hard", a], Some(&guard))?;
            git(&integration, &["clean", "-fdx"], Some(&guard))?;
            let argv = guard.wrap(strings(&[
                "git",
                "-c",
                "core.hooksPath=/dev/null",
                "merge",
                "--no-edit",
                b,
            ]))?;
            let merged = command(&argv, &integration, &git_env(), Duration::from_secs(120))?;
            if !merged.success {
                return Ok(json!({"candidate": null, "passed": false, "conflict": merged.text()}));
            }
            git(&integration, &["rev-parse", "HEAD"], Some(&guard))?
        }
    };
    let mut fetch_denied: Vec<PathBuf> = denied.iter().filter(|p| *p != private).cloned().collect();
    let check_denied = fetch_denied.clone();
    fetch_denied.extend(
        fs::read_dir(private)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p != retained),
    );
    let fetch_guard = Sandbox {
        writable: vec![retained.to_path_buf()],
        denied: fetch_denied,
        network: false,
    };
    git(
        retained,
        &["fetch", &public.join("repo").to_string_lossy(), &candidate],
        Some(&fetch_guard),
    )?;
    git(retained, &["checkout", "--detach", &candidate], None)?;
    let checks = check(retained, fixture, private, timeout, &check_denied)?;
    Ok(json!({
        "candidate": candidate,
        "passed": checks["visible"]["passed"] == true && checks["oracle"]["passed"] == true,
        "conflict": null, "visible": checks["visible"], "oracle": checks["oracle"],
    }))
}
