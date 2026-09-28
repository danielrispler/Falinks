# Edit coordination: bounded prior-art research

Research date: 2026-09-28. Decision context: [Compare edit-combination and dependency-tracking techniques for the first experiment](https://github.com/danielrispler/Falinks/issues/2).

## Outcome

**Recommendation for discussion, not an architectural decision:** test a text-based three-way merge plus explicit contract-version checks, with one serialized publication boundary. Reuse Git commands initially; do not write a parser or merge algorithm. This fits the agreed stable snapshots, Rust engine, TypeScript target, staged compilation/tests, and proposer-owned recovery. Evidence supports investigating this combination; it does not establish novelty or productivity advantage.

All six supplied URLs resolved to the expected project or paper. Repository revisions below were fetched through GitHub's API; inspected source links are pinned. Papers were read at version 1. This was a bounded source inspection, not a security audit, integration trial, or benchmark. **No system's behavior or performance was experimentally verified.** Tests present in repositories were not run. Claims absent from inspected paths are unknown, not proof the feature does not exist elsewhere.

## Three separate problems

1. **Combining edits:** preserve compatible changes from the same starting snapshot.
2. **Checking assumptions:** reject or request revalidation when a declared prerequisite changed, even when edits combine cleanly.
3. **Publishing safely:** publish precisely the candidate that passed checks, without a concurrent publication slipping between checking and accepting it.

Example: A changes `calculatePrice`; B changes `formatReceipt`. A textual merge might preserve both. If B declared dependence on pricing contract version 3 and A publishes version 4, B must still revalidate. Conversely, changing an unrelated comment should not invalidate that contract dependency. Defining “contract changed” is still a human design decision.

## Prior art and evidence

### Serena: semantic navigation and editing

**Documented:** symbol navigation, reference lookup, symbol-body replacement, and LSP/JetBrains backends. These are useful examples for future code retrieval; they are not evidence of shared-snapshot transaction guarantees. [README](https://github.com/oraios/serena/blob/7a2968335f2198b966864de1ce3655c8e485a653/README.md)

**Inspected:** `CodeEditor.replace_body` resolves a unique symbol, obtains its body positions, deletes that range, and inserts replacement text. Saving calls an atomic file-write helper. No base-snapshot or contract-version argument appears in this inspected operation. An atomic file replacement and an atomic read-check-publish transaction are different properties. [Source](https://github.com/oraios/serena/blob/7a2968335f2198b966864de1ce3655c8e485a653/src/serena/code_editor.py)

**Implication:** symbol targets can improve addressing and navigation without resolving stale assumptions. Reuse as a later integration candidate, not the initial coordination core. No concurrency or latency claims were tested.

### Plumb: guarded edits and short locks

**Documented:** a shared daemon, per-path serialization, optimistic SHA/mtime guards, LSP and tree-sitter support, and advisory intents. [README](https://github.com/plumbkit/plumb/blob/1e818c6aa500d3c230ac43028c893a6d9be58e17/README.md)

**Inspected:** `verifyExpectedVersion` checks optional expected mtime/SHA values; the session tracker also compares recorded reads. `EditFile` acquires a path lock. Workspace edits acquire locks in canonical order, prepare changes in memory, then write files, attempting rollback on error. This is not proof of crash-atomic multi-file publication or protection from writers outside those locks. [Guards](https://github.com/plumbkit/plumb/blob/1e818c6aa500d3c230ac43028c893a6d9be58e17/internal/tools/write_guards.go), [edit path](https://github.com/plumbkit/plumb/blob/1e818c6aa500d3c230ac43028c893a6d9be58e17/internal/tools/edit_file.go), [workspace edits](https://github.com/plumbkit/plumb/blob/1e818c6aa500d3c230ac43028c893a6d9be58e17/internal/tools/edit_apply.go).

**Implication:** a strong reference for actionable stale-write errors and lock scope. Whole-file expected hashes alone would reject the second independent same-file edit. We still need merge rules and separately versioned dependencies. The inspected workspace-edit source notes a UTF-16/UTF-8 offset limitation: syntax tooling does not remove representation hazards. No tests were run.

### MCP Agent Mail: coordination rather than merging

**Documented:** identities, messages, searchable history, and advisory file/glob reservations. Reservations return granted paths and conflicts; an optional pre-commit guard adds enforcement at commit time. These are collaboration signals, not a demonstrated proposal merge or declared-contract freshness protocol. [README](https://github.com/Dicklesworthstone/mcp_agent_mail/blob/4b11f26277f611e60bcfdb3858c305d1af2fcc53/README.md)

**Inspection scope:** README and actual license were read; reservation enforcement implementation was not analyzed. No execution. Do not mistake mailbox message history for versioned technical-contract validation.

**Reuse limitation:** the actual license contains a restrictive OpenAI/Anthropic rider; this is not plain MIT. It broadly defines restricted parties and prohibited access/use, including analysis and testing. Do not adopt it as an unrestricted dependency. Establish permission and suitability for the intended integration before any reuse. [Pinned license](https://github.com/Dicklesworthstone/mcp_agent_mail/blob/4b11f26277f611e60bcfdb3858c305d1af2fcc53/LICENSE)

### Aura: meaning/provenance and structured merging

**Documented:** a graph of symbols, intent and provenance alongside Git, with commit gates and function-level operations. [README](https://github.com/Naridon-Inc/aura/blob/01d9e1735e27df62838bca45d73d5204c79b9e81/README.md)

**Inspected:** the standalone Rust `aura-merge` crate has structured merge results and separate text/JSON strategies. Its text three-way implementation computes two unused diff variables and then advances line positions together in common branches. This warrants insertion/deletion alignment tests before reuse; it is not a tested defect report. The crate README explicitly lists YAML/TOML parsers as unfinished. [Implementation](https://github.com/Naridon-Inc/aura/blob/01d9e1735e27df62838bca45d73d5204c79b9e81/crates/aura-merge/src/lib.rs), [crate README](https://github.com/Naridon-Inc/aura/blob/01d9e1735e27df62838bca45d73d5204c79b9e81/crates/aura-merge/README.md).

**Implication:** useful interface and provenance prior art. The inspected crate does not establish the full application's guarantees, and the advertised meaning graph is not evidence that every assumption is inferred. Prefer a mature merger for the first experiment; revisit targeted reuse only with acceptance tests. No Aura behavior was run.

### STORM: file-level optimistic validation

**Paper claim:** *Multi-agent Collaboration with State Management* tracks observed file versions and checks the agent's read set before accepting writes. Stale writes return current content, differences and dependency information; temporary reservations mitigate repeated conflicts. The paper distinguishes local observations from a frozen repository snapshot and acknowledges terminal bypass and file granularity limits. It reports benchmark gains, not independently reproduced here. [Paper, sections 2 and E](https://arxiv.org/html/2605.20563v1)

**Implication:** particularly relevant prior art for declared freshness and actionable recovery. Checking an entire changed file conservatively rejects independent edits to different functions within it. Our chosen acceptance scenario therefore requires a different granularity or safe combination step. Intent comments communicate reasoning but do not mechanically prove semantic compatibility.

**Evidence limit:** paper only; its linked implementation at `dreamyang-liu/STORM` was not inspected or run. No code-license conclusion is made. The paper is marked CC BY 4.0; that does not establish the implementation's license.

### CoAgent: notification and repair

**Paper claim:** *CoAgent: Concurrency Control for Multi-Agent Systems* fixes a serialization order, filters reads, tracks tool footprints, and uses notifications plus compensating tool actions to reconcile speculative writes. Its serializability statement explicitly depends on protocol compliance and correct agent judgments about affected premises. It reports empirical results; these were not reproduced. [Paper, sections 4–6](https://arxiv.org/html/2606.15376v1)

**Implication:** useful for understanding why whole-task restart is costly and why recovery should preserve useful work. Its external-world, undoable-tool machinery exceeds our file-staging experiment. We should not replace deterministic dependency enforcement with an assumption that the model judges every conflict correctly.

**Evidence limit:** no implementation inspected; no reusable code license verified. The paper's arXiv non-exclusive distribution license is not a software reuse grant. The framework and its broader guarantee are not equivalent to our narrower declared-dependency checks.

## Techniques and fit

| Technique | What it gives us | What it does not give us |
|---|---|---|
| Exact patch against a base | A reviewable edit description | Automatic relocation or combination against changed content |
| Three-way text merge | Compares base, proposal, and current content; reports textual conflicts | Semantic correctness or contract freshness |
| Optimistic version checks | Detects whether specified observed versions changed before acceptance | Knowledge of undeclared assumptions |
| Symbol/range targeting | More precise addresses; potentially fewer false conflicts | Stable identity through arbitrary renames, semantic equivalence, or publication atomicity |
| Short publication lock | Serializes engine acceptance while clients reason in parallel | Exclusion of uncooperative external filesystem writers |
| Long-lived file reservations | Signals ownership and may avoid collisions | Independent same-file concurrency unless granularity changes |
| CRDT | Automatic convergence under its data model | A guarantee that converged source compiles or preserves program intent |

The Git manual defines `merge-file` as three-way merging, with conflict reporting; do not enable favor-ours/favor-theirs behavior for this experiment. Git's `update-ref` supports an expected old object ID, a useful compare-and-swap primitive. A successful ref update is not by itself an atomic checkout of several working files. [Merge manual](https://git-scm.com/docs/git-merge-file), [ref-update manual](https://git-scm.com/docs/git-update-ref).

LSP represents edits using document identities/versions and text ranges; symbols can locate ranges without requiring a custom parser. It does not define our contract-dependency protocol. [LSP specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)

Automerge illustrates CRDT-backed concurrent document editing. Convergence is attractive when replicated/offline editing is itself a requirement; one machine and two proposal clients do not yet establish that need. This is a scope recommendation, not a claim that CRDTs are incapable of use here. [Automerge introduction](https://automerge.org/docs/hello/)

## Reuse and license checklist

These are source-derived conditions to carry into component selection, not a completed legal/dependency audit. Pin the actual selected revision and examine bundled dependencies too.

- **Git executable:** recommended initial merger and snapshot infrastructure. Git's license is GPL v2 except where otherwise stated. Running installed Git differs from copying its implementation into our Rust code; bundling/distributing Git requires following its distribution terms. [COPYING](https://github.com/git/git/blob/master/COPYING)
- **Plumb:** inspected root license is MIT; copies or substantial portions must retain copyright and permission notices. Its implementation is Go, so learn from its protocol before adding an interprocess service or porting code. [License](https://github.com/plumbkit/plumb/blob/1e818c6aa500d3c230ac43028c893a6d9be58e17/LICENSE)
- **Serena:** current application is GPL-3.0-or-later; SolidLSP is separately MIT. The license overview identifies an older MIT cutoff, but this investigation did not inspect that historical version. Copying current application code carries different obligations from selecting SolidLSP. GPL distribution obligations include providing corresponding source under applicable terms. [Overview](https://github.com/oraios/serena/blob/7a2968335f2198b966864de1ce3655c8e485a653/LICENSE)
- **Aura / aura-merge:** inspected licenses are Apache-2.0. Redistribution conditions include a license copy, retained applicable notices, marking changed files, and carrying applicable NOTICE contents; patent terms also apply. Verify crate dependencies and notices before adoption. [Crate license](https://github.com/Naridon-Inc/aura/blob/01d9e1735e27df62838bca45d73d5204c79b9e81/crates/aura-merge/LICENSE), [authoritative Apache terms](https://www.apache.org/licenses/LICENSE-2.0).
- **Agent Mail:** restrictive rider described above; no recommendation to incorporate code.
- **STORM / CoAgent:** learn from the papers; software reuse remains unassessed.

## What the next decision should settle

Choose the precise proposal/publication contract, rather than adding more infrastructure:

- What constitutes an incompatible “same target” edit? Ordinary text merging may accept different lines inside one function even if intent conflicts. Do we define the first fixture textually or require explicit target-level exclusion? Do not promise all semantic conflicts are detectable.
- What exactly identifies and versions a contract? Content hashes detect content equality; monotonic versions also expose change-away-and-back. Decide whether each matters.
- Is published state an immutable repository object/reference, or live working-tree files? This determines publication and external-edit guarantees.
- Does every refreshed candidate rerun compilation/tests? It must if the candidate content changed; a passed old candidate is not evidence for the new one.
- Which paths count as external edits? Account for untracked files, generated outputs, symlinks and validation commands that write files. A preflight hash cannot exclude a writer acting immediately afterward.

For the four original scenarios, define exact before/proposal/after fixtures and both submission orders. Add a publication-race fixture and a failed-validation fixture: rejected proposals must leave published state unchanged. These are proposed checks, not implemented tests. An unrelated edit should not cause dependency revalidation merely because a global repository version changed; validation of a new combined candidate is a separate operation.

The real-agent comparison should reuse tasks and dependency information across Falinks and Git-worktree workflows. Record correctness first, then wall time, billed model cost, retries, discarded changes and coordination/validation overhead. No prior-art benchmark establishes our performance result.

## Short learning path

1. **Three-way merging (20 minutes):** read the [Git merge-file manual](https://git-scm.com/docs/git-merge-file). Draw three boxes: base, A, B. Work through changes to different lines and then the same line. Understand why the common base matters before choosing a patch format.
2. **Optimistic checks (20 minutes):** read [Plumb's short guard implementation](https://github.com/plumbkit/plumb/blob/1e818c6aa500d3c230ac43028c893a6d9be58e17/internal/tools/write_guards.go). Translate expected/current SHA into the database-style rule “update only if version still matches.” Notice that the check must be protected from a subsequent racing write.
3. **Stale context (25 minutes):** read [STORM section 2 and limitations](https://arxiv.org/html/2605.20563v1). Sketch a read set. Ask which harmless edits cause conservative rejection and which unobserved assumptions remain invisible.
4. **Symbols versus correctness (15 minutes):** read [Serena's replacement operation](https://github.com/oraios/serena/blob/7a2968335f2198b966864de1ce3655c8e485a653/src/serena/code_editor.py). A symbol resolves to positions; ask what happens when content changes after locating them.
5. **Recovery and stronger claims (30 minutes, optional):** read [CoAgent sections 3–5](https://arxiv.org/html/2606.15376v1). Focus on its assumptions rather than starting with its proof. Separate “agent says unaffected” from “engine can verify unchanged.”
6. **Rust when implementation begins:** read the [Rust Book on Result](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html) and [ownership](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html). Map conflict, stale dependency and validation failure to explicit outcomes; these concepts become useful before async runtimes or fine-grained locking.

No production implementation was created. Remaining paper-code inspection, benchmark reproduction and full dependency audits are deliberately deferred until a concrete selection requires them.
