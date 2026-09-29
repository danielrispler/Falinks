# Collaborative editing

The shared language for an engine through which coding agents propose and publish changes to a repository.

## Language

**Snapshot**:
An identifiable repository version that stays fixed for a client's reads while it prepares a proposal.

**Proposal**:
A candidate change based on a snapshot, together with its explicitly declared dependencies. It is not yet part of the published state.

**Published state**:
The repository version accepted by the engine as the shared starting point for subsequent work.

**Declared dependency**:
A versioned prerequisite that a client explicitly says its proposal relies on. It does not represent every assumption the client may have made.

**Owning symbol**:
The nearest enclosing function or method containing an edit, or the top-level declaration for an edit outside functions and methods. Separate methods have separate owners even when they belong to the same class.

**Contract**:
An explicit agreement about behavior or an interface on which a proposal may depend. Its representation and versioning rules remain to be specified.

**Revalidation**:
Reassessment of a proposal after a declared dependency changes, followed by the required publication checks.

**External edit**:
A repository change made outside the engine's publication workflow.
