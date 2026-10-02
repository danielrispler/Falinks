# Collaboration on unvalidated proposals with stable agent bases

2026-10-02. Focused logical protocol for the decision previously titled [Specify the minimum proposal and publication protocol](https://github.com/danielrispler/Falinks/issues/4). The developer narrowed the work to collaborating on changes that need not be independently valid while each agent retains a stable working base, and then requested ticket completion without further implementation.

This specifies the logical contract. Native-tool capture, concrete refresh mechanics and publication recovery remain separate follow-ups. No industry-wide novelty, completed engine, or successful runtime verification is claimed. The correctness requirements in [Define the first experiment’s correctness guarantees and failure outcomes](https://github.com/danielrispler/Falinks/issues/3) remain in force.

## Sole capability

An agent can share an exact unvalidated proposal revision. Another agent can deliberately adopt a fixed snapshot containing that revision and prepare dependent changes. The engine can form an exact group snapshot from the coupled revisions. Sharing, adopting, and publishing are different operations.

Stable means identifiable and unchanged underneath the client; it does not imply that the snapshot compiles. An agent may see its own explicit draft edits over its chosen base. Another agent's later edits or messages never silently change that base.

## Logical records

- **Snapshot:** an immutable repository tree identity. For a composed snapshot, retain the exact proposal membership and revisions that produced it. A moving group name is a locator, not a snapshot identity.
- **Working view:** the selected snapshot plus that client's draft. A refresh prepares a replacement view; it does not overwrite the previous draft before reconciliation succeeds.
- **Proposal revision:** a logical proposal identity, immutable revision identity, observed snapshot, attributable contribution relative to its recorded contribution basis, and structured declared dependencies. Record required peer proposal revisions separately from published symbol/dependency versions. Trust declaration completeness within the existing stated guarantee.
- **Group revision:** the original seed snapshot, an exact selection of proposal revisions, required membership, dependency bindings and the composed snapshot identity. Replacing a member creates a new revision; it never alters a previously captured group.

The observed snapshot and contribution basis need not be the same. An agent may have read imported peer changes while its attributable contribution remains only its own changes. Do not classify an entire imported workspace as that agent's proposal.

## Operations and outcomes

**Share** receives an attributable captured contribution and its metadata, and returns an immutable proposal revision. Independent compilation is not a precondition. A mutable directory or an agent's readiness statement is insufficient capture evidence. If ownership/capture is ambiguous, return `CaptureUnsupported` and preserve drafts.

**Compose** receives a seed and exact member revisions. Reconcile each contribution against its recorded basis, retaining exact membership. Different owning-symbol edits may combine. Text conflicts return `Conflict` with affected targets and relevant revision identities. Required peer bindings must match the selected revisions; a mismatch returns `NeedsRevalidation`. A preview can expose an incomplete or invalid group, but its missing required members and unvalidated status are explicit. It is not publication-ready.

**Refresh** receives the client's selected snapshot, attributable draft and target snapshot. Prepare the target plus the client's retained contribution in a new view. Return the new base/draft identities on success. On conflict, return `RefreshConflict` and preserve the old base, complete draft and target. Switch the client only at an explicit supported safe point. A notification or message acknowledgment does not perform this switch.

**Revise** creates a new immutable revision of a logical proposal. Carry forward that proposal's retained contribution and apply only its new edits. Imported peer contributions remain separate. A changed required peer revision triggers agent reconsideration; a clean text merge is not reconsideration.

**Offer group** fixes a complete group revision for eventual validation/publication. Later member edits remain outside it. The existing exact-candidate and all-or-none rules apply; no independent-member test gate or new scheduling policy is adopted here.

Requests should carry stable request identities when retried. Responses identify exact revisions and the preserved work, not only a mutable agent/group label. Durable deduplication and event replay belong to the publication/recovery follow-up.

## Producer/consumer example

1. A and B choose published snapshot P0.
2. A changes an API response and shares A1. A1 can break the old consumer.
3. Compose a preview G1 containing A1. G1 stays fixed despite later A edits.
4. B explicitly adopts G1, updates the consumer, and shares B1 requiring A1. B1 can fail when considered alone against P0.
5. Compose G2 from the exact A1 and B1 contributions. P0 and G1 remain available and unchanged. G2 may satisfy the combined contract; actual checks still decide that.
6. A later shares A2. G2 still identifies A1+B1. A proposed selection of A2+B1 reports B1's stale required-peer binding instead of silently reusing its readiness.
7. B adopts the chosen A2 preview, preserves its own work, reconsiders it, and shares B2 with the new binding. Compose a new group revision.

## Own-proposal refresh trap

A required implementation case is an agent adopting a group that already includes its own A1, then editing and sharing A2 under the same logical proposal identity. Computing only the local diff from that imported group loses A1's earlier contribution when the group is rebuilt from P0. Capturing the entire imported tree instead can pull peer changes into A2 or resurrect an obsolete peer revision.

The protocol therefore retains the cumulative attributable contribution independently of the selected working snapshot. A revision must preserve A1's contribution plus the new own edits, while excluding imported peer contributions. If that reconciliation is uncertain, return a conflict rather than guessing authorship. The concrete representation and algorithm are the refresh/revision follow-up's decision, not a tested capability of the unfinished Rust sketch.

## Relationship to publication

Publication remains subject to the existing correctness resolution: validate the exact complete candidate; verify the expected published base is still current; enforce edited-owner and declared-dependency freshness; publish a required group together or none. A target race preserves accepted work and rebuilds/rechecks a new candidate as required. Failed checks or conflicts preserve proposals and published state. Changed-and-restored published source still advances dependency versions.

This logical gate does not choose whether a Git ref or database pointer is authoritative. It does not claim a database transaction atomically changes Git/filesystem state. Authority, ordered events, replay and engine-restart reconciliation are deferred together, with the previous recovery requirements intact.

## Minimal implementation direction and supported limit

Rust remains the agreed engine language; TypeScript is the first editing target. Reuse Git objects and ordinary text reconciliation as the first implementation direction, rather than a custom VCS or language. Snapshot materialization may use temporary filesystem views; no long-lived per-agent branch or identical mutable-directory layout is required by this contract. Raw shared-directory writes cannot by themselves supply stable client reads.

For an initial pure core, owned edit bytes passed through the interface can form immutable revisions. Exporting a snapshot is a separate operation. Importing native edits, shell tools, formatters or generator output requires a supported writer/capture integration, including descendants. Do not promise that hooks or a completed shell launcher establish this. The concrete native integration remains unverified.

Symbol identification/versioning and conservative whole-file fallback follow the correctness resolution. This ticket adds the logical sharing/adoption/group contract; it does not select an automatic symbol identity algorithm.

## Required future checks

Verify sharing without independent validity; unchanged old snapshots and views after peer updates; exact group membership; exclusion of post-share drafts; preserved draft bytes on successful and conflicting refresh; same-file disjoint edits and actual conflicts; stale peer/published dependencies; cumulative revision after importing one's own earlier proposal; omission/replacement of imported peers without their resurrection; unsupported writer capture failing safely. Broader publication-race and restart checks remain required before a reliable engine is claimed.

These are obligations, not results. No new experiment or test run was performed to close this ticket.

## Handoff

The developer stopped implementation before completion. An unfinished, uncompiled Rust sketch is retained only in the isolated local checkout, under core/. It has no runnable check and does not yet handle the own-proposal refresh trap above. It is not included in the published specification commit. A minimal Rust toolchain was installed only under the same scratch root; main's code, Git state, dependencies and global shell configuration were preserved.

Agent scheduling, cost optimization, automatic failure routing and benchmarks are outside this focused resolution. Existing research assets remain historical evidence, rather than the rationale for an industry-wide novelty claim.
