//! Public CLI checks; no private helper assertions or model calls.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const CLI: &str = env!("CARGO_BIN_EXE_falinks-eval");
const SCRIPTED: &str = env!("CARGO_BIN_EXE_scripted-worker");
const CODEX_WORKER: &str = env!("CARGO_BIN_EXE_codex-worker");
const CLAUDE_WORKER: &str = env!("CARGO_BIN_EXE_claude-worker");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cli<S: AsRef<OsStr>>(args: impl IntoIterator<Item = S>) -> Output {
    Command::new(CLI).args(args).output().unwrap()
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn reference(fixture: &str) -> Value {
    read(root().join(format!("protected/{fixture}.json")))["reference"].clone()
}

fn tmp(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new().prefix(prefix).tempdir().unwrap()
}

fn commit(repo: &Path, message: &str) {
    for args in [&["add", "."][..], &["commit", "-m", message]] {
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(args)
            .current_dir(repo)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }
}

fn prepare(fixture: &str, repo: &Path) {
    let prepared = cli([
        OsStr::new("prepare"),
        OsStr::new(fixture),
        OsStr::new("--output"),
        repo.as_os_str(),
    ]);
    assert!(prepared.status.success(), "{}", text(&prepared));
}

fn hash(path: &str) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

/// Writes a scripted run configuration based on the checked-in template.
fn config(base: &Path, worker_script: &str, worker_command: Value, runtime: &str) -> PathBuf {
    let mut config = read(root().join("config.json"));
    for (key, value) in [
        ("worker_command", worker_command),
        ("worker_script", json!(worker_script)),
        ("model", json!("scripted")),
        ("model_version", json!("test-v1")),
        ("reasoning", json!("none")),
        ("runtime_version", json!("scripted-v1")),
        ("runtime_sha256", json!(hash(runtime))),
        ("runtime_binary", json!(runtime)),
        ("auth_file", Value::Null),
        ("runtime_home", Value::Null),
        ("runtime_writable", json!([])),
        ("evaluation_root", json!(root())),
        ("eval_binary", json!(CLI)),
        ("falinks_runner", json!(SCRIPTED)),
        // The gate must name the same runtime binary as the runs.
        ("gate_command", json!(["/bin/cp", runtime, "{output}"])),
        ("denied_roots", json!([])),
        // Generous: batch tests run alongside and slow every check.
        ("timeout_seconds", json!(300)),
        ("usage_drain_seconds", json!(1)),
    ] {
        config[key] = value;
    }
    let path = base.join("config.json");
    fs::write(&path, config.to_string()).unwrap();
    path
}

fn baseline(fixture: &str, config: &Path, run: &Path) -> Output {
    cli([
        OsStr::new("baseline"),
        OsStr::new(fixture),
        OsStr::new("--config"),
        config.as_os_str(),
        OsStr::new("--output"),
        run.as_os_str(),
    ])
}

fn successful_evidence(result: &Output, run: &Path) -> Value {
    let evidence_path = run.join("controller/result.json");
    let stderr = |agent: &str| {
        fs::read_to_string(run.join(format!("controller/{agent}.stderr"))).unwrap_or_default()
    };
    assert!(
        result.status.success(),
        "{}{}\nA: {}\nB: {}",
        text(result),
        fs::read_to_string(&evidence_path).unwrap_or_default(),
        stderr("A"),
        stderr("B")
    );
    let evidence = read(evidence_path);
    assert_schema_required_keys(&read(root().join("result-schema.json")), &evidence, "");
    evidence
}

/// Recorded evidence carries every key `result-schema.json` requires, at every depth.
fn assert_schema_required_keys(schema: &Value, value: &Value, path: &str) {
    for key in schema["required"].as_array().into_iter().flatten() {
        let key = key.as_str().unwrap();
        assert!(value.get(key).is_some(), "evidence lacks {path}/{key}");
    }
    for (key, property) in schema["properties"].as_object().into_iter().flatten() {
        if let Some(field) = value.get(key) {
            assert_schema_required_keys(property, field, &format!("{path}/{key}"));
        }
    }
    for item in value.as_array().into_iter().flatten() {
        assert_schema_required_keys(&schema["items"], item, &format!("{path}[]"));
    }
}

#[test]
fn frozen_oracles_reject_initial_and_accept_reference() {
    let result = cli(["verify"]);
    assert!(result.status.success(), "{}", text(&result));
    let evidence: Value = serde_json::from_slice(&result.stdout).unwrap();
    let fixtures = evidence["fixtures"].as_object().unwrap();
    assert_eq!(
        fixtures.keys().collect::<Vec<_>>(),
        [
            "go-indep-2",
            "go-indep-4",
            "go-page",
            "go-relationships",
            "rust-errors"
        ]
    );
    for fixture in fixtures.values() {
        assert_eq!(fixture["initial"]["passed"], false);
        assert_eq!(fixture["reference"]["passed"], true);
        assert_eq!(fixture["initial_visible"]["passed"], true);
        assert_eq!(fixture["reference_visible"]["passed"], true);
    }
}

/// README.md points at `setup.json` for the budget figures, so its totals must add up.
#[test]
fn setup_ledger_totals_match_its_sessions() {
    let ledger = read(root().join("setup.json"));
    let charged: i64 = ledger["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["charged_seconds"].as_i64().unwrap())
        .sum();
    assert_eq!(ledger["charged_total_seconds"], charged);
    let limit = ledger["limit_seconds"].as_i64().unwrap();
    assert_eq!(ledger["remaining_seconds"], limit - charged);
    assert!(limit >= charged, "setup ledger overdrawn: stop and ask");

    // Track T's own budget (#178): each session's charge is its run (with the usage drain) and
    // preparation time; implementation is recorded but not charged (Daniel, 2026-10-22, #180).
    // Usage comes from complete runs only.
    let track = &ledger["team_size_research"];
    let sessions = track["sessions"].as_array().unwrap();
    let parts = [
        "implementation_seconds",
        "run_seconds",
        "preparation_seconds",
    ];
    for session in sessions {
        let charged: i64 = parts[1..]
            .iter()
            .map(|p| session[p].as_i64().unwrap())
            .sum();
        assert_eq!(session["charged_seconds"], charged, "{session}");
    }
    let totals = &track["totals"];
    let sum = |value: &dyn Fn(&Value) -> &Value| -> i64 {
        sessions.iter().map(|s| value(s).as_i64().unwrap()).sum()
    };
    for part in parts.iter().chain(&["charged_seconds"]) {
        assert_eq!(totals[part], sum(&|s| &s[part]), "totals.{part}");
    }
    for key in totals["complete_usage"].as_object().unwrap().keys() {
        let key = key.as_str();
        let summed = sum(&|s| &s["complete_usage"][key]);
        assert_eq!(
            totals["complete_usage"][key], summed,
            "totals.complete_usage.{key}"
        );
    }
    let charged = totals["charged_seconds"].as_i64().unwrap();
    let limit = track["limit_seconds"].as_i64().unwrap();
    assert_eq!(track["remaining_seconds"], limit - charged);
    assert!(limit >= charged, "track T budget overdrawn: stop and ask");
}

/// The result schema's fixture enum names every fixture file, so registering one cannot skip it.
#[test]
fn result_schema_names_every_fixture() {
    let schema = read(root().join("result-schema.json"));
    let mut named: Vec<&str> = schema["properties"]["fixture"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    named.sort();
    let mut files: Vec<String> = fs::read_dir(root().join("fixtures"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension() == Some(OsStr::new("json")))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(named, files);
}

/// go-indep-2 is go-indep-4's semver and textwrap units, copied; keep the copies in step (#181).
#[test]
fn go_indep_2_is_a_subset_of_go_indep_4() {
    let pair =
        |dir: &str| ["2", "4"].map(|n| read(root().join(format!("{dir}/go-indep-{n}.json"))));
    let [small, large] = pair("fixtures");
    for (name, text) in small["files"].as_object().unwrap() {
        assert_eq!(&large["files"][name], text, "{name}");
    }
    let requirements = large["requirements"].as_array().unwrap();
    for requirement in small["requirements"].as_array().unwrap() {
        assert!(requirements.contains(requirement));
    }
    let [small, large] = pair("protected");
    for (name, text) in small["reference"].as_object().unwrap() {
        assert_eq!(&large["reference"][name], text, "{name}");
    }
    // Past the import block, every top-level declaration appears verbatim in the larger oracle.
    let (small, large) = (
        small["oracle"].as_str().unwrap(),
        large["oracle"].as_str().unwrap(),
    );
    let (_, declarations) = small.split_once("\n)\n").unwrap();
    for declaration in declarations.split_inclusive("\n}\n") {
        assert!(large.contains(declaration.trim()), "{declaration}");
    }
}

#[test]
fn oracles_reject_compiling_behavioral_regressions() {
    let tmp = tmp("falinks-oracle-test-");
    let mutants = [
        (
            "rust-errors",
            "src/consumer.rs",
            vec![(
                "keys.iter().try_fold(0, |sum, key| lookup(key).map(|value| sum + value))",
                "Err(LookupError::Unavailable { retryable: true })",
            )],
        ),
        (
            "go-page",
            "handler.go",
            vec![("kind := r.URL.Query().Get(\"kind\")", "kind := \"\"")],
        ),
        (
            "go-relationships",
            "producer.go",
            vec![
                ("ID:i+1", "ID:len(out)+1"),
                ("for i, line := range lines", "for _, line := range lines"),
            ],
        ),
    ];
    for (name, file, edits) in mutants {
        let base = tmp.path().join(name);
        fs::create_dir(&base).unwrap();
        let repo = base.join("repo");
        prepare(name, &repo);
        for (path, text) in reference(name).as_object().unwrap() {
            fs::write(repo.join(path), text.as_str().unwrap()).unwrap();
        }
        let mut source = fs::read_to_string(repo.join(file)).unwrap();
        for (from, to) in edits {
            assert!(source.contains(from), "{name}: mutant anchor missing");
            source = source.replace(from, to);
        }
        fs::write(repo.join(file), source).unwrap();
        commit(&repo, "Intentional behavioral defect");
        let evidence = base.join("evidence");
        let result = cli([
            OsStr::new("oracle"),
            OsStr::new(name),
            OsStr::new("--repo"),
            repo.as_os_str(),
            OsStr::new("--evidence"),
            evidence.as_os_str(),
        ]);
        assert_eq!(result.status.code(), Some(1), "{name}: {}", text(&result));
        let checks = read(evidence.join("checks.json"));
        assert_eq!(checks["visible"]["passed"], true, "{name}: {checks}");
        assert_eq!(checks["oracle"]["passed"], false, "{name}");
    }
}

#[test]
fn oracle_blocks_prior_arm_reads_and_checks_export_ignored_files() {
    let tmp = tmp("falinks-isolation-test-");
    let base = fs::canonicalize(tmp.path()).unwrap();
    let (repo, prior) = (base.join("repo"), base.join("prior-arm"));
    fs::create_dir(&prior).unwrap();
    let secret = prior.join("solution.go");
    fs::write(&secret, "protected prior-arm solution").unwrap();
    prepare("go-page", &repo);
    fs::write(
        repo.join("handler.go"),
        reference("go-page")["handler.go"].as_str().unwrap(),
    )
    .unwrap();
    fs::write(repo.join("isolation_test.go"), format!(
        "package fixture_test\nimport(\"os\";\"testing\")\n\
         func TestPriorArmUnreadable(t *testing.T) {{ if _,err:=os.ReadFile({}); err==nil {{ t.Fatal(\"prior-arm solution readable\") }} }}\n",
        json!(secret.to_string_lossy()))).unwrap();
    commit(&repo, "Isolation candidate");
    let oracle = |evidence: &str| {
        cli([
            OsStr::new("oracle"),
            OsStr::new("go-page"),
            OsStr::new("--repo"),
            repo.as_os_str(),
            OsStr::new("--evidence"),
            base.join(evidence).as_os_str(),
            OsStr::new("--deny-root"),
            prior.as_os_str(),
        ])
    };
    let result = oracle("denied-check");
    assert!(result.status.success(), "{}", text(&result));
    // Git archive would silently omit this failing focused test. Exact tree checks must see it.
    fs::write(repo.join("isolation_test.go"),
              "package fixture_test\nimport \"testing\"\nfunc TestMustNotBeIgnored(t *testing.T) { t.Fatal(\"intentional focused failure\") }\n").unwrap();
    fs::write(
        repo.join(".gitattributes"),
        "isolation_test.go export-ignore\n",
    )
    .unwrap();
    commit(&repo, "Isolation candidate");
    let result = oracle("exact-check");
    assert_eq!(result.status.code(), Some(1), "{}", text(&result));
    let checks = read(base.join("exact-check/checks.json"));
    assert_eq!(checks["visible"]["passed"], false);
    assert_eq!(checks["oracle"]["passed"], true);
}

#[test]
fn codex_bridge_waits_for_peer_events_and_retains_all_usage() {
    let tmp = tmp("falinks-codex-bridge-test-");
    let base = tmp.path();
    // The fake runtime runs inside the worker sandbox, so it must live outside the checkout.
    let runtime = base.join("fake-codex");
    fs::copy(SCRIPTED, &runtime).unwrap();
    let config = config(
        base,
        CODEX_WORKER,
        json!(["{worker}"]),
        runtime.to_str().unwrap(),
    );
    let run = base.join("run");
    let evidence = successful_evidence(&baseline("go-relationships", &config, &run), &run);
    // Codex reports its last turn before `ready`, so no final result follows it; the Codex
    // arm is outside track T (#178).
    assert_eq!(evidence["usage"]["status"], "partial");
    for agent in ["A", "B"] {
        let reports = evidence["usage"]["agents"][agent].as_array().unwrap();
        let turns = fs::read_dir(run.join("agents").join(agent).join("scratch"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("turn-"))
            .count();
        assert!(turns > 2);
        assert_eq!(reports.len(), turns);
        let numbers: Vec<u64> = reports
            .iter()
            .map(|r| r["turn"].as_u64().unwrap())
            .collect();
        assert_eq!(numbers, (1..=turns as u64).collect::<Vec<_>>());
    }
}

#[test]
fn claude_bridge_forwards_host_commands_continues_capped_turns_and_retains_usage() {
    let tmp = tmp("falinks-claude-bridge-test-");
    let base = tmp.path();
    // The fake runtime runs inside the worker sandbox, so it must live outside the checkout.
    let runtime = base.join("fake-claude");
    fs::copy(SCRIPTED, &runtime).unwrap();
    let config = config(
        base,
        CLAUDE_WORKER,
        json!(["{worker}"]),
        runtime.to_str().unwrap(),
    );
    let run = base.join("run");
    let evidence = successful_evidence(&baseline("go-relationships", &config, &run), &run);
    assert_eq!(evidence["usage"]["status"], "complete");
    for agent in ["A", "B"] {
        let reports = evidence["usage"]["agents"][agent].as_array().unwrap();
        assert_eq!(reports[0]["subtype"], "error_max_turns");
        // Capped first turn, its continuation, the plan/diff/development events.
        assert!(reports.len() >= 4, "{reports:?}");
        let transcript = fs::read_to_string(
            run.join("agents")
                .join(agent)
                .join("scratch/claude-events.jsonl"),
        )
        .unwrap();
        assert!(transcript.contains("\"done\"") && !transcript.contains("hidden"));
    }
    let events = fs::read_to_string(run.join("controller/events.jsonl")).unwrap();
    assert_eq!(events.matches("\"action\":\"message\"").count(), 2);
}

#[test]
fn batch_gates_pauses_on_infrastructure_failure_and_records_the_replacement() {
    let tmp = tmp("falinks-batch-test-");
    let config_path = config(
        tmp.path(),
        SCRIPTED,
        json!(["{worker}", "relationships"]),
        SCRIPTED,
    );
    let runs = tmp.path().join("runs");
    let batch = |outcome: &str| {
        Command::new(CLI)
            .args(["batch", "--config"])
            .arg(&config_path)
            .arg("--runs")
            .arg(&runs)
            .env("FALINKS_FAKE_OUTCOME", outcome)
            .output()
            .unwrap()
    };
    let paused = batch("infrastructure_failure");
    assert_eq!(paused.status.code(), Some(3), "{}", text(&paused));
    let ledger = read(runs.join("batch.json"));
    assert_eq!(ledger["runs"][0]["outcome"], "success");
    assert_eq!(ledger["runs"][1]["outcome"], "infrastructure_failure");
    assert_eq!(ledger["pauses"].as_array().unwrap().len(), 1);
    assert_eq!(ledger["invocations"][0]["verification"]["passed"], true);
    let early = Command::new(CLI)
        .args(["batch", "--config"])
        .arg(&config_path)
        .arg("--runs")
        .arg(tmp.path().join("scored"))
        .arg("--after-pilot")
        .arg(&runs)
        .output()
        .unwrap();
    assert!(
        text(&early).contains("Complete the unscored pilot pair"),
        "{}",
        text(&early)
    );

    let finished = batch("success");
    assert!(finished.status.success(), "{}", text(&finished));
    let ledger = read(runs.join("batch.json"));
    let replacement = &ledger["runs"][2];
    assert_eq!(replacement["run_id"], "pilot-falinks-r1");
    assert_eq!(replacement["replaces"], "pilot-falinks");
    assert_eq!(ledger["gates"].as_array().unwrap().len(), 2);
    assert!(runs.join("gate-2.json").exists());
    assert_eq!(
        read(runs.join("pilot-git/controller/result.json"))["scored"],
        false
    );
    let summary = read(runs.join("report.json"));
    assert_eq!(summary["replaced_runs"], json!(["pilot-falinks"]));
    assert_eq!(ledger["invocations"][1]["verification"]["passed"], true);

    // A finished batch runs no slot: neither a gate nor a verification.
    let idle = batch("success");
    assert!(idle.status.success(), "{}", text(&idle));
    let ledger = read(runs.join("batch.json"));
    assert_eq!(ledger["invocations"].as_array().unwrap().len(), 3);
    assert!(ledger["invocations"][2].get("verification").is_none());
    assert_eq!(ledger["gates"].as_array().unwrap().len(), 2);
}

#[test]
fn batch_tells_a_gate_that_could_not_start_from_a_failed_gate() {
    let tmp = tmp("falinks-gate-test-");
    let mut config = read(config(tmp.path(), SCRIPTED, json!(["{worker}"]), SCRIPTED));
    let mut batch = |name: &str, gate: Value| {
        config["gate_command"] = gate;
        let path = tmp.path().join(format!("{name}.json"));
        fs::write(&path, config.to_string()).unwrap();
        let runs = tmp.path().join(name);
        let output = Command::new(CLI)
            .args(["batch", "--config"])
            .arg(&path)
            .arg("--runs")
            .arg(&runs)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3), "{}", text(&output));
        let ledger = read(runs.join("batch.json"));
        assert!(ledger["runs"].as_array().unwrap().is_empty());
        assert_eq!(ledger["pauses"].as_array().unwrap().len(), 1);
        // A failed gate stops the invocation before the fixture builds.
        assert!(ledger["invocations"][0].get("verification").is_none());
        (text(&output), ledger["gates"][0].clone())
    };
    let missing = tmp.path().join("check-integration");
    let (message, gate) = batch("missing", json!([missing, "--binary", SCRIPTED]));
    assert!(message.contains("gate could not start"), "{message}");
    assert!(
        message.contains(&missing.display().to_string()),
        "{message}"
    );
    assert_eq!(gate["started"], false);

    let (message, gate) = batch("failed", json!(["/usr/bin/false", SCRIPTED]));
    assert!(message.contains("gate scenario failed"), "{message}");
    assert_eq!(gate["started"], true);
    assert_eq!(gate["passed"], false);
}

#[test]
fn fixture_shard_runs_only_its_slots_denies_siblings_and_records_concurrency() {
    let tmp = tmp("falinks-shard-test-");
    let base = fs::canonicalize(tmp.path()).unwrap();
    let config_path = config(
        &base,
        SCRIPTED,
        json!(["{worker}", "relationships"]),
        SCRIPTED,
    );
    // A completed pilot ledger; its runs are not needed to authorize scoring.
    let pilot = base.join("pilot");
    fs::create_dir(&pilot).unwrap();
    let done = |slot: &str| json!({"slot": slot, "run_id": slot, "outcome": "success"});
    fs::write(
        pilot.join("batch.json"),
        json!({"runs": [done("pilot-git"), done("pilot-falinks")]}).to_string(),
    )
    .unwrap();
    // A sibling shard with a run in progress.
    let sibling = base.join("shard-page");
    fs::create_dir(&sibling).unwrap();
    fs::write(
        sibling.join("active.json"),
        json!({"run_id": "go-page-1-git"}).to_string(),
    )
    .unwrap();
    let shard = base.join("shard-relationships");
    let result = Command::new(CLI)
        .args(["batch", "--config"])
        .arg(&config_path)
        .arg("--runs")
        .arg(&shard)
        .arg("--after-pilot")
        .arg(&pilot)
        .args(["--fixtures", "go-relationships"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", text(&result));
    let ledger = read(shard.join("batch.json"));
    let slots: Vec<&str> = ledger["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["slot"].as_str().unwrap())
        .collect();
    assert_eq!(
        slots,
        [
            "go-relationships-1-git",
            "go-relationships-1-falinks",
            "go-relationships-2-falinks",
            "go-relationships-2-git",
            "go-relationships-3-git",
            "go-relationships-3-falinks",
        ]
    );
    assert_eq!(ledger["runs"][1]["order"], 1);
    for run in ledger["runs"].as_array().unwrap() {
        let id = run["run_id"].as_str().unwrap();
        let denied = read(shard.join(format!("configs/{id}.json")))["denied_roots"].clone();
        assert!(
            denied
                .as_array()
                .unwrap()
                .contains(&json!(sibling.to_string_lossy())),
            "{id}: {denied}"
        );
        let concurrency = &read(shard.join(id).join("controller/result.json"))["concurrency"];
        assert_eq!(concurrency["start"], json!(["go-page-1-git"]), "{id}");
        assert_eq!(concurrency["stop"], json!(["go-page-1-git"]), "{id}");
    }
    assert!(!shard.join("active.json").exists());
    let summary = read(shard.join("report.json"));
    assert_eq!(
        summary["concurrent_runs"].as_array().unwrap().len(),
        6,
        "{summary}"
    );
}

#[test]
fn baseline_retains_resolved_same_handler_merge_for_final_check() {
    let tmp = tmp("falinks-conflict-test-");
    let config = config(
        tmp.path(),
        SCRIPTED,
        json!(["{worker}", "conflict"]),
        SCRIPTED,
    );
    let run = tmp.path().join("run");
    let evidence = successful_evidence(&baseline("go-page", &config, &run), &run);
    let attempts = evidence["validation_attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 3);
    assert!(
        attempts[0]["conflict"]
            .as_str()
            .is_some_and(|c| !c.is_empty())
    );
    assert_eq!(attempts[1]["passed"], true);
    assert_eq!(attempts[2]["passed"], true);
    assert_eq!(evidence["final_snapshot"], attempts[1]["candidate"]);
}

/// Replaces temporary paths with `<tmp>`, the scripted worker binary with `<scripted-worker>`
/// and each commit ID with `<commit-N>`, numbered by first appearance: the scripted conflict
/// resolution commits at the current time.
fn normalized(value: &Value, tmp: &Path) -> Value {
    let canonical = fs::canonicalize(tmp).unwrap();
    let mut text = value.to_string().replace(SCRIPTED, "<scripted-worker>");
    for path in [&canonical, tmp] {
        text = text.replace(path.to_str().unwrap(), "<tmp>");
    }
    let (mut out, mut commits) = (String::new(), Vec::<String>::new());
    let mut rest = text.as_str();
    while let Some(start) = rest.find(|c: char| c.is_ascii_hexdigit()) {
        out.push_str(&rest[..start]);
        let word = &rest[start..];
        let end = word
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(word.len());
        let token = &word[..end];
        let commit = token.len() == 40 && token.bytes().all(|b| b.is_ascii_hexdigit());
        let preceded = out.ends_with(|c: char| c.is_ascii_alphanumeric());
        if commit && !preceded {
            let n = commits.iter().position(|c| c == token).unwrap_or_else(|| {
                commits.push(token.to_string());
                commits.len() - 1
            });
            out.push_str(&format!("<commit-{}>", n + 1));
        } else {
            out.push_str(token);
        }
        rest = &word[end..];
    }
    out.push_str(rest);
    serde_json::from_str(&out).unwrap()
}

/// What the host sent one agent, as the recording `tee` saw it: the start message, then its
/// replies and the unsolicited events as two sequences. Their interleaving, like A/B order,
/// depends on thread timing.
fn host_messages(log: &Path) -> Value {
    let messages: Vec<Value> = fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let unsolicited = |m: &&Value| {
        matches!(
            m["type"].as_str(),
            Some("peer_message" | "peer_diff" | "development")
        )
    };
    assert_eq!(messages[0]["type"], "start");
    json!({
        "start": messages[0],
        "replies": messages[1..].iter().filter(|m| !unsolicited(m)).collect::<Vec<_>>(),
        "events": messages[1..].iter().filter(unsolicited).collect::<Vec<_>>(),
    })
}

/// Each development with the number of drafts every agent had shared when it fired.
fn triggers(run: &Path) -> Value {
    let mut shares = json!({"A": 0, "B": 0});
    let mut triggers = vec![];
    for line in fs::read_to_string(run.join("controller/events.jsonl"))
        .unwrap()
        .lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        match row["kind"].as_str() {
            Some("draft_shared") => {
                let agent = row["agent"].as_str().unwrap();
                shares[agent] = json!(shares[agent].as_u64().unwrap() + 1);
            }
            Some("development") => {
                triggers.push(json!({"milestone": row["milestone"]["id"], "shares": shares}))
            }
            _ => {}
        }
    }
    json!(triggers)
}

/// The two-agent golden (#179, recorded at manifest v12, whose harness is v11's): what the
/// host sends each agent, when developments fire and the instruction bytes, from scripted runs.
/// A team of two must keep it (#178 C6.1); `FALINKS_GOLDEN=update` re-records it deliberately.
#[test]
fn two_agent_runs_match_the_golden_recording() {
    let tmp = tmp("falinks-golden-test-");
    let base = tmp.path();
    let mut golden = json!({});
    for (fixture, mode) in [
        ("go-relationships", "relationships"),
        ("go-page", "conflict"),
    ] {
        // `tee` logs each host message before the scripted worker reads it; scratch is never shared.
        let command = format!("/usr/bin/tee scratch/host.jsonl | \"$0\" {mode}");
        let config = config(
            base,
            SCRIPTED,
            json!(["/bin/sh", "-c", command, "{worker}"]),
            SCRIPTED,
        );
        let run = base.join(fixture);
        successful_evidence(&baseline(fixture, &config, &run), &run);
        let agents: serde_json::Map<String, Value> = ["A", "B"]
            .iter()
            .map(|a| {
                let log = run.join("agents").join(a).join("scratch/host.jsonl");
                (a.to_string(), host_messages(&log))
            })
            .collect();
        golden[fixture] = json!({"agents": agents, "triggers": triggers(&run)});
    }
    // The Claude bridge's prompt to the runtime, logged by a wrapper around the fake runtime.
    let runtime = base.join("fake-claude");
    fs::copy(SCRIPTED, &runtime).unwrap();
    let wrapper = base.join("recording-claude");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n/usr/bin/tee \"$CLAUDE_CODE_TMPDIR/runtime-input.jsonl\" | {} \"$@\"\n",
            runtime.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let config = config(
        base,
        CLAUDE_WORKER,
        json!(["{worker}"]),
        wrapper.to_str().unwrap(),
    );
    let run = base.join("claude");
    successful_evidence(&baseline("go-relationships", &config, &run), &run);
    for agent in ["A", "B"] {
        let input = fs::read_to_string(
            run.join("agents")
                .join(agent)
                .join("scratch/runtime-input.jsonl"),
        )
        .unwrap();
        let first: Value = serde_json::from_str(input.lines().next().unwrap()).unwrap();
        golden["claude_prompts"][agent] = first["message"]["content"].clone();
    }
    let actual = serde_json::to_string_pretty(&normalized(&golden, base)).unwrap() + "\n";
    let path = root().join("tests/golden/two-agent.json");
    if std::env::var("FALINKS_GOLDEN").as_deref() == Ok("update") {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &actual).unwrap();
    }
    let expected = fs::read_to_string(&path).unwrap_or_default();
    if let Some((line, (want, got))) = expected
        .lines()
        .zip(actual.lines())
        .enumerate()
        .find(|(_, (want, got))| want != got)
    {
        panic!(
            "two-agent golden differs at line {}:\n  golden: {want}\n  actual: {got}",
            line + 1
        );
    }
    assert_eq!(
        expected.lines().count(),
        actual.lines().count(),
        "two-agent golden length differs"
    );
}

/// The bounded usage drain (#179), with a 1 s window: a final result sent at once is complete;
/// one sent only when the host stops the worker (late) or never sent (missing) is partial. The
/// earlier turn report is kept, and the clock stops before the drain in all three.
#[test]
fn usage_is_complete_only_with_final_results_inside_the_drain_window() {
    let tmp = tmp("falinks-drain-test-");
    for (mode, status, final_result) in [
        ("received", "complete", "received"),
        ("late", "partial", "late"),
        ("never", "partial", "missing"),
    ] {
        let config = config(
            tmp.path(),
            SCRIPTED,
            json!(["{worker}", "relationships", mode]),
            SCRIPTED,
        );
        let run = tmp.path().join(mode);
        let evidence = successful_evidence(&baseline("go-relationships", &config, &run), &run);
        let usage = &evidence["usage"];
        assert_eq!(usage["status"], status, "{mode}: {usage}");
        for agent in ["A", "B"] {
            assert_eq!(usage["final_results"][agent], final_result, "{mode}");
            assert_eq!(usage["agents"][agent][0]["turn"], 1, "{mode}");
            let late = usage["agents"][agent].as_array().unwrap().last().unwrap()["late"] == true;
            assert_eq!(late, mode == "late", "{mode}");
        }
        let drained = usage["drain"]["seconds"].as_f64().unwrap();
        assert_eq!(
            drained < 1.0,
            mode == "received",
            "{mode}: drained {drained} s"
        );
        let at = |kind: &str| {
            fs::read_to_string(run.join("controller/events.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|row| row["kind"] == kind)
                .unwrap()["elapsed"]
                .as_f64()
                .unwrap()
        };
        let elapsed = evidence["timing"]["elapsed_seconds"].as_f64().unwrap();
        assert!(
            at("final_validation") <= elapsed && elapsed <= at("drain"),
            "{mode}: the clock must stop at the final validation, before the drain"
        );
    }
}

/// `timing` on synthetic stream-json transcripts (`tests/timing/`), checked against values
/// computed by hand. Times are seconds after 2026-10-21T10:00:00Z.
#[test]
fn timing_reports_hand_computed_figures() {
    let figures = |run: &str| {
        let output = cli([
            OsStr::new("timing"),
            root().join("tests/timing").join(run).as_os_str(),
        ]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let trips =
        |edit: (u64, f64), refused: (u64, f64), native: (u64, f64), coordination: (u64, f64)| {
            let total = (
                edit.0 + refused.0 + native.0 + coordination.0,
                edit.1 + refused.1 + native.1 + coordination.1,
            );
            let entry =
                |(count, seconds): (u64, f64)| json!({"count": count, "model_seconds": seconds});
            json!({"edit": entry(edit), "refused_edit": entry(refused), "native": entry(native),
            "coordination": entry(coordination), "total": entry(total)})
        };
    let none = json!({"refusals": 0, "unresolved": 0, "round_trips": 0, "model_seconds": 0.0});
    let recovery = |content: &Value| json!({"content": content, "replace": none, "from": none});

    // Git: the clock starts at 0.5 (to the millisecond) and stops at 20.5.
    let git = figures("git");
    assert_eq!(git["start_precision"], "millisecond");
    assert_eq!(git["slowest_agent"], "A");
    assert_eq!(
        git["round_trips"],
        json!({"count": 6, "model_seconds": 8.25})
    );
    assert_eq!(
        git["agents"]["A"],
        json!({
            // Turns 2.5-7.0 and 12.0-24.0, the second clipped at the stopped clock: 4.5 + 8.5.
            "busy_seconds": 13.0,
            "launch_seconds": 2.0,
            // 20.5 - 2.5 - 13.0.
            "waiting_on_peer_seconds": 5.0,
            // m1 (two events, last at 4.0) 4.0-2.5; m2 6.0-4.5; m3 7.0-6.5; m4 13.5-12.0;
            // m5 15.0-14.0; m6 16.0-15.25; m7 24.0-16.5.
            "model_seconds": 14.25,
            // m1 here-document into producer.go; m4 a refused Write; m5 an Edit; m2 Bash share
            // plus Read is native; m6 only Bash ready.
            "round_trips": trips((2, 2.5), (1, 1.5), (1, 1.5), (1, 0.75)),
            // Recovery counts Falinks edits only.
            "refusal_recovery": recovery(&none),
            "request_key_refusals": {},
            // t1 0.5, t2 0.25, t3 0.5, t4 0.5, t5 0.25, t6 0.5.
            "tool_seconds": 2.5,
            // 16 (here-document) + 35 (refused Write) + 22 (Edit's new string).
            "output_bytes": 73,
            // producer.go ends "package fixture\n\n// A\n": only "// A\n" is new.
            "changed_bytes": 5,
            "results": 2,
            "usage_reports": 1,
        })
    );
    assert_eq!(
        git["agents"]["B"],
        json!({"busy_seconds": 2.5, "launch_seconds": 2.5, "waiting_on_peer_seconds": 15.0,
            "model_seconds": 2.0, "round_trips": trips((0, 0.0), (0, 0.0), (0, 0.0), (1, 2.0)),
            "refusal_recovery": recovery(&none), "request_key_refusals": {}, "tool_seconds": 0.5, "output_bytes": 0, "changed_bytes": 0, "results": 1,
            "usage_reports": 1})
    );

    // Falinks: the clock starts at 0 (to the second) and stops at 10; agent-0 resumes once.
    let falinks = figures("falinks");
    let unresolved =
        json!({"refusals": 1, "unresolved": 1, "round_trips": 1, "model_seconds": 1.0});
    assert_eq!(falinks["refusal_recovery"], recovery(&unresolved));
    // The Stale refusal is the engine's, not the adapter's key check.
    assert_eq!(falinks["request_key_refusals"], json!({}));
    assert_eq!(falinks["start_precision"], "second");
    assert_eq!(falinks["slowest_agent"], "agent-0");
    assert_eq!(
        falinks["round_trips"],
        json!({"count": 4, "model_seconds": 4.0})
    );
    assert_eq!(
        falinks["agents"]["agent-0"],
        json!({
            // Turns 1.5-4.5 and 8.0-9.25.
            "busy_seconds": 4.25,
            "launch_seconds": 1.5,
            "waiting_on_peer_seconds": 4.25,
            "model_seconds": 3.0,
            // An accepted edit with a Read, an edit refused as Stale (its request a JSON string),
            // then falinks_ready.
            "round_trips": trips((1, 1.0), (1, 1.0), (0, 0.0), (1, 1.0)),
            // The Stale refusal is never followed by an accepted edit: falinks_ready (8.0-9.0)
            // still counts.
            "refusal_recovery": recovery(&unresolved),
            "request_key_refusals": {},
            "tool_seconds": 1.75,
            // 23 + 2; the refused edit is output but changes nothing.
            "output_bytes": 25,
            "changed_bytes": 7,
            "results": 2,
            "usage_reports": 2,
        })
    );
    assert_eq!(
        falinks["agents"]["agent-1"],
        json!({"busy_seconds": 3.0, "launch_seconds": 2.0, "waiting_on_peer_seconds": 5.0,
            "model_seconds": 2.0, "round_trips": trips((0, 0.0), (0, 0.0), (1, 1.0), (0, 0.0)),
            "refusal_recovery": recovery(&none), "request_key_refusals": {}, "tool_seconds": 1.0, "output_bytes": 0, "changed_bytes": 0, "results": 1,
            "usage_reports": 1})
    );
}

/// A team of one (#178 C1) has no agent B: `timing` reads agent A alone.
#[test]
fn timing_reads_only_the_agents_of_a_team_of_one() {
    let tmp = tmp("falinks-timing-solo-");
    let run = tmp.path().join("git");
    let a = run.join("agents/A/scratch");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(run.join("controller")).unwrap();
    let events = "agents/A/scratch/claude-events.jsonl";
    fs::copy(
        root().join("tests/timing/git").join(events),
        run.join(events),
    )
    .unwrap();
    let mut result = read(root().join("tests/timing/git/controller/result.json"));
    result["team_size"] = json!(1);
    fs::write(run.join("controller/result.json"), result.to_string()).unwrap();
    let output = cli([OsStr::new("timing"), run.as_os_str()]);
    assert!(output.status.success(), "{}", text(&output));
    let figures: Value = serde_json::from_slice(&output.stdout).unwrap();
    let agents: Vec<&String> = figures["agents"].as_object().unwrap().keys().collect();
    assert_eq!(agents, ["A"]);
    assert_eq!(figures["slowest_agent"], "A");
}

#[test]
fn baseline_exchanges_unfinished_work_and_freezes_exact_candidate() {
    let tmp = tmp("falinks-baseline-test-");
    let config = config(
        tmp.path(),
        SCRIPTED,
        json!(["{worker}", "relationships"]),
        SCRIPTED,
    );
    let run = tmp.path().join("run");
    let evidence = successful_evidence(&baseline("go-relationships", &config, &run), &run);
    assert_eq!(evidence["outcome"], "success");
    assert_eq!(evidence["usage"]["status"], "unavailable");
    assert_eq!(evidence["scored"], false);
    assert_eq!(evidence["final_snapshot"].as_str().unwrap().len(), 40);
    let events: Vec<Value> = fs::read_to_string(run.join("controller/events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let developments: Vec<&Value> = events
        .iter()
        .filter(|e| e["kind"] == "development")
        .map(|e| &e["milestone"]["id"])
        .collect();
    assert_eq!(
        developments,
        [&json!("record-contract"), &json!("follow-ups")]
    );
    let agents_where = |predicate: &dyn Fn(&Value) -> bool| {
        let mut agents: Vec<&str> = events
            .iter()
            .filter(|e| predicate(e))
            .map(|e| e["agent"].as_str().unwrap())
            .collect();
        agents.sort();
        agents.dedup();
        agents
    };
    assert_eq!(agents_where(&|e| e["kind"] == "draft_shared"), ["A", "B"]);
    // Sandbox protection must be an observed denial, not a written instruction.
    assert_eq!(
        agents_where(
            &|e| e["kind"] == "worker_request" && e["request"]["text"] == "protected-read-denied"
        ),
        ["A", "B"]
    );
    assert!(!baseline("go-relationships", &config, &run).status.success());
}

/// C6.4 (#178): the scored schedule is v11's (`tests/golden/schedule-v11.json`, recorded from
/// the harness at `9564d45`), whatever `FIXTURES` gains.
#[test]
fn scored_schedule_equals_v11() {
    let output = cli(["schedule"]);
    assert!(output.status.success(), "{}", text(&output));
    let schedule: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        schedule,
        read(root().join("tests/golden/schedule-v11.json"))
    );
}

/// C6.2 (#178): a team of one gets no peer, the solo workflow and both allocations, completes,
/// and developments fire on its own shares.
#[test]
fn a_team_of_one_completes_and_developments_fire_on_its_own_shares() {
    let tmp = tmp("falinks-solo-test-");
    let config = config(
        tmp.path(),
        SCRIPTED,
        json!([
            "/bin/sh",
            "-c",
            "/usr/bin/tee scratch/host.jsonl | \"$0\" relationships",
            "{worker}"
        ]),
        SCRIPTED,
    );
    let run = tmp.path().join("run");
    let mut args = vec![OsStr::new("baseline"), OsStr::new("go-relationships")];
    args.extend([OsStr::new("--config"), config.as_os_str()]);
    args.extend([OsStr::new("--output"), run.as_os_str()]);
    args.extend([OsStr::new("--team-size"), OsStr::new("1")]);
    let evidence = successful_evidence(&cli(args), &run);
    assert_eq!(evidence["team_size"], 1);
    assert!(!run.join("agents/B").exists());
    assert_eq!(
        triggers(&run),
        json!([{"milestone": "record-contract", "shares": {"A": 1, "B": 0}},
               {"milestone": "follow-ups", "shares": {"A": 2, "B": 0}}])
    );
    let host = host_messages(&run.join("agents/A/scratch/host.jsonl"));
    let start = &host["start"];
    assert!(start.get("peer_workspace").is_none(), "{start}");
    assert_eq!(
        start["workflow"].as_str().unwrap(),
        fs::read_to_string(root().join("instructions/git-solo.md")).unwrap()
    );
    let allocation = &read(root().join("fixtures/go-relationships.json"))["allocation"];
    assert_eq!(
        start["allocation"].as_str().unwrap(),
        format!(
            "{}\n{}",
            allocation["A"].as_str().unwrap(),
            allocation["B"].as_str().unwrap()
        )
    );
    let events: Vec<&str> = host["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(events, ["development", "development"]);
    // Its one message has no peer to reach: refused, not delivered.
    assert_eq!(evidence["rejected_operations"], 1);
}

/// C7 (#178), Git arm: a scripted `claude` or `node` launch through Bash invalidates the run.
/// Both arms: hits are counted per arm and the batch stops at the first.
#[test]
fn nested_agents_invalidate_the_run_are_counted_and_stop_the_batch() {
    let tmp = tmp("falinks-nested-test-");
    for program in ["claude", "node"] {
        let base = tmp.path().join(program);
        fs::create_dir(&base).unwrap();
        let runtime = base.join(format!("launches-{program}"));
        fs::copy(SCRIPTED, &runtime).unwrap();
        let config = config(
            &base,
            CLAUDE_WORKER,
            json!(["{worker}"]),
            runtime.to_str().unwrap(),
        );
        let run = base.join("run");
        assert!(!baseline("go-relationships", &config, &run).status.success());
        let evidence = read(run.join("controller/result.json"));
        assert_eq!(evidence["outcome"], "run_invalid");
        assert_eq!(
            evidence["nested_agent"]["command"],
            format!("{program} -p 'help me'")
        );
        assert!(
            evidence["failure_reason"]
                .as_str()
                .unwrap()
                .contains("nested agent"),
            "{evidence}"
        );
    }

    // Batches: the Git arm's pilot run launches `node`; then the Falinks arm's one `claude`.
    let runtime = tmp.path().join("launches-node");
    fs::copy(SCRIPTED, &runtime).unwrap();
    let git = config(
        tmp.path(),
        CLAUDE_WORKER,
        json!(["{worker}"]),
        runtime.to_str().unwrap(),
    );
    let falinks = tmp.path().join("falinks");
    fs::create_dir(&falinks).unwrap();
    let falinks = config(
        &falinks,
        SCRIPTED,
        json!(["{worker}", "relationships"]),
        SCRIPTED,
    );
    for (arm, config, nested, paused) in [
        ("git", &git, None, "pilot-git"),
        ("falinks", &falinks, Some("claude -p x"), "pilot-falinks"),
    ] {
        // Apart from the runtime: a batch denies its runs directory's siblings to every run.
        let runs = tmp.path().join("batches").join(format!("runs-{arm}"));
        let mut command = Command::new(CLI);
        command
            .args(["batch", "--config"])
            .arg(config)
            .arg("--runs")
            .arg(&runs);
        if let Some(nested) = nested {
            command.env("FALINKS_FAKE_NESTED", nested);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(3), "{}", text(&output));
        let ledger = read(runs.join("batch.json"));
        let reason = ledger["pauses"][0]["reason"].as_str().unwrap();
        assert!(
            reason.starts_with(&format!("{paused}: nested agent")),
            "{reason}"
        );
        assert_eq!(
            ledger["runs"].as_array().unwrap().last().unwrap()["run_id"],
            paused
        );
        // Never rerun in place: resuming is refused.
        let resumed = command.output().unwrap();
        assert!(
            text(&resumed).contains("start a new batch directory"),
            "{}",
            text(&resumed)
        );
        let report = cli([OsStr::new("report"), OsStr::new("--runs"), runs.as_os_str()]);
        let report: Value = serde_json::from_slice(&report.stdout).unwrap();
        let hits = json!({"git": (arm == "git") as u8, "falinks": (arm == "falinks") as u8});
        assert_eq!(report["nested_agent_hits"], hits);
    }
}

/// C1 (#178): the track-T schedule runs one fixture at both team sizes, each run isolated as
/// in the scored batch, and records the size.
#[test]
fn track_t_batch_runs_both_team_sizes_with_run_isolation() {
    let tmp = tmp("falinks-track-t-test-");
    let config = config(
        tmp.path(),
        SCRIPTED,
        json!(["{worker}", "relationships"]),
        SCRIPTED,
    );
    let runs = tmp.path().join("runs");
    let output = Command::new(CLI)
        .args([
            "batch",
            "--schedule",
            "track-t",
            "--fixtures",
            "go-relationships",
            "--config",
        ])
        .arg(&config)
        .arg("--runs")
        .arg(&runs)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let ledger = read(runs.join("batch.json"));
    let recorded = ledger["runs"].as_array().unwrap();
    let mut sizes: Vec<(String, u64)> = vec![];
    for (index, r) in recorded.iter().enumerate() {
        assert_eq!(r["outcome"], "success", "{r}");
        let result = read(Path::new(r["run_dir"].as_str().unwrap()).join("controller/result.json"));
        assert_eq!(result["team_size"], r["team_size"]);
        assert_eq!(result["scored"], false);
        // Every earlier run is denied to this one.
        let denied = result["configuration"]["denied_roots"].as_array().unwrap();
        for earlier in &recorded[..index] {
            assert!(
                denied.contains(&earlier["run_dir"]),
                "{}",
                earlier["run_dir"]
            );
        }
        sizes.push((
            r["run_id"].as_str().unwrap().into(),
            r["team_size"].as_u64().unwrap(),
        ));
    }
    let expected: Vec<(String, u64)> = [
        (1, 1),
        (1, 2),
        (2, 2),
        (2, 1),
        (3, 1),
        (3, 2),
        (4, 2),
        (4, 1),
        (5, 1),
        (5, 2),
    ]
    .iter()
    .map(|(rep, size)| (format!("go-relationships-{rep}-git-{size}"), *size))
    .collect();
    assert_eq!(sizes, expected);
    // The batch writes the track-T report: five repetitions per team size, labelled.
    let report = read(runs.join("report.json"));
    let time = &report["fixtures"]["go-relationships"]["metrics"]["completion_seconds"];
    assert_eq!((&time["1"]["n"], &time["2"]["n"]), (&json!(5), &json!(5)));
    assert!(time["label"].is_string());
    assert_eq!(report["gaps"], json!([]));
}

/// #228 decision 28: `report` prints no source text unless `--show-code` asks for it.
#[test]
fn report_prints_reverted_lines_only_with_show_code() {
    let tmp = tmp("falinks-report-code-");
    let run = tmp.path().join("go-page-0-f");
    fs::create_dir_all(run.join("controller")).unwrap();
    let secret = "return nil // agent code";
    fs::write(
        run.join("controller/result.json"),
        json!({"outcome": "success", "edits": {"forms": {}, "reverted_peer_text":
            [{"agent": 1, "id": "b3", "path": "a.go", "peer_id": "a2", "lines": [secret, "}"]}]}})
        .to_string(),
    )
    .unwrap();
    fs::write(
        tmp.path().join("batch.json"),
        json!({"runs": [{"run_dir": run, "run_id": "go-page-0-f", "pair": "go-page-1",
            "fixture": "go-page", "arm": "falinks", "scored": true}]})
        .to_string(),
    )
    .unwrap();
    let report = |extra: &[&str]| {
        let output = cli(["report", "--runs", tmp.path().to_str().unwrap()]
            .into_iter()
            .chain(extra.iter().copied()));
        assert!(output.status.success(), "{}", text(&output));
        String::from_utf8(output.stdout).unwrap()
    };
    let hidden = report(&[]);
    assert!(!hidden.contains(secret), "{hidden}");
    let entry = &serde_json::from_str::<Value>(&hidden).unwrap()["fixtures"]["go-page"]["reverted_peer_text"]
        [0];
    assert_eq!(
        *entry,
        json!({"agent": 1, "id": "b3", "path": "a.go", "peer_id": "a2", "line_count": 2,
            "run_id": "go-page-0-f"})
    );
    let shown: Value = serde_json::from_str(&report(&["--show-code"])).unwrap();
    assert_eq!(
        shown["fixtures"]["go-page"]["reverted_peer_text"][0]["lines"],
        json!([secret, "}"])
    );
}

/// #228 decision 27: every differential property quotes its requirement sentence verbatim
/// from the frozen fixture definition, and the judge uses the run configuration's pins.
#[test]
fn code_quality_definitions_trace_to_fixture_requirements() {
    let definitions = read(root().join("code-quality.json"));
    let properties = definitions["properties"]["list"].as_array().unwrap();
    assert_eq!(properties.len(), 13);
    for property in properties {
        let fixture = read(
            root()
                .join("fixtures")
                .join(format!("{}.json", property["fixture"].as_str().unwrap())),
        );
        let requirement = &property["requirement"];
        let at = requirement["at"].as_str().unwrap();
        let sentence = requirement["text"].as_str().unwrap();
        let source = fixture
            .pointer(at)
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            source.contains(sentence),
            "{}: {sentence:?} is not in {at}",
            property["id"]
        );
    }
    let config = read(root().join("config.json"));
    let judge = &definitions["judge"];
    assert_eq!(judge["model"], config["model"]);
    assert_eq!(judge["runtime_version"], config["runtime_version"]);
}
