# Supported platforms

Falinks is developed on macOS. Support is claimed per component ([ADR 1](adr/0001-develop-on-macos-support-all-oses.md), [ADR 3](adr/0003-platform-support-is-claimed-per-component.md)). Each cell is Supported, Not yet verified (fails closed) or Unsupported by decision. A platform that is not Supported fails visibly. It never falls back to weaker containment.

| Component | macOS aarch64 | macOS x86_64 | Linux x86_64 | Linux aarch64 |
|---|---|---|---|---|
| Engine | Supported | Supported | Supported | Not yet verified (fails closed) |
| Codex adapter | Supported | Not yet verified (fails closed) | Not yet verified (fails closed) | Not yet verified (fails closed) |
| Claude Code adapter | Supported | Not yet verified (fails closed) | Supported | Supported |
| Evaluation harness | Supported | Not yet verified (fails closed) | Not yet verified (fails closed) | Not yet verified (fails closed) |

The engine's Linux x86_64 cell rests on the `linux` CI job (#54). Linux aarch64 has no CI job; the backend is only type-checked there. Windows is planned through WSL 2, which is Linux; it is not yet verified. Native Windows and the BSDs are unsupported by decision (#34, #35). The evaluation harness moves to the per-OS backends in #56. The Claude Code adapter's Linux cells rest on the daily live runs of `adapter-linux.yml` on `main` and on manual runs, on the Ubuntu 24.04 x86_64 and arm64 runners (#57, #115). If the latest live run on `main` fails for a reason other than a model refusal, those cells are unverified until a run passes; open a bug. On those runners, the Codex adapter's sandbox controls passed up to its first model turn; its model turns were not run, because there was no Codex login.

## Requirements

- **Git** 2.32 or newer on `PATH`. It is resolved once per process.
- **macOS:** `/usr/bin/sandbox-exec`. Contained commands must not run inside an outer sandbox that forbids installing another profile.
- **Linux:** kernel 6.2 or newer with Landlock in the LSM list (`/sys/kernel/security/lsm`) and Landlock ABI 3 or newer. Debian 12 (6.1) and stock RHEL 9 fail closed. No root and no user namespaces are needed. Ship the `falinks-contain` helper beside the host executable, or set its path with `configure_containment`.
- **Linux mounts:** the controller state and live workspaces must not be reachable through another mount, such as a bind mount, because required checks may read everything outside those roots. The engine does not enforce this.
- **Linux adapters:** Codex and Claude Code run worker commands under bubblewrap (`bubblewrap` package; Claude Code also needs `socat`). bubblewrap needs unprivileged user namespaces. Ubuntu 24.04 blocks them through AppArmor by default (`kernel.apparmor_restrict_unprivileged_userns=1`), and both worker sandboxes then fail closed with `bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted` (#57). Setting the sysctl to `0` needs root once. The engine needs none of this.

## Known differences

- **Escaped descendants.** On both platforms, a descendant that leaves the command's process group (for example with `setsid`) is swept and reported as ambiguous completion. Linux finds it as an orphan adopted by the helper (subreaper). macOS finds it by its inherited Seatbelt profile: after the command exits, the engine lists every process and uses the private `sandbox_check` call to keep those that may write the command's output but not its evidence (#58). A process outside the profile would not be found; we know of no way for a contained process to leave its Seatbelt profile.
- **Directory listings.** On Linux a contained command cannot list `/` or the ancestors of its readable trees; Seatbelt allows that. Reading files inside readable trees is unaffected.
- **Signals.** On both platforms a contained command can signal other processes of the same user.
- **Worker storage denial.** Seatbelt denies protected storage with `EPERM`. On Linux, Claude Code hides each read-denied directory behind an empty tmpfs. A worker read there finds nothing, and a worker write lands in the mask and is discarded. The adapter check accepts that only after the host confirms the real file is unchanged (#57). Writes to read-only paths fail with `EROFS`, and renames between them and scratch fail with `EXDEV`.

## Adapter rows

Each adapter accepts only binaries whose SHA-256 is pinned for the running OS and architecture (`std::env::consts::{OS, ARCH}`). Any other platform fails with an unsupported-platform reason before the binary is hashed or run. The pins live in `PINS` in `adapters/codex/runtime.rs` and `adapters/claude/lib.rs`. The adapter tests derive the two adapter rows from those pins and fail when they drift. The engine and evaluation-harness rows are not tested. Review, not a test, enforces that a platform gets a pin only after the adapter's controls have been re-run there.
