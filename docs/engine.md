# Controlled live edits (#20)

The `falinks` Rust library is the host-owned public protocol for this slice. `tests/protocol.rs` supplies two authenticated scripted clients using real temporary source, a bare Git object store and SQLite. No publication capability is enabled: the published pointer stays at the enrolled initial snapshot. The app-server adapter, semantic analysis, notification/review obligations, offers, validation and grouping belong to later tickets.

Run on macOS with Rust and `/usr/bin/git`:

```sh
cargo test
cargo check
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The captured-job checks install their own Seatbelt profile with `/usr/bin/sandbox-exec`. An outer sandbox that forbids installing another profile must run these checks outside that outer sandbox; a denied sandbox installation is a visible job failure, never a fallback to unrestricted execution. Other Unix platforms can use edits/storage, but captured jobs fail visibly as unsupported.

## Host and client boundary

`Engine::open(live, state, enrolled)` acquires a nonblocking OS writer lease and verifies stored history/source before returning. Controller storage must be outside the live tree. The trusted host provisions exactly two credentials, authenticates them once, and routes each runtime session to its opaque `Client`; mutation requests contain no author field. Do not expose credential provisioning, arbitrary job executable/arguments, or fault observers as agent tools. This library does not implement the downstream runtime permission profile.

Enrollment is fixed for an experiment and includes absent paths for future creation. The source universe consists of all files beneath the live root except root `.git` storage. Paths are normalized relative ASCII paths, case-unique, with no hidden/reserved components or overlapping file/ancestor enrollments. Regular files must have one link and mode 0644 or 0755; symlinks, special files, path aliases and other modes fail visibly. Unicode/hidden paths require a later explicit extension rather than silently accepting filesystem normalization aliases. Concurrent unmediated writers are outside the supported boundary; audited unexpected changes halt mutation and preserve an incident containing observed bytes, modes and symlink targets.

## Protocol

- `capture()` returns the last completed occurrence, immutable Git tree, per-path occurrence versions, bytes/modes and blob identities. It verifies retained objects and durably records the capture before returning. Live reads can expose partial installations; coherent captures never do.
- `apply(client, Request)` takes a stable client-scoped request ID, complete output replacements/deletions and expected versions for every input and output. The host supplies the complete file/package input universe; this slice does not infer semantic independence. Adding package members to `expected` checks them atomically alongside target files. A target is always an input. Independent different-file requests can survive the same initial capture.
- `Stale` returns expected versions and current bytes/versions; the durable request retains the entire proposal. Reread, rebuild and submit a new request ID. Restoring old bytes advances occurrence versions even when Git reuses identical blobs/trees. Incomplete source needs no compilation.
- `request()` and `history()` return attributed request contents, scope, outcomes and exact before/proposed revisions. Identical submissions join the short writer operation and return its recorded result. Changed contents under an existing ID fail. An interrupted request can be retried explicitly with `retry()` after restart reconciliation; prior attempts remain retained, and freshness is checked again.
- `run_job()` captures declared inputs from a recorded completed revision, prepares separate read-only input and writable output directories, and retains the complete job record/logs/artifacts. `apply_job()` applies all declared output together through the same freshness gate, including input paths that the job did not write. Live mutations do not hold the job lease.

Jobs are host-enrolled trusted formatter/generator commands, with cleared environment, fixed input/output layout, no network, filesystem read restrictions and output-only writes under macOS Seatbelt. System tool/runtime reads remain allowed. Supported tools keep descendants in the assigned process group and join them before exit; daemonizing or escaping that group is unsupported and must not be enrolled. A surviving group is killed and reported as ambiguous. A 30-second execution bound kills timed-out groups. Failed commands, undeclared/reserved output, unsupported modes/aliases and unavailable sandboxing retain evidence and cannot apply. This is a bounded trusted-tool integration, not containment of arbitrary hostile programs. A restart never adopts output from a job whose completion was not recorded; it retains the directory and records a visible integration error.

## Storage and recovery

SQLite records experiment/workspace identity, authenticated client/session identity, requests/attempts, job inputs/outputs, captures, occurrence metadata, completed and initial published pointers, and incident evidence. Git retains trees through protected `refs/falinks/trees/<tree>` refs. Controlled bytes create changed blobs; unchanged blobs are reused. Capture construction never scans changing live source. Full snapshot metadata/bytes and startup verification are intentionally sized for this small experiment; they are not a large-repository performance claim.

Each edit durably records before/proposed evidence and retains objects before installing any output. Each individual file is staged then renamed; the completed pointer and request outcome commit together only after full installation. A capture during installation returns the preceding completed revision. There is no Git/filesystem/SQLite joint transaction and no arbitrary multi-file crash-atomicity or power-loss claim.

Restart verifies all referenced snapshots and enrollments, then examines every affected and unaffected source path before restoring anything. Only an interrupted operation whose paths match recorded before/after bytes and modes can be restored to its preceding completed state. Its proposed output remains retained for explicit retry. Unknown edits, damaged/missing objects, reserved staging remnants or ambiguous paths stop startup without replacing drafts. Known completed operations stay completed despite a lost response. Incident halts cannot be cleared merely by putting the old bytes back; reconciliation is intentionally a trusted-host task outside this slice.

## Verification evidence

The public-protocol checks cover independent/stale two-client edits, incomplete code, immutable midway capture, unchanged-object reuse, restore occurrence identity, duplicate in-flight submissions, changed-ID contents, restart/retry attempt preservation, unknown-write evidence, ownership/authentication, path/mode controls, failed retention, missing snapshots, job attribution and stale complete-output rejection, denied source/controller/input access, undeclared output and surviving descendants. Actual process exits exercise before-retention, retained, midway-installation, before-commit and committed boundaries. Ambiguous recovery verifies that no file is restored before the full source boundary is checked. These checks do not establish the downstream adapter's controls or general filesystem crash atomicity.
