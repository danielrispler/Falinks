//! Keeps the local check list (`scripts/check_list.rs`, run by `ci-checks`) and the
//! cargo steps in `.github/workflows/*.yml` identical as sets.
//!
//! - `${{ matrix.* }}` arguments are ignored: they shard a check, not change it.
//! - Each `EmbarkStudios/cargo-deny-action` step counts as the `cargo deny` command
//!   its `with:` inputs describe.
//! - Non-check cargo steps are ignored: `cargo build` and `cargo run` only produce or
//!   start artifacts for later steps (for example the research probes in
//!   `sandbox-probes.yml`). Every other cargo subcommand in a workflow is a check
//!   and must be in the list.

use std::{collections::BTreeSet, fs, path::Path};

#[path = "../scripts/check_list.rs"]
mod check_list;

const NON_CHECK_SUBCOMMANDS: &[&str] = &["build", "run"];
const DENY_ACTION: &str = "EmbarkStudios/cargo-deny-action";

fn without_matrix_args(line: &str) -> String {
    let mut rest = line.to_owned();
    while let Some(start) = rest.find("${{ matrix.") {
        let end = rest[start..]
            .find("}}")
            .map_or(rest.len(), |e| start + e + 2);
        rest.replace_range(start..end, "");
    }
    rest.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

/// Builds the `cargo deny` command for the action step whose `uses:` key sits at
/// column `key_col`, from the lines that follow it.
fn deny_command(following: &[&str], key_col: usize) -> String {
    let input = |key: &str| {
        following
            .iter()
            .take_while(|l| l.trim().is_empty() || l.len() - l.trim_start().len() >= key_col)
            .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix(':').map(unquote))
            .unwrap_or("")
    };
    let manifest = match input("manifest-path") {
        "" => String::new(),
        path => format!("--manifest-path {path}"),
    };
    without_matrix_args(&format!(
        "cargo deny {} {manifest} {} {}",
        input("arguments"),
        input("command"),
        input("command-arguments"),
    ))
}

fn workflow_checks(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut checks = vec![];
    for (i, line) in lines.iter().enumerate() {
        let item = line.trim_start();
        let item = item.strip_prefix("- ").unwrap_or(item).trim_start();
        let key_col = line.len() - item.len();
        if let Some(action) = item.strip_prefix("uses:") {
            if unquote(action).starts_with(DENY_ACTION) {
                checks.push(deny_command(&lines[i + 1..], key_col));
            }
            continue;
        }
        let command = item.strip_prefix("run:").unwrap_or(item).trim();
        if let Some(args) = command.strip_prefix("cargo ") {
            let subcommand = args.split_whitespace().next().unwrap_or("");
            if !NON_CHECK_SUBCOMMANDS.contains(&subcommand) {
                checks.push(without_matrix_args(command));
            }
        }
    }
    checks
}

#[test]
fn check_list_matches_ci_workflows() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut ci = BTreeSet::new();
    for entry in fs::read_dir(&dir).expect("workflow directory") {
        let path = entry.expect("workflow entry").path();
        if path.extension().is_some_and(|e| e == "yml" || e == "yaml") {
            ci.extend(workflow_checks(
                &fs::read_to_string(&path).expect("workflow"),
            ));
        }
    }
    let local: BTreeSet<String> = check_list::CHECKS
        .iter()
        .map(|c| without_matrix_args(c))
        .collect();
    let only_ci: Vec<_> = ci.difference(&local).collect();
    let only_local: Vec<_> = local.difference(&ci).collect();
    assert!(
        only_ci.is_empty() && only_local.is_empty(),
        "check list drifted from CI\nonly in CI: {only_ci:#?}\nonly in scripts/check_list.rs: {only_local:#?}"
    );
}

#[test]
fn workflow_parsing_maps_deny_action_and_skips_non_checks() {
    let workflow = "\
jobs:
  a:
    steps:
      - run: cargo build --release --locked
      - run: cargo test --locked -- ${{ matrix.tests }}
      - name: script
        run: |
          cargo fmt --check
      - uses: EmbarkStudios/cargo-deny-action@v2
        with:
          command: check
          command-arguments: advisories bans
          arguments: --locked
          manifest-path: evaluation/Cargo.toml
      - uses: EmbarkStudios/cargo-deny-action@v2
        with:
          command: check
          arguments: --locked
";
    assert_eq!(
        workflow_checks(workflow),
        [
            "cargo test --locked --",
            "cargo fmt --check",
            "cargo deny --locked --manifest-path evaluation/Cargo.toml check advisories bans",
            "cargo deny --locked check",
        ]
    );
}
