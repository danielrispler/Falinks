//! Run every CI check locally, in order; stop at the first failure and name it.
use std::{
    path::Path,
    process::{Command, ExitCode},
};

#[path = "check_list.rs"]
mod check_list;

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let deny = Command::new("cargo").args(["deny", "--version"]).output();
    if !deny.is_ok_and(|out| out.status.success()) {
        eprintln!(
            "ci-checks: cargo-deny is missing; install it with:\n  cargo install cargo-deny --locked"
        );
        return ExitCode::FAILURE;
    }
    let total = check_list::CHECKS.len();
    for (index, check) in check_list::CHECKS.iter().enumerate() {
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
