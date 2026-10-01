# Collaborative code editing: bounded landscape

Research date: 2026-10-01. Primary sources only; official pages opened directly. No experiments, implementation audit, or novelty search. Product documentation describes supported mechanisms; vendor research reports observations, not independently verified correctness. Vocabulary follows root CONTEXT.md. The research was delegated to a background agent and reviewed in the main session.

## Primary-source competitor facts

**Claude Code agent teams — documented experimental feature.** Independent sessions share messaging, a lead, and tasks. Task dependencies block claiming until prerequisites complete; claiming uses file locking. Completion hooks can reject completion. Documentation explicitly warns that two teammates editing one file cause overwrites and recommends separate file ownership. These are task coordination and hook mechanisms, not a documented guarantee that completed work remains valid after its code dependencies change. [Official agent-teams docs](https://code.claude.com/docs/en/agent-teams)

**GitButler — documented shared-workspace branch management.** Multiple active branches occupy one worktree, with task changes committed to session branches; routing is the agent's responsibility. Dependencies can be expressed by stacking branches. Agents share files, generated output, dependencies, and app state. Documentation warns that this can conceal accidental dependencies and asks users to check independent shippability. Branch attribution therefore does not establish runtime isolation or semantic independence. [Official parallel-agents docs](https://docs.gitbutler.com/ai-agents/parallel-agents)

**Cursor, January 14, 2026 — earlier research.** The scaling experiment moved from shared coordination locks and optimistic writes to recursive planners, focused workers, and a cycle judge. It reported hundreds of workers pushing to one branch, and removed an integrator as a bottleneck. This is historical experimental evidence, not a timeless prescription: July's design introduces dedicated conflict resolution. [Scaling long-running autonomous coding](https://cursor.com/blog/scaling-agents)

**Cursor, July 20, 2026 — strongest overlap, research system.** Cursor reports a custom VCS peaking around 1,000 commits/second. Shared design documents have compile-checked references from dependent code; a reconciler merges contradictory decisions and references propagate resolutions. A neutral agent resolves merge conflicts. Multiple review lenses audit output; intentional core changes can break compilation and prompt downstream repairs. Its SQLite experiment used the 835-page manual and held-out sqllogictest queries: four-hour new-system scores were 73–85%, with all new configurations eventually reaching 100%. These are vendor-reported results on that experiment, not universal semantic correctness or evidence that these internals ship in ordinary Cursor workflows. The article describes probabilistic translation of intent and limited manual analysis of the linked output. [Agent swarms and the new model economics](https://cursor.com/blog/agent-swarm-model-economics)

**GitHub merge queues — documented integration gate.** Queue candidates are checked against the current target plus earlier queued changes using temporary merge groups; required Actions checks need the `merge_group` trigger. Removal can rebuild later candidates without the removed PR. Group-level passing checks can permit inclusion of individually failing PRs under the documented setting. Merge limits govern landing batches, not grouping CI builds. The mechanism guarantees its configured check gate, not that tests establish all behavioral properties. It already supplies meaningful combined-state validation and grouped landing. [Official merge-queue docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)

**Git worktrees — documented checkout mechanism.** Additional worktrees share a repository but have separate HEAD and index state. They provide separate working directories, not automatic isolation of external databases, services, or other shared resources. [Official git-worktree reference](https://git-scm.com/docs/git-worktree)

## Conceptual ideal — analysis, not competitor facts

The useful ideal is concurrent preparation with trustworthy publication. Each agent reads an identifiable, fixed snapshot and submits a proposal with versioned declared dependencies. Agents can disagree, explore alternatives, and negotiate contracts; the engine governs when their changes become published state.

A readiness decision should refer to exact proposal revisions, dependency versions, target snapshot, and check results. A dependency change invalidates affected readiness and requires revalidation. Declared dependencies cannot represent every assumption: undeclared behavioral coupling remains outside this guarantee unless additional analysis or checks cover it.

Group capture means freezing the exact membership and revisions of mutually dependent proposals before checking their combined candidate. Publication accepts that captured result together or rejects it; target advancement, changed members, or external edits require reassessment. A passing check on an earlier candidate must not authorize a different candidate. This is a proposed protocol property, not proof of semantic correctness.

Collaboration also needs recoverable ownership, visible reasons for blocking, cancellation, and preservation of rejected work. Messaging helps resolve intent; it should not be the sole authority for readiness.

## Practical architecture families — analysis grounded in the mechanisms above

| Family | What it handles well | Boundary / cost | Appropriate use |
| --- | --- | --- | --- |
| Shared directory + messaging / task ownership | Fast communication and immediate visibility; little checkout overhead | Overwrites and mixed state; ownership requires cooperation | Separate-file work, review, exploration |
| Per-agent worktrees / branches + integration queue | Separate edit buffers and checkout state; checks on integrated candidates | Delayed conflicts; runtime isolation requires more than worktrees | Default for independent implementation and competing attempts |
| Shared composite workspace + parallel branches | Combined feedback with separate branch attribution | Hidden cross-branch dependencies and shared runtime state | Independent tasks that benefit from one running app |
| Hierarchical planners / workers + reconciliation | Bounded contexts, explicit decision ownership, specialized conflict handling | Probabilistic decisions; orchestration and VCS complexity | Large, sustained efforts; Cursor July is substantial precedent |
| Snapshot proposals + dependency-aware publication | Explicit stale-work rejection, versioned readiness, captured dependent groups | Dependency completeness and validation scope remain limited | When exact publication semantics are the requirement |

These families compose: an orchestration hierarchy can use worktrees, and a proposal gate can sit over Git commits or parallel branches. They are not mutually exclusive products.

## Evidence-qualified gaps and differentiation

The inspected Claude and GitButler docs expose concrete overwrite/shared-state hazards. Their pages do not specify revision-bound dependency revalidation and captured-group publication. This is a documentation boundary, not proof that the products lack every such capability.

Cursor July already covers dependency-linked design decisions, downstream propagation, reconciliation, neutral conflict agents, and review. GitHub already checks combined queue candidates and rebuilds them when membership changes. A proposal described only as messaging, worktrees, dependency links, merging, or group checks would overlap heavily with existing mechanisms.

Meaningful differentiation must demonstrate a precise additional contract:

1. **Dependency revalidation:** when a declared dependency changes after preparation or validation, invalidate readiness and require agent reassessment plus the required checks for the new dependency version—even if text merges cleanly.
2. **Readiness/group capture:** bind readiness to immutable proposal revisions, dependency versions, candidate target, and group membership; reject publication if any binding changed.
3. **Boundary enforcement:** external edits and target advancement cannot silently bypass that contract; recovery preserves proposals and explains why they became stale.

A decisive example is a producer changing behavior without changing its signature while a consumer proposal still carries readiness for the old contract. Another is a mutually dependent pair validated together, followed by one member changing before publication. The claimed advantage must show the stale candidate cannot land, and compare that outcome with Cursor's reference/reconciliation mechanism and GitHub's merge-group checks. These are future proof obligations, not experiments performed here.

The shortest practical starting architecture is existing Git snapshots/worktrees plus one publication authority and explicit versioned readiness records. A custom VCS is justified only by measured limits or required semantics that existing primitives cannot enforce. No novelty claim is made; neither documentary silence nor this bounded source set establishes novelty, a research-paper contribution, or an industry-wide gap.
