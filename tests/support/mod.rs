//! Shared setup for the root engine suites: fixtures, reopening, request builders and toolchain pins.
// Each suite compiles its own copy of this module and uses only part of it.
#![allow(dead_code)]

use falinks::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

/// Asserts `result` failed for `reason`, so an unrelated failure cannot pass.
#[track_caller]
pub fn fails_with<T>(result: Result<T>, reason: &str) {
    match result {
        Ok(_) => panic!("accepted, expected a refusal for {reason:?}"),
        Err(e) => assert!(
            e.to_string().contains(reason),
            "expected {reason:?}, got: {e}"
        ),
    }
}

/// Two independent Rust files.
pub const PAIR: &[(&str, &str)] = &[("a.rs", "fn a() {}\n"), ("b.rs", "fn b() {}\n")];
/// A Go module where `Right` calls `Callee` and `Left` stands alone.
pub const GO: &[(&str, &str)] = &[
    ("go.mod", "module example.com/probe\n\ngo 1.22\n"),
    (
        "siblings.go",
        "package probe\n\nfunc Left() int {\n\treturn 1\n}\n\nfunc Right() int {\n\treturn Callee()\n}\n",
    ),
    (
        "callee.go",
        "package probe\n\nfunc Callee() int {\n\treturn 7\n}\n",
    ),
];

/// Writes `files` under `<dir>/live`, enrolls them all and authenticates agents 0 and 1.
pub fn fixture(files: &[(&str, &str)]) -> (TempDir, Engine, Client, Client) {
    let dir = tempfile::tempdir().unwrap();
    for (path, bytes) in files {
        let path = dir.path().join("live").join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let (engine, alice, bob) = reopen(dir.path(), files);
    (dir, engine, alice, bob)
}
/// Opens the engine over a fixture root's `live` and `state` directories, enrolling `files`.
pub fn open(root: &Path, files: &[(&str, &str)]) -> Result<Engine> {
    let enrolled: Vec<_> = files.iter().map(|(p, _)| *p).collect();
    Engine::open(&root.join("live"), &root.join("state"), &enrolled)
}
/// Opens a fixture root and authenticates agents 0 and 1.
pub fn reopen(root: &Path, files: &[(&str, &str)]) -> (Engine, Client, Client) {
    let engine = open(root, files).unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let bob = engine.authenticate(1, &engine.credentials()[1]).unwrap();
    (engine, alice, bob)
}

/// Replaces one file, expecting the version `capture` saw.
pub fn edit(id: &str, capture: &Capture, path: &str, bytes: &str) -> Request {
    Request {
        id: id.into(),
        expected: BTreeMap::from([(path.into(), capture.files[path].version)]),
        output: BTreeMap::from([(
            path.into(),
            Some(File {
                bytes: bytes.as_bytes().to_vec(),
                executable: false,
            }),
        )]),
        reviews: Vec::new(),
    }
}

/// The engine's shared analysis caches under a fixture root, one per configured Go toolchain.
pub fn analysis_caches(root: &Path) -> Vec<PathBuf> {
    match fs::read_dir(root.join("state/analysis-cache")) {
        Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => panic!("{e}"),
    }
}
/// Every file beneath `dir`, recursively.
pub fn files_under(dir: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(dir).into_iter().flatten() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.insert(path);
        }
    }
    found
}

pub fn pin(path: PathBuf) -> Pin {
    assert!(path.is_file(), "missing pinned tool {}", path.display());
    Pin::from_path(path).unwrap()
}
/// gopls (`FALINKS_GOPLS`, else `$GOPATH/bin/gopls`) and the `go` on PATH's `GOROOT`.
/// Resolved and hashed once per test process.
pub fn go_toolchain() -> Toolchain {
    static GO_TOOLCHAIN: std::sync::OnceLock<Toolchain> = std::sync::OnceLock::new();
    GO_TOOLCHAIN.get_or_init(resolve_go_toolchain).clone()
}
fn resolve_go_toolchain() -> Toolchain {
    let env = |key: &str| {
        let out = Command::new("go").args(["env", key]).output().unwrap();
        PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
    };
    let gopls = std::env::var_os("FALINKS_GOPLS")
        .map(PathBuf::from)
        .unwrap_or_else(|| env("GOPATH").join("bin/gopls"));
    Toolchain {
        language: Language::Go,
        executables: vec![pin(gopls), pin(env("GOROOT").join("bin/go"))],
        features: Vec::new(),
    }
}
