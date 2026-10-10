## Agent compatibility

These docs are shared by Codex and Claude Code. `$skill` means `/skill` in Claude Code (plugin skills may appear as `/mattpocock-skills:<name>`). During session openings, planning, grilling, decision questions and explanations, `docs/agents/learning.md` overrides any terse or brevity output mode.

## Agent skills

### Learning

At session start, read `docs/agents/learning.md`. Follow its state/goal brief and explanation guidance during grilling, Wayfinder, and ordinary planning. After meaningful tickets, suggest `$dsh-explain-diff` in the final message. Explanations are optional and never block ticket resolution or progress; run the skill only when explicitly invoked.

### Issue tracker

Track issues and specs in GitHub Issues for `danielrispler/Falinks`. Before issue operations, read `docs/agents/issue-tracker.md`.

### Wayfinder

For Wayfinder sessions, run `cargo run -q --bin wayfinder-startup -- [map-number]` (default: current map). Follow the startup and ticket-context rules in `docs/agents/issue-tracker.md` when creating, starting, or updating tickets.

### Triage labels

Use the five default triage labels. Before triaging issues, read `docs/agents/triage-labels.md`.

### Domain docs

Use a single-context layout: root `CONTEXT.md` and `docs/adr/`. Before exploring the codebase, read `docs/agents/domain.md`.
