# 1. Develop on macOS, support all OSes, staged by milestone

Status: accepted

## Context

Falinks is developed on macOS, and its first engine, adapter and evaluation harness rely on macOS facilities such as `sandbox-exec` (#11, #19). Future users must be supported on every major OS, so the macOS-only start must not harden into a permanent limit.

## Decision

- **Development platform:** macOS.
- **Supported platforms before the first external release:** macOS and Linux (#35).
- **Windows:** supported through WSL; native Windows is deferred to sandbox-backend research (#34).
- **Until a platform is supported**, Falinks fails closed there rather than running unconfined or partially working.

## Consequences

- CI runs the full checks on macOS and a compile-only Linux job until Linux tests land with #35, so portability drift surfaces early.
- New platform-specific code needs a fail-closed path on other OSes.
- Native Windows support requires the #34 research to pick a sandbox backend first.
