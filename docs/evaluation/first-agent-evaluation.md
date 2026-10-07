# First real-agent evaluation plan

Agreed with Daniel on 2026-10-07 in [Design the fair baseline and first real-agent evaluation](https://github.com/danielrispler/Falinks/issues/5). Planning artifact only: no engine implementation or benchmark results are claimed. Daniel explicitly waived the explanation/learning stop and requested ticket completion. This records a user waiver, not a delivered explanation or independently assessed learning completion.

## Integration scope

Keep the engine protocol independent of the agent harness. First implement and evaluate the Codex adapter using the previously verified macOS Codex CLI 0.160.0 ordinary app-server boundary. The recorded binary hash is `112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b`; this is historical evidence, not a claim that today's installation matches it. Reverify the exact runtime at startup. Other versions and harnesses need renewed capability checks. Claude Code, Antigravity, OpenCode and other adapters are subsequent integrations; do not switch providers within the scored batch.

One machine, one repository, two agents, Rust/Go editing, mediated source writes, retained completed revisions, SQLite-authoritative publication, and one reusable isolated validation workspace remain the supported boundary. Agents read evolving task-group code with ordinary read/search tools, announce plans, process or defer notifications, explicitly reconsider against current revisions, submit controlled edits and offer exact captured candidates. Coherent analysis and checks use captured inputs. An offer is not publication.

Source is read-only to ordinary agent tools; separate scratch is writable. Controller state, snapshot storage and validation storage are inaccessible to agents. Disable extra capabilities and permission escalation as prescribed by the coordination decision. Host dynamic tools authenticate runtime identity; registration alone is not enforcement. Unexpected/unmediated edits preserve evidence and stop mutation/publication.

Antigravity's possible usage advantage is Daniel's reported motivation for a future adapter investigation; no capabilities, model identity or pricing were verified here.

## Entry gate

Before scored runs, pass deterministic correctness checks for the existing coordination, attribution, notification, publication/recovery and regrouping contracts. Exercise lost-update prevention, whole-operation stale rejection, relevant dependency reconsideration, restore-to-old-bytes revision identity, draft preservation, exact offers, withdrawal/supersession, check/publication races, duplicate requests, deferred replay, disconnect/restart and guarded transitions. Include all acceptance scenarios in the linked prerequisite resolutions; this list does not replace them.

Startup preflight verifies the pinned binary/schema and effective permissions, mediated edits, representative denied native/shell/child-process source writes, controller protection, scratch access, detection of unexpected source edits, notification/reconsideration gates, and captured-candidate isolation. Verify unknown-change detection using a controlled host-side injection outside scored runs. Failed safety controls block scored runs. Finite controls do not establish protection against arbitrary unmediated writers.

## Fixtures and independent oracle

Prepare three small realistic repositories. Freeze their initial commits, task specifications, dependency information, acceptance tests and trusted check commands before scoring. Verify each oracle against the initial failing state and a known correct reference. Acceptance tests remain outside agent mutation access; both arms receive identical behavioral requirements. Model-visible focused checks may supplement the oracle.

1. **Rust structured errors.** Agent A adds structured errors to an interface; agent B updates consumers and tests across files. Verify successful behavior, error propagation and callers against the final combined candidate. Neither intermediate contribution is required to compile alone.
2. **Go filtering and pagination.** Agents add filtering and pagination to the same handler. Verify each behavior and their composition, including filtering before pagination, ordering, boundary inputs and preservation of existing behavior. The fixture creates overlapping edits and evolving-peer feedback opportunities.
3. **Go changing relationships.** Agents begin on separate producer/consumer subtasks. Task developments expose a shared contract change, followed by independent follow-up work. Deliver identical developments to both arms at predefined observable task milestones, rather than recommendation-dependent triggers. Preserve unfinished-work conditions. Do not instruct agents when to join or split; Falinks identifies useful transitions and proposes them under the agent-agreement policy.

Final fixture bytes, API names, oracle implementations and milestone definitions are evaluation setup work, not existing artifacts. Freeze and record them before pilots/scoring. Use scripted protocol checks for rare faults rather than expecting models to generate them spontaneously. Do not require a predetermined transition schedule or count.

## Fair baseline

Both arms use two agents, fresh contexts, the same model/version and reasoning configuration, task allocation, behavioral requirements, dependency information, peer announcements and feedback opportunities. Provide concise arm-specific workflow instructions of comparable scope.

The baseline uses parallel Git worktrees, may inspect/exchange unfinished diffs, communicates about dependencies and combines contributions with ordinary Git merging. It validates an exact combined candidate against equivalent compilation and acceptance checks. Falinks uses live shared working code and its enforced revision/checkpoint protocol. Give both equivalent task developments and information, without imposing Falinks' internal ledger on Git. Record instruction/tool differences as part of the treatment.

## Run sequence and limits

1. Prepare/freeze fixtures, oracle, instructions, runtime configuration and result schema; complete safety gates.
2. Run one unscored paired pilot: one fresh Falinks run and one fresh baseline run. Record fixture selection and integration repairs. If a repair changes frozen materials, version them before scoring.
3. Run three paired repetitions for each fixture: nine pairs, 18 scored runs. Alternate arm order, reverse starting order across fixtures, and record the schedule. Reset workspace, storage and agent context for every run. Do not pass solutions or transcripts between arms.
4. Bound each run to 30 minutes. Bound evaluation setup separately to eight hours; this is not an engine implementation estimate. Subscription exhaustion pauses the batch with incomplete evidence rather than causing an automatic provider switch. There is no agreed dollar ceiling or API budget.
5. Keep failed/timed-out runs in results. Record infrastructure failures separately; replacements consume usage/time and must be identified rather than silently replacing evidence. Adjust limits only between batches and record the change.

## Measurements

Start the completion clock when the runner begins preparing the arm from a clean fixture. Stop only when its final exact candidate is accepted and passes the independent oracle. Include per-run preparation, agent work, communication, checks, retries and integration. Report one-time installation/setup separately.

For each run record fixture/version, arm, runtime/model/configuration, order, initial/final snapshot, outcome, oracle results, elapsed time, timeout/failure reason, available token usage, usage-limit interruptions, rejected operations/retries, discarded or rewritten work, validation attempts, and recommendation/transition counts. If usage is unavailable, mark it unavailable; tokens do not establish dollar cost under a subscription.

Collect a lightweight timeline of agent work, waits, reconsideration, mutation rejection, validation, merging and regrouping. Distinguish observed activity from inferred reasoning time. Concurrent intervals overlap, so do not sum them into elapsed time. Preserve transcripts/events and exact candidate/check evidence for diagnosis.

Review recommendations for evidence, acceptance, timing, preservation and overhead. No transition means regrouping benefit is unestablished and calls for investigation; it is not automatically a failure. Ordinary task success cannot by itself prove the regrouping policy helped.

## Interpretation and improvement

Correctness is mandatory. Unsafe publication or lost work pauses performance evaluation until repaired and the safety gate passes again; retain the failed evidence.

A continuation signal is at least 20% lower median completion time on two of three fixtures with no lower task-success rate. Compute time comparisons on paired repetitions where both arms succeed and report the number of qualifying pairs alongside all successes/failures. No jointly successful pairs means no time comparison. With three repetitions per fixture, this is exploratory evidence, not statistical proof or general superiority. Monetary efficiency remains unestablished without comparable billing evidence; report available resource usage separately.

Mixed results, missing transitions, usage exhaustion or insufficient successful pairs are inconclusive. Missing the target is not immediate abandonment: inspect the largest observed bottleneck and propose one separately bounded improvement batch. Preserve initial results, version the changed engine and rerun both arms comparably. Do not tune against hidden oracle cases. Repeated protocol failures, inability to enforce the adapter boundary, or overhead that cannot plausibly be addressed motivate redesign. A promising result motivates a subsequent real-repository trial.

## Decision references and learning status

- [Live coordination and safe checkpoints](https://github.com/danielrispler/Falinks/issues/11#issuecomment-5995165615): adapter, permission, capture and validation boundaries.
- [Attributable revisions and dependency history](https://github.com/danielrispler/Falinks/issues/12#issuecomment-5996735000): exact contribution, offer and reconsideration records.
- [Notification handling and revalidation](https://github.com/danielrispler/Falinks/issues/17#issuecomment-5970847618): messages, deferral and revision-bound obligations.
- [Publication authority and restart recovery](https://github.com/danielrispler/Falinks/issues/13#issuecomment-5954423678): acceptance, races, retries and durable recovery.
- [Splitting and joining agent work](https://github.com/danielrispler/Falinks/issues/14#issuecomment-5999050658): recommendations, agreement, preservation and churn control.

Daniel agreed to the design through the live exchange and then explicitly requested completion without the explanation. Record this as a waiver; no unresolved understanding gap was stated. The earlier grilling HTML is a discussion aid, not a completed ticket explanation. No engine implementation or scored runs were performed. Next entry point: address the map's remaining retrieval/navigation fog before declaring the map complete, or explicitly scope it out with Daniel. No new ticket is implied by this evaluation decision, and no open child tickets require refreshed reading links after this closure.
