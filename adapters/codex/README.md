# Bounded Rust Codex adapter

Issue [#23](https://github.com/danielrispler/Falinks/issues/23) supplies the adapter slice of [#19](https://github.com/danielrispler/Falinks/issues/19). All maintained implementation, verification, probes, child-process checks, tests and Wayfinder tooling are Rust. Production coordination-engine integration remains downstream.

## Run

Use a pinned platform from [`docs/platforms.md`](../../docs/platforms.md) (today macOS arm64) and the official `openai/codex` release `rust-v0.160.0`. Extract `codex-aarch64-apple-darwin.tar.gz` and `codex-code-mode-host-aarch64-apple-darwin.tar.gz` into a dedicated directory. Name the companion `codex-code-mode-host` alongside the CLI. Startup checks both executable hashes and the emitted experimental protocol schema; it never substitutes the installed CLI.

```sh
cargo check --all-targets
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build --release --bins
./target/release/check-adapter \
  --binary /path/to/codex-aarch64-apple-darwin \
  --output /path/to/current-rust-controls.json
cargo run --bin wayfinder-startup -- --check
```

The verifier expects its sibling `sandbox-probe` executable; `--probe /path/to/sandbox-probe` can select another build explicitly. It copies the compiled Rust probe into the fixture's read-only `worker-tools` directory and records its hash. Both standalone and actual app-server command tools execute that same program. Its new-session child is the same Rust executable; no language interpreter or embedded non-Rust script is involved.

Runtime verification needs permission to launch macOS Seatbelt sandboxes, network access for two bounded real-model turns, and existing Codex authentication. An isolated `CODEX_HOME` links the existing `auth.json` without printing credentials or replacing the installed CLI/configuration. Apps, browser, computer, image generation, subagents, MCP and web search are disabled. The ordinary code-mode tool host stays enabled because the model router requires it. Extra permission requests are declined. Missing inference access or unavailable tools fails visibly.

Fixtures and server diagnostics remain under `/private/tmp/falinks-rust-adapter-*`. Evidence records sacrificial tool/command outcomes, hashes and configuration, excluding account and reasoning events. Inspect `passed` and the failure reason; failed checks exit nonzero. Old Python-era JSON is historical data, not executable code or startup authorization.

## Engine-facing boundary

`Boundary::authenticate` accepts only edit, explicit review and offer tool calls from the bound runtime thread and active turn. The trusted host supplies agent, fresh connection session and canonical workspace. Model arguments cannot select identity or expose a generic privileged filesystem/shell operation. A successful request produces `{identity, operation, request}` for an engine callback; the engine still validates contents, paths, revisions and obligations. `Boundary::record_result` updates context only for engine-accepted explicit defer/keep/revise/drop outcomes.

`Runtime` blocks engine dispatch until its fresh in-memory `ControlGate` passes every required category. A failure latches for that runtime; disconnects and attempted disabled capabilities revoke authority. The verifier uses the trusted `control_host` flag only for the fixed one-file `ControlledHost` while measuring startup controls. This host API is never an agent argument or tool. Production integration must run controls against the actual workspace/profile and guard publication/scoring with `require_supported`; a saved JSON report cannot authorize another process.

Host context uses SQLite keyed by registered thread and stable event identity. Enqueue commits before exact-turn steering. Transport acceptance and presentation never clear obligations. A supported completion/mismatch race leaves context pending for the next `turn/start`. Resume verifies agent/workspace binding, restores the named profile and runtime roots, uses persisted dynamic-tool definitions, and prepends pending/deferred context before related operations. Identical event replay preserves status; changed contents under the same event ID fail.

The engine's review acceptance and the adapter's context update are separate database commits. This fixed host does not implement general engine request idempotency or reconcile that crash gap; production integration must supply those durable-outcome guarantees.

## Controls and limits

The compiled probe checks source reads, scratch writes, shell/child source write denial, source creation/deletion/rename/replacement/alias denial, and controller/snapshot/validation read/write denial. Native helper denial has a writable positive control. Actual initial and resumed model turns execute the exact Rust probe command successfully before filesystem controls grant capability. Command arguments and all 15 observation names/results must match; no textual model claim substitutes for a tool outcome.

The real turns also exercise authenticated editing, active-turn steering, explicit deferral, offer rejection, and restored tools/reconsideration/replay. An already completed real turn ID deterministically verifies the steering race and next-boundary deferral. Separate trusted host injection verifies unknown-change detection before capability grant; final live-host injection preserves unknown bytes, stops the controlled host and revokes capability.

The CLI sandbox is launched from scratch. Using read-only source as its cwd can grant writes through workspace-root resolution, so actual app-server controls independently verify source cwd and explicit roots. Matching profile names and registration alone are insufficient.

This is a finite trusted-host boundary, not arbitrary-worker containment. App-server and the host are trusted. The fixed host enrolls one sacrificial file; its guarded inventory covers files/directories, modes, symlinks and hard links, not every transient external write. Production multi-file installation, compiler relevance, attribution/history, complete handled cursors, exact validation and publication belong to the engine/integration tickets. The fixture's eligible offer is not publication.

The JSON, SHA-256 and SQLite crates provide their established formats/mechanisms; Rust's standard library supplies process management, transport, channels and filesystem operations. No custom JSON parser, hashing algorithm or async framework is introduced.

Protocol reference: [official app-server documentation](https://learn.chatgpt.com/docs/app-server). The pinned binary's emitted experimental schemas determine compatibility. Dated evidence remains audit material; current capability always requires a fresh run of the Rust verifier.
