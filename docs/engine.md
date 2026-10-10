# Controlled live edits (#20)

The `falinks` Rust library is the host-owned public protocol for this slice. `tests/protocol.rs` supplies two authenticated scripted clients using real temporary source, a bare Git object store and SQLite. Related-scope analysis, obligations, reviews, messages, offers, replay and waits are described in [Related-scope coordination (#21)](#related-scope-coordination-21). Exact candidates, validation and publication are described in [Checkpoint publication (#22)](#checkpoint-publication-22). Join/split recommendations and workspace transitions are described in [Regrouping (#25)](#regrouping-25). The adapters are in `adapters/`.

Run on a [supported platform](platforms.md) with Rust and Git 2.32 or newer on `PATH`:

```sh
cargo test
cargo check
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Captured jobs and required checks are contained commands (`src/contain.rs`). On macOS they install their own Seatbelt profile with `/usr/bin/sandbox-exec`. An outer sandbox that forbids installing another profile must run these checks outside that outer sandbox. On Linux they run through the `falinks-contain` helper executable (ADR 2), which installs Landlock (pinned to ABI 3) and a seccomp filter that denies the `socket` and `io_uring_setup` system calls (`socketpair` stays allowed). It acts as the command's subreaper. The host may set its path with `configure_containment`; the default is `falinks-contain` beside the host executable. A denied sandbox installation, a missing Landlock or helper, or a helper protocol mismatch is a visible job failure, never a fallback to unrestricted execution. Other platforms can use edits and storage, but contained commands fail visibly as unsupported. Requirements are in [docs/platforms.md](platforms.md).

## Host and client boundary

`Engine::open(live, state, enrolled)` acquires nonblocking OS writer leases on the source root and controller directory and verifies stored history/source before returning. Controller storage must be outside the live tree. The trusted host provisions exactly two credentials, authenticates them once, and routes each runtime session to its opaque `Client`; mutation requests contain no author field. Do not expose credential provisioning, arbitrary job executable/arguments, or fault observers as agent tools. This library does not implement the downstream runtime permission profile.

Enrollment is fixed for an experiment and includes absent paths for future creation. The source universe consists of all files beneath the live root except root `.git` storage and the reserved `.falinks-writer.lock` lease. Paths are normalized relative ASCII paths, case-unique, with no hidden/reserved components or overlapping file/ancestor enrollments. Regular files must have one link and mode 0644 or 0755; symlinks, special files, path aliases and other modes fail visibly. Unicode/hidden paths require a later explicit extension rather than silently accepting filesystem normalization aliases. Concurrent unmediated writers are outside the supported boundary; audited unexpected changes halt mutation and preserve an durable incident containing observed bytes, modes and symlink targets. Startup discoveries use the same incident halt, and putting old bytes back never clears it. `Engine::incident_at(state)` reads the latest retained incident even when startup refuses; the ledger also retains prior incidents.

## Protocol

- `capture()` returns the last completed occurrence, immutable Git tree, per-path occurrence versions, bytes/modes and blob identities. It verifies retained objects and durably records the capture before returning. Live reads can expose partial installations; coherent captures never do.
- `apply(client, Request)` takes a stable client-scoped request ID, complete output replacements/deletions and expected versions for every input and output. The host supplies the complete file/package input universe; this slice does not infer semantic independence. Adding package members to `expected` checks them atomically alongside target files. A target is always an input. Independent different-file requests can survive the same initial capture.
- `Stale` returns expected versions and current bytes/versions; the durable request retains the entire proposal. Reread, rebuild and submit a new request ID. Restoring old bytes advances occurrence versions even when Git reuses identical blobs/trees. Incomplete source needs no compilation.
- `request()` and `history()` return attributed request contents, scope, outcomes and exact before/proposed revisions. Identical submissions join the short writer operation and return its recorded result. Changed contents under an existing ID fail. An interrupted request can be retried explicitly with `retry()` after restart reconciliation; prior attempts remain retained, and freshness is checked again.
- `run_job()` captures declared inputs from a recorded completed revision, prepares separate read-only input and writable output directories, and retains the complete job record/logs/artifacts. `apply_job()` applies all declared output together through the same freshness gate, including input paths that the job did not write. Live mutations do not hold the job lease.

Jobs are host-enrolled trusted formatter/generator commands, with cleared environment, fixed input/output layout, no network, filesystem read restrictions and output-only writes (plus `/dev/null`). System tool/runtime reads remain allowed. Supported tools keep descendants in the assigned process group and join them before exit; daemonizing or escaping that group is unsupported and must not be enrolled. A surviving group is killed and reported as ambiguous. On Linux the helper also sweeps descendants that escaped the group (for example with `setsid`); macOS cannot detect those (#58). A 30-second execution bound kills timed-out groups. Failed commands, undeclared/reserved output, unsupported modes/aliases and unavailable sandboxing retain evidence and cannot apply. This is a bounded trusted-tool integration, not containment of arbitrary hostile programs. A restart never adopts output from a job whose completion was not recorded; it retains the directory and records a visible integration error.

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

A change covers the owners touched by its single per-file diff hunk in both the before and after evidence, so new edges in the after state count. A related scope is the registered nodes and declared dependencies (`Scope::depends`), everything they use (transitively), and their direct users. Both sides widen the same way. Missing evidence, analysis errors or a changed manifest/lockfile make the change unbounded. A broken unit, or a non-member file inside a unit, widens to that unit's files, its direct dependencies and all of its transitive reverse dependents. A declaration node covers its nested members. Bytes outside every owner widen to the file. The model treats the absence of reference or implementation edges as independence only for healthy, quiescent, unbroken evidence. This is the model-proven independence that lets unrelated work proceed; it is not a claim that the references are exhaustive (see Limits). File/package freshness is unchanged: distinct symbols never weaken `expected` versions.

## Obligations, reviews and gates

`register(client, Scope)` records intended work as enrolled paths or `path#owner` nodes. Each scope carries `required` (the latest relevant completed revision) and `reviewed`. The live-edit message, `required` bumps and obligation events commit in the same SQLite transaction as the completed pointer. Waiters are woken only afterwards.

`review(client, Review)` must name a revision that some `capture()` returned. It records keep/revise/drop and raises `reviewed`. It clears only an obligation whose `required` is at or below that revision. A later relevant change, including one that restores old bytes, is a new occurrence and renews the requirement. `apply` returns `Unreviewed` for writes related to a pending obligation and keeps the proposal. Unrelated writes proceed. `offer` refuses while any obligation is pending, even for an older revision. Event handling never clears obligations.

## Messages, offers and replay

Events carry the SQLite commit sequence and a stable `workspace:seq` ID. Message kinds: `Plan` (client `post`, reserves nothing), `LiveEdit` (engine, on commit), and `CheckpointOffer` (`offer`). Committed acceptance is a separate `Published` event, and check results are `Validated` events (see #22). Every message names its sender, workspace, task, scope and work. A reply must repeat the original message's exact work. Offers name a captured revision and stay available through later live edits until `withdraw` or an explicit `supersedes`.

`events(client, after)` replays in order. `handle(client, seq, Processed | Deferred)` advances the durable handled position one event at a time. A deferral is stored in the same transaction, so a cursor never skips pending context. Duplicate or earlier handling is a no-op, apart from clearing a deferral. `pending(client)` restores unhandled events, deferred context and pending obligations after a reconnect or restart.

## Waits

`wait(client, Condition, timeout, cancel)` checks the durable store while holding the wake lock, so an event committed before the wait is caught up immediately. Possible outcomes are `Met`, `TimedOut`, `Cancelled` (via `cancel_token`), `Disconnected` (host `disconnect`, or a peer not authenticated since startup), `Withdrawn`, and `Superseded`, which names the replacement and is never followed automatically. A `Checkpoint` wait targets acceptance of that exact checkpoint and is met by its `Published` event.

## Limits

Analysis runs inside the short writer operation (about one analyzer run per edit; a few seconds on the fixtures). One diff hunk per file over-approximates multi-hunk edits. Reference and implementation evidence is not an exhaustive behavioral dependency model; dynamic dispatch through values, reflection or textual coupling can escape it. When analysis is configured, any external crate or package marks the input universe unknown and blocks writes until the repository is bounded, so only self-contained repositories are supported today. A review can cite any capture of a revision, not only one the reviewing client requested. Independent-symbol application stays disabled. Runtime delivery, `turn/steer` and adapter-side enforcement belong to the adapter ticket.

# Checkpoint publication (#22)

`tests/publication.rs` drives this slice through the public protocol with real Git, SQLite and the pinned `rustc` as the required compile and test checks. The checks are contained commands, like captured jobs.

## Exact candidates and offers

A candidate is a completed revision `R` over the current published revision `B`. Completed history is linear, so the candidate holds every completed team edit up to `R`, including unfinished code; there is no ready-only reconstruction. Its required members are the authors of every applied operation in `(B, R]`, kept even when a later edit overwrote their bytes, plus the members the host requires with `require_members`. A peer that was only notified, with no included operation, is not a required member. `candidate(R)` reports the binding and the members still `missing` an offer. A disconnected member's unoffered work blocks the candidate; nobody else can offer for them.

`offer` records the offer's binding: base, members, contributions (operation IDs) and the offerer's reviewed revision per scope. An offer counts toward a candidate only while it is `Available` and its binding equals the candidate's. So a changed base, membership or contribution set needs new offers, and supersession or withdrawal transfers nothing.

## Checks

When an offer completes coverage, the same SQLite commit queues a publication run. `feedback(client, id, R)` queues an early run of any captured revision; it never publishes. The host worker calls `validate()` to process the oldest queued run. The required check set comes from the host's `configure_checks`. It is held in memory, like analysis configuration, so after a restart the host must configure it again.

All runs share one validation workspace, `state/validation`, under one lease. The lease covers preparation, checks, acceptance and synchronization, so runs queue while live drafting continues. A run marks the slot dirty in SQLite before touching it. The slot is reused only when SQLite records it clean and its files still match that record exactly. Otherwise it is moved to `state/quarantine/` as evidence and rebuilt. A free lock after a crash proves nothing. Only differing files are rewritten.

Each check runs in the slot with a cleared environment and a fresh output directory (`TMPDIR`, `HOME`, `CARGO_TARGET_DIR`, `GOCACHE`). Containment denies network, any write outside that output, and reading live or controller files. A 300-second bound kills a timed-out check. On Linux the read boundary is the complement of the live and controller roots, computed when the check starts. Those roots must not be reachable through another mount; the engine does not enforce this. A directory created after the check starts is unreadable. Unlike Seatbelt, Landlock cannot let a command list `/` or the ancestors of the denied roots without granting their whole subtree, so those listings are denied. After each check the slot is verified against the candidate; a mutation refuses the run and quarantines the slot. Logs are kept in the run record. Output directories are deleted after the run. After the run the slot is synchronized to the published snapshot and recorded clean.

## Acceptance

A publication run first rechecks its gates, without running checks if one fails. After all checks pass it rechecks them again under the writer lock. The gates are: the same published base, the same complete coverage and bindings, no pending obligation for an included change, no halt, the same check set, and no unexplained live change. An unexplained change records an incident. One SQLite transaction then advances the published pointer, marks every covering checkpoint `Accepted`, records the run outcome and emits the `Published` event. The pointer names a retained revision whose per-file occurrence versions are the published versions, so restoring earlier bytes still advances them. Publication is an engine event rather than a fourth agent message kind: it has no sender, and `Message` waits must not match it. A failed gate gives `Blocked` and a `Validated` event to both agents. The check results remain feedback.

The Git ref `refs/falinks/published` mirrors the pointer. It is updated after the commit, and `Engine::open` repairs it from SQLite. A mirror failure never undoes acceptance.

A base advance returns the candidate to its members, who re-offer against the new base, and fresh checks run. Since `R` already holds the accepted work, this new exact combination has the same bytes but different bindings. A candidate at or below the published revision is refused: a newer publication is never overwritten. Relevance between the intervening work and the candidate was already enforced when the live edits were applied, because offers are refused while any obligation is pending.

## Requests and recovery

Offer and feedback IDs are idempotent: an identical repeat returns the recorded checkpoint or run, and changed contents under the same ID fail. `run(id)` and `checkpoint(id)` answer an uncertain commit. A run interrupted by a crash or error becomes `Interrupted` at restart, and nothing resumes automatically. `retry_run` re-queues it as a new attempt, with every gate and check run again. A run started before the host configured checks is also `Interrupted`, so it can be retried. Repeating `retry_run` on a committed run returns its recorded outcome. A `Failed`, `Refused` or `Blocked` run is final; changed work needs new offers, which form a new candidate. Restart preserves live drafts. A missing or corrupt retained snapshot stops startup.

| Crash boundary | Observed outcome |
| --- | --- |
| During checks / before acceptance | Unpublished, all offers still available, run `Interrupted`, dirty slot quarantined on retry. |
| Inside the acceptance transaction | Nothing accepted: no partial group, pointer or event. |
| After commit, before mirror | Published, all members `Accepted`, mirror repaired on restart, duplicates return the outcome. |

## Limits

Checks run serially in the one slot, and the host decides when to run `validate()`. Required checks are trusted host commands; containment is a bounded control, not containment of hostile tools. The engine combines automatically only along the linear completed history, so publication races always return the candidate for re-offer. An offer's recorded reviews are evidence. The enforced peer and dependency gate is that no member has a pending obligation for a change included in the candidate. A candidate blocked by a disconnected member emits no extra event: peers already receive `Disconnected`, and `candidate(R)` names the missing member. There is no power-loss guarantee, and no joint Git/SQLite transaction.

# Regrouping (#25)

`tests/regroup.rs` drives this slice through the public protocol with the real gopls/Go analysis and a trivial trusted check.

## Workspaces and placement

The engine holds one or more live roots (spaces). Space 0 is the root given to `Engine::open`; the host provisions more with `add_workspace(root)`, which must be empty, separate from storage and other roots, and is leased like the first. `placement()` names each agent's space: equal entries mean the agents are joined, which is the initial arrangement. `root(client)` is the only root the host may expose to that client's runtime, and `capture_for(client)` captures it. `capture()` is a host read of space 0, not an agent tool.

Revision numbers are global, so every occurrence stays unique across spaces. Each revision records its `space`, its `group` (the agents placed there when it was created) and `included`, the set of applied operations its contents contain. Coverage uses those: a candidate's contributions are `included(R) − included(published)`, and its members are their authors plus host-required members of the revision's `group`. Regrouping therefore never changes a held candidate's membership. A candidate is publishable only when it contains the published state and adds work. Reviews, feedback and jobs must name a revision of the client's own space; an offer may also name a retained revision whose group included the client, so it can still cover a candidate held from before a transition. Live edits renew obligations only for agents placed in the edited space; the live-edit message still reaches the peer.

## Recommendations

`reconsider()` (host, initially), `recommend(client)` (agent request) and `propose(client, Propose)` (an agent's own join/split/keep, with an explanation and an optional failure report) produce a `Proposal`. Its `Signals` are observations, not a score:

- compiler relationships between the agents' registered scopes (a scope's closure touches the other's nodes);
- declarations: scopes registering the same nodes, or a `Scope::depends` declared dependency on the other's nodes. Declared dependencies also widen obligation relevance;
- shared tasks;
- stale or unreviewed write attempts, obligations raised by agents' live edits, and waits, since the last applied transition. Obligations a transition or incorporation raised are its consequence, not evidence;
- agent reports and failure reports;
- uncertainty: no analysis, degraded evidence, unbounded relevance or an agent without scopes.

Joined agents split only when evidence is certain and shows no relationship, declaration, shared task or friction. Split agents join on a declaration, shared task, friction or uncertainty (prefer fewer groups); a compiler edge alone keeps the split. A failure report asks for the opposite arrangement. A pending proposal for the same change stands, whoever proposed it. An unchanged repeat from the same proposer since the last transition returns the earlier proposal, including a declined one. A join or split that would reverse or repeat the last applied transition becomes a keep unless scopes or relationships changed, friction appeared (for a join), or an agent reports failure; uncertainty alone does not reverse a split. There is no cooldown. After the first `reconsider()`, the engine also reconsiders at every boundary below, so materially new evidence produces a proposal without a request.

A proposal names the affected agents, their scopes, the current and target placement, the target roots, the signals, the reasoning and a `context` fingerprint of placement plus every registered scope. Every `Regrouping` state change is an event to both agents.

## Agreement

`respond(client, id, context, Agree | Decline)` must repeat the proposal's exact context. Both agents must agree; silence or disconnect agrees to nothing. A decline keeps the arrangement. Any scope or placement change makes the proposal `Stale`; a new proposal and new agreement are required. A newer join/split proposal replaces older open ones.

## Transitions

Once both agree, the engine applies the proposal at once if its safety checks hold, otherwise it records `Blocked { blocker }` and keeps the current arrangement. Blocked proposals are retried after every edit, review, registration, response, validation run and provisioned root, while the context still matches. These boundaries run after the triggering operation has committed and never turn its outcome into an error: a transition that fails is recorded as `Blocked { blocker: "transition failed: …" }`. Blockers: a halted engine, a queued publication run, a space that has not incorporated the published state, an unprovisioned or occupied target root, unattributed differences from the published state, and overlapping drafts.

- **Split** (agent 1 leaves): agent 1's unpublished operations and the files they wrote move to the target root, which receives the published state plus those files with their exact occurrences. Those paths in the shared root return to published bytes as new occurrences. If both agents' unpublished operations wrote the same file, the split waits (`interleaved unpublished drafts`) until that work is published.
- **Join**: the leaving space's unpublished files are installed into the lower-numbered space, keeping their occurrences; the union of `included` keeps both authors in coverage. Overlapping unpublished files block the join. The vacated root keeps its last revision and is overwritten only by a later split.

Transitions never publish, never reattribute operations, and never touch request records, checkpoints, runs or obligations. Old checkpoints keep their exact identity and binding; whether they still match a candidate is decided by the usual binding equality. Each agent whose view changed gets obligations for related scopes, caused by the applied `Regrouping` event. A transition records the before/after revisions of every root, retains the new snapshots and only then installs files. An interruption halts the engine. On restart every affected root must match its before or after bytes exactly; it is restored to before, and the agreed proposal applies at the next boundary.

## Publications across split workspaces

After each validation run, every occupied space that lacks the published state takes the published files that its own unpublished operations did not write, as an `Incorporated` event and a new revision. Its agents' related obligations are renewed. If the publication changed a file the space also drafted, nothing is installed; its members get one `Behind` event naming the overlaps. Its candidates cannot publish. `incorporate(client, request)` then installs the published state together with the client's resolution of every overlapping file, as one attributed operation (`Record::incorporates`) through the normal freshness and obligation gates.

## Limits

Two agents and therefore at most two occupied spaces; agent 1 is the one that leaves on a split. Engine-driven reconsideration starts with the host's first `reconsider()` and runs at operation boundaries, not on a timer. Signal analysis runs under the writer lock, like the edit gate. Draft movement is per file: two authors' drafts in one file block a split until published, and overlapping drafts block a join. There is no automatic merge of overlapping publications. Restart does not resume an interrupted transition by itself.
