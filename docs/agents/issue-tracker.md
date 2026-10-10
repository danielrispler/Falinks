# Issue tracker: GitHub

Issues and specs for `danielrispler/Falinks` live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. For multi-line bodies, write the text to a temporary file and pass `--body-file <path>` instead of `--body`.
- **Read an issue**: `gh issue view <number> --json title,url,body,labels,assignees`. Fetch comments when needed; for a linked resolution, use `gh api repos/danielrispler/Falinks/issues/comments/<comment-id> --jq .body`.
- **List issues**: `gh issue list --state open --limit 100 --json number,title,url,labels,assignees` with appropriate `--label` and `--state` filters. Load individual bodies and comments on demand.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Infer the repo from `git remote -v`; `gh` does this automatically when run inside a clone.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag.)_

When set to `yes`, PRs run through the same labels and states as issues, using the `gh pr` equivalents:

- **Read a PR**: `gh pr view <number> --comments` and `gh pr diff <number>` for the diff.
- **List external PRs for triage**: `gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments` then keep only `authorAssociation` of `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, or `NONE` (drop `OWNER`/`MEMBER`/`COLLABORATOR`).
- **Comment / label / close**: `gh pr comment`, `gh pr edit --add-label`/`--remove-label`, `gh pr close`.

GitHub shares one number space across issues and PRs, so a bare `#42` may be either: resolve with `gh pr view 42` and fall back to `gh issue view 42`.

## When a skill says "publish to the issue tracker"

Before publishing a labelled issue, run the preflight in `docs/agents/triage-labels.md`. Create a GitHub issue.

### Repository-specific `to-spec` confirmation

Synthesize the complete draft independently from the settled conversation and repo evidence. Before publication, confirm the proposed testing seams with one focused question unless they were already explicitly agreed in the current conversation. Show the proposed checks and their limits, and wait for the answer before publishing. This confirmation concerns testing seams; preserve the settled design rather than reopening a design interview. The installed skill remains unchanged.

## When a skill says "fetch the relevant ticket"

Use the **Read an issue** convention above; include relevant comments when the task depends on them.

Read relevant linked decision resolutions directly before loading full issue histories, including when synthesizing a completed map. Expand into issue bodies or comments only to resolve identifiable gaps or supersession; follow replacement resolution links when present.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single issue with **child** issues as tickets.

### Session startup

1. Run `cargo run --bin wayfinder-startup -- [map-number]`; omit the number for the current collaborative-editing map. It reads the full live map once and paginated open-child metadata in map order, including claims, open blockers and frontier eligibility. This repo uses native GitHub sub-issues and dependencies; unavailable blocker metadata is an error.
2. Use the named ticket, or the first row with `frontier: true`. Refresh eligibility if tracker state may have changed, then claim before work. The command is read-only; GitHub remains authoritative.
3. Load that ticket's body, its reading links and required skills once. Expand to other decisions or evidence when the question needs them; check for superseded decisions. Use the configured `gh` route directly. Read triage guidance when triaging.
4. Open the session with the state and goal brief described in `docs/agents/learning.md`; follow its explanation guidance when planning or asking decision questions.

### Ticket context

Resolve meaningful tickets when their agreed work and verification are complete. Follow `docs/agents/learning.md` for the optional explanation suggestion; explanation and readiness confirmation are not closure prerequisites.

Every new Wayfinder ticket must include `## Reading` before linking it to the map. List relevant current decision resolutions and research artifacts by name, with one short reason per link. Use comment permalinks for specific resolutions. When no prior material applies, write `None — no prior decisions or research required.` Reading links are a starting set, not a limit on investigation; preserve the ticket's question, scope and stop condition.

Before starting an existing ticket, add or refresh its reading section. When resolving, reopening or changing a decision, refresh reading links on affected open tickets and on tickets entering the frontier; replace superseded pointers without copying the decision detail. Ticket creation and resolution are complete only after these context updates. Backfill the currently eligible tickets first; other existing tickets are updated as their prerequisites settle or before work begins.

### Tracker operations

- **Map**: a single issue labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body. `gh issue create --label wayfinder:map`.
- **Child ticket**: an issue linked to the map as a GitHub sub-issue (`gh api` on the sub-issues endpoint). Where sub-issues aren't enabled, add the child to a task list in the map body and put `Part of #<map>` at the top of the child body. Labels: `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed, the ticket is assigned to the driving dev.
- **Blocking**: GitHub's **native issue dependencies**, the canonical, UI-visible representation. Add an edge with `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`, where `<blocker-db-id>` is the blocker's numeric **database id** (`gh api repos/<owner>/<repo>/issues/<n> --jq .id`, _not_ the `#number` or `node_id`). GitHub reports `issue_dependencies_summary.blocked_by` (open blockers only, the live gate). Where dependencies aren't available, fall back to a `Blocked by: #<n>, #<n>` line at the top of the child body. A ticket is unblocked when every blocker is closed.
- **Frontier query**: `cargo run --bin wayfinder-startup -- <map-number>`. Rows with `frontier: true` are open, unassigned and have no open blockers; first in map order wins. Where native relationships aren't available, apply the task-list and `Blocked by` conventions above manually.
- **Claim**: `gh issue edit <n> --add-assignee @me`, the session's first write.
- **Resolve**: `gh issue comment <n> --body "<answer>"`, then `gh issue close <n>`, then append a context pointer (gist + link) to the map's Decisions-so-far.
