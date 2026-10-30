//! Public CLI checks; no private helper assertions or model calls.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
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
/// A one-worktree repository for the batch's clean-checkout check.
fn checkout(base: &Path) -> PathBuf {
    let dir = base.join("checkout");
    fs::create_dir_all(&dir).unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&dir)
            .status()
            .unwrap()
            .success()
    );
    dir
}

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
            "go-invoice",
            "go-page",
            "go-relationships",
            "go-tasks",
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

    // Track T's own budget (#178), and #205's (B1) charged the same way: each session's charge
    // is its run (with the usage drain) and preparation time; implementation is recorded but not
    // charged (Daniel, 2026-10-22, #180). Usage comes from complete runs only.
    for section in ["team_size_research", "large_spec_research"] {
        let track = &ledger[section];
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
            assert_eq!(totals[part], sum(&|s| &s[part]), "{section}.totals.{part}");
        }
        for key in totals["complete_usage"].as_object().unwrap().keys() {
            let key = key.as_str();
            let summed = sum(&|s| &s["complete_usage"][key]);
            assert_eq!(
                totals["complete_usage"][key], summed,
                "{section}.totals.complete_usage.{key}"
            );
        }
    }
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

/// Fixture 3's follow-ups are independent in code (#134): B's new file uses neither
/// `Consume` nor `Record`, and A's file uses nothing from B's files.
#[test]
fn relationships_follow_ups_share_no_code() {
    let reference = reference("go-relationships");
    let source = |file: &str| reference[file].as_str().unwrap().to_string();
    for name in ["Consume", "Record", "Produce", "Stats", "Duplicates"] {
        assert!(!source("parse.go").contains(name), "parse.go uses {name}");
    }
    for name in ["Consume", "Parse", "IDs"] {
        assert!(
            !source("producer.go").contains(name),
            "producer.go uses {name}"
        );
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
        // One per follow-up: A's Duplicates and B's Parse.
        (
            "go-relationships",
            "producer.go",
            vec![("counts[record.Text] > 1", "counts[record.Text] > 0")],
        ),
        ("go-relationships", "parse.go", vec![("id <= 0 || ", "")]),
    ];
    for (index, (name, file, edits)) in mutants.into_iter().enumerate() {
        let base = tmp.path().join(format!("{index}-{name}"));
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

/// Gives a scripted configuration a runtime home, where the runtime keeps its sessions.
fn with_runtime_home(config: &Path, base: &Path) -> PathBuf {
    let home = base.join("home");
    let mut value = read(config);
    value["runtime_home"] = json!(home);
    fs::write(config, value.to_string()).unwrap();
    home
}

/// A session that records another effort than the configured one invalidates the run, like
/// another model (#205 E1). The scripted runtime copied as `effort-high` records `high`.
#[test]
fn claude_bridge_fails_a_session_at_another_effort() {
    let tmp = tmp("falinks-claude-effort-test-");
    let base = tmp.path();
    let runtime = base.join("effort-high");
    fs::copy(SCRIPTED, &runtime).unwrap();
    let config = config(
        base,
        CLAUDE_WORKER,
        json!(["{worker}"]),
        runtime.to_str().unwrap(),
    );
    with_runtime_home(&config, base);
    let run = base.join("run");
    assert!(!baseline("go-relationships", &config, &run).status.success());
    let stderr: String = ["A", "B"]
        .map(|a| fs::read_to_string(run.join(format!("controller/{a}.stderr"))).unwrap())
        .concat();
    assert!(
        stderr.contains("session effort \"high\" is not \"none\"; run invalid"),
        "{stderr}"
    );
}

/// Without its session file a run's effort cannot be verified (#205 E1). The scripted runtime
/// copied as `unrecorded` writes none; with no `runtime_home`, the runtime's home is scratch.
#[test]
fn claude_bridge_fails_a_session_without_its_file() {
    let tmp = tmp("falinks-claude-unrecorded-test-");
    let base = tmp.path();
    let runtime = base.join("unrecorded");
    fs::copy(SCRIPTED, &runtime).unwrap();
    let config = config(
        base,
        CLAUDE_WORKER,
        json!(["{worker}"]),
        runtime.to_str().unwrap(),
    );
    let run = base.join("run");
    assert!(!baseline("go-relationships", &config, &run).status.success());
    let stderr: String = ["A", "B"]
        .map(|a| fs::read_to_string(run.join(format!("controller/{a}.stderr"))).unwrap())
        .concat();
    assert!(
        stderr.contains("session effort unrecorded; run invalid"),
        "{stderr}"
    );
}

/// The bridge never passes `--effort ""`: a start without the effort is refused at once.
#[test]
fn claude_bridge_refuses_a_start_without_effort() {
    let tmp = tmp("falinks-claude-no-effort-test-");
    for model in [
        json!({"model": "scripted"}),
        json!({"model": "scripted", "reasoning": ""}),
    ] {
        let mut child = Command::new(CLAUDE_WORKER)
            .current_dir(tmp.path())
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = json!({"model": model});
        writeln!(child.stdin.take().unwrap(), "{start}").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "the start names no reasoning effort\n"
        );
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
    let home = with_runtime_home(&config, base);
    let run = base.join("run");
    let evidence = successful_evidence(&baseline("go-relationships", &config, &run), &run);
    assert_eq!(evidence["usage"]["status"], "complete");
    // The scripted runtime records the `--effort` it was given, as the real one does (#205 E1).
    let sessions: Vec<String> = fs::read_dir(home.join(".claude/projects"))
        .unwrap()
        .flat_map(|folder| fs::read_dir(folder.unwrap().path()).unwrap())
        .map(|file| fs::read_to_string(file.unwrap().path()).unwrap())
        .collect();
    assert_eq!(sessions.len(), 2);
    for session in sessions {
        assert!(session.contains("\"effort\":\"none\""), "{session}");
    }
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
            .env("FALINKS_FAKE_CHECKOUT", checkout(tmp.path()))
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
        .env("FALINKS_FAKE_CHECKOUT", checkout(tmp.path()))
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
            .env("FALINKS_FAKE_CHECKOUT", checkout(tmp.path()))
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
        .env("FALINKS_FAKE_CHECKOUT", checkout(tmp.path()))
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
/// computed by hand. Times are seconds after 2026-10-21T10:00:00Z. Step 6's overhead figures
/// (#316): tool seconds and acts by Falinks operation, and edit-gate seconds per accepted edit.
/// Changed bytes come from each arm's records (#325), for writes the transcript misreads, and
/// count only the lines each agent introduced: in each arm, one agent's write carries a file
/// its peer wrote.
#[test]
fn timing_reports_hand_computed_figures() {
    // Git's records: A's two shares, then B's two, committed over the fixture's initial files.
    // B's second share merges A's and extends A's store/store.go.
    let tmp = tmp("falinks-timing-");
    let git_run = tmp.path().join("git");
    let copied = Command::new("cp")
        .arg("-R")
        .arg(root().join("tests/timing/git"))
        .arg(&git_run)
        .status()
        .unwrap();
    assert!(copied.success());
    let repo = git_run.join("agents/repo");
    fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let identity = [
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ];
        let out = Command::new("git")
            .args(identity)
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    let initial = read(root().join("fixtures/go-relationships.json"))["files"].clone();
    let mut events = String::new();
    let mut share = |agent: &str, message: &str, files: &[(&str, &str)]| {
        for (name, content) in files {
            fs::create_dir_all(repo.join(name).parent().unwrap()).unwrap();
            fs::write(repo.join(name), content).unwrap();
        }
        git(&["add", "."]);
        git(&["commit", "-q", "-m", message]);
        let commit = git(&["rev-parse", "HEAD"]);
        if !agent.is_empty() {
            let event = json!({"kind": "draft_shared", "agent": agent, "commit": commit});
            events += &format!("{event}\n");
        }
        commit
    };
    git(&["init", "-q"]);
    let files: Vec<(&str, &str)> = initial
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str().unwrap()))
        .collect();
    let root_commit = share("", "Frozen initial task", &files);
    let consumer =
        "package fixture\n\nfunc Consume(records []string) []string {\n    return records\n}\n";
    let store = "package store\n\nfunc Get() int { return 1 }\n";
    share(
        "A",
        "A draft 1",
        &[
            ("producer.go", "package fixture\n"),
            ("store/store.go", "package store\n\nfunc Get() {}\n"),
        ],
    );
    let a2 = share("A", "A draft 2", &[("store/store.go", store)]);
    git(&["checkout", "-q", "-b", "B", &root_commit]);
    share("B", "B draft 1", &[("consumer.go", consumer)]);
    git(&["merge", "-q", "--no-edit", &a2]);
    share(
        "B",
        "B draft 2",
        &[("store/store.go", &format!("{store}func Put() {{}}\n"))],
    );
    fs::write(git_run.join("controller/events.jsonl"), events).unwrap();
    let figures = |run: &str| {
        let dir = match run {
            "git" => git_run.clone(),
            _ => root().join("tests/timing").join(run),
        };
        let output = cli([OsStr::new("timing"), dir.as_os_str()]);
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
    let idle = json!({"round_trips": 0, "model_seconds": 0.0, "unresolved": 0, "unlocated": 0});
    let unhandled = json!({"line_conflict": idle, "rework": idle, "no_fault": idle});
    // No synthetic run here integrates or meets the runtime's read-before-write refusal.
    let nothing = json!({"round_trips": 0, "model_seconds": 0.0});
    let none = json!({"refusals": 0, "unresolved": 0, "round_trips": 0, "model_seconds": 0.0});
    let recovery = |content: &Value| json!({"content": content, "replace": none, "from": none});
    let op = |acts: u64, refused: u64, seconds: f64| json!({"acts": acts, "refused": refused, "tool_seconds": seconds});
    let gate = |accepted: u64, seconds: f64, per: Value| json!({"accepted": accepted, "seconds": seconds, "per_accepted_seconds": per});

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
            // m1 here-documents into producer.go and, after a `cd`, store.go; m4 a refused
            // Write; m5 an Edit; m2 Bash share plus Read is native; m6 only Bash share and ready.
            "round_trips": trips((2, 2.5), (1, 1.5), (1, 1.5), (1, 0.75)),
            // Recovery counts Falinks edits only.
            "refusal_recovery": recovery(&none),
            "request_key_refusals": {},
            // t1 0.5, t2 0.25, t3 0.5, t4 0.5, t5 0.25, t6 0.5.
            "tool_seconds": 2.5,
            // 16 + 29 (here-documents) + 35 (refused Write) + 27 (Edit's new string).
            "output_bytes": 107,
            // From A's last share, not the transcript, which keys the here-document after the
            // `cd` as store.go and the Edit as /w/A/store/store.go: producer.go
            // "package fixture\n" matches its initial first line, 0; the new store/store.go
            // "package store\n" 14, "\n" 1, "func Get() int { return 1 }\n" 28.
            "changed_bytes": 43,
            "results": 2,
            "usage_reports": 1,
            // These Git records lack an initial snapshot and share times, so the run's
            // collisions are unavailable: every collision figure is missing, never zero.
            "collision_handling": null, "rereads": null, "coordination_overhead": null,
            // m4's Write was denied, not refused by the read-before-write rule.
            "integration": nothing, "runtime_refusals": nothing,
        })
    );
    assert_eq!(
        git["agents"]["B"],
        json!({"busy_seconds": 2.5, "launch_seconds": 2.5, "waiting_on_peer_seconds": 15.0,
            "model_seconds": 2.0, "round_trips": trips((0, 0.0), (0, 0.0), (0, 0.0), (1, 2.0)),
            "refusal_recovery": recovery(&none), "request_key_refusals": {}, "tool_seconds": 0.5, "output_bytes": 0,
            // B's own commits only: consumer.go "    return records\n" 19 and store/store.go
            // "func Put() {}\n" 14. The merge carries A's store/store.go lines, which stay A's.
            "changed_bytes": 33, "results": 1,
            "usage_reports": 1, "collision_handling": null, "rereads": null,
            "coordination_overhead": null, "integration": nothing, "runtime_refusals": nothing})
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
        json!({"count": 5, "model_seconds": 4.25})
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
            // 23 + 2; the refused edit is output but changes nothing. e1's `from` emits none.
            "output_bytes": 25,
            // From `history.json`, not the transcript, which never wrote e1's `from` scratch
            // file: producer.go's "// new\n" 7, consumer.go's "    return records\n" 19. The
            // Stale e2 changes nothing.
            "changed_bytes": 26,
            "results": 2,
            "usage_reports": 2,
            // e2's Stale names no conflict: no collision, nothing to handle or reread.
            "collision_handling": unhandled, "rereads": nothing,
            "integration": nothing, "runtime_refusals": nothing,
            // The refused e2 and falinks_ready; no inline review.
            "coordination_overhead": {"round_trips": 2, "model_seconds": 2.0},
            "review_estimate": {"characters": 0, "model_seconds": 0.0},
            // e1 2.5-3.0 accepted, e2 4.0-4.5 refused, y1 9.0-9.25.
            "operations": {"edit": op(2, 1, 1.0), "ready": op(1, 0, 0.25)},
            // Only e1 passed the gate.
            "edit_gate": gate(1, 0.5, json!(0.5)),
        })
    );
    assert_eq!(
        falinks["agents"]["agent-1"],
        json!({"busy_seconds": 3.0, "launch_seconds": 2.0, "waiting_on_peer_seconds": 5.0,
            // g1 3.0-2.0; g3 4.25-4.0; g2, text only, 5.0-4.75.
            "model_seconds": 1.5,
            // g3 is an accepted apply_job with a post: an edit.
            "round_trips": trips((1, 0.25), (0, 0.0), (1, 1.0), (0, 0.0)),
            "refusal_recovery": recovery(&none), "request_key_refusals": {},
            // b1 1.0, j1 0.25 and p1 0.5, in parallel with j1.
            "tool_seconds": 1.75, "output_bytes": 0,
            // j1's job rewrote agent-0's producer.go whole: "// new\n" is carried, agent-0's,
            // and only "// one\n" 7 is agent-1's.
            "changed_bytes": 7, "results": 1,
            "usage_reports": 1, "collision_handling": unhandled, "rereads": nothing,
            "coordination_overhead": nothing, "integration": nothing, "runtime_refusals": nothing,
            "review_estimate": {"characters": 0, "model_seconds": 0.0},
            "operations": {"apply_job": op(1, 0, 0.25), "post": op(1, 0, 0.5)},
            "edit_gate": gate(1, 0.25, json!(0.25))})
    );
}

