## Agent compatibility

These docs are shared by Codex and Claude Code. `$skill` means `/skill` in Claude Code (plugin skills may appear as `/mattpocock-skills:<name>`). During session openings, planning, grilling, decision questions and explanations, `docs/agents/learning.md` overrides any terse or brevity output mode.

## Implementation language

The engine core is Rust only, along with its adapters, controlled hosts and repository automation. Falinks supports Rust and Go projects, so tests and fixtures may also be written in Go. Use Cargo for builds, checks and tool entry points; introduce no other language.

## Supported platforms

The current list, per component, is `docs/platforms.md`. Do not infer support from issue comments.

## Checks

Before handing off a change, run the local CI checks, cheapest first:

```sh
cargo run --bin ci-checks
```

It stops at the first failing step and names it. It skips actionlint unless a workflow changed since `origin/main`, and skips the evaluation tests, which run in CI; pass `-- --all` to force both. When it reports that evaluation inputs may have changed, merge the PR only after its evaluation checks pass: `gh pr checks <n> --watch --fail-fast`. Prerequisites: Go on PATH, `shellcheck` (when workflows change), `cargo-deny` (`cargo install cargo-deny --locked`) and `cargo-nextest` (`cargo install cargo-nextest --locked`). The list lives in `scripts/check_list.rs`; `tests/check_drift.rs` keeps it in step with `.github/workflows/`. The toolchain is pinned in `rust-toolchain.toml`.

## Evaluation freeze

The evaluation sources are hash-frozen in `evaluation/manifest.json`. Before you edit `evaluation/`, read "Freeze and setup budget" in `evaluation/README.md` for the re-freeze procedure.

## Agent skills

### Learning

Before planning, grilling or explanations, read `docs/agents/learning.md`. Follow its state/goal brief and explanation guidance during grilling, Wayfinder, and ordinary planning. After meaningful tickets, suggest `$dsh-explain-diff` in the final message. Explanations are optional and never block ticket resolution or progress; run the skill only when explicitly invoked.

### Issue tracker

Track issues and specs in GitHub Issues for `danielrispler/Falinks`. To read, list or claim issues, read `docs/agents/issue-tracker.md`. To create, edit or resolve issues, read `docs/agents/ticket-authoring.md`.

### Wayfinder

For Wayfinder sessions, run `cargo run --bin wayfinder-startup -- [map-number]`. The default is the newest open `wayfinder:map` issue; the live tickets are under #19, so pass `19`. Claim a ticket with `--claim <n>`. Follow the startup steps in `docs/agents/issue-tracker.md` and the ticket-context rules in `docs/agents/ticket-authoring.md`.

### Triage labels

Use the five default triage labels. Before triaging issues, read `docs/agents/triage-labels.md`.

### Domain docs

Use a single-context layout: root `CONTEXT.md` and `docs/adr/`. Before exploring the codebase, read `docs/agents/domain.md`. Settled decisions are indexed in a `wayfinder:map`'s Decisions-so-far or a spec's Implementation Decisions; read the linked resolution comment before proposing a design change.
