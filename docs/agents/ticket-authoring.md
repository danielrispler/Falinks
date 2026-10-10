# Issue tracker: ticket authoring

Read this file before you create, edit or resolve an issue. For lookups, claims and Wayfinder startup, read `docs/agents/issue-tracker.md`.

## Write commands

- **Create an issue**: `gh issue create --title "..." --body "..."`. For multi-line bodies, write the text to a temporary file and pass `--body-file <path>`.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply or remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

## When a skill says "publish to the issue tracker"

Before you publish a labelled issue, run the preflight in `docs/agents/triage-labels.md`. Then create a GitHub issue.

### Repository-specific `to-spec` confirmation

Synthesize the complete draft independently from the settled conversation and repo evidence. Before publication, confirm the proposed testing seams with one focused question unless they were already explicitly agreed in the current conversation. Show the proposed checks and their limits, and wait for the answer before publishing. This confirmation concerns testing seams; preserve the settled design rather than reopening a design interview. The installed skill remains unchanged.

## Spec and ticket constraints

- **Evaluation freeze.** The evaluation sources are hash-frozen in `evaluation/manifest.json`. A spec or ticket that changes frozen evaluation material must say so and plan the versioned re-freeze. The procedure is in the "Freeze and setup budget" section of `evaluation/README.md`.
- **Nested agents.** A ticket that launches nested agents (`claude -p`, Codex) needs a session outside auto mode. Auto mode blocks these runs. Say so in the ticket, and raise it with the user before you start the work.

## Ticket context

Resolve meaningful tickets when their agreed work and verification are complete. Follow `docs/agents/learning.md` for the optional explanation suggestion; explanation and readiness confirmation are not closure prerequisites.

Every new Wayfinder ticket must include `## Reading` before linking it to the map. List relevant current decision resolutions and research artifacts by name, with one short reason per link. Use comment permalinks for specific resolutions. When no prior material applies, write `None — no prior decisions or research required.` Reading links are a starting set, not a limit on investigation; preserve the ticket's question, scope and stop condition.

Before starting an existing ticket, add or refresh its reading section. When resolving, reopening or changing a decision, refresh reading links on affected open tickets and on tickets entering the frontier; replace superseded pointers without copying the decision detail. Ticket creation and resolution are complete only after these context updates. Backfill the currently eligible tickets first; other existing tickets are updated as their prerequisites settle or before work begins.

## Wayfinder tracker operations

- **Map**: a single issue labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body. `gh issue create --label wayfinder:map`.
- **Child ticket**: an issue linked to the map as a GitHub sub-issue (`gh api` on the sub-issues endpoint). Where sub-issues aren't enabled, add the child to a task list in the map body and put `Part of #<map>` at the top of the child body. Labels: `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed, the ticket is assigned to the driving dev.
- **Blocking**: GitHub's **native issue dependencies**, the canonical, UI-visible representation. Add an edge with `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`, where `<blocker-db-id>` is the blocker's numeric **database id** (`gh api repos/<owner>/<repo>/issues/<n> --jq .id`, _not_ the `#number` or `node_id`). GitHub reports `issue_dependencies_summary.blocked_by` (open blockers only, the live gate). Where dependencies aren't available, fall back to a `Blocked by: #<n>, #<n>` line at the top of the child body. A ticket is unblocked when every blocker is closed.
- **Resolve**: `gh issue comment <n> --body "<answer>"`, then `gh issue close <n>`, then append a context pointer (gist + link) to the map's Decisions-so-far.
