## Agent skills

### Learning

At session start, read `docs/agents/learning.md`. Follow its state/goal brief and explanation guidance during grilling, Wayfinder, and ordinary planning. After meaningful tickets, remind Daniel to invoke `$explain-diff-personal` and wait for explicit invocation. Complete the learning stop before resolving them or moving to the next issue.

### Issue tracker

Track issues and specs in GitHub Issues for `danielrispler/Falinks`. Before issue operations, read `docs/agents/issue-tracker.md`.

### Wayfinder

For Wayfinder sessions, run `python3 scripts/wayfinder_startup.py [map-number]` (default: current map). Follow the startup and ticket-context rules in `docs/agents/issue-tracker.md` when creating, starting, or updating tickets.

### Triage labels

Use the five default triage labels. Before triaging issues, read `docs/agents/triage-labels.md`.

### Domain docs

Use a single-context layout: root `CONTEXT.md` and `docs/adr/`. Before exploring the codebase, read `docs/agents/domain.md`.
