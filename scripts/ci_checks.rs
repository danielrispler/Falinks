//! Run every CI check locally, in order; stop at the first failure and name it.
use std::{
    fs,
    path::Path,
    process::{Command, ExitCode},
};

#[path = "check_list.rs"]
mod check_list;

const PROBE_ARG: &str = "\"$probe\"";

/// Standalone probe crates, as the `probes` CI job finds them: every
/// `adapters/*/Cargo.toml` that declares its own `[workspace]`.
fn probe_manifests(root: &Path) -> Vec<String> {
    let mut manifests: Vec<String> = fs::read_dir(root.join("adapters"))
        .expect("adapters directory")
        .map(|entry| entry.expect("adapters entry").file_name())
        .map(|name| format!("adapters/{}/Cargo.toml", name.to_string_lossy()))
        .filter(|manifest| {
            fs::read_to_string(root.join(manifest))
                .is_ok_and(|text| text.lines().any(|line| line.trim() == "[workspace]"))
        })
        .collect();
    manifests.sort();
    manifests
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let deny = Command::new("cargo").args(["deny", "--version"]).output();
    if !deny.is_ok_and(|out| out.status.success()) {
        eprintln!(
            "ci-checks: cargo-deny is missing; install it with:\n  cargo install cargo-deny --locked"
        );
        return ExitCode::FAILURE;
    }
    let probes = probe_manifests(&root);
    let checks: Vec<String> = check_list::CHECKS
        .iter()
        .flat_map(|check| {
            if check.contains(PROBE_ARG) {
                probes.iter().map(|p| check.replace(PROBE_ARG, p)).collect()
            } else {
                vec![check.to_string()]
            }
        })
        .collect();
    let total = checks.len();
    for (index, check) in checks.iter().enumerate() {
        let step = index + 1;
        eprintln!("ci-checks [{step}/{total}]: {check}");
        let mut args = check.split_whitespace();
        let status = Command::new(args.next().expect("non-empty check"))
            .args(args)
            .current_dir(&root)
            .status();
        if !status.is_ok_and(|s| s.success()) {
            eprintln!("ci-checks: step {step}/{total} failed: {check}");
            return ExitCode::FAILURE;
        }
    }
    eprintln!("ci-checks: all {total} checks passed");
    ExitCode::SUCCESS
}
