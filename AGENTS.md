## Agent compatibility

These docs are shared by Codex and Claude Code. `$skill` means `/skill` in Claude Code (plugin skills may appear as `/mattpocock-skills:<name>`). During session openings, planning, grilling, decision questions and explanations, `docs/agents/learning.md` overrides any terse or brevity output mode.

## Implementation language

The engine core is Rust only, along with its adapters, controlled hosts and repository automation. Falinks supports Rust and Go projects, so tests and fixtures may also be written in Go. Use Cargo for builds, checks and tool entry points; introduce no other language.

## Checks

CI runs these commands (`cargo deny` through its GitHub action, on Linux); run them before handing off a change:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo fmt --manifest-path evaluation/Cargo.toml --check
cargo fetch --locked --manifest-path evaluation/Cargo.toml
cargo clippy --locked --offline --manifest-path evaluation/Cargo.toml --all-targets -- -D warnings
cargo test --locked --offline --manifest-path evaluation/Cargo.toml
cargo deny --locked check advisories bans
cargo deny --locked --manifest-path evaluation/Cargo.toml check advisories bans
```

The toolchain is pinned in `rust-toolchain.toml`. The evaluation tests need Go on PATH.

## Agent skills

### Learning

At session start, read `docs/agents/learning.md`. Follow its state/goal brief and explanation guidance during grilling, Wayfinder, and ordinary planning. After meaningful tickets, suggest `$dsh-explain-diff` in the final message. Explanations are optional and never block ticket resolution or progress; run the skill only when explicitly invoked.

### Issue tracker

Track issues and specs in GitHub Issues for `danielrispler/Falinks`. Before issue operations, read `docs/agents/issue-tracker.md`.

### Wayfinder

For Wayfinder sessions, run `cargo run --bin wayfinder-startup -- [map-number]` (default: current map). Follow the startup and ticket-context rules in `docs/agents/issue-tracker.md` when creating, starting, or updating tickets.

### Triage labels

Use the five default triage labels. Before triaging issues, read `docs/agents/triage-labels.md`.

### Domain docs

Use a single-context layout: root `CONTEXT.md` and `docs/adr/`. Before exploring the codebase, read `docs/agents/domain.md`. Settled decisions are indexed in the current map's Decisions-so-far; read the relevant linked resolution before proposing a design change.
