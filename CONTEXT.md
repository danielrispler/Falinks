# Collaborative editing

The shared language for an engine through which coding agents propose and publish changes to a repository.

## Language

**Snapshot**:
An immutable, identifiable repository version used as a recorded base or captured validation candidate.

**Live workspace**:
Shared working files used by agents collaborating on related subtasks. It includes unfinished changes, so its contents may change between reads.

**Validation workspace**:
A repository directory used to materialize a captured candidate for checks, isolated from live edits and held exclusively for a validation run.

**Work group**:
Agents collaborating on related tasks. Membership expresses collaboration intent, not ownership of a physical directory or membership of every captured publication candidate.

**Regrouping proposal**:
An identified proposal to join, split or keep work groups, naming the affected agents, task scopes, target grouping, intended workspace arrangement and supporting evidence.

**Regrouping transition**:
Application of an agreed grouping and workspace arrangement while preserving work and its revision, candidate and publication obligations. It does not imply publication approval.

**Incorporation**:
Bringing the published state into a split live workspace without overwriting that workspace's unfinished drafts. A file the publication and the drafts both changed needs an explicit, attributed resolution.

**Proposal**:
A candidate change based on a snapshot, together with its dependency information. It is not yet part of the published state.

**Plan**:
An agent's announced intention to change identified work. It is advisory and does not reserve that work.

**Live edit**:
A change applied to the live workspace, which may still be unfinished or unvalidated.

**Checkpoint offer**:
A message identifying exact work made available for combination and validation, without asserting publication.

**Checkpoint supersession**:
Explicit replacement of a checkpoint offer by another identified offer. The earlier checkpoint retains its identity and recorded history.

**Checkpoint withdrawal**:
Explicit removal of a checkpoint offer from availability, preserving the referenced work and recorded history.

**Publication**:
Acceptance of an exact checked candidate as the published state.

**Published state**:
The repository version accepted by the engine as the shared starting point for subsequent work.

**Declared dependency**:
A versioned prerequisite that a client explicitly says its proposal relies on. It does not represent every assumption the client may have made.

**Related scope**:
The source symbols relevant to an edit through compiler-derived semantic dependencies, supplemented by agent-declared dependencies. Where that boundary is uncertain, the scope conservatively includes the affected files or packages.

**Owning symbol**:
The nearest enclosing function or method containing an edit, or the top-level declaration for an edit outside functions and methods. Separate methods have separate owners even when they belong to the same class.

**Contract**:
An explicit agreement about behavior or an interface on which a proposal may depend. Its representation and versioning rules remain to be specified.

**Revalidation**:
Reassessment of affected planned or proposed work after a relevant change, followed by the required checks before publication.

**External edit**:
A repository change made outside the engine's publication workflow.

**Supported language**:
A programming language whose repositories Falinks can coordinate, including related scope and checks. Currently Rust and Go. Distinct from a client, which is an authenticated agent session.

**Worker runtime**:
The agent harness that runs one agent session and presents its tools and messages. Falinks reaches it only through an adapter.

**Presentation boundary**:
A point at which a worker runtime shows pending messages to its agent. Presentation is not processing, acknowledgment or revalidation.

**Captured job**:
A host-enrolled trusted formatter or generator run against a captured revision of a client's workspace. Its declared output can be applied as that client's edit.
_Avoid_: Job command, generator run

**Required check**:
A host-enrolled trusted command, such as compilation or fixed tests, that must pass on a candidate before publication.
_Avoid_: Validation command, test step

**Contained command**:
A captured job or a required check, run under the containment guarantee.
_Avoid_: Sandboxed command, sandboxed tool

**Containment guarantee**:
What every contained command receives on every supported platform: no network, reads limited to its command kind's boundary, writes only under its output, a cleared environment, process-group containment within a bounded run time, and refusal to run when these cannot be enforced. It bounds trusted tools; it does not contain hostile programs.
_Avoid_: Sandbox contract (a contract is an agreement a proposal depends on)

**Supported platform**:
An operating system on which a Falinks component upholds its guarantees and CI or recorded evidence proves it. Support is claimed per component: the engine and each worker-runtime adapter separately.
_Avoid_: Development platform (the OS Falinks is built on, currently macOS)
