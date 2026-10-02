# Checked live edits — THROWAWAY, issue #15

Question: can the simplest engine-mediated check-and-write operation prevent
lost updates while two simulated clients work concurrently on live shared source?

From the repository root, with Rust installed:

```sh
cargo run --offline --manifest-path prototype/checked-live-edits/Cargo.toml
```

This dependency-free command runs the prototype and its assertion-based check.
Every read, accepted edit, and conflict prints the relevant revision and complete
source bytes; conflicts also print the preserved proposed edit. Assertions are
active in release builds too. A barrier ensures both clients read before either
submits a mutation. The lock winner can vary; the observations and assertions
do not depend on which client wins. There are no timing sleeps.

## Observations and verdict

- Different files: both clients finish, both accepted edits remain, zero retries.
- Same target from the same revision: exactly one write succeeds. The stale
  request retains its path, expected revision, replacement, and current source;
  the accepted source remains unchanged.
- Disjoint targets in the same file: exactly one initial write conflicts.
  The losing client rereads and rebuilds its replacement from current bytes;
  one retry preserves both edits. Changing only the rejected edit's expected
  revision would overwrite the peer's work.
- A subsequent ordinary read observes completed shared edits, including the
  peer's edit when the losing client rereads before retrying.
- A deliberately incomplete Rust source string is editable and remains
  incomplete after acceptance. Intermediate compilation is not checked.

**Verdict:** yes, within this model. A single mutex covers revision comparison
and replacement, preventing two stale proposals from both being accepted.
Revision numbers belong to files, so edits to another file do not invalidate a
proposal. This does **not** establish independent-symbol acceptance within a file.

## Limits

- One global mutex serializes every source read and mutation, including different
  files. It provides mutual exclusion, not a FIFO queue or a fairness guarantee.
  Client preparation runs concurrently outside the lock; there is no throughput
  or latency claim.
- The live workspace is an in-memory map of UTF-8 fixture strings. Only the engine
  mutates it. No real filesystem/tool-boundary enforcement is demonstrated.
- The experimental edit is a whole-file replacement plus a per-file expected
  revision. Files are fixed for this probe; revisions last only for the process.
  No automatic merge, symbol tracking, file creation/deletion, or external edits.
- Retry rebuilding is supplied by the two known fixture edits. It is not a
  general conflict-resolution algorithm and cannot safely resolve competing
  intentions for the same target.
- No model integration, compiler admission, formatter/generator, permissions
  work, publication/checkpoints, persistence/recovery, split/join scheduling,
  UI, or performance benchmark. No production engine decision is implemented.

Primary source: branch `prototype/checked-live-edits`; ticket
[danielrispler/Falinks#15](https://github.com/danielrispler/Falinks/issues/15).
