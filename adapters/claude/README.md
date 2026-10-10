# Pinned Claude Code worker adapter

Issue [#33](https://github.com/danielrispler/Falinks/issues/33) ports the [#23](https://github.com/danielrispler/Falinks/issues/23) adapter to Claude Code, following the [#32 resolution](https://github.com/danielrispler/Falinks/issues/32#issuecomment-6096677771). It reuses `Boundary`, `ControlGate`, `ControlledHost`, unknown-change detection and `sandbox-probe` from `falinks-host` (`adapters/codex`). Only the transport (stream-json), pins and permission configuration are new. Production engine integration remains in #26.

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

The worker runs `claude -p` over stream-json with `--setting-sources ""`, inline-generated `--settings`, `--strict-mcp-config`, `--disable-slash-commands`, `--tools Bash,Read,Glob,Grep`, `--permission-mode dontAsk`, the pinned `--model` and a host-chosen `--session-id`. Its environment drops inherited `CLAUDE*` variables and sets `DISABLE_AUTOUPDATER=1`. Settings enable the sandbox with no unsandboxed escape: writes only to `scratch/`; source, tools and storage are write-denied; controller, snapshot and validation storage are read-denied. Edit, Write, web, Agent/Task and Skill are denied.

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

The failure latches the fresh `ControlGate`, and engine calls stop. `require_supported` also checks that the process is alive and re-hashes the binary.

## Delivery under the #11 amendment

Claude Code has no exact-turn guard. `attention` persists the obligation, then sends a user message. Each delivery records whether a turn was active at send time and the `init` span in which the `isReplay` echo presented it. Sending, presentation and later-turn arrival never clear an obligation. Only an engine-accepted, revision-bound review does. The `ControlledHost` gate rejects offers while any obligation is pending, so a parallel review and offer cannot bypass the order.

Resume re-passes every launch flag with `--resume`, re-verifies `init` (same session) and prepends host-persisted pending/deferred context to the first message.

## Controls and evidence

`check-claude-adapter` runs three real turns:

1. The exact `sandbox-probe` command runs through the actual Bash tool, and all 15 checks must pass. Then a mediated edit runs. Its E1 notice is sent mid-turn and presented in the same turn, then explicitly deferred. An offer is rejected.
2. An E2 notice is sent after `result`. It is presented in a new turn and stays pending.
3. After resume, the first offer is rejected and the probe passes again. E1 and E2 are reconsidered, and the final offer is eligible (not publication).

Unknown-change detection runs on a separate injected fixture before the grant. A final live injection preserves the bytes, stops the host and revokes capability.

The steering and completion-race controls are verified in the first process and recorded again in the resumed gate within the same verifier run. A saved report never authorizes another process. `evidence/2026-10-10-controls.json` is audit material: it contains stream events without reasoning blocks, host calls, deliveries and controls. Current capability always requires a fresh run.

## Limits

These are finite probes on one binary and machine; any version change requires renewed verification. Managed settings still apply under `--setting-sources ""`; init verification covers their visible effects (tools, MCP servers, plugins), not hidden hooks. The race control sends at the completion boundary rather than racing the final text generation. Interrupt and cancel-queued controls are not exercised here. The model-identity check makes a safeguard refusal fail the run; prompts use neutral wording to avoid it.
