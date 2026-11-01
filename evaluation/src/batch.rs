//! Pilot and scored-batch orchestration (#27). Runs slots sequentially from the frozen
//! schedule, resumes from its ledger, and pauses (exit 3) rather than improvising: on
//! subscription exhaustion or near-exhaustion, an infrastructure failure (rerun to add an
//! identified replacement), or an unsafe outcome (repair, renew gates, start a new batch
//! directory). A usage-limited run is also replaced on resume, with its evidence kept.
use crate::{
    FIXTURES, LARGE_SPEC_FIXTURE, Result, bail, collisions, git, read,
    report::{large_spec_report, report, step_6_report, track_t_report, wave_report},
    root, schedule, sha256, timing, tokens, utc_now, verify, write,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// Pause before the next run when any reported usage window reaches this utilization.
const PAUSE_UTILIZATION: f64 = 0.9;

/// Claude Code's `~/.claude/projects/` folder name for a working directory.
pub fn project_folder(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Highest utilization in the last usage report of any worker.
fn utilization(result: &Value) -> f64 {
    let mut highest: f64 = 0.0;
    for reports in result["usage"]["agents"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, r)| r)
    {
        let windows = &reports
            .as_array()
            .and_then(|r| r.last())
            .map_or(Value::Null, |r| r["rate_limit"]["unifiedWindows"].clone());
        for window in windows.as_object().into_iter().flatten().map(|(_, w)| w) {
            highest = highest.max(window["utilization"].as_f64().unwrap_or(0.0));
        }
    }
    highest
}

/// The OS and architecture this host runs; a batch, its shards and its pilot share one.
pub fn platform() -> String {
    format!("{}-{}", env::consts::OS, env::consts::ARCH)
}

/// Refuses a ledger with an invocation recorded on another platform, or on none.
fn one_platform(ledger: &Value) -> Result<()> {
    let here = platform();
    for invocation in ledger["invocations"].as_array().into_iter().flatten() {
        if invocation["platform"] != here.as_str() {
            bail!(
                "Batch recorded on platform {}, this host is {here}; one platform per batch, so start a new batch directory",
                invocation["platform"]
            );
        }
    }
    Ok(())
}

/// Slots in schedule order: the unscored pilot pair, or the nine scored pairs.
pub fn slots(pilot: bool) -> Vec<Value> {
    let schedule = schedule();
    let pairs: Vec<Value> = if pilot {
        vec![
            json!({"pair": "pilot", "fixture": schedule["pilot"]["fixture"], "arms": schedule["pilot"]["arms"]}),
        ]
    } else {
        schedule["pairs"].as_array().unwrap().clone()
    };
    pairs
        .iter()
        .flat_map(|pair| {
            pair["arms"].as_array().unwrap().iter().enumerate().map(move |(order, arm)| {
                json!({"slot": format!("{}-{}", pair["pair"].as_str().unwrap(), arm.as_str().unwrap()),
                    "pair": pair["pair"], "fixture": pair["fixture"], "arm": arm, "order": order, "scored": !pilot})
            })
        })
        .collect()
}

/// Track T's slots (#178 C1): Git arm only, each fixture five times at team sizes 1 and 2
/// (#182: five repetitions per cell upfront), alternating which size runs first. Unscored,
/// and outside the scored schedule.
pub fn track_t_slots() -> Vec<Value> {
    let mut slots = vec![];
    // Track T's five fixtures, whatever `FIXTURES` gains (#205's go-invoice is sealed).
    for (index, name) in FIXTURES[..5].iter().enumerate() {
        for repetition in 1..=5 {
            let sizes = if (index + repetition) % 2 == 1 {
                [1, 2]
            } else {
                [2, 1]
            };
            for (order, size) in sizes.into_iter().enumerate() {
                slots.push(json!({"slot": format!("{name}-{repetition}-git-{size}"),
                    "pair": format!("{name}-{repetition}"), "fixture": name, "arm": "git",
                    "order": order, "scored": false, "team_size": size}));
            }
        }
    }
    slots
}

/// Step 6's slots (#185, #316): `go-indep-4` five times in each of three arms, one agent, two
/// Git agents and two Falinks agents. Each repetition runs every arm once, and the order rotates
/// across repetitions, so each arm runs first, second and third at least once. Unscored.
pub fn step_6_slots() -> Vec<Value> {
    rotations("go-indep-4", 5, false)
}

/// #205's `large-spec` slots (M1): `go-invoice` eight times in each of step 6's three arms,
/// rotating the same way, so each arm runs first two or three times. Scored, with no pilot pair.
pub fn large_spec_slots() -> Vec<Value> {
    rotations(LARGE_SPEC_FIXTURE, 8, true)
}

/// #205's engine waves (#335), unscored, one batch directory each: `wave-1` to `wave-4` (the
/// cap) run two Falinks agents four times and two Git agents once on the dev fixture, then two
/// Falinks agents on one small fixture, rotating by wave; `sweep`, the final regression sweep,
/// runs two Falinks agents once on each small fixture; `confirm` gives a sweep regression's fix
/// its two confirming dev runs. None for any other name.
pub fn wave_slots(name: &str) -> Option<Vec<Value>> {
    let small = &FIXTURES[..5];
    let dev = |i: usize| (format!("go-tasks-{i}-falinks-2"), "go-tasks", "falinks");
    let planned: Vec<(String, &str, &str)> = match name {
        "sweep" => small
            .iter()
            .map(|f| (format!("{f}-falinks-2"), *f, "falinks"))
            .collect(),
        "confirm" => (1..=2).map(dev).collect(),
        _ => {
            let wave: usize = name.strip_prefix("wave-")?.parse().ok()?;
            if !(1..=4).contains(&wave) {
                return None;
            }
            // Four waves reach go-indep-2; go-indep-4 is in the sweep.
            let regression = small[wave - 1];
            let mut planned: Vec<_> = (1..=4).map(dev).collect();
            planned.push(("go-tasks-5-git-2".into(), "go-tasks", "git"));
            planned.push((format!("{regression}-falinks-2"), regression, "falinks"));
            planned
        }
    };
    let slots = planned
        .into_iter()
        .enumerate()
        .map(|(order, (slot, fixture, arm))| {
            json!({"slot": slot, "pair": name, "fixture": fixture, "arm": arm, "order": order,
            "scored": false, "team_size": 2})
        });
    Some(slots.collect())
}

/// `count` rotations of one agent, two Git agents and two Falinks agents on `fixture`, the
/// order shifting by one each rotation.
fn rotations(fixture: &str, count: usize, scored: bool) -> Vec<Value> {
    let mut arms = [("git", 1), ("git", 2), ("falinks", 2)];
    let mut slots = vec![];
    for repetition in 1..=count {
        for (order, (arm, size)) in arms.into_iter().enumerate() {
            slots.push(
                json!({"slot": format!("{fixture}-{repetition}-{arm}-{size}"),
                "pair": format!("{fixture}-{repetition}"), "fixture": fixture, "arm": arm,
                "order": order, "scored": scored, "team_size": size}),
            );
        }
        arms.rotate_left(1);
    }
    slots
}

fn pause(ledger: &mut Value, path: &Path, reason: String) -> Result<i32> {
    ledger["pauses"]
        .as_array_mut()
        .unwrap()
        .push(json!({"utc": utc_now(), "reason": reason}));
    write(path, ledger)?;
    eprintln!("Batch paused: {reason}");
    Ok(3)
}

/// Outcomes whose slot is rerun, as an identified replacement, when the batch resumes.
const REPLACEABLE: [&str; 2] = ["infrastructure_failure", "usage_limit"];

pub(crate) fn replaceable(run: &Value) -> bool {
    REPLACEABLE.contains(&run["outcome"].as_str().unwrap_or("infrastructure_failure"))
}

/// A batch directory's lock naming its run in progress, read by sibling shards.
const ACTIVE: &str = "active.json";

/// Removes the lock on every exit, including errors, so siblings never see a finished run.
struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Runs in progress in sibling batch directories (other shards, the pilot), by their locks.
/// shortcut: a shard killed (not erroring) mid-run leaves a stale lock, which over-reports concurrency (never
/// under-reports); delete the dead shard's `active.json` before resuming others if that matters.
fn active_siblings(runs: &Path) -> Result<Vec<String>> {
    let parent = runs.parent().ok_or("runs directory needs a parent")?;
    let mut active: Vec<String> = fs::read_dir(parent)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p != runs)
        .filter_map(|p| read(p.join(ACTIVE)).ok())
        .filter_map(|lock| lock["run_id"].as_str().map(String::from))
        .collect();
    active.sort();
    Ok(active)
}

