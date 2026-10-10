# Controlled live edits (#20)

The `falinks` Rust library is the host-owned public protocol for this slice. `tests/protocol.rs` supplies two authenticated scripted clients using real temporary source, a bare Git object store and SQLite. No publication capability is enabled: the published pointer stays at the enrolled initial snapshot. Related-scope analysis, obligations, reviews, messages, offers, replay and waits are described in [Related-scope coordination (#21)](#related-scope-coordination-21). The app-server adapter, validation, publication and grouping belong to later tickets.

Run on macOS with Rust and `/usr/bin/git`:

```sh
cargo test
cargo check
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The captured-job checks install their own Seatbelt profile with `/usr/bin/sandbox-exec`. An outer sandbox that forbids installing another profile must run these checks outside that outer sandbox; a denied sandbox installation is a visible job failure, never a fallback to unrestricted execution. Other Unix platforms can use edits/storage, but captured jobs fail visibly as unsupported.

## Host and client boundary

`Engine::open(live, state, enrolled)` acquires nonblocking OS writer leases on the source root and controller directory and verifies stored history/source before returning. Controller storage must be outside the live tree. The trusted host provisions exactly two credentials, authenticates them once, and routes each runtime session to its opaque `Client`; mutation requests contain no author field. Do not expose credential provisioning, arbitrary job executable/arguments, or fault observers as agent tools. This library does not implement the downstream runtime permission profile.

Enrollment is fixed for an experiment and includes absent paths for future creation. The source universe consists of all files beneath the live root except root `.git` storage and the reserved `.falinks-writer.lock` lease. Paths are normalized relative ASCII paths, case-unique, with no hidden/reserved components or overlapping file/ancestor enrollments. Regular files must have one link and mode 0644 or 0755; symlinks, special files, path aliases and other modes fail visibly. Unicode/hidden paths require a later explicit extension rather than silently accepting filesystem normalization aliases. Concurrent unmediated writers are outside the supported boundary; audited unexpected changes halt mutation and preserve an durable incident containing observed bytes, modes and symlink targets. Startup discoveries use the same incident halt, and putting old bytes back never clears it. `Engine::incident_at(state)` reads the latest retained incident even when startup refuses; the ledger also retains prior incidents.

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

# Related-scope coordination (#21)

`tests/coordination.rs` drives this slice through the same public protocol, with the real analyzers. Requirements beyond the commands above: the `rust-analyzer` and `rust-src` toolchain components (pinned in `rust-toolchain.toml`), Go on `PATH`, and `gopls` v0.22.0 at `$(go env GOPATH)/bin/gopls` or `FALINKS_GOPLS`.

## Evidence

`configure_analysis(toolchains)` is a host call. Each `Toolchain` pins executables by SHA-256: `rust-analyzer`, `cargo`, `rustc` for Rust; `gopls`, `go` for Go. Features are Cargo features or Go build tags. A changed pin fails visibly at configuration. Cached evidence is re-verified on every use: if a pinned binary has been replaced, it is degraded and widened. Configuration lives in memory; after restart relevance stays conservative until the host reconfigures.

Evidence is a pure function of an exact Git tree and the toolchain identity, cached in SQLite by both. A source or configuration change produces a new tree and so new evidence; old ranges are never reused. Each analysis writes the captured bytes to a disposable copy with a cleared environment (offline Cargo, `GOENV=off`, `GOTOOLCHAIN=local`, `GOPROXY=off`, no cgo) and checks afterwards that the analysis did not alter its inputs.

- Units: `cargo metadata --locked --offline` crates, or `go list -deps` main-module packages, with their internal dependencies. `cargo check` and `go vet` errors mark a unit broken.
- Owners: LSP hierarchical `documentSymbol`. Functions and methods at any depth, plus every top-level declaration. An edit belongs to the smallest owner that contains it.
- Relationships: LSP `references` for every symbol, mapped to the owner of each use, plus `implementation` locations linked in both directions, so trait and interface implementations relate without a reference. rust-analyzer results count only after a quiescent `ok` server status. Go uses gopls, the public LSP built on `go/packages` and `go/types`, so one Rust client serves both languages.
- Unknown input universe: external crates/packages, build scripts, proc macros or cgo. When analysis is configured, any write whose before or after evidence names one is rejected with "recapture required".

## Relevance

A change covers the owners touched by its single per-file diff hunk in both the before and after evidence, so new edges in the after state count. A related scope is the registered nodes, everything they use (transitively), and their direct users. Both sides widen the same way. Missing evidence, analysis errors or a changed manifest/lockfile make the change unbounded. A broken unit, or a non-member file inside a unit, widens to that unit's files, its direct dependencies and all of its transitive reverse dependents. A declaration node covers its nested members. Bytes outside every owner widen to the file. The model treats the absence of reference or implementation edges as independence only for healthy, quiescent, unbroken evidence. This is the model-proven independence that lets unrelated work proceed; it is not a claim that the references are exhaustive (see Limits). File/package freshness is unchanged: distinct symbols never weaken `expected` versions.

## Obligations, reviews and gates

`register(client, Scope)` records intended work as enrolled paths or `path#owner` nodes. Each scope carries `required` (the latest relevant completed revision) and `reviewed`. The live-edit message, `required` bumps and obligation events commit in the same SQLite transaction as the completed pointer. Waiters are woken only afterwards.

`review(client, Review)` must name a revision that some `capture()` returned. It records keep/revise/drop and raises `reviewed`. It clears only an obligation whose `required` is at or below that revision. A later relevant change, including one that restores old bytes, is a new occurrence and renews the requirement. `apply` returns `Unreviewed` for writes related to a pending obligation and keeps the proposal. Unrelated writes proceed. `offer` refuses while any obligation is pending, even for an older revision. Event handling never clears obligations.

## Messages, offers and replay

Events carry the SQLite commit sequence and a stable `workspace:seq` ID. Message kinds: `Plan` (client `post`, reserves nothing), `LiveEdit` (engine, on commit), `CheckpointOffer` (`offer`) and `Publication` (reserved for committed acceptance; never emitted in this slice). Every message names its sender, workspace, task, scope and work. A reply must repeat the original message's exact work. Offers name a captured revision and stay available through later live edits until `withdraw` or an explicit `supersedes`.

`events(client, after)` replays in order. `handle(client, seq, Processed | Deferred)` advances the durable handled position one event at a time. A deferral is stored in the same transaction, so a cursor never skips pending context. Duplicate or earlier handling is a no-op, apart from clearing a deferral. `pending(client)` restores unhandled events, deferred context and pending obligations after a reconnect or restart.

## Waits

`wait(client, Condition, timeout, cancel)` checks the durable store while holding the wake lock, so an event committed before the wait is caught up immediately. Possible outcomes are `Met`, `TimedOut`, `Cancelled` (via `cancel_token`), `Disconnected` (host `disconnect`, or a peer not authenticated since startup), `Withdrawn`, and `Superseded`, which names the replacement and is never followed automatically. A `Checkpoint` wait targets acceptance of that exact checkpoint. Publication is disabled, so in this slice such a wait can end only through one of the non-met outcomes.

## Limits

Analysis runs inside the short writer operation (about one analyzer run per edit; a few seconds on the fixtures). One diff hunk per file over-approximates multi-hunk edits. Reference and implementation evidence is not an exhaustive behavioral dependency model; dynamic dispatch through values, reflection or textual coupling can escape it. When analysis is configured, any external crate or package marks the input universe unknown and blocks writes until the repository is bounded, so only self-contained repositories are supported today. A review can cite any capture of a revision, not only one the reviewing client requested. Independent-symbol application stays disabled. Runtime delivery, `turn/steer` and adapter-side enforcement belong to the adapter ticket.
