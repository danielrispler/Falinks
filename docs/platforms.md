# Supported platforms

Falinks is developed on macOS and supported per component ([ADR 1](adr/0001-develop-on-macos-support-all-oses.md), [ADR 3](adr/0003-platform-support-is-claimed-per-component.md)). This file is the one current list. A platform that is not marked Supported fails closed there; it never runs with weaker containment.

| Component | macOS (arm64) | Linux (x86_64, arm64) |
| --- | --- | --- |
| Engine | Supported: `macos` CI job | Supported: `linux` CI job (#54) |
| Codex adapter | Supported: pinned binary (#11) | Not yet verified (#55, #57) |
| Claude Code adapter | Supported: pinned binary (#32, #33) | Not yet verified (#55, #57) |
| Evaluation harness | Supported (#19) | Not yet verified (#56) |

**Windows** is supported through WSL 2, which runs a Linux kernel and uses the Linux column. Native Windows and the BSDs are unsupported by decision (#34, #35).

## Requirements

- **Git** 2.32 or newer on `PATH`. It is resolved once per process.
- **macOS:** `/usr/bin/sandbox-exec`. Contained commands must not run inside an outer sandbox that forbids installing another profile.
- **Linux:** kernel 6.2 or newer with Landlock in the LSM list (`/sys/kernel/security/lsm`) and Landlock ABI 3 or newer. Debian 12 (6.1) and stock RHEL 9 fail closed. No root and no user namespaces are needed. Ship the `falinks-contain` helper beside the host executable, or set its path with `configure_containment`.
- **Linux mounts:** the controller state and live workspaces must not be reachable through another mount, such as a bind mount, because required checks may read everything outside those roots.

## Known differences

- **Escaped descendants.** On Linux, a descendant that leaves the command's process group (for example with `setsid`) is swept and reported as ambiguous completion. On macOS it is not detected; it keeps its sandbox but can outlive the command. Tracked in #58.
- **Signals.** On both platforms a contained command can signal other processes of the same user.
