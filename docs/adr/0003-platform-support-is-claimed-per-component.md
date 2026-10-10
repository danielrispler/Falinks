# 3. Platform support is claimed per component

Status: accepted

Refines [ADR 1](0001-develop-on-macos-support-all-oses.md).

## Context

Spec #35 claimed Linux support only once the full suite passed on Linux and the adapter checks had been re-run there. Codex and Claude Code sandbox themselves with bubblewrap on Linux, which needs unprivileged user namespaces. Ubuntu 24.04 restricts those by default (#34). The engine's own backend does not need them. Tying the engine's Linux support to the adapters could leave the engine unsupported for reasons outside its control.

## Decision

Support is claimed per component and per platform. The engine is supported on a platform when its CI job passes the full suite there. Each worker-runtime adapter claims a platform separately, after its controls are re-run there (#11). `docs/platforms.md` holds the current matrix; anything not yet verified fails closed.
