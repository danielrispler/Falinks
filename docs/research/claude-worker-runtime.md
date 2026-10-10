# Claude Code as the subscription-backed worker runtime

Issue [#32](https://github.com/danielrispler/Falinks/issues/32), 10 October 2026. Research evidence and a recommendation only; no production adapter is built here.

## Question

Can the pinned macOS `claude` CLI, driven from a Rust host over `claude -p --input-format stream-json --output-format stream-json` and authenticated by the existing subscription login, satisfy the [#23](https://github.com/danielrispler/Falinks/issues/23) adapter controls? Where it cannot, what is the smallest recorded relaxation that keeps the safety invariants?

## Answer

Yes, as a **port of #23** that requires **one amendment to the [#11](https://github.com/danielrispler/Falinks/issues/11#issuecomment-5995165615) delivery contract**: Claude Code has no exact-turn guard on message delivery. Every other control was reproduced on the actual binary, and three controls are new and mandatory: init verification on every turn, model identity checks, and re-supplying all controls on resume.

## Setup

- Binary: Claude Code `2.1.287`, macOS arm64, SHA-256 `6eab8333fe2121553100d8f40bfada384a3e989b94f947e18ba6677a6fcb41ea`. A copy of the Homebrew cask executable is placed in a dedicated pinned directory; `brew upgrade` cannot replace it. Worker environment sets `DISABLE_AUTOUPDATER=1`.
- Authentication: existing `claude.ai` subscription login (`apiKeySource: "none"`). `--bare` cannot be used: it reads only API keys.
- Fixture: sacrificial `source/`, `scratch/`, `controller/`, `snapshots/`, `validation/`, a `scratch/alias` symlink to source, and the existing compiled `sandbox-probe` from `adapters/codex`.
- Worker launch (cwd = `source/`):

```sh
claude -p --no-session-persistence --model claude-opus-5-5 \
  --input-format stream-json --output-format stream-json --verbose \
  --replay-user-messages --include-hook-events \
  --setting-sources "" --settings settings.json \
  --mcp-config mcp.json --strict-mcp-config --disable-slash-commands \
  --tools Bash,Read,Glob,Grep --permission-mode dontAsk
```

- Probe: `adapters/claude-probe` (Rust, throwaway). `claude-probe mcp` is the stdio MCP server for `falinks_edit/review/offer`; `claude-probe hook` is the `PreToolUse` hook; `claude-probe drive` sends a JSON step script over stream-json and logs both directions. Settings, MCP config and scripts are in `adapters/claude-probe/scripts/` with `{ROOT}` placeholders.
- Evidence: curated summary in `adapters/claude-probe/evidence/2026-10-10-probe.json`. Raw logs contained runtime messaging tokens and stayed in `/private/tmp`.
- Usage: 11 short model turns, including one concurrent pair.

## Findings

### Subscription use

Executed: all runs authenticated with the subscription login; two concurrent workers both succeeded (about 17 s each). Every turn emits a `rate_limit_event` with five-hour and seven-day utilization (12% and 1% after the probes), so the host can pause a batch before exhaustion, as [#5](https://github.com/danielrispler/Falinks/issues/5#issuecomment-6045386497) requires.

Terms, quoted:

- [Consumer Terms](https://www.anthropic.com/legal/consumer-terms) §3 forbid access "through automated or non-human means, whether through a bot, script, or otherwise" "except when you are accessing our Services via an Anthropic API Key or where we otherwise explicitly permit it".
- [Claude Code legal and compliance](https://code.claude.com/docs/en/legal-and-compliance): OAuth "is designed to support ordinary use of Claude Code and other native Anthropic applications"; "Advertised usage limits for Pro and Max plans assume ordinary, individual usage of Claude Code and the Agent SDK"; developers may not "route requests through Free, Pro, or Max plan credentials on behalf of their users"; this does not "prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription".

Assessment (not legal advice): the subscriber running the unmodified binary's documented headless mode, for their own bounded two-agent evaluation, fits the documented personal use. Falinks must never intermediate credentials or run Claude Code for anyone else. Whether 18 scored runs count as "ordinary, individual usage" is not stated anywhere; the batch stays small and pauses on rate-limit signals. No documented concurrent-session limit was found.

### Pinning

The exact version and executable hash are verified. Stream-json shapes recorded: `system/init` (tools, MCP servers, plugins, skills, slash commands, permission mode, model, capabilities), `user` with `isReplay`/`uuid`, `assistant`, `result` (with `modelUsage`, `permission_denials`, `terminal_reason`), `rate_limit_event`, `control_response`, and hook lifecycle events. **`system/init` is emitted at the start of every turn**, not only once, so per-turn verification is cheap.

### Isolation from the developer's configuration

`--setting-sources ""`, inline `--settings`, `--strict-mcp-config` and `--disable-slash-commands` kept user plugins, hooks, skills, `CLAUDE.md` and MCP servers out: init listed only built-in plugins, no skills or slash commands, and the single `falinks` MCP server. Built-in subagent definitions still appear in `agents`, but the `Agent` tool itself is absent.

Gap: even with `--no-session-persistence`, the CLI created an empty `~/.claude/projects/<encoded-cwd>/memory/` directory. Without that flag it writes the transcript there. A dedicated `CLAUDE_CONFIG_DIR` might avoid this but is untested with keychain authentication.

### Mediated tools and identity

All three tools are served by the Rust stdio MCP server. What reaches the server:

- Process environment: `CLAUDE_CODE_SESSION_ID` (the runtime session) plus host-supplied `env` (agent, workspace). The docs omit this variable; the binary sets it.
- Every `tools/call` carries `_meta["claudecode/toolUseId"]`.
- The `PreToolUse` hook receives `session_id`, `tool_use_id`, `prompt_id`, `tool_name`, `tool_input`, `mcp_server`, `cwd`, `transcript_path` and `permission_mode`, and can rewrite input through `updatedInput`.

The hook stamped the runtime `session_id` and `tool_use_id` into every falinks call, and these matched the server's `CLAUDE_CODE_SESSION_ID` and `_meta` tool-use ID. A host-level forgery (input containing `falinks_session: "forged-agent-B"`) was overwritten with the runtime value. Concurrent workers each carried their own session. The model itself declined to pass a forged identity argument.

`prompt_id` identifies the turn: tool calls made after a mid-turn message kept the turn's `prompt_id`, and each new turn received a new one. It is visible to hooks only, not to stream-json senders.

Binding (agreed design): the server's launch identity is primary; the hook stamp is a per-call cross-check against `CLAUDE_CODE_SESSION_ID` and `_meta`. A call without a stamp, or with a mismatching one, is rejected. Hooks and MCP servers run outside the sandbox and are configured only by the host.

### Write denial and storage protection

All 15 `sandbox-probe` controls passed through the actual Bash tool in the initial run and again after resume: source write, create, delete, rename and replace, alias write and child-process write denied; controller, snapshot and validation reads and writes denied; source readable; scratch writable. A direct `echo … > source.txt` and `cat controller/secret.txt` were denied, and the Read tool's attempt on `validation/` appeared in `permission_denials`. Native Edit and Write are absent (`--tools` plus deny rules). The only source change went through `falinks_edit`.

Unlike Codex, `cwd = source` is safe here: `sandbox.filesystem.denyWrite` on source overrides the default cwd write permission. Sandbox settings and permission rules are independent; both must be configured.

### Disabled capabilities

Init lists exactly `Bash, Glob, Grep, Read` plus the three falinks tools. Web search and fetch, Agent/Task, Skill and other MCP servers are unavailable, and `dontAsk` denies anything unlisted. A denied attempt is observable in `permission_denials`; an absent tool cannot be attempted at all.

New risk: **model refusal fallback.** Opus 5.5's safeguards stopped a control prompt that talked about denials and bypasses (`api_refusal_category: "cyber"`), and Claude Code silently continued on `claude-opus-4-8` (`model_refusal_fallback` event; `modelUsage` lists both models). That would break the same-model baseline. Mandatory control: any `model_refusal_fallback` event, an init model other than the pinned one, or more than one model in `modelUsage` invalidates the run. All later runs used neutral wording and passed this check.

### Steering and the completion race

Observed:

- **Sent during an active turn** (while a Bash call ran): the message was presented at the **next tool-result boundary of the same turn** (`isReplay` echo immediately after that tool result; same `prompt_id`). The agent acted on it within the turn, and one `result` closed both.
- **Sent after `result`**: a new turn started (new init, new `prompt_id`).
- **Interrupt then message**: `control_response` returned `still_queued: []`, the turn ended `error_during_execution`, and the message started the next turn. Nothing was lost.
- No send-side turn identifier or expected-turn guard exists. Init advertises `msg_lifecycle_v1` and `interrupt_cancel_queued_v1`; these were not explored.
- Not observed: a message arriving while the final text is generated, after the last tool call. It is presumed to land in the next turn; the relaxation below covers either outcome.
- The model issued several tool calls in one step (`falinks_review` and `falinks_offer` together). Prompt order is not an ordering guarantee; the engine gate must enforce review-before-offer.

### Resume

`--resume <session_id>` with the same launch flags restored the conversation (the code word survived), the session ID, tools, MCP server and sandbox (all 15 controls passed again), and the hook stamp. `--resume` **without** those flags restored the conversation with the full default toolset, unsandboxed Bash and no falinks tools. Controls come only from the launch, never from the session. Mandatory: re-pass all flags, verify init, then replay host-persisted pending and deferred context before related operations, as #11 already requires.

## Recommendation

Build the Claude adapter as a **port of #23**: reuse `Boundary`, `ControlGate`, `ControlledHost`, `sandbox-probe` and unknown-change detection. Replace the transport (stream-json instead of app-server), the pins and the permission profile (settings, sandbox, hooks and MCP config instead of the named profile). Add three startup and per-turn controls:

1. **Init verification** at every turn: exact tool list, only the `falinks` MCP server, no user plugins, skills or slash commands, `dontAsk`, the pinned model.
2. **Model identity**: any refusal fallback or model change invalidates the run (and a scored run).
3. **Resume** re-passes every control flag and repeats init verification before context replay.

Amend the #11 "Runtime delivery, deferral and reconnect" paragraph. Proposed wording:

> Persist the message/obligation before requesting attention. Where the runtime offers an exact-turn guard (Codex `turn/steer` with `expectedTurnId`), use it. Where it has none (Claude Code stream-json), delivery is presentation only: the host may send at any time, and the runtime presents the message at its next supported presentation boundary — the next tool-result boundary of an active turn, or a new turn. Never infer handling from send, transport acceptance, replay echo or presentation. Obligations stay pending until an engine-accepted, revision-bound review, and relevant write/offer gates stay closed until then. A message that lands in a later turn, or after an interrupt, changes nothing: the obligation is still pending. The host defines a turn as the span from its sent message to the next `result`; the runtime's `prompt_id`, visible to hooks, binds tool calls to that turn.

Failure cases this must survive: a message racing completion (lands in the next turn; the obligation remains); an interrupt (no message loss observed; the obligation remains); and parallel tool calls (the gate rejects an offer issued alongside its own review until that review is engine-accepted).

Also amend #5: Claude Code becomes the first evaluated runtime, with the same pinned model in both arms and the model-identity check as a safety gate. The Codex adapter and its evidence stay as historical, unmaintained work.

## Limits

Eleven bounded turns on one machine and one binary are not exhaustive bypass testing. The sandbox and hooks are Claude Code features: renewed verification is required on any version change. Unexplored: `CLAUDE_CONFIG_DIR` isolation with keychain authentication, the `msg_lifecycle_v1` and cancel-queued controls, and the final-text completion race. Raw hook and MCP logs from the first run were overwritten before archiving; later runs repeated the identity observation. The terms assessment is an interpretation, not legal advice.
