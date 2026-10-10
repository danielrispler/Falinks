# Pinned Claude Code worker adapter

Issue [#33](https://github.com/danielrispler/Falinks/issues/33) ports the [#23](https://github.com/danielrispler/Falinks/issues/23) adapter to Claude Code, following the [#32 resolution](https://github.com/danielrispler/Falinks/issues/32#issuecomment-6096677771). It reuses `Boundary`, `ControlGate`, `ControlledHost`, unknown-change detection and `sandbox-probe` from `falinks-host` (`adapters/codex`). Only the transport (stream-json), pins and permission configuration are new. [#26](https://github.com/danielrispler/Falinks/issues/26) connects it to the production engine; see [Production integration](#production-integration-26).

## Run

Use macOS arm64 and Claude Code `2.1.287` (SHA-256 `6eab8333…cb41ea`). Copy the executable into a dedicated directory so a package upgrade cannot replace it. Startup checks the hash before running anything and never substitutes another installed CLI.

```sh
cargo build --release --bins
./target/release/check-claude-adapter \
  --binary /path/to/pinned/claude-2.1.287 \
  --output /path/to/current-claude-controls.json
```

The verifier expects sibling `sandbox-probe` and `falinks-claude-tool` executables (`--probe` and `--tool` override them). It needs the existing subscription login, network access for three short model turns, and permission to launch the Claude Code sandbox. It creates a fixture under `/private/tmp/falinks-claude-adapter-*` and removes only that fixture's session folder from `~/.claude/projects/`. Failed checks exit nonzero; inspect `passed` and `failure`.

## Launch profile

The worker runs `claude -p` over stream-json with `--setting-sources ""`, inline-generated `--settings`, `--strict-mcp-config`, `--disable-slash-commands`, `--tools Bash,Read,Glob,Grep`, `--permission-mode dontAsk`, `--max-turns 12`, the pinned `--model` and a host-chosen `--session-id`. Its environment drops inherited `CLAUDE*` variables and sets `DISABLE_AUTOUPDATER=1`. Settings enable the sandbox with no unsandboxed escape: writes only to `scratch/`; source, tools and storage are write-denied; controller, snapshot and validation storage are read-denied. Edit, Write, web, Agent/Task and Skill are denied.

The cwd is `scratch/`, not `source/`. At startup the trusted CLI creates `.claude/.cc-writes/` in its cwd. In `source/`, unknown-change detection correctly stopped the host before the first mediated edit.

## Engine-facing boundary

`falinks-claude-tool` provides the `falinks` stdio MCP server and the `PreToolUse` hook. Claude Code runs both outside the worker sandbox. Neither touches the workspace: both forward to the trusted host over `controller/host.sock`, authenticated by a per-launch token in `controller/token` (mode 0600, read-denied to the worker).

`Session` is the process-free core. The hook registers the runtime's own stamp before each falinks call: session, `prompt_id` (the turn) and `tool_use_id`, plus the tool name and input. The MCP call then carries `CLAUDE_CODE_SESSION_ID` and `_meta["claudecode/toolUseId"]`. The host rejects a call without a matching stamp. It also rejects a call from another session, outside an `init`…`result` span, or from a stale turn. Input different from what the hook saw is rejected too. The call is then shaped for `Boundary::authenticate`, which binds host identity (agent, connection, session, turn, canonical workspace) and rejects caller-supplied identity or another workspace.

Every turn's `system/init` must match the pinned version, model, `dontAsk` mode, session, cwd, exact tool list, the single `falinks` MCP server, the three built-in plugins, and empty skills and slash commands. These cause a failure:

- any unrecorded stream-json shape
- a `model_refusal_fallback`
- an assistant message from another model
- `modelUsage` naming anything but the pinned model
- a tool outside the list

The failure latches the fresh `ControlGate`, engine calls stop, and the worker process group is stopped at once, so native tools stop too. `require_supported` also checks that the process is alive and re-hashes the binary.

## Delivery under the #11 amendment

Claude Code has no exact-turn guard. `attention` persists the obligation, then sends a user message. Each delivery records whether a turn was active at send time and the `init` span in which the `isReplay` echo presented it. Sending, presentation and later-turn arrival never clear an obligation. Only an engine-accepted, revision-bound review does. The `ControlledHost` gate rejects offers while any obligation is pending, so a parallel review and offer cannot bypass the order.

Resume re-passes every launch flag with `--resume`, re-verifies `init` (same session) and prepends host-persisted pending/deferred context to the first message.

## Controls and evidence

`check-claude-adapter` runs three real turns:

1. The exact `sandbox-probe` command runs through the actual Bash tool, and all 15 checks must pass. A native Read of `validation/` must appear in `permission_denials`. Then a mediated edit runs. Its E1 notice is sent mid-turn and presented in the same turn, then explicitly deferred. An offer is rejected.
2. An E2 notice is sent after `result`. It is presented in a new turn and stays pending.
3. After resume, the first offer is rejected and the probe passes again. E1 and E2 are reconsidered, and the final offer is eligible (not publication).

Unknown-change detection runs on a separate injected fixture before the grant. A final live injection preserves the bytes, stops the host and revokes capability.

The steering and completion-race controls are verified in the first process and recorded again in the resumed gate within the same verifier run. A saved report never authorizes another process. `evidence/2026-10-10-controls.json` is audit material: it contains stream events without reasoning blocks, host calls, deliveries and controls. Current capability always requires a fresh run.

## Limits

These are finite probes on one binary and machine; any version change requires renewed verification. Managed settings still apply under `--setting-sources ""`; init verification covers their visible effects (tools, MCP servers, plugins), not hidden hooks. The race control sends at the completion boundary rather than racing the final text generation. Interrupt and cancel-queued controls are not exercised here. Stream-json emits `init` only after the first user message, so a resumed worker receives its replayed context in that message, before `init` can be checked. Mediated calls wait for `init`, and a mismatch stops the worker. Native Edit/Write and web tools are shown absent by `init`, not attempted. `rate_limit_event` lines are recorded but not yet acted on. The model-identity check makes a safeguard refusal fail the run; prompts use neutral wording to avoid it.

## Production integration (#26)

`Launch` names what one worker launch may see: the readable live root, its scratch cwd, its controller, the read-only worker tools, other protected storage, and the `falinks` tool list. Settings, the `init` tool check and the boundary's registered operations all come from it. The #33 fixture is `Launch::fixture`.

`engine_host::EngineHost` maps an authenticated envelope onto `falinks::Engine` as that agent's `Client`. The agent-facing operations and their request shapes are listed in `engine_host::OPERATIONS`; `every_advertised_tool_maps_to_an_engine_operation` checks that each one dispatches. Host-only calls (checks, validation, workspaces, reconsideration, disconnect, run retry, space-0 capture) are never tools. Engine byte arrays become `content` text. The only job program is the host-enrolled `gofmt -w`; agents name paths. An agent mistake is a refusal. A foreign agent, a workspace the agent has left, or an engine incident is an error, and it latches the worker's gate.

### Run the gate

```sh
cargo build --release --bins
./target/release/check-integration \
  --binary /path/to/pinned/claude-2.1.287 \
  --output /path/to/integration-report.json
```

It needs the same pinned binary, login and sandbox permission as `check-claude-adapter`, plus Go and `gopls` (or `FALINKS_GOPLS`), like `tests/regroup.rs`. It creates `/private/tmp/falinks-integration-*` with two live roots (`space0`, `space1`), engine `state`, one `scratch` and `controller` per agent, and `worker-tools`. The fixture is a small Go module used only for this gate, never a scored evaluation fixture. Required checks are `go vet ./...` and `go test ./...`. Failed runs exit nonzero; a saved report never authorizes another run.

### Host-scripted phases

Real agents make every tool call; the host sets turn order, barriers and faults:

1. **controls**: both workers at once. The sandbox probe passes through the real Bash tool against the production layout (live root, scratch, engine state, own and peer controller). A native Read of the engine ledger is denied. Scopes are registered.
2. **drafts**: unfinished but compiling edits in two files of the shared workspace. Agent 1's early offer holds a team candidate that still needs agent 0.
3. **split**: the engine recommends a split; both agents agree. Agent 1 is resumed with `--resume` against `space1`. Its draft moves with it and its early offer stays bound to the old candidate.
4. **checks_while_drafting**: agent 1's exact offer queues checks. The validator is held while agent 0 drafts newer work, then publishes exactly the offered candidate.
5. **join**: agent 1 registers work that depends on agent 0's and proposes a join; both agree; agent 1 resumes in `space0`.
6. **steering**: after agent 1's draft, only agent 0 is stepped until its non-compiling draft commits. Agent 1's next mediated call waits, so the obligation notice is presented in the same turn. Agent 1 sees the incomplete peer draft through a native Read. Its related write must first come back `Unreviewed`, and it applies only after an engine-accepted review. A two-file request bound to an old version is then rejected whole and retained (**stale**).
7. **accepted_recovery**: agent 0's steering is checked the same way, against a declared dependency on agent 1's `Summary`. Agent 0 then requests feedback on the exact two-member revision, and agent 1 drafts past it before the held run is processed (**team_checks_while_drafting**). Both offer the exact team candidate. A child host process validates and exits at `Stage::Accepted`, after the commit and before the Git mirror. Engine and workers restart. The mirror is repaired, both workers observe `Accepted`, and their duplicate offers return the recorded checkpoint.
8. **deferral_and_jobs**: agent 0 changes `Label`. Agent 1 defers its first unhandled event, and its worker restarts with `--resume`. The resumed prompt replays the deferred event and the pending obligation. The resumed worker's related write must come back `Unreviewed` until it reviews. It then formats its draft with the enrolled gofmt job and applies the job output.
9. **completion_race**: on both workers, a notice sent after `result` is presented in a new turn, and the engine event stays unhandled.
10. **unknown_change**: host bytes written into the live root halt the engine. Both workers' next mediated calls latch their gates. A host-queued check run cannot publish afterwards, and the injected bytes are preserved.

The report keeps runtime, binary, probe, tool, Go and gopls hashes, launches and settings, every stream event without reasoning, mediated calls, deliveries, history, runs, validator results and per-worker controls. The gate passes only when every phase passes and both workers verified all `REQUIRED` controls.

### Delivery during integration

Engine events are persisted before any notice. A worker in an active turn is sent new events at once. An idle worker gets them with its next prompt, together with its pending obligations. Claude Code may replay several queued messages as one user message, so the session clears every sent message the replay contains.

### Limits

These are finite host-scripted runs on one machine and binary. Steering is forced by host barriers, not natural timing. Claude Code blocks long `sleep` commands, so a worker cannot be asked to wait. The model can trigger a safeguard refusal fallback; the model-identity control then invalidates the run, and prompts use neutral wording to make that less likely. `wait` is not a tool, because a blocking call would stall the single host thread.
