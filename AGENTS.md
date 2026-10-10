## Implementation language

Falinks is 100% Rust. Write all executable project code in Rust, including the engine, adapters, controlled hosts, tests, verification probes, fixtures and repository automation. Use Cargo for builds, checks and tool entry points; implement helpers in Rust rather than introducing another language.

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

Use a single-context layout: root `CONTEXT.md` and `docs/adr/`. Before exploring the codebase, read `docs/agents/domain.md`.
