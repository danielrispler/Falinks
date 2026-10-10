# 2. Linux containment runs in a helper executable

Status: accepted

## Context

On Linux, contained commands use Landlock, seccomp and a per-command subreaper (#34). The engine is a library inside a multithreaded host. It must not become a subreaper itself: reaping adopted orphans with `waitpid(-1)` would steal other threads' child statuses.

## Decision

A single-threaded `falinks-contain` helper executable, built from the engine crate, launches every contained command on Linux. It installs seccomp and Landlock in its child, enforces the time bound, sweeps adopted descendants and reports the outcome. The host gives the helper's path, defaulting to the helper beside the running executable. A missing helper or a protocol-version mismatch fails closed.

## Considered options

- **Forked launcher inside the engine process (rejected).** It needs no second executable, but its post-fork code may not allocate or take locks while other engine threads run. A violation deadlocks only intermittently, so tests cannot prove it absent. It would also rely on undocumented details of how `std::process::Command` reports exec errors.
- **Helper executable (chosen).** It matches the structure the #34 probes verified: a separate process installs the controls and then execs. A packaging fault fails closed and visibly; a post-fork fault would fail silently and only sometimes.

## Consequences

- Linux hosts ship and locate a second executable. Integration tests get it from Cargo.
- Hashing the helper adds nothing: anyone who can replace it can replace the engine. The version handshake guards against skew.
- macOS keeps `sandbox-exec`, which already runs as a separate executable.
