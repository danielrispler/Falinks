/// The canonical ordered check list that CI runs. `ci-checks` runs it locally and
/// `tests/check_drift.rs` keeps `.github/workflows/` in step with it.
/// Arguments are whitespace-separated; none may contain spaces.
pub const CHECKS: &[&str] = &[
    "cargo fmt --check",
    "cargo clippy --locked --all-targets -- -D warnings",
    "cargo test --locked",
    "cargo fmt --manifest-path evaluation/Cargo.toml --check",
    "cargo fetch --locked --manifest-path evaluation/Cargo.toml",
    "cargo clippy --locked --offline --manifest-path evaluation/Cargo.toml --all-targets -- -D warnings",
    "cargo test --locked --offline --manifest-path evaluation/Cargo.toml -- --test-threads=1",
    "cargo deny --locked check advisories bans",
    "cargo deny --locked --manifest-path evaluation/Cargo.toml check advisories bans",
];
