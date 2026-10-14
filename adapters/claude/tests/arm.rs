//! The Falinks evaluation arm without a model: host-scripted tool envelopes on the frozen
//! `go-relationships` fixture drive drafts, milestones, checks, publication and readiness.
use falinks::Language;
use falinks::{Check, Engine};
use falinks_claude::arm::{
    Arm, Tools, agent_name, denied_roots, deny_check_reads, enrollment, language, worker_protected,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}
fn read(path: PathBuf) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
/// The evaluation crate is standalone; build its CLI for the oracle seam.
fn eval() -> PathBuf {
    let manifest = repo().join("evaluation/Cargo.toml");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--offline",
            "--quiet",
            "--bin",
            "falinks-eval",
            "--manifest-path",
        ])
        .arg(&manifest)
        .status()
        .unwrap();
    assert!(status.success());
    repo().join("evaluation/target/debug/falinks-eval")
}
fn go_env(key: &str) -> PathBuf {
    let out = Command::new("go").args(["env", key]).output().unwrap();
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

struct Run {
    arm: Arm,
    dir: tempfile::TempDir,
}
impl Run {
    fn call(&mut self, agent: usize, operation: &str, request: Value) -> Value {
        let root = self.arm.engine.root(self.arm.client(agent)).unwrap();
        let envelope = json!({"identity": {"agent": agent_name(agent), "workspace": root},
            "operation": operation, "request": request});
        self.arm.call(agent, &envelope).unwrap()
    }
    /// Replace one file with `content` at its current version.
    fn edit(&mut self, agent: usize, id: &str, path: &str, content: &str) -> Value {
        let capture = self.call(agent, "capture", json!({}));
        let version = &capture["files"][path]["version"];
        self.call(
            agent,
            "edit",
            json!({"id": id, "expected": {path: version}, "output": {path: {"content": content}}}),
        )
    }
    fn milestone(&mut self) -> String {
        let development = self.arm.outbox.remove(0);
        assert_eq!(development["type"], "development");
        development["id"].as_str().unwrap().to_string()
    }
}

fn setup(name: &str) -> Run {
    let dir = tempfile::Builder::new()
        .prefix("falinks-arm-")
        .tempdir_in("/private/tmp")
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    let eval = eval();
    let space = root.join("space0");
    let prepared = Command::new(&eval)
        .args(["prepare", name, "--output"])
        .arg(&space)
        .output()
        .unwrap();
    assert!(prepared.status.success(), "{prepared:?}");
    fs::remove_file(space.join(".gitignore")).unwrap();
    let fixture = read(repo().join(format!("evaluation/fixtures/{name}.json")));
    let language = language(fixture["language"].as_str().unwrap()).unwrap();
    let files: Vec<String> = fixture["files"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let enrolled = enrollment(&files, language);
    assert!(!enrolled.contains(&".gitignore".to_string()));
    let enrolled: Vec<&str> = enrolled.iter().map(String::as_str).collect();
    let engine = Engine::open(&space, &root.join("state"), &enrolled).unwrap();
    let gopls = std::env::var_os("FALINKS_GOPLS")
        .map(PathBuf::from)
        .unwrap_or_else(|| go_env("GOPATH").join("bin/gopls"));
    let go = go_env("GOROOT").join("bin/go");
    let tools = Tools {
        go: go.clone(),
        gopls,
        rust: Path::new(env!("CARGO")).parent().unwrap().to_owned(),
    };
    tools.configure(&engine, language).unwrap();
    // Legitimate checks pass under the runner's boundary: the whole run directory denied.
    let denied = deny_check_reads(&engine, &root, &[]).unwrap();
    let clients = [0, 1].map(|a| engine.authenticate(a, &engine.credentials()[a]).unwrap());
    let private = root.join("controller");
    fs::create_dir(&private).unwrap();
    let arm = Arm::new(
        Arc::new(engine),
        clients,
        go.with_file_name("gofmt"),
        fixture,
        eval,
        private,
        denied,
    );
    Run { arm, dir }
}

/// `claude-worker` (standalone evaluation crate) copies the adapter's turn cap.
#[test]
fn git_arm_bridge_uses_the_adapter_turn_cap() {
    let args = falinks_claude::launch_args(Path::new("/c"), "s", false);
    let cap = &args[args.iter().position(|a| a == "--max-turns").unwrap() + 1];
    let bridge = fs::read_to_string(repo().join("evaluation/src/bin/claude-worker.rs")).unwrap();
    assert!(bridge.contains(&format!("const MAX_TURNS: &str = \"{cap}\";")));
    assert!(language("python").is_err());
}

/// Both arms launch with the same isolation flags (evaluation/README.md "Paired evaluation").
#[test]
fn git_arm_bridge_launches_with_the_adapter_isolation_flags() {
    let args = falinks_claude::launch_args(Path::new("/c"), "s", false);
    let bridge = fs::read_to_string(repo().join("evaluation/src/bin/claude-worker.rs")).unwrap();
    let bridge: String = bridge.split_whitespace().collect();
    for flag in [
        "--setting-sources",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--permission-mode",
    ] {
        let at = args.iter().position(|a| a == flag).unwrap();
        // A flag's value follows it, unless the next argument is another flag.
        let literal = match args.get(at + 1).filter(|v| !v.starts_with("--")) {
            Some(value) => format!("\"{flag}\",\"{value}\","),
            None => format!("\"{flag}\","),
        };
        assert!(bridge.contains(&literal), "claude-worker lacks {literal}");
    }
}

#[test]
fn enrollment_adds_optional_test_files_and_skips_reserved_paths() {
    let files = ["go.mod", ".gitignore", "producer.go"].map(String::from);
    assert_eq!(
        enrollment(&files, Language::Go),
        ["go.mod", "producer.go", "producer_test.go"]
    );
    let files = ["Cargo.toml", "src/lib.rs", "src/catalog.rs"].map(String::from);
    assert_eq!(
        enrollment(&files, Language::Rust),
        [
            "Cargo.toml",
            "src/catalog.rs",
            "src/lib.rs",
            "tests/catalog.rs"
        ]
    );
}

/// Checks cannot read the run directory or any configured root, as the Git arm's cannot; the
/// slot stays readable (#80).
#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the evaluation sandbox is macOS-only"
)]
fn checks_are_denied_the_run_directory_and_every_resolved_root() {
    let tmp = tempfile::tempdir().unwrap();
    let run = tmp.path().join("run");
    let space = run.join("space0");
    fs::create_dir_all(&space).unwrap();
    fs::write(space.join("a.txt"), "a").unwrap();
    fs::create_dir(run.join("scratch")).unwrap();
    fs::write(run.join("scratch/other"), "OTHER").unwrap();
    let reference = tmp.path().join("reference");
    fs::create_dir(&reference).unwrap();
    fs::write(reference.join("oracle"), "SECRET").unwrap();
    // The configured checkout is a worktree (#105): the main checkout, its `.git` and every
    // other worktree are derived, not configured. A worktree whose directory is gone is skipped.
    let main = tmp.path().join("main");
    fs::create_dir_all(main.join("evaluation")).unwrap();
    fs::write(main.join("evaluation/manifest.json"), "{}").unwrap();
    fs::write(main.join("oracle"), "SECRET").unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .arg("-C")
            .arg(&main)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(status.success());
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    for name in ["checkout", "other", "gone"] {
        git(&["worktree", "add", "-q", "--detach", &format!("../{name}")]);
    }
    let [checkout, other] = ["checkout", "other"].map(|name| tmp.path().join(name));
    fs::remove_dir_all(tmp.path().join("gone")).unwrap();
    let evaluation = checkout.join("evaluation");

    let engine = Engine::open(&space, &run.join("controller/state"), &["a.txt"]).unwrap();
    // A misspelled, placeholder or malformed entry fails setup instead of failing open.
    assert!(
        denied_roots(
            &json!({"denied_roots": [reference, tmp.path().join("missing")],
            "evaluation_root": evaluation})
        )
        .is_err()
    );
    assert!(
        denied_roots(&json!({"denied_roots": reference, "evaluation_root": evaluation})).is_err()
    );
    // An evaluation root outside Git leaves nothing to derive, so it fails setup too.
    assert!(denied_roots(&json!({"denied_roots": [], "evaluation_root": reference})).is_err());
    let roots =
        denied_roots(&json!({"denied_roots": [reference], "evaluation_root": evaluation})).unwrap();
    let canonical = |p: &Path| p.canonicalize().unwrap();
    let derived = [&checkout, &main.join(".git"), &main, &checkout, &other].map(|p| canonical(p));
    assert_eq!(roots[0], canonical(&reference));
    assert_eq!(roots[1..], derived);
    let protected = worker_protected(&run, &roots);
    assert!(derived.iter().all(|root| protected.contains(root)));
    let denied = deny_check_reads(&engine, &run, &roots).unwrap();
    assert_eq!(denied[0], canonical(&run));
    assert_eq!(denied[1..], roots);

    let cat = |name: &str, path: &Path| Check {
        name: name.into(),
        program: "/bin/cat".into(),
        args: vec![path.to_str().unwrap().into()],
    };
    engine
        .configure_checks(vec![
            cat("slot", Path::new("a.txt")),
            cat("run", &run.join("scratch/other")),
            cat("reference", &reference.join("oracle")),
            cat("checkout", &checkout.join("oracle")),
            cat("git", &main.join(".git/HEAD")),
            cat("main", &main.join("oracle")),
            cat("other", &other.join("oracle")),
        ])
        .unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    engine.capture().unwrap();
    engine.feedback(&alice, "probe", 0).unwrap();
    let results = engine.validate().unwrap().unwrap().attempts[0]
        .results
        .clone();
    let passed: Vec<bool> = results.iter().map(|r| r.passed).collect();
    assert_eq!(
        passed,
        [true, false, false, false, false, false, false],
        "{results:?}"
    );
}

