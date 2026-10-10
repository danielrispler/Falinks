//! The Falinks evaluation arm without a model: host-scripted tool envelopes on the frozen
//! `go-relationships` fixture drive drafts, milestones, checks, publication and readiness.
use falinks::Engine;
use falinks::Language;
use falinks_claude::arm::{Arm, Tools, agent_name, enrollment, language};
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
    _dir: tempfile::TempDir,
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
        vec![],
    );
    Run { arm, _dir: dir }
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

/// The engine runs the Git arm's visible Rust commands with the pinned toolchain in its sandbox.
#[test]
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
