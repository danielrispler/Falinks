# Learning through Falinks

Falinks is an educational project first. This workflow governs Daniel and the agent working together. Its goal is to build the understanding needed to explain the system, predict behavior, challenge decisions, and propose the next idea. Daniel has TypeScript/full-stack experience and is learning Rust.

The main risk is cognitive debt: the project advancing faster than Daniel's mental model. Agent implementation and verification can proceed autonomously within the agreed scope; understanding remains a required ticket deliverable.

## Session opening

Begin each session with a brief orientation: the current state, relevant settled decisions, unresolved questions or blockers, today's goal, and its stop condition. Use the current ticket/map and required reading when working through Wayfinder; use the current task and available context for other sessions. Give enough context to resume without reconstructing the previous conversation, and distinguish recorded facts from assumptions. Briefly revisit a relevant learning gap or concept when it helps today's work.

## Planning and questions

Apply this teaching approach during grilling, Wayfinder, and ordinary planning, including before implementation exists. Establish background and intuition using the project's vocabulary before technical alternatives. For each decision question, explain the dilemma, why it matters, the viable options and their consequences, a concrete scenario or failure that makes the difference visible, and the recommendation with its reasoning. Make clear what the answer will decide or unblock.

Support deeper discovery through predictions, experiments, and hints. Give Daniel an opportunity to reason about a scenario before revealing its outcome when useful; provide a direct explanation when requested. Build shared understanding before asking Daniel to choose. Batch independent questions with sufficient context for each; questions dependent on unanswered choices wait for a later round. Find environmental facts yourself rather than turning them into questions for Daniel.

## Ticket workflow

Design together → Agent implements → Tests → Explain diff → Read core code → Quiz → Simulator if needed → Next ticket.

Apply this to every meaningful ticket, rather than every small commit. A meaningful ticket introduces or changes a mechanism, consequential design decision, failure behavior, or substantial body of knowledge. Research, design, and prototype tickets also need a learning deliverable. Combine routine edits into the relevant ticket's explanation.

1. **Design together.** Use the planning approach above to agree on the problem, design, scope, and relevant learning goals.
2. **Implement and verify.** Work independently within the agreed design. Run checks appropriate to the change, and distinguish verified behavior from assumptions and remaining uncertainty. For research/design tickets, check the evidence and consistency of the decision instead of inventing implementation or test results.
3. **Explain the ticket.** After the meaningful unit of work, remind Daniel to invoke `$dsh-explain-diff` and wait for that explicit request before loading or running the global skill. The reminder does not complete the learning deliverable. Explain the full ticket outcome, including its surrounding system and relevant changes across commits. Save one self-contained HTML page at `~/code-explanations/Falinks/YYYY-MM-DD-<ticket-or-topic>.html` and return its absolute path as a clickable link.
4. **Read the core.** Identify the 3–5 most important code passages, ordered by how the mechanism works. Give verified file/line references, explain why each matters, and state what to notice. Use fewer when the work has fewer relevant passages. For tickets without code changes, identify the relevant decision/evidence passages and explain that distinction.
5. **Use the HTML knowledge check.** Put open-ended questions about explanation, prediction, failure cases, trade-offs, and a possible modification in the explanation HTML, with reasoning available to reveal. Daniel works through these there; do not repeat them or require a second quiz in chat. Discuss answers or offer a new scenario only when Daniel requests it or raises an understanding gap.
6. **Explore a micro-world when useful.** Use a small simulator or explorable trace for concurrency, locks, revisions, recovery, event ordering, and other behavior that remains hard to visualize. Let Daniel change inputs/order, inspect state, and predict outcomes. Name the model's simplifications and check its examples against the actual mechanism or documented design. Introduce it earlier when it would help the design or explanation.
7. **Complete the learning stop.** The HTML check should cover explaining the problem and chosen mechanism, predicting a new scenario, and reasoning about an important invariant, failure, or trade-off. After the explanation is delivered, Daniel's statement that he is ready to continue completes this self-directed learning stop unless he identifies an unresolved gap. Record that as Daniel's confirmation, rather than claiming independently assessed answers. Help with any gap he raises before advancing; preparation can continue while learning.

## What the explanation covers

- The problem and prior behavior, with enough background to understand the change.
- The architecture, component relationships, and how the outcome connects to the rest of the system.
- Key decisions, alternatives, and consequences, including why the chosen approach fits.
- Invariants: what must remain true, where that is enforced, and what happens if it is violated.
- Failure cases and recovery, with concrete inputs and event sequences.
- Important Rust concepts used by the change, taught in context with TypeScript comparisons when helpful.
- The ordered walkthrough, core reading passages, validation evidence, and unresolved limitations.

Ground claims in inspected code, tests, current decisions, and primary sources where needed. Label a toy model, proposed behavior, or inference explicitly so a teaching diagram cannot silently become a specification.

## Continuity and ticket resolution

Record the explanation path, core reading pointers, concepts explored, remaining understanding gaps, and learning status with the relevant ticket's session outcome. Local HTML paths are for Daniel's machine; the ticket's durable decision and evidence must remain understandable without access to those files. At the next session, briefly recap the model and any gap Daniel raised; use the existing HTML check rather than repeating its questions in chat.

Implementation/testing complete and learning complete are distinct states in the session record. Resolve a meaningful ticket only when both its work and its learning stop are complete, then follow `docs/agents/issue-tracker.md` for closure, reading-link updates, and the map pointer. Keep canonical decisions in their ticket resolutions, domain vocabulary in `CONTEXT.md`, and architecture records in `docs/adr/` when warranted.

This guide is the source of the Falinks learning defaults. The global explanation skill supplies the reusable artifact format. Learning stops here are human learning steps; the engine's **checkpoint offer** retains its existing meaning in `CONTEXT.md`.
