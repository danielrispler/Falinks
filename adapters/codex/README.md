# Bounded Codex adapter

Issue [#23](https://github.com/danielrispler/Falinks/issues/23) implements the adapter slice of [#19](https://github.com/danielrispler/Falinks/issues/19). Python's standard library supplies the host transport and controlled checks; the production coordination engine remains Rust and is not implemented here.

## Run

Use macOS arm64 and the official `openai/codex` release `rust-v0.160.0`. Extract `codex-aarch64-apple-darwin.tar.gz` and `codex-code-mode-host-aarch64-apple-darwin.tar.gz` into a dedicated directory. Name the companion `codex-code-mode-host` alongside the CLI. The adapter checks both executable hashes and the emitted experimental protocol schema before connecting. It never falls back to the installed CLI.

```sh
PYTHONPYCACHEPREFIX=/private/tmp/falinks-pycache python3 -m unittest discover -s adapters/codex -v
PYTHONPYCACHEPREFIX=/private/tmp/falinks-pycache python3 -m py_compile adapters/codex/*.py
PYTHONPYCACHEPREFIX=/private/tmp/falinks-pycache python3 adapters/codex/check_adapter.py \
  --binary /path/to/codex-aarch64-apple-darwin --output /path/to/current-controls.json
```

The verifier needs permission to launch macOS Seatbelt sandboxes, network access for two bounded real-model turns, and an existing Codex `auth.json`. It uses an isolated `CODEX_HOME` with a link to the existing credentials; it neither prints credentials nor changes the installed CLI/configuration. Optional apps, browser, computer, image generation, subagents, MCP and web search are disabled. The ordinary tool host remains enabled because the current model router requires it. No API key or new dependency is needed. Missing inference access or unavailable tools fails the check.

Fixtures and evidence are retained under `/private/tmp/falinks-adapter-*`. Controller/context databases, snapshots, validation files and runtime state sit outside agent access. Source is readable; distinct scratch is writable. The executable bundle is read-only. Evidence retains only fixture tool/command outcomes and configuration, excluding account and reasoning events. Inspect `passed` and the failure reason; exit status is nonzero for an unsupported or failed check.

## Engine-facing boundary

`Boundary.dispatch` accepts only the three registered tools: edit, explicit review and offer. The host registers an agent/workspace binding and establishes the active runtime turn. It supplies `{identity, operation, request}` to a trusted engine callback. `identity` contains agent, fresh connection session, runtime thread/turn and the canonical authorized workspace. Neither model arguments nor author strings can choose identity. Unknown tools, stale turns, another thread/workspace and identity-bearing request fields are rejected before dispatch. The engine must validate operation contents and revisions; no generic privileged filesystem or shell callback exists.

`Runtime` defaults to blocking engine calls until its in-memory `ControlGate` has every fresh required control. The trusted verifier opts into `control_host=True` only for `ControlledHost`, whose edit is restricted to one sacrificial file and fixed bytes. This switch is a trusted host API, never an agent tool or caller argument. Production integration must run the controls against its actual workspace/profile and guard publication/scoring with `require_supported`; it must not import a JSON report to authorize another host. A failed control stays latched even if subsequently marked passing. Disconnect and disabled-capability attempts also revoke capability.

Host context is durable SQLite state keyed by runtime thread and stable event ID. Enqueue precedes exact-turn attention. Transport acceptance and prompt presentation do not clear context. Only an engine-accepted explicit defer or keep/revise/drop updates it; the engine remains responsible for matching source revisions and reconsideration obligations. Completion races retain the notice for the next `turn/start`. Resume checks the saved agent/workspace binding, restores the named profile and workspace roots, uses the runtime's persisted tool definitions, and prepends all pending/deferred host context before operations. Duplicate identical events preserve their original status; changed contents under the same ID fail.

## Controls and limits

The runnable check verifies current binary/schema, effective named profile/roots, disabled features, native helper denial with a writable positive control, source reads, shell and new-session child write denial, source creation/deletion/rename/replacement/alias denial, controller/snapshot/validation read and write denial, and scratch access. Actual initial and resumed model turns execute the exact machine-readable sandbox probe through the app-server command tool, verifying source/child write denial, protected storage, aliases and writable scratch before those controls can grant capability. They also exercise mediated editing, active steering, explicit deferral, offer rejection and resumed reconsideration/replay. A real completed turn's ID exercises the steering completion race. Reconnect repeats local controls against the same directories and restores obligations before related tools. A separate trusted injection checks detection before capability grant; the final deliberate live-host injection preserves unknown bytes and revokes capability, leaving that disposable host stopped.

This is a finite trusted-host boundary, not containment of an arbitrary worker. App-server and the host remain trusted and can access source/state outside the agent sandbox. The CLI `sandbox` command is run from scratch; setting its working directory to the read-only source can grant source writes through workspace-root resolution. App-server controls separately verify its explicit runtime roots and source working directory. Registration, a matching profile name or a historical report alone cannot pass startup.

The controlled host implements only a fixed one-file edit and revision-bound fixture obligations. Production multi-file installation, attribution/history, idempotent engine requests, compiler relevance, publication, validation and the complete handled cursor belong to downstream engine/integration tickets. The fixture inventory scan detects unexpected bytes, paths, modes and aliases at guarded operations; it does not detect every transient write or attribute arbitrary writers. Production integration must supply its authoritative completed-revision inventory and stop on unknown changes.

Protocol reference: [official app-server documentation](https://learn.chatgpt.com/docs/app-server). The pinned binary's generated experimental schemas, not evolving documentation, determine compatibility. Recorded evidence in `evidence/` is review material; rerun the verifier for current capability.

`evidence/2026-10-10-controls.json` supersedes the October 8 run, which lacked actual app-server storage/scratch probes. The older report and failed attempts remain for audit; they cannot satisfy the corrected startup gate.
