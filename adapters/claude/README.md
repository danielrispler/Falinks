# Pinned Claude Code worker adapter

Issue [#33](https://github.com/danielrispler/Falinks/issues/33) ports the [#23](https://github.com/danielrispler/Falinks/issues/23) adapter to Claude Code, following the [#32 resolution](https://github.com/danielrispler/Falinks/issues/32#issuecomment-6096677771). It reuses `Boundary`, `ControlGate`, `ControlledHost`, unknown-change detection and `sandbox-probe` from `falinks-host` (`adapters/host`). Only the transport (stream-json), pins and permission configuration are new. [#26](https://github.com/danielrispler/Falinks/issues/26) connects it to the production engine; see [Production integration](#production-integration-26).

## Run

Use a pinned platform from [`docs/platforms.md`](../../docs/platforms.md) and Claude Code `2.1.309`, whose per-platform SHA-256 is in `PINS`. On Linux use the glibc `linux-x64`/`linux-arm64` release binary and meet the Linux adapter requirements there. Copy the executable into a dedicated directory so a package upgrade cannot replace it. Startup checks the hash before running anything and never substitutes another installed CLI.

```sh
cargo build --release --bins
./target/release/check-claude-adapter \
  --binary /path/to/pinned/claude-2.1.309 \
  --output /path/to/current-claude-controls.json
```

The verifier expects sibling `sandbox-probe`, `falinks-claude-tool` and `falinks-claude-shell` executables (`--probe` and `--tool` override the first two; the shell sits beside the tool); a missing one fails before any runtime launch. It needs the existing subscription login and network access for three short model turns. It creates a fixture under `/tmp/falinks-claude-adapter-*` (`/private/tmp` on macOS) and removes only that fixture's session folder from `~/.claude/projects/`. The evidence's `outcome` is `passed`, `failed` or `inconclusive`. A failed check exits 1; inspect `failure`. A `model_refusal_fallback` still fails the run as below, but exits 2 and prints `inconclusive: model refusal (not evidence either way)`.

### Moving the pin

To move to a new Claude Code release:

1. Read the release's `CHANGELOG.md` entry for changes to the launch profile, `CLAUDE_CODE_SHELL_PREFIX`, stream-json events, hooks, MCP and the macOS sandbox profile.
2. Copy the `darwin-arm64`, `linux-x64` and `linux-arm64` checksums from `$DIST/VERSION/manifest.json` into `PINS`, and set `VERSION`. `DIST` is in `.github/workflows/adapter-linux.yml`. Check a local copy against its checksum.
3. Change the version in this README and `tests/session.rs`. Change it in `shell.rs`, `tests/shell.rs` and `docs/platforms.md` once the behaviour they describe is checked on the new binary, including the hand mirror of Claude Code's Seatbelt profile.
4. The Falinks arm and the judge require the evaluation's runtime to match. Set `runtime_version`, `runtime_sha256` (macOS arm64) and `runtime_binary` in `evaluation/config.json`, the version in `evaluation/README.md` and the judge configurations in `evaluation/tests/harness.rs`. Move `code-quality.json`'s `judge.runtime_version` by a versioned amendment, then re-freeze and record the pin in `setup.json`'s `status` (see "Freeze and setup ledger" in `evaluation/README.md`). If a final freeze names the old version and its batch has not finished, ask before moving.
5. Run `check-claude-adapter` and `check-integration` on the new binary, then `cargo run --bin ci-checks`. Suggest the heavy evaluation tests on the branch.

## Launch profile

The worker runs `claude -p` over stream-json with `--setting-sources ""`, inline-generated `--settings`, `--strict-mcp-config`, `--disable-slash-commands`, `--tools Bash,Glob,Grep,Read` (from `NATIVE`, the list `init` must report), `--permission-mode dontAsk`, `--max-turns 12`, the pinned `--model` and a host-chosen `--session-id`. Its environment drops inherited `CLAUDE*` variables except `CLAUDE_CODE_OAUTH_TOKEN`, the subscription login that `claude setup-token` creates for CI, and sets `DISABLE_AUTOUPDATER=1`. Like the Git-arm worker, it sets `GOCACHE=scratch/go-cache`, `GOTOOLCHAIN=local` and `GOPROXY=off`: the sandbox denies Go's default cache (#112). Edit, Write, web, Agent/Task and Skill are denied.

Settings turn Claude Code's own sandbox off and set `CLAUDE_CODE_SHELL_PREFIX` to `worker-tools/falinks-claude-shell` (#236, #295). Claude Code runs each Bash command, foreground or background, as `PREFIX 'COMMAND'`. The wrapper confines itself, then runs `/bin/bash -c COMMAND`: Seatbelt on macOS, the engine's Landlock and seccomp on Linux. A confined command writes only beneath `scratch/` (its `TMPDIR` is `scratch/.tmp`) and `/dev/null`, cannot read controller, snapshot or validation storage, and has no network. Every failure exits 126 before the command runs. Claude Code also prefixes stdio MCP servers; the `falinks` relay's exact command line is the one the wrapper runs unconfined. The exec-form hook is not prefixed. `tests/shell.rs` checks the wrapper directly.

The cwd is `scratch/`, not `source/`. At startup the trusted CLI creates `.claude/.cc-writes/` in its cwd. In `source/`, unknown-change detection correctly stopped the host before the first mediated edit. Bash keeps its working directory across turns while it stays beneath the launch cwd; the pinned CLI resets it to the launch cwd when it leaves (#110).

## Engine-facing boundary

`falinks-claude-tool` provides the `falinks` stdio MCP server and the `PreToolUse` hook. Both run unconfined, outside the worker sandbox. Neither touches the workspace: both forward to the trusted host over `controller/host.sock`, authenticated by a per-launch token in `controller/token` (mode 0600, read-denied to the worker).

`Session` is the process-free core. The hook registers the runtime's own stamp before each falinks call: session, `prompt_id` (the turn) and `tool_use_id`, plus the tool name and input. The MCP call then carries `CLAUDE_CODE_SESSION_ID` and `_meta["claudecode/toolUseId"]`. The host rejects a call without a matching stamp. It also rejects a call from another session, outside an `init`…`result` span, or from a stale turn. Input different from what the hook saw is rejected too. The call then goes through `falinks_host::mediate`, the mediation sequence every runtime shares: it refuses once the gate has failed, authenticates through `Boundary::authenticate`, calls the engine, latches the gate on an engine error and records the result. `Boundary::authenticate` binds host identity (agent, connection, session, turn, canonical workspace). The tool input is the request itself, and an identity key in it, `workspace` included, is refused as caller-supplied identity. Each engine and evaluation-host tool's input schema gives every request key its JSON type and requires none: untyped, the runtime sends arrays as encoded strings (#239), and the adapter's key check stays the only one. This adapter adds only its transport checks before it: the host-channel token, the session and turn span, and the hook stamp. The sandbox-probe contract (`probe_args`, `probe_controls`, `PROBE_CONTROLS`) and pinned-binary verification (`verify_pinned`) live in `falinks-host` too.

Every turn's `system/init` must match the pinned version, model, `dontAsk` mode, session, exact tool list, the single `falinks` MCP server, the three built-in plugins, and empty skills and slash commands. Its cwd, with symlinks resolved, must be `scratch/` or a directory beneath it. These cause a failure:

- any unrecorded stream-json shape
- a `model_refusal_fallback`
- an assistant message from another model
- `modelUsage` naming anything but the pinned model
- a model reply in the session's own file (`~/.claude/projects/<cwd>/<session>.jsonl`, the only place the runtime records effort) at an effort other than the pinned `medium`, or no such file, checked at each `result`; the runtime's own `<synthetic>` replies carry no effort and are skipped
- a tool outside the list

The failure latches the fresh `ControlGate`, engine calls stop, and the worker is shut down, so native tools stop too. Shutdown closes input and waits up to 20 s, then sends `SIGTERM` to the worker's process group so it can flush its session, waits up to 5 s, then sends `SIGKILL` to the group. Relocation and resume use the same shutdown. `require_supported` also checks that the process is alive and re-hashes the binary.

## Delivery under the #11 amendment

Claude Code has no exact-turn guard. `attention` persists the obligation, then sends a user message. Each delivery records whether a turn was active at send time and the `init` span in which the `isReplay` echo presented it. Sending, presentation and later-turn arrival never clear an obligation. Only an engine-accepted, revision-bound review does. The `ControlledHost` gate rejects offers while any obligation is pending, so a parallel review and offer cannot bypass the order.

Resume re-passes every launch flag with `--resume`, re-verifies `init` (same session) and prepends host-persisted pending/deferred context to the first message.

## Controls and evidence

`check-claude-adapter` runs three real turns:

1. The exact `sandbox-probe` command runs through the actual Bash tool, and all 15 checks must pass, with every protected read and write refused, never masked. A native Read of `validation/` must appear in `permission_denials`. Then a mediated edit runs. Its E1 notice is sent mid-turn and presented in the same turn, then explicitly deferred. An offer is rejected.
2. An E2 notice is sent after `result`. It is presented in a new turn and stays pending.
3. After resume, the first offer is rejected and the probe passes again, in the foreground and with `run_in_background`. The session must have no `sandbox_instructions` attachment. E1 and E2 are reconsidered, and the final offer is eligible (not publication).

Unknown-change detection runs on a separate injected fixture before the grant. A final live injection preserves the bytes, stops the host and revokes capability.

The steering and completion-race controls are verified in the first process and recorded again in the resumed gate within the same verifier run. A saved report never authorizes another process. `evidence/2026-10-10-controls.json` (macOS) and `evidence/2026-10-14-linux/` (Ubuntu 24.04; its README records the hosts) are audit material: they contain stream events without reasoning blocks, host calls, deliveries and controls. Current capability always requires a fresh run.

## Limits

These are finite probes on one binary and machine; any version change requires renewed verification. Managed settings still apply under `--setting-sources ""`; init verification covers their visible effects (tools, MCP servers, plugins), not hidden hooks. The race control sends at the completion boundary rather than racing the final text generation. Interrupt and cancel-queued controls are not exercised here. Stream-json emits `init` only after the first user message, so a resumed worker receives its replayed context in that message, before `init` can be checked. Mediated calls wait for `init`, and a mismatch stops the worker. Native Edit/Write and web tools are shown absent by `init`, not attempted. `rate_limit_event` lines are recorded but not yet acted on. The model-identity check makes a safeguard refusal fail the run; prompts use neutral wording to avoid it.

## Production integration (#26)

`Launch` names what one worker launch may see: the readable live root, its scratch cwd, its controller, the read-only worker tools, other protected storage, and the `falinks` tool list. Settings, the `init` tool check and the boundary's registered operations all come from it. The #33 fixture is `Launch::fixture`.

`engine_host::EngineHost` maps an authenticated envelope onto `falinks::Engine` as that agent's `Client`. The agent-facing operations and their request shapes are listed in `engine_host::OPERATIONS`; `every_advertised_tool_maps_to_an_engine_operation` checks that each one dispatches. Host-only calls (checks, validation, workspaces, reconsideration, disconnect, run retry, space-0 capture, `pending` and `events`) are never tools. Notices and prompts carry what agents need from `pending` and `events` (#116 decision 4). Engine byte arrays become `content` text. The only job program is the host-enrolled `gofmt -w`; agents name paths. An agent mistake is a refusal. A foreign agent, a workspace the agent has left, or an engine incident is an error, and it latches the worker's gate.

### Run the gate

```sh
cargo build --release --bins
./target/release/check-integration \
  --binary /path/to/pinned/claude-2.1.309 \
  --output /path/to/integration-report.json
```

It needs the same pinned binary, login and sibling executables as `check-claude-adapter`, plus Go and `gopls` (or `FALINKS_GOPLS`), like `tests/regroup.rs`. It creates `/tmp/falinks-integration-*` (`/private/tmp` on macOS) with two live roots (`space0`, `space1`), engine `state`, one `scratch` and `controller` per agent, and `worker-tools`. The fixture is a small Go module used only for this gate, never a scored evaluation fixture. Required checks are `go vet ./...` and `go test ./...`. Failed runs exit nonzero; a saved report never authorizes another run. `evidence/2026-10-10-integration.json` is the first fully passing report (audit material only).

### Host-scripted phases

Real agents make every tool call; the host sets turn order, barriers and faults:

1. **controls**: both workers at once. The sandbox probe passes through the real Bash tool against the production layout (live root, scratch, engine state, own and peer controller). A native Read of the engine ledger is denied. Scopes are registered.
2. **drafts**: unfinished but compiling edits in two files of the shared workspace. Agent 1's early offer holds a team candidate that still needs agent 0.
3. **split**: agent 0 drafts again, so agent 1's early offer is no longer pending. The engine recommends a split; both agents agree. Agent 1 is resumed with `--resume` against `space1`. Its draft moves with it and its early offer stays bound to the old candidate.
4. **checks_while_drafting**: agent 1's exact offer queues checks. The validator is held while agent 0 drafts newer work, then publishes exactly the offered candidate.
5. **join**: agent 1 registers work that depends on agent 0's and proposes a join; both agree; agent 1 resumes in `space0`.
6. **steering**: after agent 1's draft, only agent 0 is stepped until its non-compiling draft commits. Agent 1's next mediated call waits, so the obligation notice is presented in the same turn. Agent 1 sees the incomplete peer draft through a native Read. Its related write must first come back `Unreviewed`, and it applies only after an engine-accepted review. A two-file request bound to an old version is then rejected whole and retained (**stale**).
7. **accepted_recovery**: agent 0's steering is checked the same way, against a declared dependency on agent 1's `Summary`. Agent 0 then requests feedback on the exact two-member revision, and agent 1 drafts past it before the held run is processed (**team_checks_while_drafting**). Both offer the exact team candidate. A child host process validates and exits at `Stage::Accepted`, after the commit and before the Git mirror. Engine and workers restart. The mirror is repaired, both workers observe `Accepted`, and their duplicate offers return the recorded checkpoint.
8. **deferral_and_jobs**: agent 0 changes `Label`. Agent 1 defers its first unhandled event, and its worker restarts with `--resume`. The resumed prompt replays the deferred event and the pending obligation. The resumed worker's related write must come back `Unreviewed` until it reviews. It then formats its draft with the enrolled gofmt job and applies the job output.
9. **completion_race**: on both workers, a notice sent after `result` is presented in a new turn, and the engine event stays unhandled.
10. **unknown_change**: host bytes written into the live root halt the engine. Both workers' next mediated calls latch their gates, the published state does not change, and the injected bytes are preserved. A feedback run queued after the halt still runs its checks; it can never publish. The halt's publication gate is covered deterministically by `unexplained_source_writes_stop_publication_and_keep_evidence`.

The report keeps runtime, binary, probe, tool, Go and gopls hashes, each launch's exact arguments and settings as the runtime used them, every stream event without reasoning, mediated calls, deliveries, history, runs, validator results and per-worker controls. The gate passes only when every phase passes and both workers verified all `REQUIRED` controls.

### Delivery during integration

Engine events are persisted before any notice. A worker in an active turn is sent new events at once. An idle worker gets them with its next prompt, together with its pending obligations. Claude Code may replay several queued messages as one user message, so the session clears every sent message the replay contains.

### Limits

These are finite host-scripted runs on one machine and binary. Steering is forced by host barriers, not natural timing. Claude Code blocks long `sleep` commands, so a worker cannot be asked to wait. The model can trigger a safeguard refusal fallback; the model-identity control then invalidates the run, and prompts use neutral wording to make that less likely. `wait` is not a tool, because a blocking call would stall the single host thread.

## Evaluation arm (#27)

`falinks-arm FIXTURE --config FILE --output DIR [--pair NAME] [--order N] [--scored true]` runs one Falinks-arm evaluation run. Usually `falinks-eval batch` runs it; see `evaluation/README.md`. `arm::Arm` is the runtime-independent core: the production engine on the exported fixture, the Git arm's milestone triggers, and the host tools `ack`, `check` and `ready`. It calls the shared `falinks-eval oracle` on a commit of each exact capture. A `check` commits the capture on the driver thread, then runs the oracle on a worker thread, so the driver keeps serving the other agent; the answer reaches the waiting call when the oracle finishes (#200). Every other call, and the final oracle once both agents are ready, stays on the driver thread. `tests/arm.rs` drives it with scripted envelopes, so no model is involved. Those tests cover drafts and milestones, pass/fail-only checks, publication and readiness, refused edits after `ready`, the Rust visible checks under the engine sandbox, and the regrouping result counts of a run that splits and then joins.

The runner launches both workers with the #26 profile and `control_host` set. The batch's fresh `check-integration` run supplies the controls; any later control failure still latches the gate and invalidates the run. The runner calls `reconsider()` once so the engine recommends at later boundaries. After an applied split or join, the moved worker is resumed in its new root.

The opening prompt carries the initial protocol state from `engine_host::opening`: a capture given to the agent, as its revision and file versions, and the empty pending state. It carries no file contents, because the Git arm's prompt has none (#5). A notice whose events renew the recipient's obligation also carries a capture given to that recipient (`engine_host::notice_capture`): the revision, the file versions and the content of each file whose version changed since the last capture the host gave it. The agent can then review inline on its next `edit` or `offer` with no `capture` call. The engine records the capture when the host sends it, not when the agent sees it (#116 decision 3).

The result follows `evaluation/result-schema.json`. Retained evidence includes:

- the timeline (`events.jsonl`)
- engine history, runs, proposals and validator log
- regrouping counts (`Arm::regrouping`, #134): proposals by action and proposer, agreements, declines, `Blocked` proposals with blockers, transitions, `handle` calls on `Regrouping` events, and round-trips containing `recommend`, `propose` or `respond`
- transcripts without reasoning (`transcripts.json`). Each `host_call` record also holds `received` (the socket thread got the request), `started` (the driver began serving it) and `finished` (its answer was ready), in seconds since `Session::clock`; `falinks-arm` sets it to the run-start clock that `events.jsonl` `elapsed` uses. An `edit`, `retry` or `incorporate` record that reached the engine also holds `stages`, the engine's stage report: `{stage, seconds}` in the order the edit reached them. Agents see none of these fields.
- oracle evidence for every candidate (`controller/oracle/`)
- per-turn usage with the last `rate_limit_event`, drained after the clock stops: every call is refused while the host waits up to `usage_drain_seconds` for each worker to end its turn (#179)
- the run's `identity` (#185): the manifest version, `adapter_commit` (the `HEAD` of the checkout its binaries sit in, since the manifest does not cover the adapter) and `tool_sha256`. A runner outside a Git checkout fails before any launch.

Limits: the runner has not yet run against real agents. The pilot is its first real exercise. Natural-timing runs may meet stream-json shapes the gate has not recorded (for example, a compaction boundary in a long session). The adapter fails closed on those, so the run is recorded as an infrastructure failure.