/// The first path the gate needs but lacks: its program and, for `check-integration`, the
/// worker tools it copies (`--probe`/`--tool`, else its siblings). The names mirror the
/// defaults in `adapters/claude/check_integration.rs`; this crate cannot import them.
fn gate_missing(argv: &[String]) -> Option<PathBuf> {
    let program = Path::new(&argv[0]);
    let mut needed = vec![program.to_path_buf()];
    if program.file_name() == Some("check-integration".as_ref()) {
        for (flag, sibling) in [
            ("--probe", "sandbox-probe"),
            ("--tool", "falinks-claude-tool"),
        ] {
            let given = argv
                .iter()
                .position(|a| a == flag)
                .and_then(|i| argv.get(i + 1));
            needed.push(given.map_or_else(|| program.with_file_name(sibling), PathBuf::from));
        }
    }
    needed.into_iter().find(|path| !path.is_file())
}

/// Names the first variable that would bypass the subscription login or switch off
/// Claude Code's global prompt-cache scope (#206 decision 10).
fn credential_refusal<S: AsRef<str>>(names: impl IntoIterator<Item = S>) -> Option<String> {
    const EXACT: [&str; 4] = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS",
    ];
    names
        .into_iter()
        .map(|n| n.as_ref().to_string())
        .find(|n| EXACT.contains(&n.as_str()) || n.starts_with("DISABLE_PROMPT_CACHING"))
}

