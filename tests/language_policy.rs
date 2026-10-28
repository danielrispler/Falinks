//! Enforces the implementation-language rule (map #1): Rust implements Falinks;
//! Go appears only as supported-language fixture/test code under `evaluation/`.
//! The studio (`apps/studio/`) is outside the rule (`apps/studio/AGENTS.md`).

use std::{fs, path::Path, process::Command};

const DATA_EXTENSIONS: &[&str] = &["md", "json", "jsonl", "toml", "lock", "yml", "yaml", "svg"];
const DATA_NAMES: &[&str] = &[".gitignore", "CODEOWNERS", "LICENSE-MIT", "LICENSE-APACHE"];

fn violation(path: &str) -> Option<&'static str> {
    if path.starts_with("apps/studio/") {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.').map(|(_, e)| e);
    let allowed = DATA_NAMES.contains(&name)
        || matches!(extension, Some(e) if e == "rs" || DATA_EXTENSIONS.contains(&e))
        || (extension == Some("go") && path.starts_with("evaluation/"));
    if !allowed {
        return Some("not Rust, supported-language fixture code, or data/docs");
    }
    // Inner attributes (`#![...]`) are Rust, not shebangs.
    let bytes = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).ok()?;
    (bytes.starts_with(b"#!") && !bytes.starts_with(b"#![")).then_some("shebang script")
}

#[test]
fn tracked_files_follow_the_language_policy() {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git is on PATH");
    assert!(output.status.success(), "git ls-files failed");
    let listing = String::from_utf8(output.stdout).expect("UTF-8 paths");
    let violations: Vec<String> = listing
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter_map(|path| violation(path).map(|why| format!("{path}: {why}")))
        .collect();
    assert!(violations.is_empty(), "{violations:#?}");
}
