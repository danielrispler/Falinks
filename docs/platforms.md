# Supported platforms

Falinks is developed on macOS. Support is claimed per component ([ADR 1](adr/0001-develop-on-macos-support-all-oses.md), [ADR 3](adr/0003-platform-support-is-claimed-per-component.md)). Each cell is Supported, Not yet verified (fails closed) or Unsupported by decision. A platform that is not Supported fails visibly. It never falls back to weaker containment.

| Component | macOS aarch64 | macOS x86_64 | Linux x86_64 | Linux aarch64 |
|---|---|---|---|---|
| Engine | Supported | Supported | Supported | Not yet verified (fails closed) |
| Codex adapter | Supported | Not yet verified (fails closed) | Not yet verified (fails closed) | Not yet verified (fails closed) |
| Claude Code adapter | Supported | Not yet verified (fails closed) | Not yet verified (fails closed) | Not yet verified (fails closed) |
| Evaluation harness | Supported | Not yet verified (fails closed) | Not yet verified (fails closed) | Not yet verified (fails closed) |

The engine's Linux x86_64 cell rests on the `linux` CI job (#54). Linux aarch64 has no CI job; the backend is only type-checked there. Windows is planned through WSL 2, which is Linux; it is not yet verified. Native Windows and the BSDs are unsupported by decision (#34, #35). The evaluation harness moves to the per-OS backends in #56.

## Requirements

- **Git** 2.32 or newer on `PATH`. It is resolved once per process.
- **macOS:** `/usr/bin/sandbox-exec`. Contained commands must not run inside an outer sandbox that forbids installing another profile.
- **Linux:** kernel 6.2 or newer with Landlock in the LSM list (`/sys/kernel/security/lsm`) and Landlock ABI 3 or newer. Debian 12 (6.1) and stock RHEL 9 fail closed. No root and no user namespaces are needed. Ship the `falinks-contain` helper beside the host executable, or set its path with `configure_containment`.
- **Linux mounts:** the controller state and live workspaces must not be reachable through another mount, such as a bind mount, because required checks may read everything outside those roots. The engine does not enforce this.

## Known differences

- **Escaped descendants.** On Linux, a descendant that leaves the command's process group (for example with `setsid`) is swept and reported as ambiguous completion. On macOS it is not detected; it keeps its sandbox but can outlive the command. Tracked in #58.
- **Directory listings.** On Linux a contained command cannot list `/` or the ancestors of its readable trees; Seatbelt allows that. Reading files inside readable trees is unaffected.
- **Signals.** On both platforms a contained command can signal other processes of the same user.

## Adapter rows

Each adapter accepts only binaries whose SHA-256 is pinned for the running OS and architecture (`std::env::consts::{OS, ARCH}`). Any other platform fails with an unsupported-platform reason before the binary is hashed or run. The pins live in `PINS` in `adapters/codex/runtime.rs` and `adapters/claude/lib.rs`. The adapter tests derive the two adapter rows from those pins and fail when they drift. The engine and evaluation-harness rows are not tested. Review, not a test, enforces that a platform gets a pin only after the adapter's controls have been re-run there.
