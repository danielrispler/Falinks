//! Run every CI check locally, cheapest first; stop at the first failure and name it.
//! The evaluation tests and actionlint run only when their inputs changed since
//! `origin/main`; pass `--all` to run them anyway.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::Instant,
};

#[path = "check_list.rs"]
mod check_list;

const PROBE_ARG: &str = "\"$probe\"";
/// Path prefixes that trigger `check_list::ACTIONLINT`.
const WORKFLOW_INPUTS: &[&str] = &[".github/workflows/"];

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
/// Renames list both paths, so moving a file out of `evaluation/` still counts.
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
    let diff = git(&["diff", "--no-renames", "--name-only", base.trim()])?;
    let untracked = git(&["ls-files", "--others", "--exclude-standard"])?;
    Some(
        diff.lines()
            .chain(untracked.lines())
            .map(String::from)
            .collect(),
    )
}

fn changed_under(changed: &[String], inputs: &[&str]) -> bool {
    changed
        .iter()
        .any(|file| inputs.iter().any(|input| file.starts_with(input)))
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
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
    // The engine suite unwraps `go env` and pins gopls, so fail here, not after the build.
    let go_path = Command::new("go").args(["env", "GOPATH"]).output();
    let Some(go_path) = go_path.ok().filter(|out| out.status.success()) else {
        eprintln!("ci-checks: Go is missing from PATH; install the version in docs/platforms.md");
        return ExitCode::FAILURE;
    };
    let gopls = std::env::var_os("FALINKS_GOPLS").map_or_else(
        || PathBuf::from(String::from_utf8_lossy(&go_path.stdout).trim()).join("bin/gopls"),
        PathBuf::from,
    );
    if !gopls.is_file() {
        eprintln!(
            "ci-checks: gopls is missing at {}; install the version in CONTRIBUTING.md",
            gopls.display()
        );
        return ExitCode::FAILURE;
    }
    let all = std::env::args().any(|arg| arg == "--all");
    let changed = changed_files(&root);
    let run_for =
        |inputs: &[&str]| all || changed.as_ref().is_none_or(|c| changed_under(c, inputs));
    let run_evaluation = run_for(check_list::EVALUATION_INPUTS);
    let run_actionlint = run_for(WORKFLOW_INPUTS);
    // actionlint skips shellcheck silently when it is missing; CI's image bundles it.
    if run_actionlint
        && !Command::new("shellcheck")
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    {
        eprintln!(
            "ci-checks: shellcheck is missing from PATH; install it (brew install shellcheck)"
        );
        return ExitCode::FAILURE;
    }
    let probes = probe_manifests(&root);
    let checks: Vec<String> = check_list::CHECKS
        .iter()
        .filter(|check| run_evaluation || **check != check_list::EVALUATION_TESTS)
        .filter(|check| run_actionlint || **check != check_list::ACTIONLINT)
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
    if !run_actionlint {
        eprintln!(
            "ci-checks: workflows unchanged since origin/main; skipping actionlint (--all runs it)"
        );
    }
    let total = checks.len();
    let started = Instant::now();
    let mut times = vec![];
    for (index, check) in checks.iter().enumerate() {
        let step = index + 1;
        eprintln!("ci-checks [{step}/{total}]: {check}");
        let mut args = check.split_whitespace();
        let step_started = Instant::now();
        let status = Command::new(args.next().expect("non-empty check"))
            .args(args)
            .current_dir(&root)
            .status();
        times.push((step_started.elapsed().as_secs_f64(), check));
        if !status.is_ok_and(|s| s.success()) {
            eprintln!("ci-checks: step {step}/{total} failed: {check}");
            return ExitCode::FAILURE;
        }
    }
    for (seconds, check) in times {
        eprintln!("ci-checks: {seconds:>6.1}s  {check}");
    }
    let seconds = started.elapsed().as_secs_f64();
    eprintln!("ci-checks: all {total} checks passed in {seconds:.1}s");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_evaluation_inputs_trigger_the_evaluation_tests() {
        let changed = |files: &[&str]| {
            changed_under(
                &files.iter().map(|f| f.to_string()).collect::<Vec<_>>(),
                check_list::EVALUATION_INPUTS,
            )
        };
        assert!(changed(&["src/lib.rs", "evaluation/src/main.rs"]));
        assert!(changed(&[".cargo/config.toml"]));
        assert!(changed(&["rust-toolchain.toml"]));
        assert!(!changed(&["src/lib.rs", ".github/workflows/checks.yml"]));
        assert!(!changed(&["evaluation.md"]));
        assert!(!changed(&[]));
    }

    #[test]
    fn the_package_has_no_dependencies_outside_its_tests() {
        let sections: Vec<&str> = include_str!("Cargo.toml")
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with('[') && line.contains("dependencies"))
            .collect();
        assert_eq!(sections, ["[dev-dependencies]"]);
    }

    #[test]
    fn moving_a_file_out_of_evaluation_counts_as_an_evaluation_change() {
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(repo.path())
                .output()
                .unwrap()
                .status;
            assert!(status.success(), "git {args:?}");
        };
        fs::create_dir(repo.path().join("evaluation")).unwrap();
        fs::write(repo.path().join("evaluation/notes.txt"), "same content\n").unwrap();
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(&["mv", "evaluation/notes.txt", "notes.txt"]);
        let changed = changed_files(repo.path()).unwrap();
        assert!(
            changed.contains(&"evaluation/notes.txt".to_string()),
            "{changed:?}"
        );
        assert!(changed_under(&changed, check_list::EVALUATION_INPUTS));
    }
}
