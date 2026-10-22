//! One or two concurrent worker processes in Git worktrees, mediated by JSON lines on stdin/stdout.
use crate::{
    Result, Sandbox, bail, check, command, fixture, frozen, git, git_env, initial, kill_group,
    protected_roots, read, resolve, root, sha256, strings, unix_now, utc_now, write,
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

/// The other agent in a team of two; a team of one has no peer.
fn peer(team: &[&'static str], agent: &str) -> Option<&'static str> {
    team.iter().copied().find(|a| *a != agent)
}

struct Run {
    /// `AGENTS` cut to the team size (#178 C1).
    team: &'static [&'static str],
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

    /// Records one worker turn report. The first after the agent's accepted `ready` is its final
    /// result; usage is complete only when every agent's final result arrived within the drain.
    fn usage(&mut self, agent: &str, mut report: Value, after_ready: bool, arrival: &str) {
        if arrival == "late" {
            report["late"] = true.into();
        }
        let usage = &mut self.evidence["usage"];
        let reports = &mut usage["agents"][agent];
        if reports.is_null() {
            *reports = json!([]);
        }
        reports.as_array_mut().unwrap().push(report);
        if after_ready && usage["final_results"][agent].is_null() {
            usage["final_results"][agent] = arrival.into();
        }
        let complete = self
            .team
            .iter()
            .all(|a| usage["final_results"][a] == "received");
        usage["status"] = if complete { "complete" } else { "partial" }.into();
    }

    /// `SIGTERM` to every worker's process group, 3 s to exit, then `SIGKILL`. Runs once.
    fn stop(&mut self) -> Result<()> {
        let mut children = std::mem::take(&mut self.children);
        for (child, _) in children.values_mut() {
            if child.try_wait()?.is_none() {
                kill_group(child.id(), libc::SIGTERM);
            }
        }
        let grace = Instant::now() + Duration::from_secs(3);
        for (child, _) in children.values_mut() {
            while child.try_wait()?.is_none() && Instant::now() < grace {
                thread::sleep(Duration::from_millis(10));
            }
            kill_group(child.id(), libc::SIGKILL);
            child.wait()?;
        }
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
    team_size: usize,
) -> Result<i32> {
    if !(1..=2).contains(&team_size) {
        bail!("team size must be 1 or 2");
    }
    let (started, start_unix) = (Instant::now(), unix_now());
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
        "order": order, "pair": pair, "scored": scored, "team_size": team_size, "outcome": "infrastructure_failure",
        "usage": {"status": "unavailable", "agents": {}, "final_results": {},
            "format": "per-turn result reports; not cumulative totals or cost"},
        "usage_limit_interruptions": [], "rejected_operations": 0, "retries": 0,
        "discarded_or_rewritten_work": {"status": "unavailable"}, "validation_attempts": [],
        "recommendations": 0, "transitions": 0,
        "timing": {"start_utc": utc_now(), "start_unix": start_unix, "endpoint": "clean preparation through exact candidate passing checks"},
        "tool_differences": read(root().join("config.json"))?["tool_differences"],
    });
    let timeout = config["timeout_seconds"].as_f64().unwrap_or(0.0);
    let mut run = Run {
        team: &AGENTS[..team_size],
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
    run.stop()?;
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
    let drain = match config["usage_drain_seconds"].as_f64() {
        Some(seconds) if (0.0..=60.0).contains(&seconds) => Duration::from_secs_f64(seconds),
        _ => bail!("usage_drain_seconds must be 0 to 60"),
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
    let team = run.team;
    for &agent in team {
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
        reopened: Vec::new(),
        network: false,
    };

    let (tx, incoming): (_, Receiver<(&'static str, Option<String>)>) = channel();
    let worker_script = resolve(Path::new(
        config["worker_script"]
            .as_str()
            .ok_or("worker_script must be a path")?,
    ));
    for &agent in team {
        let scratch = public.join(agent).join("scratch");
        let mut env: Vec<(String, String)> = env::vars()
            .filter(|(k, _)| {
                [
                    // Claude Code finds its Keychain login by `USER`.
                    "USER",
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
        let mut reopened = Vec::new();
        if let Some(home) = config["runtime_home"].as_str() {
            // `~/.claude/projects` is denied (other sessions' transcripts); the runtime may still
            // keep this worker's own session, whose cwd is its scratch. Created first, because
            // Landlock grants only paths that exist.
            let own = Path::new(home)
                .join(".claude/projects")
                .join(crate::batch::project_folder(&scratch));
            fs::create_dir_all(&own)?;
            reopened.push(own);
        }
        let sandbox = Sandbox {
            writable,
            denied: denied.clone(),
            reopened,
            network: true,
        };
        let argv = sandbox.wrap(argv)?;
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
        // A team of one gets no peer workspace, the solo workflow and every allocation, A then B (C2, C3).
        let peer_workspace = peer(team, agent).map(|p| public.join(p));
        let (allocation, workflow) = match peer_workspace {
            Some(_) => (fixture["allocation"][agent].clone(), "instructions/git.md"),
            None => {
                let parts: Vec<&str> = AGENTS
                    .iter()
                    .filter_map(|a| fixture["allocation"][a].as_str())
                    .collect();
                (parts.join("\n").into(), "instructions/git-solo.md")
            }
        };
        let mut start = json!({
            "type": "start", "agent": agent, "workspace": public.join(agent),
            "peer_workspace": peer_workspace, "integration_workspace": public.join("integration"),
            "task": task, "development_count": milestones.len(), "allocation": allocation,
            "workflow": fs::read_to_string(root().join(workflow))?,
            "runtime_binary": config["runtime_binary"], "runtime_version": config["runtime_version"],
            "runtime_home": config["runtime_home"],
            "model": {"model": config["model"], "model_version": config["model_version"], "reasoning": config["reasoning"]},
        });
        if start["peer_workspace"].is_null() {
            start.as_object_mut().unwrap().remove("peer_workspace");
        }
        run.send(agent, start)?;
        run.event("agent_started", json!({"agent": agent}))?;
    }
    drop(tx);

    let mut shares: BTreeMap<&str, Option<String>> = team.iter().map(|a| (*a, None)).collect();
    let mut sequence: BTreeMap<&str, usize> = team.iter().map(|a| (*a, 0)).collect();
    let mut milestone_floor = sequence.clone();
    let mut milestone_index: usize = 0;
    let mut awaiting_ack: BTreeSet<&str> = BTreeSet::new();
    let mut ready: BTreeSet<&str> = BTreeSet::new();
    let mut check_number = 0;
    let mut final_resolution: Option<String> = None;
    let public_git = |work: &Path, args: &[&str]| git(work, args, Some(&public_guard));

    while ready.len() < team.len() {
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
        let peer = peer(team, agent);
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
                let Some(peer) = peer else {
                    refuse(run, "a team of one has no peer")?;
                    continue;
                };
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
                run.usage(
                    agent,
                    request["usage"].clone(),
                    ready.contains(agent),
                    "received",
                );
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
            // The worker saw its agent start a nested agent (#178 C7); it exits next.
            "nested_agent" => {
                run.evidence["nested_agent"] = request["detail"].clone();
                bail!("{agent} launched a nested agent; run invalid");
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
                if let Some(peer) = peer {
                    run.send(
                        peer,
                        json!({"type": "peer_diff", "sender": agent, "commit": commit, "diff": patch}),
                    )?;
                }
                // Every agent in the team has shared since the last development; alone, its own share.
                if awaiting_ack.is_empty()
                    && milestone_index < milestones.len()
                    && team.iter().all(|a| sequence[a] > milestone_floor[a])
                {
                    let milestone = milestones[milestone_index].clone();
                    milestone_index += 1;
                    awaiting_ack = team.iter().copied().collect();
                    ready.clear();
                    run.event("development", json!({"milestone": milestone}))?;
                    let mut development = json!({"type": "development"});
                    development
                        .as_object_mut()
                        .unwrap()
                        .extend(milestone.as_object().unwrap().clone());
                    for member in team {
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
    drain_usage(run, &incoming, drain)
}

/// After the clock stops (#179): refuse every request while waiting up to `window` for each
/// worker's final result, then stop the workers. Without this the host stopped reading as soon
/// as both agents were ready, before their final turns reported. A report flushed while the
/// workers stop is kept as late; a worker stopped before its final result has none.
fn drain_usage(
    run: &mut Run,
    incoming: &Receiver<(&'static str, Option<String>)>,
    window: Duration,
) -> Result<()> {
    let started = Instant::now();
    run.event("drain", json!({"window_seconds": window.as_secs_f64()}))?;
    let finished =
        |run: &Run, agent: &str| !run.evidence["usage"]["final_results"][agent].is_null();
    let mut closed = BTreeSet::new();
    let team = run.team;
    while team
        .iter()
        .any(|a| !finished(run, a) && !closed.contains(a))
    {
        let remaining = (started + window).saturating_duration_since(Instant::now());
        let (agent, line) = match incoming.recv_timeout(remaining) {
            Ok(message) => message,
            Err(_) => break,
        };
        let Some(line) = line else {
            closed.insert(agent);
            continue;
        };
        let request: Value = serde_json::from_str(&line)?;
        run.event(
            "worker_request",
            json!({"agent": agent, "request": request}),
        )?;
        // A worker that already exited cannot take the reply.
        let _ = if request["action"] == "usage" {
            run.usage(agent, request["usage"].clone(), true, "received");
            run.send(agent, json!({"type": "ok", "action": "usage"}))
        } else {
            run.event(
                "drain_refused",
                json!({"agent": agent, "action": request["action"]}),
            )?;
            run.send(
                agent,
                json!({"type": "refused", "action": request["action"],
                "reason": "the run is finished; no further requests"}),
            )
        };
    }
    run.stop()?;
    // The reader threads pass on what the workers wrote before they stopped, then end.
    let flushed = Instant::now() + Duration::from_secs(1);
    while let Ok((agent, line)) =
        incoming.recv_timeout(flushed.saturating_duration_since(Instant::now()))
    {
        let request: Value = match line.map(|l| serde_json::from_str(&l)) {
            Some(Ok(request)) => request,
            _ => continue,
        };
        if request["action"] == "usage" {
            run.usage(agent, request["usage"].clone(), true, "late");
        }
    }
    let usage = &mut run.evidence["usage"];
    for agent in team {
        if usage["final_results"][agent].is_null() {
            usage["final_results"][agent] = "missing".into();
        }
    }
    usage["drain"] = json!({"window_seconds": window.as_secs_f64(),
        "seconds": started.elapsed().as_secs_f64()});
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
        reopened: Vec::new(),
        network: false,
    };
    let Some(drafts) = shares
        .values()
        .map(Option::as_ref)
        .collect::<Option<Vec<_>>>()
    else {
        let needed = if shares.len() == 2 {
            "Both drafts are needed"
        } else {
            "A shared draft is needed"
        };
        return Ok(json!({"candidate": null, "passed": false, "conflict": needed}));
    };
    // Agents may resolve conflicts with ordinary Git in the integration worktree.
    let candidate = match resolved.filter(|r| !r.is_empty()) {
        Some(resolved) => {
            if resolved.len() != 40 || !resolved.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
            {
                bail!("Resolved candidate must be an exact 40-character Git commit ID");
            }
            for commit in &drafts {
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
            git(&integration, &["reset", "--hard", drafts[0]], Some(&guard))?;
            git(&integration, &["clean", "-fdx"], Some(&guard))?;
            for draft in &drafts[1..] {
                let argv = guard.wrap(strings(&[
                    "git",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "merge",
                    "--no-edit",
                    draft,
                ]))?;
                let merged = command(&argv, &integration, &git_env(), Duration::from_secs(120))?;
                if !merged.success {
                    return Ok(
                        json!({"candidate": null, "passed": false, "conflict": merged.text()}),
                    );
                }
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
        reopened: Vec::new(),
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