/// The host `check` runs the agents' tests in the oracle seam; they cannot read anything else
/// in the run, such as the peer's controller material (#80).
#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the evaluation sandbox is macOS-only"
)]
fn the_host_check_denies_agent_tests_the_rest_of_the_run() {
    let mut run = setup("go-relationships");
    let peer = run
        .dir
        .path()
        .canonicalize()
        .unwrap()
        .join("agent-1/controller");
    fs::create_dir_all(&peer).unwrap();
    fs::write(peer.join("settings.json"), "SECRET").unwrap();
    let mut files = read(repo().join("evaluation/protected/go-relationships.json"))["reference"]
        .as_object()
        .unwrap()
        .clone();
    let probe = format!(
        "package fixture\n\nimport (\n\t\"os\"\n\t\"testing\"\n)\n\nfunc TestPeerIsUnreadable(t *testing.T) {{\n\tif _, err := os.ReadFile({:?}); err == nil {{\n\t\tt.Fatal(\"peer controller material readable\")\n\t}}\n}}\n",
        peer.join("settings.json").to_str().unwrap()
    );
    files.insert("producer_test.go".into(), json!(probe));
    let capture = run.call(0, "capture", json!({}));
    let mut expected = serde_json::Map::new();
    let mut output = serde_json::Map::new();
    for (path, text) in &files {
        expected.insert(path.clone(), capture["files"][path]["version"].clone());
        output.insert(path.clone(), json!({"content": text}));
    }
    let edit = run.call(
        0,
        "edit",
        json!({"id": "probe", "expected": expected, "output": output}),
    );
    assert_eq!(edit["accepted"], true, "{edit}");
    let check = run.call(0, "check", json!({}));
    assert_eq!(check["passed"], true, "{check}");
}

