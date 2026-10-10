/// The canonical ordered check list that CI runs. `ci-checks` runs it locally and
/// `tests/check_drift.rs` keeps `.github/workflows/` in step with it.
/// Arguments are whitespace-separated; none may contain spaces. A check with the
/// `"$probe"` argument runs once per standalone probe crate: each `adapters/*/Cargo.toml`
/// that declares its own `[workspace]`, as the `probes` job in `checks.yml` discovers them.
pub const CHECKS: &[&str] = &[
    "cargo fmt --check",
    "cargo clippy --locked --all-targets -- -D warnings",
    "cargo test --locked",
    "cargo fmt --manifest-path evaluation/Cargo.toml --check",
    "cargo fetch --locked --manifest-path evaluation/Cargo.toml",
    "cargo clippy --locked --offline --manifest-path evaluation/Cargo.toml --all-targets -- -D warnings",
    "cargo test --locked --offline --manifest-path evaluation/Cargo.toml -- --test-threads=1",
    "cargo fmt --manifest-path \"$probe\" --check",
    "cargo clippy --locked --manifest-path \"$probe\" --all-targets -- -D warnings",
    "cargo deny --locked check advisories bans",
    "cargo deny --locked --manifest-path evaluation/Cargo.toml check advisories bans",
];
