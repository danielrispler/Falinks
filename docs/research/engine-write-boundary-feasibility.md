# Engine-mediated source writes: narrow feasibility

2026-10-02. Evidence for [Specify live shared-workspace coordination and safe checkpoints](https://github.com/danielrispler/Falinks/issues/11). The selected direction is a live team workspace, including unfinished edits, with engine-mediated mutations and explicit coherent publication checkpoints. This report tests one enforcement primitive; it does not resolve the human decision or implement the engine.

## Result

Follow-up: [one actual-model tool-bridge trial](engine-tool-bridge-trial.md) exercised a different path using an ordinary trusted app-server, thread-level source restrictions and a host dynamic edit tool. Its bounded successful result and native/child coverage gaps do not change the whole-worker startup limit below. Statements about no inference in this report refer to its original sandbox and worker probes.

**Observed on macOS with Codex 0.160.0:** a source subtree under `/private/tmp` remained readable while eight direct mutation attempts failed with `EPERM`. Writes outside that subtree succeeded both inside the working directory and in a separate `/private/tmp` directory. Thus the narrow source read rule overrode an active inherited temporary-directory write grant in this probe.

A host-side fixture then changed the source with a matching expected content hash and rejected a second edit using the stale hash. This demonstrates the separation between a sandboxed reader and a host writer. It does **not** demonstrate authenticated client attribution, a concurrent atomic compare-and-write, or complete protection across actual Codex model tools.

**Whole-worker startup remains blocked before initialization:** the first attempt could not initialize default SQLite state under the user's `.codex`. A retry redirected SQLite and logs to documented scratch paths without changing OS permissions; SQLite files were created, but the worker still exited with an unidentified `Operation not permitted`. No whole-worker filesystem API or model-tool enforcement was demonstrated.

## Interface and reproducible check

Pinned executable: `/opt/homebrew/Caskroom/codex/0.160.0/bin/codex`; version `codex-cli 0.160.0`; SHA-256 `112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b`. `/usr/bin/python3` supplies the stdlib-only check. No dependencies, inference, model turns, credential inspection, global config changes, or main-worktree edits were used.

Runnable asset: [check.py](engine-write-boundary-assets/check.py). It creates fresh task-specific directories, valid sacrificial source files, and a symlink to an existing source file. The installed `codex sandbox --help` exposes `-P/--permission-profile` and `-C/--cd`; this version has no `sandbox macos` subcommand. The operative invocation is:

```sh
codex sandbox -P falinks-boundary-trial -C <trial-root> \
  -c 'permissions.falinks-boundary-trial={extends=":workspace",filesystem={"<trial-root>/source"="read"},network={enabled=false}}' \
  -- /usr/bin/python3 -c <probe-program> <trial-root> <outside-tmp-control>
```

The inline TOML profile is invocation-local and extends the built-in workspace profile. Current beta permission docs describe more-specific filesystem entries overriding broader entries, `read` allowing reads while disallowing mutation, and inherited `:tmpdir`/`:slash_tmp` write grants. [Permission profiles](https://learn.chatgpt.com/docs/permissions). The positive outside-workspace control establishes that this run retained a usable temporary-directory write path; the source protection was not obtained by making everything unwritable.

Replay into a new evidence filename; the first command needs an environment where native Seatbelt setup is permitted:

```sh
python3 docs/research/engine-write-boundary-assets/check.py /private/tmp/falinks-boundary-replay.json
python3 docs/research/engine-write-boundary-assets/check.py --verify /private/tmp/falinks-boundary-replay.json
```

Initial nested execution could not start Seatbelt: `sandbox-exec: sandbox_apply: Operation not permitted`. This setup failure is retained in [blocked.json](engine-write-boundary-assets/blocked.json); it is not a denied-write result. A reviewed scratch-only outer launch then permitted the **same restrictive inner sandbox** to start. The successful final evidence is [enforced.json](engine-write-boundary-assets/enforced.json), using source `/private/tmp/falinks-write-boundary-qofe1jdn/source` and outside control `/private/tmp/falinks-tmp-control-ct8sffm4/control.txt`. No source policy was broadened to obtain the successful result.

## Observed operations

| Operation | Result |
| --- | --- |
| Read known source file | Succeeded, exact original bytes |
| Write scratch file in working directory | Succeeded |
| Write control in a separate temporary directory | Succeeded |
| Direct source-file write | `EPERM` |
| Delete existing source file | `EPERM` |
| Rename existing source file | `EPERM` |
| Replace source file with a writable external file | `EPERM` |
| Create symlink inside source | `EPERM` |
| Write through an external symlink targeting source | `EPERM` |
| Rename whole source directory | `EPERM` |
| Known child launched in a new process session writes source | `EPERM`; child completed |
| Source manifest/content before controller edit | Unchanged |
| Host edit with expected hash | Accepted; new content hash recorded |
| Second host edit with stale expected hash | Rejected; accepted contents preserved |

Saved-evidence verification passed. The host edit uses one caller, checks a hash, and replaces one known fixture file. The `client-a` label is synthetic test data. Production needs a trusted client identity, a serialized check/write queue, validated paths, and durable events; the script deliberately implements none of those services. The new-session child is one inheritance check, not exhaustive detached-descendant coverage.

## Tool-boundary feasibility and limits

The installed generated `v2/ThreadStartParams.ts` exposes named `permissions`, `runtimeWorkspaceRoots`, and `dynamicTools`. `v2/TurnStartParams.ts` can select a permissions profile for later turns. `v2/DynamicToolCallParams.ts` carries thread, turn and call identifiers for client-executed tools. These are useful surfaces for a host controller to expose an edit operation and associate requests with sessions. They do not establish that the model will select it correctly, or that supplying it removes native editing tools. [Dynamic tool flow](https://learn.chatgpt.com/docs/app-server#dynamic-tool-calls-experimental).

**Host-controller inference:** a controller outside the agent's restrictive source policy can perform accepted mutations while ordinary agent shell commands continue reading the live source. Formatters/generators must run as controlled engine jobs with an explicit affected-file boundary and completion rule; their privileged execution cannot be handed to arbitrary agent commands. The observed fixture supports the basic privilege split, not those job semantics or publication safety.

**Untested actual-model behavior:** no turn was run, so native `apply_patch`, ordinary read tools, formatter/generator dispatch, MCP writers, child agents, permission escalation, and the complete live app-server profile wiring were not exercised. This probe must not be described as proof that built-in source writes are prevented in the final integration. The installed `command/exec` schema documents standalone sandboxed execution without a turn; even a successful command test would not establish all model-tool enforcement. [Command execution](https://learn.chatgpt.com/docs/app-server#command-execution), [sandbox scope](https://learn.chatgpt.com/docs/sandboxing).

Dynamic tools add a client call path; they are not a replacement guarantee for built-ins. Pre-tool hooks can help deny supported calls, but official documentation lists specialized-path exceptions and says `write_stdin` does not repeat pre-tool interception. Hooks therefore cannot close this enforcement gap by themselves. [Tool coverage](https://learn.chatgpt.com/docs/hooks#tool-coverage).

### Whole-worker containment: exposed launcher, observed startup limit

The installed help exposes `--stdio`; the check actually attempted to put the **app-server worker itself** under the same source-read OS profile:

```sh
codex sandbox -P falinks-worker -C <team-root> -- codex app-server --stdio
```

The worker exited with code 1 before any JSON-RPC response. Its stderr was:

```text
Error: failed to initialize sqlite state runtime under /Users/danielrispler/.codex: failed to initialize state runtime at /Users/danielrispler/.codex
```

Initial evidence: [worker.json](engine-write-boundary-assets/worker.json), [worker.stderr.log](engine-write-boundary-assets/worker.stderr.log). The harness prepared valid sacrificial source content and sent only `initialize`; startup failed before it could exercise `fs/readFile` or `fs/writeFile`.

Official configuration docs provide `sqlite_home` for runtime databases and `log_dir` for logs. The parent authorized a narrow retry with invocation-local overrides, creating only `<trial-root>/worker-state` and `<trial-root>/worker-logs`; the same OS source-read rule was retained. [Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference), [sample configuration](https://learn.chatgpt.com/docs/config-file/config-sample).

```sh
# Additional argv on the INNER app-server only:
-c 'sqlite_home="<trial-root>/worker-state"' -c 'log_dir="<trial-root>/worker-logs"'
```

Retry evidence: [worker-scratch-state.json](engine-write-boundary-assets/worker-scratch-state.json), [worker-scratch-state.stderr.log](engine-write-boundary-assets/worker-scratch-state.stderr.log). Runtime SQLite files appeared in `/private/tmp/falinks-worker-boundary-cbu2z10k/worker-state`; their contents were not inspected. The worker then exited 1 before any RPC response with `Error: Operation not permitted (os error 1)`. The remaining denied resource is **unidentified**. Source contents remained unchanged; no grant was broadened to diagnose or bypass the denial.

Neither attempt started an ephemeral thread, registered a dynamic tool, started a turn, or requested nested sandbox execution. No home/source permissions were broadened; `HOME`/`CODEX_HOME` and global config stayed unchanged. The investigation stopped after the supported scratch-state override still failed.

```sh
python3 docs/research/engine-write-boundary-assets/check.py --worker-default /private/tmp/falinks-worker-default-replay.json
python3 docs/research/engine-write-boundary-assets/check.py --worker /private/tmp/falinks-worker-replay.json
# Current profile: both recorded probes ended with startup failure, exit 2.
python3 docs/research/engine-write-boundary-assets/check.py --verify-worker /private/tmp/falinks-worker-replay.json
```

Both saved-startup-limit verifications passed; they check zero RPC responses, a nonzero worker exit, and the recorded error for each configuration. This verifies the recorded **limits**, not source enforcement by a working app-server.

**Diagnostic-only follow-up:** installed `codex sandbox --help` explicitly exposes `--log-denials`. The same scratch-state worker probe was repeated with only that instrumentation added to its sandbox command. A PID tracker identified the worker process; captured diagnostics were retained only for that PID or the exact trial root, plus the command's own setup/error messages. Unrelated log content was redacted before persistence. Codex printed `=== Sandbox denials ===` but no detailed denial matched either filter. A repeat accepting all numeric PID formats gave the same limit.

Final diagnostic evidence: [worker-denials-pid-filter.json](engine-write-boundary-assets/worker-denials-pid-filter.json), [filtered stderr](engine-write-boundary-assets/worker-denials-pid-filter.stderr.log); earlier diagnostic [worker-denials.json](engine-write-boundary-assets/worker-denials.json) is preserved. Both ended with worker exit 1, zero RPC responses, generic `Operation not permitted`, and unchanged source. Detailed denial capture was unavailable for this short probe; absence of a matching trace does not establish that no denial occurred. The exact path/syscall remains unknown, so the evidence cannot distinguish another home runtime/cache path, required Unix socket/IPC, or a startup service/syscall. No further config override is justified by this trace.

```sh
python3 docs/research/engine-write-boundary-assets/check.py --worker-denials /private/tmp/falinks-worker-denials-replay.json
python3 docs/research/engine-write-boundary-assets/check.py --verify-worker /private/tmp/falinks-worker-denials-replay.json
```

**Remaining inference from the command/child probe:** restricting a working app-server process should constrain source writes in that process and local descendants regardless of individual `apply_patch` callbacks; trusted dynamic edit handlers could run in the outside host. Both observed startup failures prevent calling that integration supported or verified as-is.

The generated `SandboxPolicy` exposes `externalSandbox`, and official docs describe it as skipping Codex's own sandbox when the server is already externally sandboxed. It was **not selected**: the attempted filesystem API calls needed no nested command sandbox, and it would not solve denied state initialization. [App-server transport and external sandbox](https://learn.chatgpt.com/docs/app-server).

Still to establish without broad home/source grants: identify the remaining startup denial and obtain successful initialization, then test host filesystem APIs, exact writable cache/session/log/socket paths outside source, permitted authentication reads/network access, local Code Mode host and IPC, dynamic-call handling, native tools and child lifetimes. A privileged daemon, external MCP server, remote execution host, or broad controller command handler lies outside this worker's OS boundary and must not become an alternate source writer. The test's `network.enabled=false` profile is insufficient for an inference worker as written. Do not equate observed command-level enforcement with verified whole-worker containment.

Before calling the selected integration verified, exercise the active model's native patch and shell paths under the same source read profile, verify no approval/escalation or alternate host/MCP path can mutate source, and test controller-only generator jobs and safe checkpoints. Unknown descendants, aliases beyond the tested symlink, filesystem races, other platforms, trusted external editors, crash recovery, and publication durability remain unsupported here. This is a bounded feasibility finding, not a benchmark or production engine.

Artifacts remain in `/private/tmp/falinks-stable-view-feasibility-w_fi1lvg` on scratch branch `research/stable-view-feasibility`. This research agent made no main-worktree changes; main HEAD remains `caabadc245e05d8444c4e5c63a2356710e20bcd0`, while the parent session concurrently edited `CONTEXT.md`. No tracker writes, publication, or merge occurred.
