# Studio research: data sources a control room can read (#282)

**Answer.** Without handling any credential, a local app can read seven sources: Claude Code hooks (pushed the moment something happens), `claude agents --json` (polled session state, including "waiting for approval"), the status line, OpenTelemetry (pushed in batches every 5 or 60 seconds, off by default), session transcripts (complete but in an internal format), Git, and GitHub through `gh`. They all fit one shape: a timestamped event or snapshot, tagged with a session ID and a folder. Falinks adds meaning: plans, refused edits with reasons, required reviews, checks tied to an exact version, and overlaps. Its records already carry Claude's session ID, but today they are reachable only inside the Falinks host process and in files written at the end of a run.<br>
**Evidence: moderate.** What each source carries comes from official docs and the repo's code. Speeds are documented figures, not measurements. The pilot files show only one Claude Code version (2.1.308).<br>
**Hypotheses.** Supports "Evidence, not promises" (fully with Falinks, partly without it) and "Move learning into waiting time" (waiting moments and individual changes can be seen live). Gives a warning for "A game that rewards understanding, not volume": the ready-made metrics all count volume.<br>
**For Daniel.** Claude Code's [agent view page](https://code.claude.com/docs/en/agent-view#monitor-sessions-with-agent-view), from "Monitor sessions with agent view" through "Read session state from a script" (15 min). The [hooks guide, "Get notified when Claude needs input"](https://code.claude.com/docs/en/hooks-guide#get-notified-when-claude-needs-input), with the [Notification timing table](https://code.claude.com/docs/en/hooks#notification) (10 min).

---

## How to read this report

Read any section on its own. Each one opens with its point in one line.

- A claim with a link or a `path:line` is **established** by that source. Repo code is cited at commit `d8f5700`.
- A sentence that starts with **Inference:** is my own reasoning. No source states it.
- Pilot observations come from the unscored Track P pilots in `local-evidence/2026-10-23-track-p-pilots-248/`. That folder exists only on Daniel's machine. I checked that every `result.json` in it says `"scored": false`. I sampled file structure and record types only. I did not open the per-agent `token`, `settings.json` or `mcp.json` files, and I did not open the contents of attachments with names such as `credential_org` or `environment`.

Terms used throughout:

- **Push** means the source tells the app when something happens. **Poll** means the app has to ask again and again.
- **JSONL** is a file with one JSON object per line. New lines are added at the end, so a reader can follow the file as it grows.
- **Documented** means the vendor describes the format publicly and keeps it stable. **Internal** means the format can change in any release.

## What "without credentials" means here

The app never holds a token. It runs CLIs as Daniel and reads files that the agent tools write for scripts.

