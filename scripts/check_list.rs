/// The canonical ordered check list that CI runs. `ci-checks` runs it locally and
/// `tests/check_drift.rs` keeps `.github/workflows/` in step with it.
/// Arguments are whitespace-separated; none may contain spaces. A check with the
/// `"$probe"` argument runs once per standalone probe crate: each `adapters/*/Cargo.toml`
/// that declares its own `[workspace]`, as the `probes` job in `checks.yml` discovers them.
pub const CHECKS: &[&str] = &[
    "cargo fmt --check",
    "cargo clippy --locked --all-targets -- -D warnings",
    "cargo fetch --locked --manifest-path evaluation/Cargo.toml",
    "cargo nextest run --locked",
    "cargo fmt --manifest-path evaluation/Cargo.toml --check",
    "cargo clippy --locked --offline --manifest-path evaluation/Cargo.toml --all-targets -- -D warnings",
    EVALUATION_TESTS,
    "cargo fmt --manifest-path \"$probe\" --check",
    "cargo clippy --locked --manifest-path \"$probe\" --all-targets -- -D warnings",
    "cargo deny --locked check advisories bans",
    "cargo deny --locked --manifest-path evaluation/Cargo.toml check advisories bans",
];

/// The evaluation test step: slow, so `ci-checks` runs it only when one of
/// `EVALUATION_INPUTS` changed, as the `paths:` filter in `evaluation.yml` does.
pub const EVALUATION_TESTS: &str =
    "cargo test --locked --offline --manifest-path evaluation/Cargo.toml -- --test-threads=1";

/// Path prefixes that trigger the evaluation tests; `tests/check_drift.rs` keeps
/// them equal to the `paths:` filter in `evaluation.yml`.
pub const EVALUATION_INPUTS: &[&str] = &[
    "evaluation/",
    "rust-toolchain.toml",
    ".cargo/",
    ".github/workflows/evaluation.yml",
];
