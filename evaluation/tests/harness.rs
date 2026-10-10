//! Public CLI checks; no private helper assertions or model calls.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
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
        ("timeout_seconds", json!(90)),
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
    read(evidence_path)
}

#[test]
fn frozen_oracles_reject_initial_and_accept_reference() {
    let result = cli(["verify"]);
    assert!(result.status.success(), "{}", text(&result));
    let evidence: Value = serde_json::from_slice(&result.stdout).unwrap();
    let fixtures = evidence["fixtures"].as_object().unwrap();
    assert_eq!(
        fixtures.keys().collect::<Vec<_>>(),
        ["go-page", "go-relationships", "rust-errors"]
    );
    for fixture in fixtures.values() {
        assert_eq!(fixture["initial"]["passed"], false);
        assert_eq!(fixture["reference"]["passed"], true);
        assert_eq!(fixture["initial_visible"]["passed"], true);
        assert_eq!(fixture["reference_visible"]["passed"], true);
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
    assert_eq!(evidence["usage"]["status"], "available");
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
    assert_eq!(evidence["usage"]["status"], "available");
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
