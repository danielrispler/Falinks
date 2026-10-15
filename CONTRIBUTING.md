# Contributing to Falinks

Thanks for helping. Falinks is small and opinionated, so read this first. It saves both of us time.

## Before you start

- Open or comment on an issue before larger changes. Settled decisions live in `docs/adr/` and the current Wayfinder map (#19). Read the relevant one before proposing a design change.
- Learn the vocabulary in [CONTEXT.md](CONTEXT.md). Code, docs and issues use those exact terms.
- Follow [CODING_STANDARDS.md](CODING_STANDARDS.md).

## Language

The engine, adapters, hosts and repository automation are Rust only. Tests and fixtures may also use Go, because Falinks coordinates Rust and Go projects. Do not add another language.

## Setup

You need:

- The Rust toolchain pinned in `rust-toolchain.toml` (rustup installs it automatically).
- Git 2.32 or newer.
- Go on `PATH`, and `gopls` v0.22.0 (`go install golang.org/x/tools/gopls@v0.22.0`).
- `cargo-deny`: `cargo install cargo-deny --locked`.
- `cargo-nextest`: `cargo install cargo-nextest --locked`.
- A [supported platform](docs/platforms.md).

## Run the checks

Run every CI check locally, cheapest first, before you open a pull request:

```sh
cargo run --bin ci-checks
```

It stops at the first failing step and names it. The slow evaluation tests run only when `evaluation/`, `rust-toolchain.toml` or `.cargo/` changed since `origin/main`, as in CI; `cargo run --bin ci-checks -- --all` runs them anyway. The list lives in `scripts/check_list.rs`, and `tests/check_drift.rs` keeps it in step with `.github/workflows/`. If you add a workflow step, add it to the list too.

To rerun one engine test, filter by name: `cargo nextest run <name>`. For one evaluation test: `cargo test --locked --offline --manifest-path evaluation/Cargo.toml -- --test-threads=1 <name>`.

## Evaluation sources

`evaluation/` is hash-frozen in `evaluation/manifest.json`. Read "Freeze and setup budget" in [evaluation/README.md](evaluation/README.md) before editing anything there.

## Pull requests

- One topic per pull request. Link the issue it closes.
- Update docs when behavior, vocabulary or platform support changes.
- Never claim platform support without verifying it. See [docs/platforms.md](docs/platforms.md).

## Issues and labels

Bugs and features use the issue forms. New issues get `needs-triage`. A maintainer then moves them to `ready-for-agent`, `ready-for-human` or `needs-info`.

The live Claude Code adapter check on Linux (`adapter-linux.yml`) spends the maintainer's Claude subscription, so pull requests don't run it. It runs daily at 01:00 UTC when `main` has new commits, and when the maintainer starts it by hand. Agents follow "Live adapter checks" in [docs/agents/ticket-authoring.md](docs/agents/ticket-authoring.md).

## License

By contributing, you agree that your contributions are dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the user's option, without additional terms.
