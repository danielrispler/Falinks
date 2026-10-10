# Issue #24 implementation review

Fixed point: `b9e304a91e0d1f835ff937101b916a4ff62b46df`, the new worktree's starting commit.
Two independent code-review agents reviewed Standards and Spec, then rechecked the fixes.

## Standards

Hard documented violations: none. Actionable complexity smells: none.
One correctness finding: per-agent usage overwrote previous turns. Fixed by
retaining per-turn reports and distinguishing partial reporting. The CLI
regression checks that all runtime turn reports survive in the result.
Final verdict: pass; no concrete remaining blocker found.

## Spec

Initial findings: telemetry receipts could wake waiting models; usage retained
only the final turn. Both are fixed. The follow-up isolation finding—configured
prior-run exclusions did not reach compilation/tests—is fixed and covered by
an oracle CLI regression. Host Git subprocesses run within the protection
boundary; raw Git tree blobs preserve committed bytes regardless of archive
attributes; captured source and oracles are read-only during checks.
Final verdict: no remaining blocking findings within issue #24's setup scope.
Authenticated model runs, runtime capability gates, engine integration and
scored evaluation remain explicitly unverified downstream work.

Summary: Standards 0 unresolved; Spec 0 unresolved. Execution evidence is in
`verification.json`; reviewers inspected code and tests rather than rerunning them.

## Rust port (2026-10-10)

The harness (`run.py`), Codex bridge (`codex_worker.py`) and CLI tests were
ported to the standalone Rust crate in this directory; fixture, protected and
instruction bytes are unchanged. Behavior is a one-to-one port of the reviewed
Python, with two deliberate differences: checks default `RUSTUP_HOME` to the
host's `~/.rustup` (rustup proxies otherwise resolve toolchains through the
isolated `HOME`), and the scripted run timeout in tests is 90 seconds because
Rust runs the six integration tests in parallel.

`freeze` was rerun because harness bytes are hashed: only harness/crate hashes
changed; all three initial commits reproduce identically. No scored batch has
used the previous manifest. A later `cargo fmt` pass refroze harness hashes again
with initial commits unchanged. A mutation removing the controller denial makes the
baseline isolation test fail. The port was not re-reviewed by independent
Standards/Spec agents.
