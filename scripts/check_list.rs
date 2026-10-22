/// The canonical check list that CI runs. `ci-checks` runs it locally, cheapest
/// checks first so formatting, lint and advisory failures surface before the slow
/// tests; `tests/check_drift.rs` keeps `.github/workflows/` in step with it as a set.
/// `ACTIONLINT` is the exception: CI runs it as a Docker image, so the drift test
/// compares only its version.
/// Arguments are whitespace-separated; none may contain spaces. A check with the
/// `"$probe"` argument runs once per standalone probe crate: each `adapters/*/Cargo.toml`
/// that declares its own `[workspace]`, as the `probes` job in `checks.yml` discovers them.
pub const CHECKS: &[&str] = &[
    "cargo fmt --check",
    "cargo fmt --manifest-path evaluation/Cargo.toml --check",
    "cargo fmt --manifest-path \"$probe\" --check",
    ACTIONLINT,
    // Before the offline evaluation clippy and before nextest, whose arm tests build
    // the evaluation CLI offline.
    "cargo fetch --locked --manifest-path evaluation/Cargo.toml",
    "cargo clippy --locked --all-targets -- -D warnings",
    "cargo clippy --locked --offline --manifest-path evaluation/Cargo.toml --all-targets -- -D warnings",
    "cargo clippy --locked --manifest-path \"$probe\" --all-targets -- -D warnings",
    "cargo deny --locked check advisories bans",
    "cargo deny --locked --manifest-path evaluation/Cargo.toml check advisories bans",
    "cargo nextest run --locked",
    EVALUATION_TESTS,
];

/// Lints `.github/workflows/`, and their `run:` steps through shellcheck when it is on
/// `PATH`. Its version matches the Docker image `checks.yml` runs; `tests/check_drift.rs`
/// keeps the two equal. `ci-checks` runs it only when a workflow changed.
pub const ACTIONLINT: &str = "go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.12";

/// The evaluation test step: slow, so CI runs it only on PRs that change one of
/// `EVALUATION_INPUTS`, and `ci-checks` only with `--all`.
pub const EVALUATION_TESTS: &str =
    "cargo test --locked --offline --manifest-path evaluation/Cargo.toml -- --test-threads=1";

/// Path prefixes that trigger the evaluation tests in CI; `tests/check_drift.rs`
/// keeps them equal to the `paths:` filter in `evaluation.yml`.
pub const EVALUATION_INPUTS: &[&str] = &[
    "evaluation/",
    "rust-toolchain.toml",
    ".cargo/",
    ".github/workflows/evaluation.yml",
];