`gh` and `claude` use their own stored logins when Daniel runs them. The app only reads their output. Hooks and the status line are scripts that Claude Code runs and feeds JSON on standard input ([hooks](https://code.claude.com/docs/en/hooks#common-input-fields), [status line](https://code.claude.com/docs/en/statusline#available-data)). OpenTelemetry can send to a collector on `localhost` with no authentication header ([configuration](https://code.claude.com/docs/en/monitoring-usage#common-configuration-variables)). **Inference:** no source in this report needs the app to read a credential store. Two places put credential-like material close to useful data, and the app has to step around both: the Falinks controller directory (see "Falinks today") and the `~/.claude` folder, which holds both transcripts and settings.

## The signal map

Most sources show activity and the moment of waiting. None names the phase or says "stuck". Only Falinks binds a check to an exact version.

| Source | Phase | Progressing or stuck | Waiting for approval | What changed | Checked or refused | How fast | Documented? |
|---|---|---|---|---|---|---|---|
| Hooks | Inferred from tools and tasks | Activity per tool call | Yes: `PermissionRequest` at once, `Notification` after about 6 s | File paths and inputs of edit tools | Tool failures, auto-mode denials | Pushed at the event | Yes |
| `claude agents --json` | No | `busy`, `waiting`, `idle`; `working`, `blocked`, `done`, `failed` | Yes: `waitingFor` | No | `failed` only | Poll | Yes, "the supported way" |
| Status line | No | Cost and time going up | No (it hides while a permission prompt is open) | Total lines added and removed | No | Pushed per assistant message, 300 ms debounce, interactive only | Yes |
| OpenTelemetry | No | Event rate, API errors | Only after the decision (`tool_decision`) | `lines_of_code` count | `tool_result` success or failure, `tool_decision` reject | Batched: events every 5 s, metrics every 60 s | Yes |
| Transcripts | Inferred | Everything, with a short lag | Inferred | Full tool inputs and outputs | Permission decisions, sandbox fields | File grows as the session runs | Location yes, format no |
| Git | No | Commits and dirty files | No | Exact diffs | No | Poll or folder watching | Yes (porcelain formats) |
| `gh` | No | Check states | Review state | PR diff | CI checks per commit | Poll (10 s default in `--watch`) | Yes |
| Falinks | Yes: plan, edit, review, offer, check, published, ready | Refusals, retries, blocked proposals, halts | Only agent-to-agent (reviews owed); nothing asks Daniel | Every edit with author, note, exact before and after | Required checks bound to revision and tree; every refusal with its reason | Recorded at each operation; outside the host only at the end of a run today | Repo docs; no stable external format |

The sections below give the source for each cell.

## Claude Code: hooks

Hooks are the fastest documented source. They push one JSON object per lifecycle event, and they have a cost: by default Claude Code waits for them.

A **hook** is a command, HTTP endpoint, MCP tool or prompt that Claude Code runs at a named point in its life cycle ([hooks reference](https://code.claude.com/docs/en/hooks)). The reference lists more than 30 events. The ones that matter here:

- **Session life:** `SessionStart` (its `source` is `startup`, `resume`, `clear`, `compact` or `fork`), `SessionEnd`, `Stop` (Claude finished a response), `StopFailure` (the turn ended on an API error), and `SubagentStart` and `SubagentStop`.
- **Work:** `PreToolUse`, `PostToolUse`, `PostToolUseFailure` and `PostToolBatch`. `PostToolUse` carries `tool_name`, `tool_input`, `tool_response`, `tool_use_id` and an optional `duration_ms`. `PostToolUseFailure` adds `error` and `is_interrupt`.
- **Plans and tasks:** `TaskCreated` and `TaskCompleted` carry `task_subject` and `task_description`. Also `WorktreeCreate`, `WorktreeRemove`, `PreCompact`, `PostCompact` and `FileChanged`.
- **Approval:** `PermissionRequest`, `PermissionDenied` and `Notification`. These are covered in the next section.

Every hook receives the same base fields: `session_id`, `transcript_path`, `cwd`, `permission_mode`, `hook_event_name` and `prompt_id`. Inside a subagent it also gets `agent_id` and `agent_type` ([common input fields](https://code.claude.com/docs/en/hooks#common-input-fields)).

How fast: hooks run synchronously by default. Claude Code waits for them, though matching hooks run in parallel. A command hook can set `"async": true` to run in the background. An `http` hook POSTs the same JSON to a URL such as a local server ([execution model](https://code.claude.com/docs/en/hooks)). **Inference:** an observer should use async command hooks or a fast local HTTP endpoint, so that watching never slows the agent.

Where they come from: hooks are set up in settings files or in a plugin's `hooks/hooks.json` ([hook locations](https://code.claude.com/docs/en/hooks)). **Inference:** shipping the observer as a plugin avoids editing Daniel's own `settings.json`. That matters because settings files can also hold secrets.

Private data: `tool_input` holds whole file contents for Write, full Bash commands and edited text. `last_assistant_message` holds the model's prose.

## Claude Code: the approval moment

There are three documented live signals for "Claude is waiting for Daniel". Their timing differs by seconds, which matters for a control room.

- **`PermissionRequest`** fires when Claude Code is about to ask for permission. It carries `tool_name`, `tool_input` and `permission_suggestions`, and it can even answer the request ([PermissionRequest](https://code.claude.com/docs/en/hooks#permissionrequest)). The reference itself recommends it for running a hook "immediately when Claude asks for permission" ([Notification note](https://code.claude.com/docs/en/hooks#notification)).
- **`Notification`** with the type `permission_prompt` fires only after the prompt has waited about six seconds, and each keystroke delays it. `idle_prompt` fires about 60 seconds after Claude finished responding, if Daniel has not typed since. Other types include `elicitation_dialog`, plus `agent_needs_input` and `agent_completed` for background sessions while agent view is open ([Notification table](https://code.claude.com/docs/en/hooks#notification)). The input carries `message`, `title` and `notification_type`.
- **`PermissionDenied`** fires when auto mode denies a call. Its `reason` field holds the denial reason ([hooks reference](https://code.claude.com/docs/en/hooks)).

In headless runs, denials show up as `permission_denied` system messages and in the final `permission_denials` list ([headless](https://code.claude.com/docs/en/headless#turn-off-permission-prompts-in-unattended-runs)). The Agent SDK's `canUseTool` callback pauses the agent until the host answers ([user input](https://code.claude.com/docs/en/agent-sdk/user-input)). That callback belongs to whichever program launched the session, so an outside observer cannot use it.

**Inference:** for the problem "I miss the moment an agent is waiting for my approval", `PermissionRequest` gives the moment itself, and `Notification` gives a gentler "you seem away" signal. The six-second and 60-second delays are a ready-made calm default, in the sense of the alarm-fatigue research in `problems.md`.

## Claude Code: `claude agents --json`

This is the only documented way to see every session's state from outside, without any setup. It is a poll.

`claude agents --json` prints active sessions as a JSON array and exits: every live session, plus background sessions that are still working or blocked even after their process has exited. Add `--all` to include finished ones ([list sessions as JSON](https://code.claude.com/docs/en/agent-view#list-sessions-as-json)). The fields:

- `cwd`, `kind` (`interactive` or `background`) and `startedAt`, always.
- `pid` and `status` (`busy`, `waiting` or `idle`), while the process is alive.
- `waitingFor`, when the status is `waiting`. It is `permission prompt`, `input needed`, `sandbox request`, `worker request` or `dialog open`.
- `state` (`working`, `blocked`, `done`, `failed` or `stopped`), for background sessions only.
- `sessionId` and `name`, when set.

The docs call this "the supported way to read session state from outside Claude Code". They also say the files under `~/.claude/jobs/` are "not a stable interface" ([read session state from a script](https://code.claude.com/docs/en/agent-view#read-session-state-from-a-script)). Agent view's own one-line row summaries, written by a Haiku-class model, are not in the JSON field list ([row summaries](https://code.claude.com/docs/en/agent-view#row-summaries)).

**Inference:** `waitingFor` and `state` answer "which session needs me" across all of Daniel's terminals with one command and no hooks installed. That makes this the cheapest source for a first version. It cannot say what changed or what was checked.

## Claude Code: status line

The status line pushes a rich snapshot to a script, but only in an interactive terminal, and it goes quiet when the session is idle.

The script receives `session_id`, `session_name`, `prompt_id`, `transcript_path`, `cwd`, the `workspace` (including `git_worktree` and the repository identity), `model`, the `cost` object (`total_cost_usd`, `total_duration_ms`, `total_api_duration_ms`, `total_lines_added`, `total_lines_removed`), `context_window`, `rate_limits`, `pr` (number, URL, `review_state`) and `worktree` ([available data](https://code.claude.com/docs/en/statusline#available-data)). A separate `subagentStatusLine` receives one `tasks` entry per subagent, with `status` (`running`, `completed`, `failed`, `killed`), `label`, `tokenCount` and the last 16 token samples ([subagent status lines](https://code.claude.com/docs/en/statusline#subagent-status-lines)).

It updates when a new assistant message arrives, when `/compact` finishes, when the permission mode changes, and on a few other triggers. Updates are debounced at 300 ms. The triggers "can go quiet when the main session is idle", and a `refreshInterval` of 1 second or more can re-run the script on a timer ([when it updates](https://code.claude.com/docs/en/statusline#when-the-status-line-updates)). The status line hides while a permission prompt is on screen ([how status lines work](https://code.claude.com/docs/en/statusline#how-status-lines-work)).

**Inference:** a status-line script can also append each snapshot to a file the app reads. That is a cheap progress feed, carrying cost, time, lines and context fill, with no tool contents in it, so it is the least private source. The token samples per subagent are a raw signal for "still progressing". Headless (`-p`) sessions have no terminal UI, so they would not produce it. The docs do not say this outright.

## Claude Code: OpenTelemetry

OpenTelemetry is documented telemetry. It is batched, off by default, and it hides content by default, but it always carries identity.

**OpenTelemetry (OTel)** is an open standard for exporting metrics, events and traces. It sends them over **OTLP** (its wire protocol) to a **collector** program. Claude Code exports only when `CLAUDE_CODE_ENABLE_TELEMETRY=1` and an exporter are set. By default, events are sent every 5 seconds and metrics every 60 seconds ([configuration](https://code.claude.com/docs/en/monitoring-usage#common-configuration-variables)).

- **Metrics:** session count, lines of code, pull requests, commits, cost, tokens, edit-tool decisions and active time ([metrics](https://code.claude.com/docs/en/monitoring-usage#metrics)).
- **Events** include `user_prompt`, `assistant_response`, `tool_result` (`success`, `duration_ms`, `error_type`), `api_request`, `api_error`, `tool_decision` (`decision` accept or reject, plus its `source`), permission-mode changes, compaction, subagent completion and hook execution ([events](https://code.claude.com/docs/en/monitoring-usage#events)).
- **Joins:** `prompt.id` ties all events of one prompt together. `tool_use_id` matches the one that hooks receive. `message.uuid` matches the transcript line ([correlation attributes](https://code.claude.com/docs/en/monitoring-usage#event-correlation-attributes)).
- **Traces:** an optional beta adds spans linking a prompt to its API calls and tool runs ([traces](https://code.claude.com/docs/en/monitoring-usage#traces-beta)).

Private data: prompt text, tool details and tool content are left out unless `OTEL_LOG_USER_PROMPTS`, `OTEL_LOG_TOOL_DETAILS` or `OTEL_LOG_TOOL_CONTENT` is turned on. But every metric and event carries `user.email`, `organization.id` and the account UUID when signed in. The email and organization ID are "always included when available" ([standard attributes](https://code.claude.com/docs/en/monitoring-usage#standard-attributes)).

**Inference:** OTel's ready-made numbers count volume: lines, commits, PRs, tokens and cost. If a game were built on them, it would reward volume, which is the very thing "A game that rewards understanding, not volume" wants to avoid. OTel also has to be set in the environment before Claude Code starts, so it cannot see sessions that were already running.

## Claude Code: session transcripts

Transcripts are the most complete record, and the least stable. Anthropic says the format is internal.

Claude Code stores each session as JSONL at `~/.claude/projects/<project>/<session-id>.jsonl`. "The entry format is internal to Claude Code and changes between versions", and the docs point scripts to `/export`, headless output, hooks or the SDK instead ([where transcripts are stored](https://code.claude.com/docs/en/sessions#where-transcripts-are-stored)). Transcripts are plaintext and kept for 30 days by default ([data usage](https://code.claude.com/docs/en/data-usage)). The file "is written asynchronously and may lag the in-memory conversation" ([`transcript_path`](https://code.claude.com/docs/en/hooks)). Subagents get their own files under `subagents/` ([SubagentStop](https://code.claude.com/docs/en/hooks)).

Pilot observation, Claude Code 2.1.308 (`p-rust/controller/claude-projects/…/ec2d2800-….jsonl`): one agent session of about 58 seconds wrote 140 lines and 312 KB. It held ten line types: `user`, `assistant`, `attachment`, `queue-operation`, `api-request`, `api-request-blob`, `api-request-shape`, `atis-latch`, `last-prompt` and `cost-state`. Only user and assistant messages map onto anything the docs describe. Some fields are useful to a control room:

- `permissionDecision` on tool results, such as accepted by config rule from flag settings.
- `sandboxed` and `sandboxViolations`.
- `cwd` and `gitBranch` on every message line, and `promptId` on user lines.
- A `cost-state` line with total cost, API time, tool time and lines added and removed.

Thinking blocks were stored as empty text plus a signature. An `api-request-shape` line holds the full system prompt. Attachments include types named `credential_org` and `environment`. I did not open their contents.

**Inference:** following transcripts gives the richest story ("it read X, ran the tests, they failed, it edited Y"). But any Claude Code release can break a parser, and the files are large, so the app must read them incrementally. Reading them is also exactly the "Daniel's own data" question that the map has not yet specified.

## Claude Code: headless stream and the Agent SDK

Streaming output is real-time and documented, but only the program that launched the session can see it.

`claude -p --output-format stream-json` writes one JSON event per line: `system/init` (model, tools, MCP servers), assistant and user messages, `system/api_retry`, and a final `result` with cost and usage ([headless](https://code.claude.com/docs/en/headless#stream-responses)). The Agent SDK yields the same families: `SystemMessage` (including `compact_boundary`), `AssistantMessage`, `UserMessage`, `ResultMessage`, plus rate-limit and task events ([message types](https://code.claude.com/docs/en/agent-sdk/agent-loop#message-types)).

Pilot observation (`p-rust/controller/transcripts.json`): the Falinks host recorded this stream for each worker. It held `system/init`, `system/hook_response` (one per Falinks tool call), `user`, `assistant`, `rate_limit_event` (five-hour and seven-day use) and `result`. One `result` had the subtype `error_max_turns`.

**Inference:** this source suits a later version where the control room launches agents itself. For sessions Daniel starts in a terminal it offers nothing.

## Git and worktrees

Git gives the exact "what changed", in formats that are stable for scripts. It does not know who changed what, or why.

`git status --porcelain` "will remain stable across Git versions and regardless of user configuration". Version 2 adds branch headers and per-file detail ([git status](https://git-scm.com/docs/git-status#_porcelain_format_version_1)). `git worktree list --porcelain` gives one record per worktree, with its path, `HEAD`, branch and `locked` or `prunable` flags ([git worktree](https://git-scm.com/docs/git-worktree#_porcelain_format)). Claude Code puts agent worktrees under `.claude/worktrees/` ([worktrees](https://code.claude.com/docs/en/worktrees)), and the status line names the current one (`workspace.git_worktree`).

**Inference:** polling `git status` and `git diff` per worktree every few seconds, or watching the folders for file changes, shows uncommitted agent edits as they happen. Commits only appear when an agent commits. Git cannot tell an agent's edit from Daniel's, so attributing a change needs a join with hooks (`PostToolUse` paths) or with Falinks. The private data is the code itself.

## GitHub through `gh`

`gh` gives issues, PRs, reviews and CI checks as JSON. It runs on Daniel's existing login, and it must be polled.

`gh pr checks --json` returns `name`, `state`, `bucket` (`pass`, `fail`, `pending`, `skipping`, `cancel`), `startedAt`, `completedAt`, `link` and `workflow`. `--watch` refreshes every 10 seconds by default ([gh pr checks](https://cli.github.com/manual/gh_pr_checks)). `gh pr view --json` includes `statusCheckRollup`, `reviewDecision`, `latestReviews`, `files`, `additions` and `deletions` ([gh pr view](https://cli.github.com/manual/gh_pr_view)). `gh run watch` refreshes every 3 seconds ([gh run watch](https://cli.github.com/manual/gh_run_watch)). Authenticated REST calls share a budget of 5,000 requests per hour ([rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)).

**Inference:** this is the "what was checked" source for work that has been pushed. A CI check names the commit it ran on, so it is exact for that commit, but it says nothing about local runs. Webhooks would push instead, but they need a public endpoint, which a local app does not have.

## A second agent tool has the same families

Codex offers the same four kinds of source, which supports a shape that is not specific to Claude.

Codex's `notify` setting runs a program on `agent-turn-complete`, with `thread-id`, `turn-id`, `cwd`, `input-messages` and `last-assistant-message`. Codex also has lifecycle hooks, an opt-in OTel export with events such as `codex.tool_decision` and `codex.tool_result`, and local transcripts under `~/.codex` ([Codex advanced config](https://developers.openai.com/codex/config-advanced), which redirects to `learn.chatgpt.com`). I did not read the Codex hooks event list.

## What no source says directly

Phase and "stuck" are always inferences. The control room must compute them, and that computation should be visible to Daniel.

- **Phase.** Claude Code has no phase field. The nearest signals are `TaskCreated` and `TaskCompleted` subjects, the mix of tools in use (reading, editing, running tests), plan mode in `permission_mode`, and agent view's model-written summaries. Those summaries are not in the JSON.
- **Stuck.** Nothing reports "stuck". The candidates are a session that stays `working` or `busy` with no new hook events, repeated `PostToolUseFailure` events, OTel `api_error` events (or `system/api_retry` in a launched session's stream), or the same check failing again and again.

**Inference:** any "stuck" rule is a heuristic, and it can raise false alarms. Showing the evidence behind it ("no tool call for 4 minutes, last 3 test runs failed") keeps it honest. That is the same spirit as "Evidence, not promises".

## Falinks: what it records

Falinks records the meaning that the other sources lack: intent, peer reviews, refusals with reasons, and checks bound to an exact version.

The engine's event stream has one entry type, `Body` ([`src/coordination.rs:150-203`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/coordination.rs#L150-L203)). Each event has a commit sequence number and a stable `workspace:seq` ID ([`src/coordination.rs:205-210`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/coordination.rs#L205-L210)):

- **`Message`** comes in three kinds: `Plan`, `LiveEdit` and `CheckpointOffer` ([`src/coordination.rs:65-69`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/coordination.rs#L65-L69)). A plan is an agent's announced intention. A live edit carries the agent's required short note on why it made the change ([`docs/engine.md:38`](https://github.com/danielrispler/Falinks/blob/d8f5700/docs/engine.md#L38)). A **checkpoint offer** names exact work ready for checks ([`CONTEXT.md:37-38`](https://github.com/danielrispler/Falinks/blob/d8f5700/CONTEXT.md#L37-L38)).
- **`Obligation`** and **`Reviewed`**: a related change created a review the peer owes, and the peer answered keep, revise or drop, with a note.
- **`Validated`** and **`Published`**: results of the required checks on an exact revision and Git tree, and the acceptance of that exact candidate ([`src/publication.rs:508`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/publication.rs#L508), [`src/publication.rs:692`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/publication.rs#L692)). A **required check** is a trusted command, such as the tests, that must pass before publication ([`CONTEXT.md:111-112`](https://github.com/danielrispler/Falinks/blob/d8f5700/CONTEXT.md#L111-L112)).
- **`Regrouping`**, **`Incorporated`** and **`Behind`**: join or split proposals, and drafts that overlap a publication.
- **`Withdrawn`**, **`Superseded`** and **`Disconnected`**.

Every edit ends with an outcome: `Applied`, `Stale` (the files moved on), `Interrupted`, `Rejected` (with a reason), or `Unreviewed` (a review is owed first) ([`src/lib.rs:175-193`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/lib.rs#L175-L193)). A **halt** is the engine refusing all further changes. It is either an interruption or an **incident**, a workspace state the engine cannot explain ([`src/lib.rs:1822-1831`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/lib.rs#L1822-L1831)). `history()` returns every request with its author, scope, outcome and exact before and proposed revisions ([`docs/engine.md:40`](https://github.com/danielrispler/Falinks/blob/d8f5700/docs/engine.md#L40)). The regrouping signals name `overlap` (both agents touched the same function), `compiler` links, `retries` and uncertainty ([`docs/engine.md:164-171`](https://github.com/danielrispler/Falinks/blob/d8f5700/docs/engine.md#L164-L171)). `apply_timed` reports how long each edit stage took. That report is host-only ([`docs/engine.md:41`](https://github.com/danielrispler/Falinks/blob/d8f5700/docs/engine.md#L41)).

Around the engine, the Claude adapter keeps more ([`adapters/claude/README.md:119-126`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/claude/README.md#L119-L126)): the worker stream without reasoning, every mediated tool call with `received`, `started` and `finished` times, notice deliveries, and per-turn usage.

Pilot observation, `p-rust` (a run of about 56 seconds):

- The engine's `events` table held plans, live edits, obligations and reviews in plain words. One review note read "Inspected both lookup callers: total keeps try_fold so it returns the first LookupError unchanged…".
- Four operations were refused, and each reason was recorded. Examples: an offer refused with "unreviewed obligations block the offer", and two edits refused as `Unreviewed`, naming the review that was owed.
- Delivery records show whether each notice was sent mid-turn and in which turn the agent saw it.

**Inference:** this is the material "Evidence, not promises" needs. It is also good material for "Move learning into waiting time": the agents' own review notes are small, specific explanations that arrive while the work is still happening.

## Falinks today: how a second program could read it

Falinks has no outside read interface yet. The records live inside the host process and its private storage, and leave as files when a run ends.

- **The engine is a library with one owner.** `Engine::open` takes an exclusive, non-blocking OS lease and fails with "engine writer already owned" for anyone else ([`src/lib.rs:770-789`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/lib.rs#L770-L789)). Reads like `events`, `pending` and `history` are host calls, never agent tools ([`adapters/claude/README.md:73`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/claude/README.md#L73)).
- **The ledger is SQLite in WAL mode** ([`src/lib.rs:285`](https://github.com/danielrispler/Falinks/blob/d8f5700/src/lib.rs#L285)). WAL ("write-ahead logging") lets readers read while a writer writes. But the tables store JSON bodies whose shape is the engine's internal shape (pilot observation), so reading it directly is the same trap as parsing Claude transcripts.
- **Run files mostly appear at the end.** In the evaluation runner, some `events.jsonl` rows are written live ([`adapters/claude/falinks_arm.rs:118-124`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/claude/falinks_arm.rs#L118-L124)). The arm's milestone, validation and ready rows, `history.json` and `transcripts.json` are written only after the run ([`adapters/claude/falinks_arm.rs:527-537`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/claude/falinks_arm.rs#L527-L537)). The pilot files match: the late rows carry no `elapsed` time.
- **A secret sits next to the evidence.** Each worker's controller directory holds `controller/token`, the per-launch token for the host channel ([`adapters/claude/README.md:26`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/claude/README.md#L26)). The pilot folders hold `token`, `settings.json` and `mcp.json` beside `claude.stderr`.
- **Falinks workers are sealed from Daniel's settings.** Workers launch with `--setting-sources ""` and drop inherited `CLAUDE*` environment variables, except the login token ([`adapters/claude/README.md:20`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/claude/README.md#L20)). `--setting-sources` picks which settings files load ([CLI reference](https://code.claude.com/docs/en/cli-reference)).

**Inference:** a Falinks-run agent would fire none of Daniel's user hooks, would show no status line, and would export no OTel, because `CLAUDE_CODE_ENABLE_TELEMETRY` is a `CLAUDE*` variable. So for Falinks sessions, the Falinks host must be the source. It already captures the worker stream that hooks would have seen. The engine work this implies, a filtered read-only feed for a second process, belongs to the map's "Engine needs" item.

## The common shape

Every source reduces to two kinds of record, events and snapshots. The keys that join them already exist, Falinks included. This section is inference, offered as input to the map's data-source decision, not as a decision.

**Inference:** sources come in two kinds.

- **Events:** something happened at a time. Hooks, OTel events, transcript lines, the stream, Falinks events, Git commits.
- **Snapshots:** what is true now. `claude agents --json`, the status line, `git status`, `gh pr checks`, and Falinks `pending`, `candidate` and `placement`.

A control room needs both: events for the story, snapshots to recover after it starts late or misses something. A record that fits all sources needs only these parts:

- which **source** it came from;
- **when** it happened (the source's own time, if it has one) and when the app received it;
- **which session**, by the keys in the table below;
- a **kind** from a small shared vocabulary, such as started, working, waiting on Daniel, refused, changed files, check result, turn ended, ended;
- a short **summary**, and the source's raw payload, marked as metadata only or as containing content;
- a **link back** to the original: a transcript line, a `tool_use_id`, a commit, a PR or a Falinks event ID.

The join keys that already exist:

| Key | Claude Code | Git and GitHub | Falinks |
|---|---|---|---|
| Session | `session_id` in hooks, status line, OTel `session.id`, `sessionId` in agent JSON | none | `thread` in every mediated call's identity ([`adapters/host/lib.rs:454`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/host/lib.rs#L454)); pilot: `thread` equals the Claude session UUID |
| Turn | `prompt_id` (hooks, status line, OTel `prompt.id`) | none | `turn`, checked against the hook's `prompt_id` ([`adapters/claude/README.md:28`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/claude/README.md#L28)) |
| Tool call | `tool_use_id` (hooks, OTel) | none | `_meta["claudecode/toolUseId"]` on each call ([`adapters/claude/README.md:28`](https://github.com/danielrispler/Falinks/blob/d8f5700/adapters/claude/README.md#L28)) |
| Place | `cwd`, `worktree.path` | worktree path, branch, PR | `workspace` root, agent `scratch` cwd |
| Version | none | commit SHA, tree ID | revision and Git tree ID ([`docs/engine.md:48`](https://github.com/danielrispler/Falinks/blob/d8f5700/docs/engine.md#L48)) |

**Inference:** because Falinks already tags its calls with Claude's own session, turn and tool-call IDs, a Falinks source can be added later as one more producer of the same records. It brings its own kinds (plan, review owed, refused with reason, checked at revision R, published, overlap) and attaches them to sessions the app already shows. Of all the sources, Falinks events are the best behaved: they are ordered, durable and resumable from a cursor ([`docs/engine.md:91-98`](https://github.com/danielrispler/Falinks/blob/d8f5700/docs/engine.md#L91-L98)). OTel's `event.sequence` counts per process, not per session ([correlation attributes](https://code.claude.com/docs/en/monitoring-usage#event-correlation-attributes)), and hooks carry no sequence at all.

## Private data, by source

Every content-bearing source carries code. Even the "metadata only" sources carry identity or paths.

| Source | Carries |
|---|---|
| `claude agents --json` | Folder paths, session names, process IDs |
| Status line | Paths, repo identity, PR URL, cost, rate-limit use |
| OTel (defaults) | Email, organization and account IDs, repo URL, tool names, timings; content only if turned on |
| Hooks | Full tool inputs and outputs: file contents, commands, model prose |
| Transcripts | Everything above, plus prompts, system prompt shape and account-related attachments; plaintext, 30 days |
| Git, `gh` | Code, commit messages, PR text, reviewer names |
| Falinks run files | Full file bytes for every revision (pilot `history.json`), agent notes and plans, absolute paths; the host-channel `token` in the same folder |

## Open questions

These are the questions the research could not settle, and what each means for the map.

1. **Does `claude agents --json` list headless (`-p`) and SDK sessions?** The docs say "every live session", but also that `--bg` refuses `-p` ([agent view](https://code.claude.com/docs/en/agent-view)). I did not run it, because it would list Daniel's real sessions. *Matters for the v1 data sources:* if it does not list them, Falinks workers and other launched agents are invisible to the cheapest source.
2. **Real speeds are unmeasured.** The figures above are documented defaults: hooks run at the event, `Notification` after about 6 s or 60 s, OTel every 5 s or 60 s, transcripts "may lag". *Matters for "Move learning into waiting time":* an explanation has to arrive while Daniel is still waiting. A small timing prototype could measure this.
3. **Can "stuck" be told apart from "thinking hard" reliably?** No source says so, and the heuristics are untested on real runs. *Matters for the control room's main job:* if "Blind during a run" is the main job, a false "stuck" alarm costs trust.
4. **Parse transcripts or not?** They are the richest source, but Anthropic calls the format internal, and they hold everything private. *Matters for the map's "Daniel's own data" item and for v1:* a v1 built only on documented sources is safer but sees less.
5. **Hooks need installing.** A plugin avoids editing `settings.json`. Whether the app may install anything into Claude Code at all is undecided. *Matters for v1* and for "composed mostly from existing tools".
6. **What should a Falinks feed export?** File bytes, or only paths and hashes? Notes? Never the token. *Matters for the map's "Engine needs" item.* The shape above suggests a filtered, cursor-based event feed plus snapshot calls, readable by a second process.
7. **Version drift.** The docs describe features newer than the pilots' pinned 2.1.308. For example, `parent_agent_id` needs 2.1.310 ([standard attributes](https://code.claude.com/docs/en/monitoring-usage#standard-attributes)). I did not check which version introduced `claude agents --json`. *Matters for v1:* Falinks pins one Claude Code version, so the app cannot assume the newest features there.
8. **Codex parity is only partly checked.** I read one config page, not the hooks reference. *Matters for the common-shape claim* if the studio should support Codex as well.

## Hypotheses, in more detail

How this research bears on each hypothesis in `problems.md` that it touches.

- **Evidence, not promises.** *Supported.* Without Falinks, the app can show which commands ran and whether they failed (hooks, OTel), what was denied, and CI checks per pushed commit (`gh`). **Inference:** it cannot tie a local test run to the exact version it tested, because the files may change after the run. Falinks can, because `Validated` names the revision and tree, and every refusal comes with its reason.
- **Move learning into waiting time.** *Supported, as far as data goes.* The waiting moments can be detected live (`PermissionRequest`, `Notification`, `Stop`, agent `state`), and so can each change (`PostToolUse` inputs, `git diff`, Falinks live edits with notes). Whether learning in those moments helps is for the other research tickets.
- **Non-blocking.** *Not tested here.* Every source can be read without pausing the agent, provided hooks run async.
- **Serious control-room style.** *Fits.* OTel is literally telemetry, and the notification types are ready-made alerts with built-in delays.
- **A game that rewards understanding, not volume.** *Cautioned.* The ready-made metrics (lines, commits, PRs, tokens, cost) all measure volume. A game would need its own measures.
