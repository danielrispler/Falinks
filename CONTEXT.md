# Collaborative editing

The shared language for an engine through which coding agents propose and publish changes to a repository.

## Language

**Snapshot**:
An immutable, identifiable repository version used as a recorded base or captured validation candidate.

**Live workspace**:
Shared working files used by agents collaborating on related subtasks. It includes unfinished changes, so its contents may change between reads.

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