/// Collision handling cost, rereads and coordination overhead (#337) on synthetic runs
/// (`tests/timing/*-collision`) whose records hold one line conflict each, checked against
/// values computed by hand. Times are seconds after 2026-10-21T10:00:00Z, the runs' start.
/// In both, the peer changes `consumer.go`'s return line first and the agent's change of the
/// same line collides with it.
#[test]
fn timing_reports_collision_handling_rereads_and_overhead() {
    let tmp = tmp("falinks-timing-collisions-");
    let copy = |name: &str| {
        let to = tmp.path().join(name);
        let copied = Command::new("cp")
            .arg("-R")
            .arg(root().join("tests/timing").join(name))
            .arg(&to)
            .status()
            .unwrap();
        assert!(copied.success());
        to
    };
    let figures = |run: &Path| {
        let output = cli([OsStr::new("timing"), run.as_os_str()]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let initial = read(root().join("fixtures/go-relationships.json"))["files"]["consumer.go"]
        .as_str()
        .unwrap()
        .to_string();
    let consume = |body: &str| {
        format!("package fixture\n\nfunc Consume(records []string) []string {{\n{body}}}\n")
    };
    let theirs = consume("    return records\n");
    let ours = consume("    return append(records[:0:0], records...)\n");
    let cost = |trips: u64, seconds: f64| json!({"round_trips": trips, "model_seconds": seconds, "unresolved": 0, "unlocated": 0});
    let idle = cost(0, 0.0);
    let unhandled = json!({"line_conflict": idle, "rework": idle, "no_fault": idle});

    // Falinks: agent-1's b1 (op 1) writes consumer.go version 1; agent-0's a1 (op 2) still
    // expects version 0 and is refused Stale; its a2 (op 3) edits version 1.
    let run = copy("falinks-collision");
    let file = |text: &str| json!({"bytes": text.as_bytes(), "executable": false});
    let version = |v: u64, text: &str| json!({"consumer.go": {"version": v, "file": file(text)}});
    let noted = consume("    // Consume returns its input.\n    return records\n");
    let history = json!([
        {"operation": 1, "agent": 1, "request": {"id": "b1", "expected": {"consumer.go": 0},
            "output": {"consumer.go": file(&theirs)}, "reviews": []},
         "before": {"revision": 0, "files": version(0, &initial)},
         "proposed": {"revision": 1, "files": version(1, &theirs)},
         "outcome": {"Applied": {"revision": 1}}, "incorporates": null},
        {"operation": 2, "agent": 0, "request": {"id": "a1", "expected": {"consumer.go": 0},
            "output": {"consumer.go": file(&ours)}, "reviews": []},
         "before": {"revision": 1, "files": {}}, "proposed": null,
         "outcome": {"Stale": {"conflicts": {"consumer.go": {"expected": 0,
            "current": {"version": 1, "file": file(&theirs)}}}}}, "incorporates": null},
        {"operation": 3, "agent": 0, "request": {"id": "a2", "expected": {"consumer.go": 1},
            "output": {"consumer.go": file(&noted)}, "reviews": []},
         "before": {"revision": 1, "files": version(1, &theirs)},
         "proposed": {"revision": 2, "files": version(2, &noted)},
         "outcome": {"Applied": {"revision": 2}}, "incorporates": null},
    ]);
    fs::write(run.join("controller/history.json"), history.to_string()).unwrap();
    let falinks = figures(&run);
    let agent_0 = &falinks["agents"]["agent-0"];
    // f2 1.5 (4.0-2.5), f3 0.5 and f4 0.75 after a1's refusal, to a2: the round-trips of
    // refusal recovery for `content`, the form a1 used.
    assert_eq!(
        agent_0["collision_handling"],
        json!({"line_conflict": cost(3, 2.75), "rework": idle, "no_fault": idle})
    );
    assert_eq!(
        agent_0["refusal_recovery"]["content"],
        json!({"refusals": 1, "unresolved": 0, "round_trips": 3, "model_seconds": 2.75})
    );
    // f2's Read of consumer.go, which a1's Stale names; f3 reads producer.go.
    assert_eq!(
        agent_0["rereads"],
        json!({"round_trips": 1, "model_seconds": 1.5})
    );
    // a2's reviews, `[{"decision":"Keep","note":"ok","revision":1,"scope":"s"}]`: 58
    // characters at 3.56 ms, 0.206 s.
    assert_eq!(
        agent_0["review_estimate"],
        json!({"characters": 58, "model_seconds": 0.206})
    );
    // Refused a1 1.0, ready 1.0 and the reread 1.5, plus the reviews' 0.20648.
    assert_eq!(
        agent_0["coordination_overhead"],
        json!({"round_trips": 3, "model_seconds": 3.706})
    );
    let agent_1 = &falinks["agents"]["agent-1"];
    assert_eq!(agent_1["collision_handling"], unhandled);
    assert_eq!(
        agent_1["rereads"],
        json!({"round_trips": 0, "model_seconds": 0.0})
    );
    // ready, 3.0-1.75.
    assert_eq!(
        agent_1["coordination_overhead"],
        json!({"round_trips": 1, "model_seconds": 1.25})
    );

    // Git: A shares a1 at 2.25 and B shares b1 at 3.0, each from the initial commit; A's
    // merge of b1 in agents/integration conflicts at 5.5.
    let run = copy("git-collision");
    let repo = run.join("agents/repo");
    fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let identity = [
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ];
        let out = Command::new("git")
            .args(identity)
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    let commit = |files: &[(&str, &str)], message: &str| {
        for (name, content) in files {
            fs::write(repo.join(name), content).unwrap();
        }
        git(&["add", "."]);
        git(&["commit", "-q", "-m", message]);
        git(&["rev-parse", "HEAD"])
    };
    git(&["init", "-q"]);
    let files = read(root().join("fixtures/go-relationships.json"))["files"].clone();
    let files: Vec<(&str, &str)> = files
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str().unwrap()))
        .collect();
    let base = commit(&files, "Frozen initial task");
    let a1 = commit(&[("consumer.go", &theirs)], "A draft 1");
    git(&["checkout", "-q", "-b", "B", &base]);
    let b1 = commit(&[("consumer.go", &ours)], "B draft 1");
    git(&["checkout", "-q", "-b", "integration", &a1]);
    git(&["merge", "-q", "--no-edit", "-s", "ours", &b1]);
    let merged = git(&["rev-parse", "HEAD"]);
    let events = [("A", &a1, 2.25), ("B", &b1, 3.0)]
        .map(|(agent, commit, elapsed)| {
            json!({"kind": "draft_shared", "agent": agent, "commit": commit, "elapsed": elapsed})
                .to_string()
        })
        .join("\n");
    fs::write(run.join("controller/events.jsonl"), events).unwrap();
    let mut result = read(run.join("controller/result.json"));
    result["initial_snapshot"] = json!(base);
    result["final_snapshot"] = json!(merged);
    fs::write(run.join("controller/result.json"), result.to_string()).unwrap();
    let git = figures(&run);
    let a = &git["agents"]["A"];
    // m3's merge (5.0-4.0) and m5's Edit under agents/integration (7.5-6.75), beside their
    // categories, native and edit.
    assert_eq!(
        a["integration"],
        json!({"round_trips": 2, "model_seconds": 1.75})
    );
    assert!(a["round_trips"].get("integration").is_none());
    assert_eq!(
        a["runtime_refusals"],
        json!({"round_trips": 0, "model_seconds": 0.0})
    );
    // From m4, the first input at the CONFLICT's 5.5, to m6's passing check: 1.0, 0.75, 0.25.
    assert_eq!(
        a["collision_handling"],
        json!({"line_conflict": cost(3, 2.0), "rework": idle, "no_fault": idle})
    );
    // m4's `cat consumer.go`.
    assert_eq!(
        a["rereads"],
        json!({"round_trips": 1, "model_seconds": 1.0})
    );
    // Coordination m2 0.25, m6 0.25, m7 0.5; integration 1.75; the reread 1.0.
    assert_eq!(
        a["coordination_overhead"],
        json!({"round_trips": 6, "model_seconds": 3.75})
    );
    assert!(a.get("review_estimate").is_none());
    let b = &git["agents"]["B"];
    assert_eq!(b["collision_handling"], unhandled);
    // Share 0.5 and ready 0.5.
    assert_eq!(
        b["coordination_overhead"],
        json!({"round_trips": 2, "model_seconds": 1.0})
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
            .env("FALINKS_FAKE_CHECKOUT", checkout(tmp.path()))
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
        assert_eq!(report["control_failure_runs"], 0);
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
        .env("FALINKS_FAKE_CHECKOUT", checkout(tmp.path()))
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

/// A milestone's `files` are what the Falinks arm enrolls hidden until delivery (#250): each is
/// named in that milestone's requirements, new to the fixture, and part of the reference.
#[test]
fn milestone_files_are_named_new_and_in_the_reference() {
    for entry in fs::read_dir(root().join("fixtures")).unwrap() {
        let path = entry.unwrap().path();
        let fixture = read(&path);
        let name = fixture["name"].as_str().unwrap();
        let protected = root().join(format!("protected/{name}.json"));
        for milestone in fixture["milestones"].as_array().unwrap() {
            let requirements = milestone["requirements"].to_string();
            for file in milestone["files"].as_array().into_iter().flatten() {
                let file = file.as_str().unwrap();
                assert!(
                    requirements.contains(file),
                    "{name}: {file} not in its requirements"
                );
                assert!(
                    fixture["files"].get(file).is_none(),
                    "{name}: {file} already exists"
                );
                assert!(
                    read(&protected)["reference"].get(file).is_some(),
                    "{name}: {file} not in the reference"
                );
            }
        }
    }
}

/// Commits `files` over a prepared fixture repository and returns the commit.
fn snapshot(fixture: &str, repo: &Path, files: &[(&str, &str)]) -> String {
    fs::create_dir_all(repo.parent().unwrap()).unwrap();
    prepare(fixture, repo);
    for (name, text) in files {
        fs::write(repo.join(name), text).unwrap();
    }
    if !files.is_empty() {
        commit(repo, "Final snapshot");
    }
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8(head.stdout).unwrap().trim().to_string()
}

/// #230, seam A: `quality` measures synthetic runs in both arms' storage layouts, and `report`
/// reads the results into `code_quality`. Every value is computed by hand on the formatted
/// trees. #232, seam B: with `--judge`, the fake runtime rates each measured snapshot.
#[test]
fn quality_measures_both_arms_and_report_shows_code_quality() {
    let tmp = tmp("falinks-quality-");
    let batch = tmp.path();
    let result = |run: &Path, value: Value| {
        fs::create_dir_all(run.join("controller")).unwrap();
        fs::write(run.join("controller/result.json"), value.to_string()).unwrap();
    };

    // Git arm, rust-errors. consumer.rs against the formatted initial: total's two-line chain
    // becomes three lines, and describe's `match` and its closing `}` become `return match` and
    // `};` (4 removed, 5 added). catalog.rs gains a blank production line and a 14-line inline
    // test module with two #[test] functions. Production: 4 + 5 + 1 = 10. clippy's
    // needless_return is on an added line and counts; the unused import on the fixture's
    // unchanged line 1 does not.
    let rust_git = batch.join("rust-errors-1-git");
    let catalog = "pub fn lookup(key: &str) -> Result<u32, String> {
    match key {
        \"apple\" => Ok(3),
        \"pear\" => Ok(7),
        \"offline\" => Err(\"unavailable\".into()),
        _ => Err(\"missing\".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::lookup;

    #[test]
    fn apple() {
        assert_eq!(lookup(\"apple\"), Ok(3));
    }

    #[test]
    fn pear() {
        assert_eq!(lookup(\"pear\"), Ok(7));
    }
}
";
    let consumer = "use crate::catalog::lookup;

pub fn total(keys: &[&str]) -> Result<u32, String> {
    keys.iter().try_fold(0, |sum, key| {
        crate::catalog::lookup(key).map(|value| sum + value)
    })
}

pub fn describe(key: &str) -> String {
    return match crate::catalog::lookup(key) {
        Ok(value) => format!(\"{key}={value}\"),
        Err(error) => format!(\"error:{error}\"),
    };
}
";
    let sha = snapshot(
        "rust-errors",
        &rust_git.join("agents/repo"),
        &[("src/catalog.rs", catalog), ("src/consumer.rs", consumer)],
    );
    result(
        &rust_git,
        json!({"arm": "git", "outcome": "success", "final_snapshot": sha}),
    );

    // Falinks arm, rust-errors: the final snapshot does not parse, so rustfmt fails.
    let rust_falinks = batch.join("rust-errors-1-falinks");
    let sha = snapshot(
        "rust-errors",
        &rust_falinks.join("controller/oracle/candidate-1"),
        &[("src/consumer.rs", "pub fn total(\n")],
    );
    result(
        &rust_falinks,
        json!({"arm": "falinks", "outcome": "success", "final_snapshot": sha}),
    );

    // Falinks arm, go-relationships, written unformatted. Formatted, producer.go replaces
    // `import "strings"` with a 4-line import block and appends a blank line and the 3-line
    // Label: 1 removed + 8 added = 9 production lines. The Sprintf on an added line is one
    // printf finding. producer_test.go adds 13 test lines and two Test functions. The final
    // snapshot is the second of two oracle candidates.
    let go_falinks = batch.join("go-relationships-1-falinks");
    let oracle = go_falinks.join("controller/oracle");
    snapshot("go-relationships", &oracle.join("candidate-1"), &[]);
    let producer = "package fixture

import (
    \"fmt\"
    \"strings\"
)

func Produce(lines []string) []string {
    out := make([]string, 0, len(lines))
    for _, line := range lines { out = append(out, strings.TrimSpace(line)) }
    return out
}

func Label(n int) string {
    return fmt.Sprintf(\"%s\", n)
}
";
    let test = "package fixture

import \"testing\"

func TestProduce(t *testing.T) {
	if got := Produce([]string{\" a \"}); got[0] != \"a\" {
		t.Fatal(got)
	}
}

func TestLabel(t *testing.T) {
	_ = Label(1)
}
";
    let sha = snapshot(
        "go-relationships",
        &oracle.join("candidate-2"),
        &[("producer.go", producer), ("producer_test.go", test)],
    );
    result(
        &go_falinks,
        json!({"arm": "falinks", "outcome": "success", "final_snapshot": sha}),
    );

    // Git arm, go-relationships: the final oracle failed, so the run is not measured.
    let go_git = batch.join("go-relationships-1-git");
    result(&go_git, json!({"arm": "git", "outcome": "task_failure"}));

    let entry = |run: &Path, fixture: &str, arm: &str| {
        json!({"run_dir": run, "run_id": run.file_name().unwrap().to_str().unwrap(),
            "pair": format!("{fixture}-1"), "fixture": fixture, "arm": arm, "scored": true})
    };
    fs::write(
        batch.join("batch.json"),
        json!({"runs": [
            entry(&rust_git, "rust-errors", "git"),
            entry(&rust_falinks, "rust-errors", "falinks"),
            entry(&go_falinks, "go-relationships", "falinks"),
            entry(&go_git, "go-relationships", "git"),
        ], "invocations": [{"commit": "f00d", "go": "go version go0.0 test/arch"}]})
        .to_string(),
    )
    .unwrap();

    // The judge's runtime: the scripted worker under the frozen pin. Calls run in ledger order,
    // three ratings then one guess per formatted snapshot. rust-errors-1-git: one unparseable
    // reply retried, and a correct guess. go-relationships-1-falinks: the first rating fails
    // twice, so its ratings are missing, and its guess is unparseable.
    let judge = tmp.path().join("judge");
    fs::create_dir(&judge).unwrap();
    let config = judge.join("config.json");
    fs::write(
        &config,
        json!({"runtime_version": "2.1.308", "model": "claude-opus-5-5",
            "runtime_binary": SCRIPTED, "runtime_sha256": hash(SCRIPTED)})
        .to_string(),
    )
    .unwrap();
    let rating = |r: [u64; 4]| {
        json!({"maintainability": {"rating": r[0], "reason": "a."},
            "duplication": {"rating": r[1], "reason": "b."},
            "unnecessary_changes": {"rating": r[2], "reason": "c."},
            "test_quality": {"rating": r[3], "reason": "d."}})
        .to_string()
    };
    fs::write(
        judge.join("replies.json"),
        json!([
            rating([4, 5, 3, 2]),
            "not json",
            rating([2, 5, 4, 2]),
            rating([3, 5, 4, 1]),
            "A",
            null,
            null,
            rating([3; 4]),
            rating([3; 4]),
            "maybe"
        ])
        .to_string(),
    )
    .unwrap();
    let measured = Command::new(CLI)
        .args(["quality", "--runs", batch.to_str().unwrap(), "--judge"])
        .arg(&config)
        .env("FALINKS_FAKE_JUDGE", &judge)
        .output()
        .unwrap();
    assert!(measured.status.success(), "{}", text(&measured));
    assert_eq!(read(judge.join("replies.json")), json!([]));
    let prompts = read(judge.join("prompts.json"));
    let prompts: Vec<&str> = prompts
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(prompts.len(), 10);
    // The material: no file names, no reference, no arm-identifying words. The requirement
    // text is the same for both arms and may name files (fixture 3 v2), so it is left out.
    let parse = reference("go-relationships")["parse.go"]
        .as_str()
        .unwrap()
        .to_string();
    let parse = parse.lines().find(|l| l.contains("func Parse")).unwrap();
    let fixture = read(root().join("fixtures/go-relationships.json"));
    let requirements: Vec<&str> = std::iter::once(&fixture["requirements"])
        .chain(
            fixture["milestones"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| &m["requirements"]),
        )
        .flat_map(|r| r.as_array().unwrap())
        .map(|r| r.as_str().unwrap())
        .collect();
    for prompt in [prompts[0], prompts[5]] {
        let lower = requirements
            .iter()
            .fold(prompt.to_string(), |p, r| p.replace(r, ""))
            .to_lowercase();
        for word in [
            ".go",
            ".rs",
            "falinks",
            "worktree",
            "git ",
            "workspace",
            parse,
        ] {
            assert!(!lower.contains(&word.to_lowercase()), "{word} in {prompt}");
        }
    }
    assert!(
        prompts[5].contains("+func Label(n int) string {"),
        "{}",
        prompts[5]
    );
    assert!(prompts[5].contains("func TestLabel"));
    for run in [&rust_git, &rust_falinks, &go_falinks, &go_git] {
        assert!(!run.join("quality.json").exists());
    }
    let quality = read(batch.join("quality.json"));
    // The real gates hold. The String errors still compile against the property suite: R2's
    // sums match the reference, while lookup's errors (R1), total's first error (R3) and
    // describe (R4) differ.
    assert_eq!(
        quality["property_gates"],
        json!({"go-relationships": "passed", "rust-errors": "passed"})
    );
    let rust = &quality["runs"]["rust-errors-1-git"];
    assert_eq!(
        rust["metrics"],
        json!({"failing_properties": {"value": 3, "properties": ["R1", "R3", "R4"]},
            "production_lines_changed": {"value": 10}, "test_lines_added": {"value": 14},
            "test_functions": {"value": 2},
            "clippy_findings": {"value": 1, "lints": {"clippy::needless_return": 1}}}),
        "{rust}"
    );
    for tool in ["go", "rustc", "clippy", "rustfmt"] {
        assert!(rust["versions"][tool].is_string());
    }
    assert!(rust["go_note"].as_str().unwrap().contains("go0.0"));
    assert_eq!(quality["measurement"]["batch_commits"], json!(["f00d"]));
    assert!(quality["measurement"]["commit"].is_string());
    // Produce still returns []string, so the suite does not compile: missing, with the error.
    let go = &quality["runs"]["go-relationships-1-falinks"];
    let mut go_metrics = go["metrics"].clone();
    let uncompiled = go_metrics["failing_properties"].take();
    let uncompiled = uncompiled["missing"].as_str().unwrap();
    assert!(
        uncompiled.starts_with("the property suite does not compile")
            && uncompiled.contains("Record"),
        "{uncompiled}"
    );
    assert_eq!(
        go_metrics,
        json!({"failing_properties": null, "production_lines_changed": {"value": 9},
            "test_lines_added": {"value": 13}, "test_functions": {"value": 2},
            "vet_findings": {"value": 1, "lints": {"printf": 1}}}),
        "{go}"
    );
    let rated = &rust["judge"];
    assert_eq!(
        rated["ratings"]["maintainability"],
        json!({"value": 3, "spread": [2, 4], "ratings": [4, 2, 3], "reasons": ["a.", "a.", "a."]})
    );
    assert_eq!(rated["guess"], json!({"arm": "git", "correct": true}));
    assert_eq!(rated["calls"].as_array().unwrap().len(), 5);
    assert_eq!(
        rated["calls"][0]["usage"],
        json!({"input_tokens": 5, "output_tokens": 7})
    );
    assert!(
        rated["calls"][1]["error"]
            .as_str()
            .unwrap()
            .starts_with("unparseable")
    );
    let unrated = &go["judge"];
    assert!(
        unrated["ratings"]["missing"]
            .as_str()
            .unwrap()
            .contains("after one retry")
    );
    assert!(
        unrated["guess"]["missing"]
            .as_str()
            .unwrap()
            .starts_with("unparseable guess")
    );
    assert!(quality["runs"]["rust-errors-1-falinks"]["judge"]["missing"].is_string());
    let broken = &quality["runs"]["rust-errors-1-falinks"]["metrics"];
    for metric in [
        "production_lines_changed",
        "clippy_findings",
        "failing_properties",
    ] {
        let reason = broken[metric]["missing"].as_str().unwrap();
        assert!(reason.starts_with("rustfmt failed"), "{reason}");
    }

    let output = cli(["report", "--runs", batch.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let rel = &report["fixtures"]["go-relationships"]["code_quality"];
    assert_eq!(
        rel["falinks"]["production_lines_changed"],
        json!({"values": [9.0], "median": 9.0, "range": [9.0, 9.0], "n": 1, "missing": 0,
            "missing_reasons": [null]})
    );
    assert_eq!(
        rel["git"]["test_functions"]["missing_reasons"],
        json!(["not measured: outcome task_failure"])
    );
    assert_eq!(
        rel["falinks"]["vet_findings"]["lints"],
        json!([{"printf": 1}])
    );
    assert_eq!(rel["labels"]["vet_findings"], "incomplete");
    assert!(rel["reference_production_lines_changed"]["value"].is_u64());
    // Recalibrated on fixture 3 v2 (#263), with maintainability dropped for go-relationships alone.
    assert_eq!(
        rel["judge"]["git"]["maintainability"],
        "Failed calibration; not reported."
    );
    assert_eq!(rel["judge"]["blinding"]["arm_guessed"], "arm guessed 0/0");
    assert_eq!(rel["judge"]["blinding"]["missing_guesses"], 1);
    let blinding = &report["fixtures"]["rust-errors"]["code_quality"]["judge"]["blinding"];
    assert_eq!(blinding["arm_guessed"], "arm guessed 1/1");
    let treatment = rel["treatment_properties"].to_string();
    assert!(treatment.contains("fixes the file set") && treatment.contains("rustfmt job"));
    let rust = &report["fixtures"]["rust-errors"]["code_quality"];
    assert_eq!(rust["git"]["clippy_findings"]["values"], json!([1.0]));
    assert_eq!(rust["git"]["failing_properties"]["values"], json!([3.0]));
    assert_eq!(
        rust["git"]["failing_properties"]["properties"],
        json!([["R1", "R3", "R4"]])
    );
    assert!(
        rust["falinks"]["clippy_findings"]["missing_reasons"][0]
            .as_str()
            .unwrap()
            .starts_with("rustfmt failed")
    );
    // Measuring again without --judge keeps the recorded ratings.
    let again = cli(["quality", "--runs", batch.to_str().unwrap()]);
    assert!(again.status.success(), "{}", text(&again));
    let kept = read(batch.join("quality.json"));
    assert_eq!(
        kept["runs"]["rust-errors-1-git"]["judge"],
        quality["runs"]["rust-errors-1-git"]["judge"]
    );
}

/// #231, #228 decision 13: when the reference fails a property, or a broken copy passes its
/// property, the fixture gets no property values, and every run's reason names the property
/// and the gate. The other metrics are still measured.
#[test]
fn property_gates_withhold_values_and_name_the_property() {
    let tmp = tmp("falinks-gates-");
    let batch = tmp.path().join("batch");
    let run = batch.join("rust-errors-1-git");
    let reference = reference("rust-errors");
    let files: Vec<(&str, &str)> = reference
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str().unwrap()))
        .collect();
    let sha = snapshot("rust-errors", &run.join("agents/repo"), &files);
    fs::create_dir_all(run.join("controller")).unwrap();
    fs::write(
        run.join("controller/result.json"),
        json!({"arm": "git", "outcome": "success", "final_snapshot": sha}).to_string(),
    )
    .unwrap();
    fs::write(
        batch.join("batch.json"),
        json!({"runs": [{"run_dir": run, "run_id": "rust-errors-1-git", "pair": "rust-errors-1",
            "fixture": "rust-errors", "arm": "git", "scored": true}], "invocations": []})
        .to_string(),
    )
    .unwrap();
    let real = root().join("protected/properties");
    let suite = fs::read_to_string(real.join("rust-errors.rs")).unwrap();
    let broken = fs::read_to_string(real.join("broken.json")).unwrap();

    // A suite one input short on every property: the reference fails R1 first. A no-op edit
    // (formatting removes the space) as R2's broken copy: it passes R2.
    for (suite, broken, reason) in [
        (
            suite.replace("INPUTS: usize = 500", "INPUTS: usize = 499"),
            broken.clone(),
            "property R1 failed the reference gate",
        ),
        (
            suite.clone(),
            broken.replace(r#""new": "try_fold(1,""#, r#""new": "try_fold(0 ,""#),
            "property R2 failed the broken-copy gate: its broken copy passes it",
        ),
    ] {
        let suites = tmp.path().join("suites");
        fs::create_dir_all(&suites).unwrap();
        fs::write(suites.join("rust-errors.rs"), suite).unwrap();
        fs::write(suites.join("broken.json"), broken).unwrap();
        let measured = Command::new(CLI)
            .args(["quality", "--runs", batch.to_str().unwrap()])
            .env("FALINKS_PROPERTY_SUITES", &suites)
            .output()
            .unwrap();
        assert!(measured.status.success(), "{}", text(&measured));
        let quality = read(batch.join("quality.json"));
        let gate = quality["property_gates"]["rust-errors"].as_str().unwrap();
        assert!(gate.starts_with(reason), "{gate}");
        let metrics = &quality["runs"]["rust-errors-1-git"]["metrics"];
        assert_eq!(metrics["failing_properties"], json!({"missing": gate}));
        assert_eq!(metrics["test_functions"], json!({"value": 0}));
        assert_eq!(
            quality["measurement"]["property_suites"],
            suites.to_str().unwrap()
        );
    }
}

/// #233: `known-answer` names each check a copy breaks, passes the frozen base, and refuses to
/// calibrate a subset with a calibrated fixture (#263).
#[test]
fn known_answer_names_broken_checks_and_refuses_a_calibrated_subset() {
    let dir = tmp("falinks-known-answer-test-");
    let mut set = read(root().join("protected/known-answer.json"));
    let copy = &mut set["go-page"]["copies"]["maintainability"];
    let new = (copy["old"].as_str().unwrap())
        .replace("err != nil || value < 0", "err != nil || value<=0");
    copy["new"] = new.into();
    let path = dir.path().join("known-answer.json");
    fs::write(&path, set.to_string()).unwrap();
    let checked = Command::new(CLI)
        .args(["known-answer", "--fixtures", "go-page"])
        .env("FALINKS_KNOWN_ANSWER", &path)
        .output()
        .unwrap();
    assert_eq!(checked.status.code(), Some(1), "{}", text(&checked));
    let checks: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let fixture = &checks["checks"]["go-page"];
    for label in [
        "reference",
        "duplication",
        "unnecessary_changes",
        "test_quality",
    ] {
        assert_eq!(fixture[label], json!([]), "{label}: {checks}");
    }
    let failed: Vec<&str> = fixture["maintainability"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    for expected in [
        "the formatter changes it",
        "failing_properties",
        "the oracle or its tests fail",
    ] {
        assert!(failed.iter().any(|f| f.starts_with(expected)), "{failed:?}");
    }
    let partial = cli([
        "known-answer",
        "--fixtures",
        "go-page",
        "--judge",
        "config.json",
    ]);
    assert!(!partial.status.success());
    assert!(
        text(&partial).contains("calibration needs every fixture"),
        "{}",
        text(&partial)
    );
}
