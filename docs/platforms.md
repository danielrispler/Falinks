# Supported platforms

Falinks is developed on macOS. Support is claimed per component (#35). Each cell is Supported, Not yet verified (fails closed) or Unsupported by decision. A platform that is not Supported fails visibly. It never falls back to weaker containment.

| Component | macOS aarch64 | macOS x86_64 | Linux x86_64 | Linux aarch64 |
|---|---|---|---|---|
| Engine | Supported | Supported | Not yet verified (fails closed) | Not yet verified (fails closed) |
| Codex adapter | Supported | Not yet verified (fails closed) | Not yet verified (fails closed) | Not yet verified (fails closed) |
| Claude Code adapter | Supported | Not yet verified (fails closed) | Not yet verified (fails closed) | Not yet verified (fails closed) |

Windows is supported through WSL, which is Linux. Native Windows is a later decision.

## Adapter rows

Each adapter accepts only binaries whose SHA-256 is pinned for the running OS and architecture (`std::env::consts::{OS, ARCH}`). Any other platform fails with an unsupported-platform reason before the binary is hashed or run. The pins live in `PINS` in `adapters/codex/runtime.rs` and `adapters/claude/lib.rs`. The adapter tests derive the two adapter rows from those pins and fail when they drift. The engine row is not tested. Review, not a test, enforces that a platform gets a pin only after the adapter's controls have been re-run there.
