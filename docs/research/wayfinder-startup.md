# Falinks Wayfinder startup

## Agreed constraints

- Keep Matt Pocock's Wayfinder skill unchanged. Improve its use in Falinks.
- Reduce unnecessary startup time and context without weakening decision quality.
- Judge improvement against observed startup behavior; there is no fixed time or token target.
- Add a small repo command and a short startup instruction. Fetch live tracker state; do not introduce a local cache.
- Give tickets explicit links to relevant decisions and research. These are starting points, with further reading allowed when needed.
- Backfill the two eligible tickets first. Require reading sections on every new ticket and update existing tickets as they enter the frontier or before work starts. Refresh affected links when decisions change.

## Observed baseline

One Falinks session on 2026-10-02 took 102 seconds from the user prompt to research delegation. Five startup tool-output batches delivered 90,000 text characters. Recorded per-request input grew from 21,717 to 42,831 tokens; this is context growth, not cumulative billed usage. This single sample does not establish the reported 30% across sessions.

The largest batch delivered 40,148 characters from bootstrap reads and broad tool discovery. It enumerated up to 100 tool descriptions even though `docs/agents/issue-tracker.md` specifies `gh`. Startup also reread the supplied Wayfinder skill and loaded triage guidance during planning. Other batches included relevant decision evidence; the session did not load every child ticket's body.

The [collaborative editing map](https://github.com/danielrispler/Falinks/issues/1) measured 11,891 characters when inspected. Its existing Notes already request short sessions and historical evidence on demand. The repo's issue-list example nevertheless requests bodies and comments, and its frontier procedure has no compact runnable query.

Baseline source: local session `rollout-2026-10-02T21-04-23-01a0fdc9-eaf0-77d1-968f-8c99f6658e89.jsonl`, inspected for Falinks startup events and aggregate counts only.

## Implementation

- One read-only command returns the full map once and its open children in map order, with titles, links, assignees and open-blocker counts. It does not claim or resolve tickets.
- A short repo instruction points sessions to that command. Use `gh` directly, read required skills once, and load triage guidance only for triage.
- Load the chosen ticket after selecting it; claim before working as Wayfinder requires. Read linked resolutions and artifacts as needed, checking for reopened or superseded decisions.
- Keep canonical decision detail in ticket resolutions. Do not copy full decisions into another local summary or change the map's destination.

The startup command is `cargo run --bin wayfinder-startup -- [map-number]`; `--check` runs its offline selection checks. The standing startup and ticket-context rules live in `docs/agents/issue-tracker.md`, reached through `AGENTS.md`.

Reading sections were added to [Specify live-workspace notification handling and revalidation](https://github.com/danielrispler/Falinks/issues/17) and [Probe captured Rust/Go analysis and owning-symbol correspondence](https://github.com/danielrispler/Falinks/issues/18). Their questions, scope and stop conditions were preserved.

## Evaluation

Compare fresh-session startup time and added context with the baseline. Also check that the session selects an eligible ticket, respects existing decisions, and retrieves additional evidence when needed. Moving reading to another agent alone does not demonstrate total token savings.

Offline checks cover pagination order, closed/claimed/blocked tickets, an empty frontier and refusal to treat unknown blockers as eligible. Live verification returned six open tickets and the expected two eligible tickets in map order, with 13,204 output characters and no child bodies or comments. This measures the helper's map/metadata output, not the complete next session's context or decision quality.

The baseline is historical evidence, not a second source of tracker state. Fresh-session time and context savings remain to be measured.
