//! Run every CI check locally, in order; stop at the first failure and name it.
//! The evaluation tests run only when their inputs changed since `origin/main`,
//! as in CI; pass `--all` to run them anyway.
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

/// Files changed since the merge base with `origin/main`, committed or not, plus
/// untracked files. `None` when git cannot tell, so the caller runs everything.
fn changed_files(root: &Path) -> Option<Vec<String>> {
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let base = git(&["merge-base", "origin/main", "HEAD"])?;
    let diff = git(&["diff", "--name-only", base.trim()])?;
    let untracked = git(&["ls-files", "--others", "--exclude-standard"])?;
    Some(
        diff.lines()
            .chain(untracked.lines())
            .map(String::from)
            .collect(),
    )
}

fn evaluation_changed(changed: &[String]) -> bool {
    changed.iter().any(|file| {
        check_list::EVALUATION_INPUTS
            .iter()
            .any(|input| file.starts_with(input))
    })
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (tool, install) in [
        ("deny", "cargo install cargo-deny --locked"),
        ("nextest", "cargo install cargo-nextest --locked"),
    ] {
        let found = Command::new("cargo").args([tool, "--version"]).output();
        if !found.is_ok_and(|out| out.status.success()) {
            eprintln!("ci-checks: cargo-{tool} is missing; install it with:\n  {install}");
            return ExitCode::FAILURE;
        }
    }
    let all = std::env::args().any(|arg| arg == "--all");
    let run_evaluation = all || changed_files(&root).is_none_or(|c| evaluation_changed(&c));
    let probes = probe_manifests(&root);
    let checks: Vec<String> = check_list::CHECKS
        .iter()
        .filter(|check| run_evaluation || **check != check_list::EVALUATION_TESTS)
        .flat_map(|check| {
            if check.contains(PROBE_ARG) {
                probes.iter().map(|p| check.replace(PROBE_ARG, p)).collect()
            } else {
                vec![check.to_string()]
            }
        })
        .collect();
    if !run_evaluation {
        eprintln!(
            "ci-checks: evaluation inputs unchanged since origin/main; skipping its tests (--all runs them)"
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_evaluation_inputs_trigger_the_evaluation_tests() {
        let changed = |files: &[&str]| {
            evaluation_changed(&files.iter().map(|f| f.to_string()).collect::<Vec<_>>())
        };
        assert!(changed(&["src/lib.rs", "evaluation/src/main.rs"]));
        assert!(changed(&[".cargo/config.toml"]));
        assert!(changed(&["rust-toolchain.toml"]));
        assert!(!changed(&["src/lib.rs", ".github/workflows/checks.yml"]));
        assert!(!changed(&["evaluation.md"]));
        assert!(!changed(&[]));
    }
}