/// The engine runs the Git arm's visible Rust commands with the pinned toolchain in its sandbox.
#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the evaluation sandbox is macOS-only"
)]
fn rust_visible_checks_pass_the_reference_under_the_engine() {
    let mut run = setup("rust-errors");
    let reference = read(repo().join("evaluation/protected/rust-errors.json"))["reference"].clone();
    let capture = run.call(0, "capture", json!({}));
    let mut expected = serde_json::Map::new();
    let mut output = serde_json::Map::new();
    for (path, text) in reference.as_object().unwrap() {
        expected.insert(path.clone(), capture["files"][path]["version"].clone());
        output.insert(path.clone(), json!({"content": text}));
    }
    let edit = run.call(
        0,
        "edit",
        json!({"id": "r", "expected": expected, "output": output}),
    );
    assert_eq!(edit["accepted"], true, "{edit}");
    let revision = run.call(0, "capture", json!({}))["revision"]
        .as_u64()
        .unwrap();
    let client = run.arm.client(0).clone();
    run.arm
        .engine
        .feedback(&client, "visible", revision)
        .unwrap();
    let checked = run.arm.engine.validate().unwrap().unwrap();
    assert_eq!(
        checked.outcome,
        Some(falinks::RunOutcome::Passed),
        "{checked:?}"
    );
    assert_eq!(run.call(0, "check", json!({}))["passed"], true);
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the evaluation sandbox is macOS-only"
)]
fn milestones_checks_publication_and_readiness_decide_the_outcome() {
    let mut run = setup("go-relationships");
    let reference =
        read(repo().join("evaluation/protected/go-relationships.json"))["reference"].clone();
    let text = |file: &str| reference[file].as_str().unwrap().to_string();
    let original = |run: &mut Run, file: &str| {
        run.call(0, "capture", json!({"paths": [file]}))["files"][file]["content"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // A non-compiling draft from one agent is not yet a trigger.
    let draft = original(&mut run, "producer.go") + "\nfunc Broken( {\n";
    assert_eq!(run.edit(0, "a1", "producer.go", &draft)["accepted"], true);
    assert!(run.arm.outbox.is_empty());
    assert_eq!(run.call(0, "ready", json!({}))["accepted"], false);
    let draft = original(&mut run, "consumer.go") + "\n// consumer draft\n";
    assert_eq!(run.edit(1, "b1", "consumer.go", &draft)["accepted"], true);
    let first = run.milestone();
    assert_eq!(first, "record-contract");

    assert_eq!(
        run.call(0, "ack", json!({"milestone": "follow-ups"}))["accepted"],
        false
    );
    for agent in [0, 1] {
        assert_eq!(
            run.call(agent, "ack", json!({"milestone": first}))["accepted"],
            true
        );
    }
    run.edit(
        0,
        "a2",
        "producer.go",
        &(text("producer.go") + "\n// contract draft\n"),
    );
    assert!(
        run.arm.outbox.is_empty(),
        "one agent's new draft is not a trigger"
    );
    run.edit(1, "b2", "consumer.go", &text("consumer.go"));
    let second = run.milestone();
    for agent in [0, 1] {
        run.call(agent, "ack", json!({"milestone": second}));
    }
    run.edit(0, "a3", "producer.go", &text("producer.go"));

    // Checks return pass/fail only and are retained as host evidence.
    let check = run.call(1, "check", json!({}));
    assert_eq!(check["passed"], true, "{check}");
    assert!(check.get("oracle").is_none());
    assert_eq!(
        run.call(0, "ready", json!({}))["accepted"],
        false,
        "unpublished work"
    );

    let revision = run.call(0, "capture", json!({}))["revision"].clone();
    for agent in [0, 1] {
        let offer = run.call(
            agent,
            "offer",
            json!({"id": "final", "task": "relationships", "revision": revision, "scope": [], "text": "done"}),
        );
        assert_eq!(offer["accepted"], true, "{offer}");
    }
    while run.arm.engine.validate().unwrap().is_some() {}
    assert_eq!(
        run.arm.engine.published().unwrap().revision,
        revision.as_u64().unwrap()
    );

    assert_eq!(run.call(0, "ready", json!({}))["accepted"], true);
    assert_eq!(
        run.edit(0, "a4", "producer.go", "package feed\n")["accepted"],
        false
    );
    assert!(run.arm.outcome.is_none());
    assert_eq!(run.call(1, "ready", json!({}))["accepted"], true);
    let outcome = run.arm.outcome.clone().unwrap();
    assert_eq!(outcome["outcome"], "success", "{outcome}");
    assert_eq!(outcome["oracle"]["passed"], true);
    assert_eq!(run.arm.validation_attempts.len(), 2);
}
