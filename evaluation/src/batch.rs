//! Pilot and scored-batch orchestration (#27). Runs slots sequentially from the frozen
//! schedule, resumes from its ledger, and pauses (exit 3) rather than improvising: on
//! subscription exhaustion or near-exhaustion, an infrastructure failure (rerun to add an
//! identified replacement), or an unsafe outcome (repair, renew gates, start a new batch
//! directory). A usage-limited run is also replaced on resume, with its evidence kept.
use crate::{Result, bail, read, report::report, schedule, sha256, utc_now, verify, write};
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

fn replaceable(run: &Value) -> bool {
    REPLACEABLE.contains(&run["outcome"].as_str().unwrap_or("infrastructure_failure"))
}

/// `after_pilot` names the completed pilot batch directory; without it this is the pilot.
pub fn batch(config_path: &Path, runs: &Path, after_pilot: Option<&Path>) -> Result<i32> {
    let config = read(config_path)?;
    let runtime = &config["runtime_binary"];
    if !config["gate_command"]
        .as_array()
        .is_some_and(|argv| argv.contains(runtime))
    {
        bail!("gate_command must check the same runtime_binary as the runs");
    }
    let pilot = after_pilot.is_none();
    if let Some(dir) = after_pilot {
        // Scoring follows a completed unscored pilot pair.
        let ledger = read(dir.join("batch.json"))?;
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
    // An unsafe outcome means a repaired engine: its results belong in a new batch.
    if ledger["runs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["outcome"] == "safety_failure")
    {
        bail!(
            "This batch recorded a safety failure; after repair and renewed gates, start a new batch directory"
        );
    }
    let config_sha = sha256(&fs::read(config_path)?);
    // Limits change only between recorded batches: one configuration per batch directory.
    if let Some(first) = ledger["invocations"].as_array().unwrap().first()
        && first["config_sha256"] != config_sha.as_str()
    {
        bail!("Run configuration changed inside this batch; record a new batch directory");
    }
    ledger["invocations"].as_array_mut().unwrap().push(
        json!({"utc": utc_now(), "pilot": pilot, "config_sha256": config_sha, "configuration": config}),
    );
    if verify()? != 0 {
        return pause(
            &mut ledger,
            &ledger_path,
            "frozen fixture/oracle verification failed".into(),
        );
    }
    let mut gated = false;
    for slot in slots(pilot) {
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
            let status = Command::new(&argv[0]).args(&argv[1..]).status()?;
            ledger["gates"].as_array_mut().unwrap().push(
                json!({"utc": utc_now(), "argv": argv, "report": output, "passed": status.success()}),
            );
            if !status.success() {
                return pause(
                    &mut ledger,
                    &ledger_path,
                    "safety gate failed; scoring blocked".into(),
                );
            }
            gated = true;
        }
        let run_id = match previous.len() {
            0 => id.clone(),
            n => format!("{id}-r{n}"),
        };
        let run_dir = runs.join(&run_id);
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
        fs::create_dir_all(runs.join("configs"))?;
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
        eprintln!("Running {run_id}");
        let status = command.status()?;
        let result_path = run_dir.join("controller/result.json");
        let mut result = read(&result_path).unwrap_or_else(|_| {
            json!({"outcome": "infrastructure_failure", "failure_reason": format!("runner exited {status} without evidence")})
        });
        if let Some(replaced) = previous.last() {
            result["replaces"] = replaced["run_id"].clone();
            if result_path.exists() {
                write(&result_path, &result)?;
            }
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
        ledger["runs"].as_array_mut().unwrap().push(entry);
        write(&ledger_path, &ledger)?;
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
    let summary = report(&results(&runs)?);
    write(runs.join("report.json"), &summary)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(0)
}

/// Every recorded run's evidence, in ledger order.
pub fn results(runs: &Path) -> Result<Vec<Value>> {
    let ledger = read(runs.join("batch.json"))?;
    let mut out = vec![];
    for entry in ledger["runs"].as_array().into_iter().flatten() {
        let mut result = read(
            Path::new(entry["run_dir"].as_str().unwrap_or_default()).join("controller/result.json"),
        )
        .unwrap_or_else(|_| json!({"outcome": "infrastructure_failure"}));
        for key in ["run_id", "pair", "fixture", "arm", "scored"] {
            result[key] = entry[key].clone();
        }
        if !entry["replaces"].is_null() {
            result["replaces"] = entry["replaces"].clone();
        }
        out.push(result);
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

    #[test]
    fn utilization_reads_the_last_report_of_each_worker() {
        let result = json!({"usage": {"agents": {
            "A": [{"rate_limit": {"unifiedWindows": {"five_hour": {"utilization": 0.95}}}},
                  {"rate_limit": {"unifiedWindows": {"five_hour": {"utilization": 0.5}}}}],
            "B": [{"rate_limit": {"unifiedWindows": {"seven_day": {"utilization": 0.7}}}}]}}});
        assert_eq!(utilization(&result), 0.7);
    }
}
