# Supported platforms

Falinks is developed on macOS. Support is claimed per component (#35). A platform that is not Supported fails closed with a visible unsupported-platform reason. It never falls back to weaker containment.

| Component | macOS | Linux |
|---|---|---|
| Engine | Supported | Not yet verified (fails closed) |
| Codex adapter | Supported (aarch64) | Not yet verified (fails closed) |
| Claude Code adapter | Supported (aarch64) | Not yet verified (fails closed) |

Windows is reached through WSL, which is Linux. Native Windows is a later decision.

## Adapter rows

Each adapter accepts only binaries whose SHA-256 is pinned for the running OS and architecture (`std::env::consts::{OS, ARCH}`). The pins live in `PINS` in `adapters/codex/runtime.rs` and `adapters/claude/lib.rs`. A platform gets a pin only after the adapter's controls have been re-run there. The adapter tests derive the rows above from those pins and fail when this table drifts.