/// Refuses a checkout with other worktrees (#206 decision 5): `list` is `git worktree list`.
fn one_worktree(list: &str) -> Result<()> {
    if list.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        bail!(
            "Run the batch from a checkout with no other worktrees; git worktree list shows:\n{list}"
        );
    }
    Ok(())
}

/// `after_pilot` names the completed pilot batch directory; without it this is the pilot,
/// unless `schedule` names a track-T schedule instead: `track-t` (team sizes) or `step-6`
/// (three arms), #205's scored `large-spec`, or one of #205's engine waves (`wave_slots`).
/// `fixtures` limits the batch to those fixtures' slots, so shards can run in parallel.
pub fn batch(
    config_path: &Path,
    runs: &Path,
    after_pilot: Option<&Path>,
    schedule: Option<&str>,
    fixtures: &[&str],
) -> Result<i32> {
    if let Some(name) = fixtures.iter().find(|f| !FIXTURES.contains(f)) {
        bail!("Unknown fixture {name}; expected some of {FIXTURES:?}");
    }
    if let Some(name) =
        credential_refusal(env::vars_os().map(|(k, _)| k.to_string_lossy().into_owned()))
    {
        bail!(
            "Unset {name}: every run uses the subscription login and Claude Code's global prompt-cache scope"
        );
    }
    // The harness tests point this at a scratch repository; real batches use this checkout.
    let checkout = env::var_os("FALINKS_FAKE_CHECKOUT").map_or_else(root, PathBuf::from);
    let worktrees = git(&checkout, &["worktree", "list"], None)?;
    one_worktree(&worktrees)?;
    let config = read(config_path)?;
    let runtime = &config["runtime_binary"];
    if !config["gate_command"]
        .as_array()
        .is_some_and(|argv| argv.contains(runtime))
    {
        bail!("gate_command must check the same runtime_binary as the runs");
    }
    let pilot = after_pilot.is_none() && schedule.is_none();
    if let Some(dir) = after_pilot {
        // Scoring follows a completed unscored pilot pair.
        let ledger = read(dir.join("batch.json"))?;
        one_platform(&ledger)?;
        let done = slots(true).iter().all(|slot| {
            let mut runs = ledger["runs"].as_array().into_iter().flatten();
            runs.rfind(|r| r["slot"] == slot["slot"])
                .is_some_and(|r| !replaceable(r))
        });
        if !done {
            bail!("Complete the unscored pilot pair before scoring");
        }
    }
    fs::create_dir_all(runs)?;
    let runs = runs.canonicalize()?;
    let ledger_path = runs.join("batch.json");
    let mut ledger = read(&ledger_path)
        .unwrap_or_else(|_| json!({"runs": [], "gates": [], "pauses": [], "invocations": []}));
    one_platform(&ledger)?;
    // An unsafe outcome means a repaired engine: its results belong in a new batch.
    let recorded = |outcome: &str| {
        ledger["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["outcome"] == outcome)
    };
    if recorded("safety_failure") {
        bail!(
            "This batch recorded a safety failure; after repair and renewed gates, start a new batch directory"
        );
    }
    // A nested agent (#178 C7) is never rerun in place: decide what changed, then start anew.
    if recorded("run_invalid") {
        bail!(
            "This batch recorded an invalid run (nested agent); after review, start a new batch directory"
        );
    }
    let config_sha = sha256(&fs::read(config_path)?);
    // Limits change only between recorded batches: one configuration per batch directory.
    if let Some(first) = ledger["invocations"].as_array().unwrap().first()
        && first["config_sha256"] != config_sha.as_str()
    {
        bail!("Run configuration changed inside this batch; record a new batch directory");
    }
    let slots: Vec<Value> = match schedule {
        None => slots(pilot),
        Some("track-t") => track_t_slots(),
        Some("step-6") => step_6_slots(),
        Some("large-spec") => large_spec_slots(),
        Some(other) => wave_slots(other).ok_or(format!("Unknown schedule {other}"))?,
    }
    .into_iter()
    .filter(|s| fixtures.contains(&s["fixture"].as_str().unwrap()))
    .collect();
    if slots.is_empty() {
        bail!("No slots for fixtures {fixtures:?} in this batch");
    }
    let after_pilot = match after_pilot {
        Some(dir) => Value::from(dir.canonicalize()?.to_string_lossy().into_owned()),
        None => Value::Null,
    };
    // The code-quality measurement records both commits and checks it uses this Go (#228 decisions 26, 29a).
    let commit = git(&root(), &["rev-parse", "HEAD"], None).ok();
    // As the checks resolve it: PATH's go, never a downloaded toolchain.
    let go = Command::new("go")
        .arg("version")
        .env("GOTOOLCHAIN", "local")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    ledger["invocations"].as_array_mut().unwrap().push(json!({"utc": utc_now(), "pilot": pilot, "track_t": matches!(schedule, Some("track-t" | "step-6")), "schedule": schedule,
        "after_pilot": after_pilot, "platform": platform(), "fixtures": fixtures, "config_sha256": config_sha, "configuration": config,
        "commit": commit, "go": go, "worktrees": worktrees}));
    let mut gated = false;
    for slot in slots {
        let id = slot["slot"].as_str().unwrap().to_string();
        let previous: Vec<Value> = ledger["runs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["slot"] == id.as_str())
            .cloned()
            .collect();
        if previous.last().is_some_and(|r| !replaceable(r)) {
            continue;
        }
        if !gated {
            // A saved report never authorizes a run: renew the gate in every invocation.
            let output = runs.join(format!(
                "gate-{}.json",
                ledger["gates"].as_array().unwrap().len() + 1
            ));
            let argv: Vec<String> = config["gate_command"]
                .as_array()
                .ok_or("gate_command must be argv")?
                .iter()
                .map(|a| {
                    a.as_str()
                        .unwrap_or_default()
                        .replace("{output}", &output.to_string_lossy())
                })
                .collect();
            let started = match gate_missing(&argv) {
                Some(path) => Err(format!("missing {}", path.display())),
                None => Command::new(&argv[0])
                    .args(&argv[1..])
                    .status()
                    .map_err(|e| format!("{}: {e}", argv[0])),
            };
            let mut gate = json!({"utc": utc_now(), "argv": argv, "report": output,
                "started": started.is_ok(), "passed": started.as_ref().is_ok_and(|s| s.success())});
            let blocked = match &started {
                Err(reason) => {
                    gate["failure"] = reason.as_str().into();
                    Some(format!(
                        "safety gate could not start ({reason}); scoring blocked"
                    ))
                }
                Ok(status) if !status.success() => {
                    Some("safety gate scenario failed; scoring blocked".into())
                }
                Ok(_) => None,
            };
            ledger["gates"].as_array_mut().unwrap().push(gate);
            if let Some(reason) = blocked {
                return pause(&mut ledger, &ledger_path, reason);
            }
            // Verified only once a slot will run, and never remembered across invocations.
            let passed = verify()? == 0;
            ledger["invocations"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap()["verification"] = json!({"utc": utc_now(), "passed": passed});
            if !passed {
                return pause(
                    &mut ledger,
                    &ledger_path,
                    "frozen fixture/oracle verification failed".into(),
                );
            }
            gated = true;
        }
        let run_id = match previous.len() {
            0 => id.clone(),
            n => format!("{id}-r{n}"),
        };
        let run_dir = runs.join(&run_id);
        // Locked before denials are listed, so this run's own lock is denied too; likewise the
        // configurations directory, which a batch's first run would otherwise find undenied.
        let lock = Lock(runs.join(ACTIVE));
        write(&lock.0, &json!({"run_id": run_id, "utc": utc_now()}))?;
        fs::create_dir_all(runs.join("configs"))?;
        let concurrent_at_start = active_siblings(&runs)?;
        let mut run_config = config.clone();
        // Deny every other run, ledger, configuration and gate output in both arms, and sibling
        // batch directories such as the pilot, plus configured roots.
        let mut denied: Vec<Value> = config["denied_roots"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        // Other Claude Code sessions' transcripts; each Git-arm worker re-opens only its own.
        denied.push(projects(&config)?.to_string_lossy().into_owned().into());
        let parent = runs.parent().ok_or("runs directory needs a parent")?;
        let siblings = fs::read_dir(parent)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| *p != runs);
        let others = fs::read_dir(&runs)?.flatten().map(|e| e.path());
        for path in siblings.chain(others).filter(|p| *p != run_dir) {
            denied.push(path.to_string_lossy().into_owned().into());
        }
        run_config["denied_roots"] = denied.into();
        let run_config_path = runs.join("configs").join(format!("{run_id}.json"));
        write(&run_config_path, &run_config)?;
        let arm = slot["arm"].as_str().unwrap();
        let program = match arm {
            "git" => env::current_exe()?,
            _ => PathBuf::from(
                config["falinks_runner"]
                    .as_str()
                    .ok_or("falinks_runner must be a path")?,
            ),
        };
        let mut command = Command::new(program);
        if arm == "git" {
            command.arg("baseline");
        }
        command
            .arg(slot["fixture"].as_str().unwrap())
            .arg("--config")
            .arg(&run_config_path)
            .arg("--output")
            .arg(&run_dir)
            .args(["--pair", slot["pair"].as_str().unwrap()])
            .args(["--order", &slot["order"].to_string()])
            .args(["--scored", &slot["scored"].to_string()]);
        if let Some(size) = slot["team_size"].as_u64() {
            command.args(["--team-size", &size.to_string()]);
        }
        eprintln!("Running {run_id}");
        let status = command.status()?;
        let concurrency = json!({"start": concurrent_at_start, "stop": active_siblings(&runs)?});
        drop(lock);
        let result_path = run_dir.join("controller/result.json");
        let mut result = read(&result_path).unwrap_or_else(|_| {
            json!({"outcome": "infrastructure_failure", "failure_reason": format!("runner exited {status} without evidence")})
        });
        result["concurrency"] = concurrency.clone();
        if let Some(replaced) = previous.last() {
            result["replaces"] = replaced["run_id"].clone();
        }
        if result_path.exists() {
            write(&result_path, &result)?;
        }
        archive(&config, &run_dir)?;
        let outcome = result["outcome"]
            .as_str()
            .unwrap_or("infrastructure_failure")
            .to_string();
        let mut entry = slot.clone();
        entry["run_id"] = run_id.clone().into();
        entry["run_dir"] = run_dir.to_string_lossy().into_owned().into();
        entry["outcome"] = outcome.clone().into();
        entry["replaces"] = result["replaces"].clone();
        entry["concurrency"] = concurrency;
        ledger["runs"].as_array_mut().unwrap().push(entry);
        write(&ledger_path, &ledger)?;
        if outcome == "run_invalid" {
            return pause(
                &mut ledger,
                &ledger_path,
                format!(
                    "{run_id}: {}; run invalid. Stop and ask before resuming",
                    if result["control_failure"].is_null() {
                        format!("nested agent launched ({})", result["nested_agent"])
                    } else {
                        format!(
                            "control failure while a runtime stopped ({})",
                            result["control_failure"]
                        )
                    }
                ),
            );
        }
        match outcome.as_str() {
            "safety_failure" => {
                return pause(
                    &mut ledger,
                    &ledger_path,
                    format!(
                        "{run_id}: unsafe publication or lost work; repair, renew gates, keep evidence"
                    ),
                );
            }
            "usage_limit" => {
                return pause(
                    &mut ledger,
                    &ledger_path,
                    format!("{run_id}: subscription usage exhausted; no provider switch"),
                );
            }
            "infrastructure_failure" => {
                return pause(
                    &mut ledger,
                    &ledger_path,
                    format!(
                        "{run_id}: infrastructure failure ({}); rerun to add an identified replacement",
                        result["failure_reason"].as_str().unwrap_or("unknown")
                    ),
                );
            }
            _ => {}
        }
        if utilization(&result) >= PAUSE_UTILIZATION {
            return pause(
                &mut ledger,
                &ledger_path,
                format!(
                    "{run_id}: usage window at {:.0}%",
                    utilization(&result) * 100.0
                ),
            );
        }
    }
    write(&ledger_path, &ledger)?;
    let results = results(&runs)?;
    let summary = match schedule {
        Some("track-t") => track_t_report(&results),
        Some("large-spec") => large_spec_report(&results),
        Some("step-6") => step_6_report(&results),
        // A wave or the sweep: the slots above refused any other name.
        Some(_) => wave_report(&results),
        None => report(&results, false),
    };
    write(runs.join("report.json"), &summary)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(0)
}

/// Every recorded run's evidence, in ledger order.
pub fn results(runs: &Path) -> Result<Vec<Value>> {
    let ledger = read(runs.join("batch.json"))?;
    let prices = tokens::prices()?;
    let quality = read(runs.join("quality.json")).unwrap_or(Value::Null);
    // Written by `duplicates` (#334): the classified duplicate-work candidates.
    let duplicates = read(runs.join("duplicates.json")).unwrap_or(Value::Null);
    let mut out = vec![];
    for entry in ledger["runs"].as_array().into_iter().flatten() {
        let run_dir = Path::new(entry["run_dir"].as_str().unwrap_or_default());
        let mut result = read(run_dir.join("controller/result.json"))
            .unwrap_or_else(|_| json!({"outcome": "infrastructure_failure"}));
        result["tokens"] = tokens::tokens(run_dir, &result, &prices)?;
        for key in ["run_id", "pair", "fixture", "arm", "scored", "concurrency"] {
            result[key] = entry[key].clone();
        }
        // Track-T slots name their team size, which a run without a result lacks.
        for key in ["replaces", "team_size"] {
            if !entry[key].is_null() {
                result[key] = entry[key].clone();
            }
        }
        result["developments"] = developments(run_dir);
        result["collisions"] = collisions::collisions(run_dir, &result, &duplicates);
        result["discarded_or_rewritten_work"] = collisions::waste(&result["collisions"]);
        // A run without readable transcripts has no model time: missing, never zero.
        result["transcript"] =
            timing::timing(run_dir, &result["collisions"]).unwrap_or(Value::Null);
        result["worker_requests"] = worker_requests(run_dir);
        // Written by `quality` (#230); a run it has not measured has none.
        result["quality"] = quality["runs"][entry["run_id"].as_str().unwrap_or_default()].clone();
        out.push(result);
    }
    Ok(out)
}

/// When the host's developments arrived, in seconds after the host started the run, and how
/// many drafts were shared after the first (#178: development arrival and rework), from the
/// run's events; null without them.
fn developments(run_dir: &Path) -> Value {
    let Some(rows) = events(run_dir) else {
        return Value::Null;
    };
    let at = |kind: &str| -> Vec<f64> {
        rows.iter()
            .filter(|r| r["kind"] == kind)
            .filter_map(|r| r["elapsed"].as_f64())
            .collect()
    };
    let arrivals = at("development");
    let after = arrivals
        .first()
        .map(|first| at("draft_shared").iter().filter(|s| *s > first).count());
    json!({"arrival_seconds": arrivals, "drafts_shared_after_first": after})
}

/// The rows of a run's `controller/events.jsonl`, or None without it.
fn events(run_dir: &Path) -> Option<Vec<Value>> {
    let text = fs::read_to_string(run_dir.join("controller/events.jsonl")).ok()?;
    Some(
        text.lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
    )
}

/// The requests the Git arm's host received, by action (#205: protocol acts), from the run's
/// events; null without them. A Falinks run's events have none.
fn worker_requests(run_dir: &Path) -> Value {
    let Some(rows) = events(run_dir) else {
        return Value::Null;
    };
    let mut actions = serde_json::Map::new();
    for row in rows.iter().filter(|r| r["kind"] == "worker_request") {
        let action = row["request"]["action"].as_str().unwrap_or_default();
        let count = actions.entry(action).or_insert(json!(0));
        *count = json!(count.as_u64().unwrap_or(0) + 1);
    }
    actions.into()
}

/// Every run of one batch split across shard directories. The shards must share one
/// configuration, one pilot and one platform, and no run may appear in two of them.
pub fn merged(dirs: &[&Path]) -> Result<Vec<Value>> {
    let mut identity = None;
    let mut out: Vec<Value> = vec![];
    for dir in dirs {
        let ledger = read(dir.join("batch.json"))?;
        for invocation in ledger["invocations"].as_array().into_iter().flatten() {
            let this = json!([
                invocation["config_sha256"],
                invocation["after_pilot"],
                invocation["platform"]
            ]);
            if *identity.get_or_insert_with(|| this.clone()) != this {
                bail!("Shards must share one configuration, one pilot and one platform");
            }
        }
        for result in results(dir)? {
            if out.iter().any(|r| r["run_id"] == result["run_id"]) {
                bail!("Run {} appears in more than one shard", result["run_id"]);
            }
            out.push(result);
        }
    }
    Ok(out)
}

/// The runtime's `~/.claude/projects`, where Claude Code keeps every session transcript.
fn projects(config: &Value) -> Result<PathBuf> {
    let home = config["runtime_home"]
        .as_str()
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
        .ok_or("no runtime home")?;
    Ok(home.join(".claude/projects"))
}

/// Move this run's Claude Code session folders into its private evidence, so no later run
/// can read them through `~/.claude/projects`.
fn archive(config: &Value, run_dir: &Path) -> Result<()> {
    let projects = projects(config)?;
    let prefix = project_folder(run_dir);
    let target = run_dir.join("controller/claude-projects");
    for entry in fs::read_dir(&projects).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            fs::create_dir_all(&target)?;
            fs::rename(entry.path(), target.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_guard_names_each_refused_variable() {
        for name in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS",
            "DISABLE_PROMPT_CACHING",
            "DISABLE_PROMPT_CACHING_SONNET",
        ] {
            assert_eq!(credential_refusal(["HOME", name]).as_deref(), Some(name));
        }
        assert_eq!(
            credential_refusal(["HOME", "PATH", "ANTHROPIC_MODEL"]),
            None
        );
        assert_eq!(credential_refusal(Vec::<String>::new()), None);
    }

    #[test]
    fn batch_refuses_other_worktrees() {
        assert!(one_worktree("/w/Falinks  72e8eaa [main]\n").is_ok());
        let two = "/w/Falinks  72e8eaa [main]\n/w/Falinks/.claude/worktrees/x  1234567 [x]\n";
        assert!(
            one_worktree(two)
                .unwrap_err()
                .to_string()
                .contains("worktrees/x")
        );
    }

    #[test]
    fn project_folders_follow_claude_code_naming() {
        assert_eq!(
            project_folder(Path::new(
                "/Users/x/work/Falinks/.claude/worktrees/issue-21"
            )),
            "-Users-x-work-Falinks--claude-worktrees-issue-21"
        );
    }

    #[test]
    fn pilot_is_one_unscored_pair_and_the_batch_has_eighteen_alternating_slots() {
        let pilot = slots(true);
        assert_eq!(pilot.len(), 2);
        assert!(
            pilot
                .iter()
                .all(|s| s["scored"] == false && s["fixture"] == "go-relationships")
        );
        let scored = slots(false);
        assert_eq!(scored.len(), 18);
        assert_eq!(scored[0]["slot"], "rust-errors-1-git");
        assert_eq!(scored[2]["slot"], "rust-errors-2-falinks");
        assert_eq!(scored[1]["order"], 1);
    }

    /// #205's scored fixture is sealed until its pilots, so no track-T slot may reach it.
    #[test]
    fn track_t_keeps_its_fifty_slots_without_go_invoice() {
        let slots = track_t_slots();
        assert_eq!(slots.len(), 50);
        assert!(slots.iter().all(|s| s["fixture"] != "go-invoice"));
    }

    #[test]
    fn step_6_runs_each_arm_in_every_position_over_five_rotations() {
        let slots = step_6_slots();
        assert_eq!(slots.len(), 15);
        let arm = |s: &Value| format!("{}-{}", s["arm"].as_str().unwrap(), s["team_size"]);
        for (index, rotation) in slots.chunks(3).enumerate() {
            let pair = format!("go-indep-4-{}", index + 1);
            let mut arms: Vec<String> = rotation.iter().map(arm).collect();
            for (order, slot) in rotation.iter().enumerate() {
                assert_eq!(slot["slot"], format!("{pair}-{}", arms[order]));
                assert_eq!(
                    (&slot["pair"], &slot["order"]),
                    (&json!(pair), &json!(order))
                );
                assert_eq!(
                    (&slot["fixture"], &slot["scored"]),
                    (&json!("go-indep-4"), &json!(false))
                );
            }
            arms.sort();
            assert_eq!(arms, ["falinks-2", "git-1", "git-2"]);
        }
        let first: Vec<String> = slots.iter().step_by(3).map(arm).collect();
        assert_eq!(first, ["git-1", "git-2", "falinks-2", "git-1", "git-2"]);
        for name in ["git-1", "git-2", "falinks-2"] {
            let mut orders: Vec<u64> = slots
                .iter()
                .filter(|s| arm(s) == name)
                .map(|s| s["order"].as_u64().unwrap())
                .collect();
            orders.sort();
            orders.dedup();
            assert_eq!(orders, [0, 1, 2], "{name}");
        }
    }

    /// #205's engine waves (#335): four Falinks runs and one Git run on go-tasks, then one
    /// Falinks regression run whose fixture rotates through the five small fixtures.
    #[test]
    fn each_wave_runs_go_tasks_five_times_and_rotates_its_regression_run() {
        let slots = wave_slots("wave-2").unwrap();
        let names: Vec<&str> = slots.iter().map(|s| s["slot"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "go-tasks-1-falinks-2",
                "go-tasks-2-falinks-2",
                "go-tasks-3-falinks-2",
                "go-tasks-4-falinks-2",
                "go-tasks-5-git-2",
                "go-page-falinks-2",
            ]
        );
        for (order, slot) in slots.iter().enumerate() {
            assert_eq!(
                (
                    &slot["pair"],
                    &slot["order"],
                    &slot["scored"],
                    &slot["team_size"]
                ),
                (&json!("wave-2"), &json!(order), &json!(false), &json!(2))
            );
        }
        let regression = |n: u32| wave_slots(&format!("wave-{n}")).unwrap()[5]["fixture"].clone();
        assert_eq!(
            [1, 3, 4].map(regression),
            ["rust-errors", "go-relationships", "go-indep-2"]
        );
        // The cap is four waves; the sweep follows the last engine change.
        assert!(wave_slots("wave-5").is_none() && wave_slots("wave-0").is_none());
        let runs = |name: &str| -> Vec<(String, String)> {
            let slots = wave_slots(name).unwrap();
            let field = |s: &Value, key: &str| s[key].as_str().unwrap().to_string();
            slots
                .iter()
                .map(|s| (field(s, "fixture"), field(s, "arm")))
                .collect()
        };
        let falinks = |fixtures: &[&str]| -> Vec<(String, String)> {
            fixtures
                .iter()
                .map(|f| (f.to_string(), "falinks".into()))
                .collect()
        };
        let small = [
            "rust-errors",
            "go-page",
            "go-relationships",
            "go-indep-2",
            "go-indep-4",
        ];
        assert_eq!(runs("sweep"), falinks(&small));
        // A regression the sweep finds gets a fix and two confirming dev runs.
        assert_eq!(runs("confirm"), falinks(&["go-tasks", "go-tasks"]));
    }

    /// #205: eight scored rotations on go-invoice, each arm first two or three times.
    #[test]
    fn large_spec_runs_eight_scored_rotations_on_go_invoice() {
        let slots = large_spec_slots();
        assert_eq!(slots.len(), 24);
        let arm = |s: &Value| format!("{}-{}", s["arm"].as_str().unwrap(), s["team_size"]);
        for (index, rotation) in slots.chunks(3).enumerate() {
            let pair = format!("go-invoice-{}", index + 1);
            let mut arms: Vec<String> = rotation.iter().map(arm).collect();
            for (order, slot) in rotation.iter().enumerate() {
                assert_eq!(slot["slot"], format!("{pair}-{}", arms[order]));
                assert_eq!(
                    (&slot["pair"], &slot["order"], &slot["scored"]),
                    (&json!(pair), &json!(order), &json!(true))
                );
                assert_eq!(slot["fixture"], "go-invoice");
            }
            arms.sort();
            assert_eq!(arms, ["falinks-2", "git-1", "git-2"]);
        }
        let first: Vec<String> = slots.iter().step_by(3).map(arm).collect();
        assert_eq!(
            first,
            [
                "git-1",
                "git-2",
                "falinks-2",
                "git-1",
                "git-2",
                "falinks-2",
                "git-1",
                "git-2"
            ]
        );
        // go-invoice is registered (#332), so the default fixture filter keeps every slot.
        assert!(
            slots
                .iter()
                .all(|s| FIXTURES.contains(&s["fixture"].as_str().unwrap()))
        );
    }

    /// A Git run's protocol acts are its host's requests, counted by action from its events.
    #[test]
    fn worker_requests_count_host_requests_by_action() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(worker_requests(tmp.path()), Value::Null);
        fs::create_dir_all(tmp.path().join("controller")).unwrap();
        let rows = [
            json!({"kind": "worker_request", "request": {"action": "share"}}),
            json!({"kind": "worker_request", "request": {"action": "share"}}),
            json!({"kind": "worker_request", "request": {"action": "usage"}}),
            json!({"kind": "development", "elapsed": 3.0}),
        ];
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        fs::write(tmp.path().join("controller/events.jsonl"), text).unwrap();
        assert_eq!(worker_requests(tmp.path()), json!({"share": 2, "usage": 1}));
    }

    /// Writes a batch directory of successful `slots`: Git 1000 s; Falinks 900 s on go-page, else 700 s.
    fn recorded(dir: &Path, slots: &[Value], pilot: &str) {
        fs::create_dir_all(dir).unwrap();
        let mut entries = vec![];
        for slot in slots {
            let mut entry = slot.clone();
            let id = slot["slot"].as_str().unwrap();
            let run_dir = dir.join(id);
            fs::create_dir_all(run_dir.join("controller")).unwrap();
            let secs = match (slot["arm"].as_str(), slot["fixture"].as_str()) {
                (Some("git"), _) => 1000.0,
                (_, Some("go-page")) => 900.0,
                _ => 700.0,
            };
            write(
                run_dir.join("controller/result.json"),
                &json!({"outcome": "success", "timing": {"elapsed_seconds": secs}, "transitions": 0}),
            )
            .unwrap();
            entry["run_id"] = id.into();
            entry["run_dir"] = run_dir.to_string_lossy().into_owned().into();
            entries.push(entry);
        }
        let invocation =
            json!({"config_sha256": "c0", "after_pilot": pilot, "platform": platform()});
        write(
            dir.join("batch.json"),
            &json!({"runs": entries, "invocations": [invocation]}),
        )
        .unwrap();
    }

    #[test]
    fn report_over_fixture_shards_equals_report_over_one_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let all = slots(false);
        recorded(&tmp.path().join("one"), &all, "/p");
        let one = report(&results(&tmp.path().join("one")).unwrap(), false);
        let shards: Vec<PathBuf> = FIXTURES.map(|f| tmp.path().join(f)).to_vec();
        for (fixture, dir) in FIXTURES.iter().zip(&shards) {
            let only: Vec<Value> = all
                .iter()
                .filter(|s| s["fixture"] == *fixture)
                .cloned()
                .collect();
            recorded(dir, &only, "/p");
        }
        let dirs: Vec<&Path> = shards.iter().map(PathBuf::as_path).collect();
        let split = report(&merged(&dirs).unwrap(), false);
        assert_eq!(one["gaps"], json!([]));
        for key in ["fixtures", "successes", "gaps"] {
            assert_eq!(split[key], one[key], "{key}");
        }
        assert_eq!(
            split["fixtures"]["go-page"]["median_seconds"],
            json!({"git": 1000.0, "falinks": 900.0})
        );

        // Shards of another pilot, or the same shard twice, are refused.
        let other = tmp.path().join("other");
        recorded(&other, &[], "/q");
        assert!(merged(&[dirs[0], &other]).is_err());
        assert!(merged(&[dirs[0], dirs[0]]).is_err());
    }

    #[test]
    fn a_batch_its_shards_and_its_pilot_share_one_platform() {
        let tmp = tempfile::tempdir().unwrap();
        let (here, there) = (tmp.path().join("here"), tmp.path().join("there"));
        recorded(&here, &[], "/p");
        recorded(&there, &[], "/p");
        let mut ledger = read(there.join("batch.json")).unwrap();
        ledger["invocations"][0]["platform"] = "plan9-mips".into();
        write(there.join("batch.json"), &ledger).unwrap();

        let refused = merged(&[&here, &there]).unwrap_err().to_string();
        assert!(refused.contains("platform"), "{refused}");
        assert!(one_platform(&read(here.join("batch.json")).unwrap()).is_ok());
        let refused = one_platform(&ledger).unwrap_err().to_string();
        assert!(refused.contains("plan9-mips"), "{refused}");
        // A ledger from before platforms were recorded cannot be resumed or extended.
        ledger["invocations"][0]["platform"] = Value::Null;
        assert!(one_platform(&ledger).is_err());
    }

    #[test]
    fn utilization_reads_the_last_report_of_each_worker() {
        let result = json!({"usage": {"agents": {
            "A": [{"rate_limit": {"unifiedWindows": {"five_hour": {"utilization": 0.95}}}},
                  {"rate_limit": {"unifiedWindows": {"five_hour": {"utilization": 0.5}}}}],
            "B": [{"rate_limit": {"unifiedWindows": {"seven_day": {"utilization": 0.7}}}}]}}});
        assert_eq!(utilization(&result), 0.7);
    }
}
