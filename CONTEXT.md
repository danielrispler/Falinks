# Collaborative editing

The shared language for an engine through which coding agents propose and publish changes to a repository.

## Language

**Snapshot**:
An immutable, identifiable repository version used as a recorded base or captured validation candidate.

**Live workspace**:
Shared working files used by agents collaborating on related subtasks. It includes unfinished changes, so its contents may change between reads.

**Proposal**:
A candidate change based on a snapshot, together with its dependency information. It is not yet part of the published state.

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
Reassessment of a proposal after a declared dependency changes, followed by the required publication checks.

**External edit**:
A repository change made outside the engine's publication workflow.
