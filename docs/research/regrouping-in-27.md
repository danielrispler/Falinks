# Why regrouping never triggered in #27

Issue [#124](https://github.com/danielrispler/Falinks/issues/124), 14 October 2026. Research evidence and a recommendation. No code changes.

## Question

No Falinks-arm run in the #27 batch produced a regrouping transition ([#27 resolution](https://github.com/danielrispler/Falinks/issues/27#issuecomment-6168318968)). Why not? Did the engine ever recommend a join or split, and did agents ever ask for one?

The ticket names four directions:

1. Whether agents called `recommend`, `propose` or `respond`, and what the engine answered.
2. The engine's durable wait records, which exist as evidence for regrouping recommendations.
3. Whether the fixtures' milestones create conditions the [#14 policy](https://github.com/danielrispler/Falinks/issues/14#issuecomment-5999050658) recognizes. Fixture 3 (go-relationships) was designed for a changing relationship.
4. Whether the instructions leave agents any reason to ask. [#5](https://github.com/danielrispler/Falinks/issues/5#issuecomment-6045386497) forbids telling them when to join or split.

The evidence covers all 12 Falinks-arm runs: the 9 scored runs and the 3 pilot attempts (`pilot`, `pilot2`, `pilot3`).

## Answer

- **The engine reconsidered throughout every run, and it answered Keep every time.** The evaluation host turned on engine-driven reconsideration at setup. The engine made 53 regrouping proposals, 39 of them in the scored runs, and every one was Keep. #27 reported "0 recommendations" because its count leaves out Keep proposals.
- **Agents never called `recommend`, `propose` or `respond`.** None of the 1,167 tool calls (886 in scored runs) was a regrouping call. Every session exposed the three tools under their expected names. The relay works: every pre-batch gate applied a split and a join through it. Asking would not have changed anything, because `recommend` runs the same code as the engine's own reconsideration and would have returned the same Keep.
- **A split was unreachable, and a join was impossible.** Agents start joined, so the only transition the engine can recommend is a split. The split rule requires that no collaboration signal is present at all. In every run, the two registered scopes were related by declaration: one agent declared a dependency on the other's code, or both registered the same node. Every run also had friction from the first peer live edit. Friction counts since the last applied transition, so without a transition it never resets. Once joined agents have any friction, the engine recommends Keep for the rest of the run.
- **Wait records played no part.** The `waits` table was empty in all 12 ledgers. Only the engine's `wait` call writes it, and no host or adapter calls `wait`. When agents waited by ending their turn, the engine recorded nothing.
- **Fixture 3 never looked independent to the engine.** In each scored go-relationships run, each phase had exactly one accepted edit per agent. Every edit raised a review obligation for the peer, including the follow-up edits meant to be independent. All six follow-up reviews decided Keep, with notes such as "No consumer impact". The fixture's own dependency text announces the producer/consumer coupling at the start. Its follow-up `Render` still takes `[]Record`.
- **The instructions gave agents no reason to ask.** The only workflow sentence about regrouping presents it as something the engine proposes. The tool descriptions say what the tools do, not what a join or split changes. Meanwhile the engine sent its Keep proposals to both agents as events, 4–5 per run. Agents handled every one without comment.
- **Recommendation: change the policy and fixture 3 together, through a grilling ticket.** Instruction changes should only describe the fields and evidence the policy reads. Changing the instructions alone cannot produce a transition. With no change, the next batch will almost certainly report 0 transitions again.

## Evidence

Run names below drop the `-falinks` suffix. The ledger, transcript and gate queries are listed under [Sources](#sources) as Q1–Q14. Agent 0 is allocation A and agent 1 is allocation B ([`adapters/claude/falinks_arm.rs:285`](../../adapters/claude/falinks_arm.rs)).

### 1. Calls to `recommend`, `propose` and `respond`

No agent called a regrouping tool in any run (Q1).

| Run | Kind | Fixture | Outcome, elapsed | Tool calls | `recommend` / `propose` / `respond` | Engine proposals | Non-Keep proposals | Transitions |
|---|---|---|---|---|---|---|---|---|
| pilot | pilot attempt (#110) | go-relationships | infrastructure_failure, 67.0 s | 35 | 0 | 4 | 0 | 0 |
| pilot2 | pilot attempt (#112) | go-relationships | success, 172.1 s | 122 | 0 | 5 | 0 | 0 |
| pilot3 | recorded pilot | go-relationships | success, 227.6 s | 124 | 0 | 5 | 0 | 0 |
| rust-errors-1 | scored | rust-errors | success, 112.0 s | 78 | 0 | 4 | 0 | 0 |
| rust-errors-2 | scored | rust-errors | success, 111.3 s | 73 | 0 | 4 | 0 | 0 |
| rust-errors-3 | scored | rust-errors | success, 118.9 s | 87 | 0 | 4 | 0 | 0 |
| go-page-1 | scored | go-page | success, 333.3 s | 107 | 0 | 4 | 0 | 0 |
| go-page-2 | scored | go-page | success, 304.2 s | 108 | 0 | 4 | 0 | 0 |
| go-page-3 | scored | go-page | success, 292.0 s | 112 | 0 | 5 | 0 | 0 |
| go-relationships-1 | scored | go-relationships | success, 177.7 s | 111 | 0 | 5 | 0 | 0 |
| go-relationships-2 | scored | go-relationships | success, 177.4 s | 104 | 0 | 5 | 0 | 0 |
| go-relationships-3 | scored | go-relationships | success, 172.9 s | 106 | 0 | 4 | 0 | 0 |
| **Scored total** | | | | **886** | **0** | **39** | **0** | **0** |
| **Pilot total** | | | | **281** | **0** | **14** | **0** | **0** |

The scored tool-call counts match the #27 [evidence record](https://github.com/danielrispler/Falinks/issues/27#issuecomment-6168534279). Outcomes and elapsed times come from each run's `result.json`.

Because no agent called the tools, the engine never answered a request. Every proposal in the ledgers has `proposer: null`, which means the engine made it, not an agent (Q3). The `result.json` field `recommendations` counts only engine proposals whose action is not Keep ([`adapters/claude/falinks_arm.rs:433-439`](../../adapters/claude/falinks_arm.rs)). That is why #27 reports 0.

### 2. What the engine answered on its own

**Engine-driven reconsideration was on.** The Falinks arm provisions a second live root (`space1`) and calls `reconsider()` before the agents start ([`adapters/claude/falinks_arm.rs:602-605`](../../adapters/claude/falinks_arm.rs)). `reconsider()` sets `regroup_enabled` ([`src/regroup.rs:375-380`](../../src/regroup.rs)). After that, the engine reconsiders after every registration, review, edit, validation run and response ([`src/regroup.rs:712-716`](../../src/regroup.rs); [`docs/engine.md:167`](../engine.md)). It stores a new proposal only when the evidence changes materially. Otherwise it returns the earlier proposal and emits nothing ([`src/regroup.rs:603-624`](../../src/regroup.rs), [`src/regroup.rs:1079-1089`](../../src/regroup.rs)). Every ledger has `regroup_enabled = 1`, two provisioned spaces, no `regroup_mark` and no `shifts` row: no transition was ever applied (Q2).

**Every run followed the same sequence (Q3, Q7):**

| Proposal | When the engine made it | Decision and the engine's reasoning |
|---|---|---|
| `regroup:1` | Host `reconsider()` at setup, before agents start | Keep: "uncertain evidence [agent 0 registered no scope, agent 1 registered no scope]; prefer fewer groups" |
| `regroup:2` | After the first agent registered a scope | Keep: uncertain, because the other agent has no scope |
| `regroup:3` | After the second agent registered | Keep: "ongoing collaboration", with a non-empty `declared` list and 0 retries, 0 reconsiderations, 0 waits |
| `regroup:4` | Right after the first obligation raised by a peer live edit (11 of 12 runs) | Keep: "ongoing collaboration", reconsiderations 1 |
| `regroup:5` (some runs) | Analysis evidence degraded by a broken draft (pilot2, pilot3, go-relationships-1, go-relationships-2) | Keep: "uncertain evidence [degraded evidence for tree …]; prefer fewer groups" |

go-page-3 differs only in order: its `regroup:4` is the degraded-evidence Keep, and its `regroup:5` is "ongoing collaboration" with reconsiderations 2. In total the scored runs produced 18 "no scope" Keeps, 18 "ongoing collaboration" Keeps and 3 "degraded evidence" Keeps.

**From the starting arrangement, only a split was possible.** The initial arrangement is joined: both agents are placed in space 0 ([`docs/engine.md:129`](../engine.md)). For joined agents, `decide` can return only Split or Keep. It returns Split only when no agent reports failure, `uncertain` is empty, and `declared`, `shared_tasks`, `compiler` and friction are all empty or zero ([`src/regroup.rs:1093-1129`](../../src/regroup.rs)). A Join request while joined becomes Keep ([`src/regroup.rs:558-561`](../../src/regroup.rs)). [`docs/engine.md:144`](../engine.md) states the same rule: "Joined agents split only when evidence is certain and shows no relationship, declaration, shared task or friction."

**Every run had a declaration from the moment both agents registered (Q4).** A pair counts as `declared` when one scope's registered nodes or declared dependencies touch the other's nodes ([`src/regroup.rs:482-487`](../../src/regroup.rs); [`src/coordination.rs:29-33`](../../src/coordination.rs)). Each agent registered exactly once and never changed its scope. Every run shows three distinct proposal contexts: no scopes, one scope, two scopes (Q4).

| Run | What made the pair `declared` | Same `task` label on both scopes |
|---|---|---|
| pilot | Each depends on the other's function (`consumer.go#Consume`, `producer.go#Produce`) | yes, `go-relationships` |
| pilot2 | Each depends on the other's function | no (`producer` / `go-relationships`) |
| pilot3 | B depends on `producer.go#Produce` | yes |
| rust-errors-1, -2, -3 | B depends on `src/catalog.rs`, A's file | yes, `rust-errors` |
| go-page-1, -2, -3 | Both register `handler.go#Handler` | no |
| go-relationships-1 | A depends on `consumer.go`, B on `producer.go#Produce` | no (`producer` / `go-relationships`) |
| go-relationships-2, -3 | Each depends on the other's function | yes, `go-relationships` |

`shared_tasks` compares the free-text `task` field that agents choose ([`src/regroup.rs:503-507`](../../src/regroup.rs)). Seven of 12 runs had the same label on both scopes, usually the fixture name. Because `declared` is checked first and skips the compiler check for that pair ([`src/regroup.rs:482-487`](../../src/regroup.rs)), the `compiler: []` in every proposal says nothing about compiler relationships.

**Asking would have returned the same Keep.** `recommend(client)` calls the same `recommendation(None, None)` as the host and boundary reconsideration ([`src/regroup.rs:381-385`](../../src/regroup.rs), [`src/regroup.rs:714-716`](../../src/regroup.rs)). With unchanged evidence it returns the earlier proposal ([`src/regroup.rs:615-623`](../../src/regroup.rs)). At every point in every run, `decide` had one of two Keep reasons. Before both agents registered, one had no scope, which is uncertain evidence. After both registered, `declared` was non-empty. Inference: an agent's `recommend` at any moment would have returned Keep and cost one round-trip.

### 3. Wait records and the other friction signals

**No wait records exist.** The `waits` table has three columns, `id`, `agent` and `condition`, with no timestamp or duration ([`src/regroup.rs:132`](../../src/regroup.rs)). Only `Engine::wait` inserts a row ([`src/coordination.rs:953-967`](../../src/coordination.rs)). `wait` is not an agent tool ([`adapters/claude/engine_host.rs:184-185`](../../adapters/claude/engine_host.rs); [`docs/engine.md:171`](../engine.md)). No host, adapter or evaluation code calls it: `rg -n "Condition::(Message|Checkpoint)" adapters evaluation/src src` finds only its own definition in `src/coordination.rs`. Only `tests/` call it. All 12 ledgers have 0 wait rows (Q2). So the records cannot say how long anyone waited, between which agents, or for what. Agents waited by ending their turn, as the prompt asks ([`adapters/claude/falinks_arm.rs:290`](../../adapters/claude/falinks_arm.rs)). The engine sees none of that. The closest measure is #116's: agents were in a turn for 85% of elapsed time ([#116 resolution](https://github.com/danielrispler/Falinks/issues/116#issuecomment-6169349150)).

**The other friction signals were present in every run.** The query below applies the engine's own SQL ([`src/regroup.rs:508-524`](../../src/regroup.rs)) with the mark at zero, since no transition ever set it (Q5). These are the values an agent's `recommend` would have read at the end of each run. With two agents, every signal is between the same pair.

| Run | Retries (`Stale` or `Unreviewed` edits) | Reconsiderations (obligations from peer live edits) | Waits | `decide` for joined agents |
|---|---|---|---|---|
| pilot | 0 | 2 | 0 | Keep |
| pilot2 | 0 | 6 | 0 | Keep |
| pilot3 | 3 | 7 | 0 | Keep |
| rust-errors-1 | 1 | 4 | 0 | Keep |
| rust-errors-2 | 2 | 4 | 0 | Keep |
| rust-errors-3 | 3 | 4 | 0 | Keep |
| go-page-1 | 1 | 5 | 0 | Keep |
| go-page-2 | 1 | 5 | 0 | Keep |
| go-page-3 | 0 | 4 | 0 | Keep |
| go-relationships-1 | 1 | 6 | 0 | Keep |
| go-relationships-2 | 2 | 6 | 0 | Keep |
| go-relationships-3 | 1 | 6 | 0 | Keep |

The scored retries total 12: 9 `Unreviewed` and 3 `Stale`. Adding pilot3's 3 `Unreviewed` gives #27's figures of 12 `Unreviewed` and 3 `Stale` refusals. The 9 scored runs had 44 live edits and 44 reconsiderations. Of their 43 reviews, 38 decided Keep and 5 decided Revise (Q6).

**The joined arrangement locks in.** Friction is `retries + reconsiderations + waits > 0` ([`src/regroup.rs:47-51`](../../src/regroup.rs)). The engine counts it since the mark of the last applied transition ([`src/regroup.rs:25-26`](../../src/regroup.rs), [`src/regroup.rs:508-524`](../../src/regroup.rs)). Only an applied transition writes the mark ([`src/regroup.rs:909-915`](../../src/regroup.rs)), and it defaults to zero ([`src/regroup.rs:167-169`](../../src/regroup.rs)). Being joined is itself what creates reconsiderations: in a joined arrangement, every related peer live edit renews an obligation. After a split, live edits renew obligations only for agents in the edited space ([`docs/engine.md:131`](../engine.md)). So once joined agents have any friction, every later engine recommendation is Keep, until a transition happens, and only a split could be that transition. The only ways out are an agent's own `propose` of a split, or a proposal with `failing: true` ([`src/regroup.rs:525-533`](../../src/regroup.rs), [`src/regroup.rs:1105-1111`](../../src/regroup.rs)). In every run, the first obligation was caused by the first live edit (Q7). So the lock-in started with that edit, before the first milestone, which needs a draft from both agents.

**Did the #14 conditions hold?** #14 has no numeric thresholds. It says to reconsider "initially, on an agent request, at clean boundaries, or when the engine detects materially new evidence." The engine did reconsider initially, at boundaries and on new evidence; no agent made a request. #14's conditions for a join (from a split) never applied, because the agents were never split. The implemented conditions for a split never held.

### 4. Whether the fixtures create conditions the policy recognizes

**go-page and rust-errors.** In go-page, both agents edited `handler.go` and `handler_test.go` in every run (Q14). In rust-errors, the fixture text says "A return-type change requires coordinated consumer changes" ([`evaluation/fixtures/rust-errors.json:22`](../../evaluation/fixtures/rust-errors.json)). Inference: Keep was the right answer for both fixtures. In go-page, a split would also have been blocked by `interleaved unpublished drafts` until publication ([`src/regroup.rs:785-790`](../../src/regroup.rs)). #5 designed only fixture 3 around a change in relationship.

**Fixture 3 (go-relationships) as designed.** #5 describes it this way: "Agents begin on separate producer/consumer subtasks. Task developments expose a shared contract change, followed by independent follow-up work" ([`docs/evaluation/first-agent-evaluation.md:27`](../evaluation/first-agent-evaluation.md)). The milestones are `record-contract` and then `follow-ups` with `Stats` for A and `Render` for B ([`evaluation/fixtures/go-relationships.json:21-36`](../../evaluation/fixtures/go-relationships.json)). Each milestone fires when both agents have a new accepted draft since their last acknowledgment ([`adapters/claude/arm.rs:365-385`](../../adapters/claude/arm.rs)).

**Fixture 3 as the engine saw it.** Each agent made exactly one accepted edit per phase: `edit+` then `ack` then `edit+` then `ack` then `edit+` (Q13). That pins the phase boundaries to revisions 2 and 4. Every edit raised an obligation for the peer (Q6):

| Run | Phase (elapsed, s) | Accepted edits | Obligations for the peer | Peer review decisions |
|---|---|---|---|---|
| go-relationships-1 | initial (0–45.2) | rev 1 (B), rev 2 (A) | 2 | Keep, Revise |
| | `record-contract` (45.2–96.4) | rev 3 (B), rev 4 (A) | 2 | Keep, Keep |
| | `follow-ups` (96.4–177.7) | rev 5 (B, `Render`), rev 6 (A, `Stats`) | 2 | Keep, Keep |
| go-relationships-2 | initial (0–40.8) | rev 1 (A), rev 2 (B) | 2 | Keep, Revise |
| | `record-contract` (40.8–96.2) | rev 3 (B), rev 4 (A) | 2 | Keep, Keep |
| | `follow-ups` (96.2–177.4) | rev 5 (B, `Render`), rev 6 (A, `Stats`) | 2 | Keep, Keep |
| go-relationships-3 | initial (0–39.4) | rev 1 (A), rev 2 (B) | 2 | Keep, Revise |
| | `record-contract` (39.4–83.7) | rev 3 (B), rev 4 (A) | 2 | Keep, Keep |
| | `follow-ups` (83.7–172.9) | rev 5 (A, `Stats`), rev 6 (B, `Render`) | 2 | Keep, Keep |

Milestone times come from each run's `controller/events.jsonl` (Q12). The run ends at the `result.json` elapsed time.

What this shows:

- **The coupling is visible, but only in the review decisions.** In each run, the one Revise is the review of the second initial draft, written after `record-contract` arrived. go-relationships-2's note says: "Peer consumer draft still []string; record-contract development replaces contract. I will define Record in producer.go and switch Produce to []Record." The policy counts every obligation the same way, whatever the review decided.
- **The follow-up phase never looked independent.** Both follow-up edits raised obligations in every run, and all six reviews decided Keep. go-relationships-2's notes say: "Render added on top of unchanged Consume; Record contract intact. No impact on Produce/Stats." and "Stats added; Produce/Record unchanged. No consumer impact."

Why the fixture did not produce the independent-then-coupled-then-independent pattern:

1. **The fixture's dependency text announces the coupling at the start.** It says "Initially Produce returns []string consumed by Consume. Developments may change that shared contract." ([`evaluation/fixtures/go-relationships.json:18-19`](../../evaluation/fixtures/go-relationships.json)). Inference: agents declared `depends` on each other's functions because of that sentence. The first phase was therefore never "separate" in the engine's evidence. In the fixture bytes, `Consume` does not call `Produce` (`evaluation/fixtures/go-relationships.json`, `files`), so only the declaration related the scopes at that point.
2. **Scopes never changed.** Each agent registered once, before its first edit (Q4). The `depends` lists stayed in force through `follow-ups`. The `register` description says only "declare intended work before editing" ([`adapters/claude/engine_host.rs:207-210`](../../adapters/claude/engine_host.rs)).
3. **The follow-ups are not independent in code.** `Render(records []Record)` is built on `Consume` and on `Record`, which A declares in `producer.go` ([`evaluation/fixtures/go-relationships.json:33-34`](../../evaluation/fixtures/go-relationships.json)). Only `Stats` is independent of the shared contract.
4. **Friction from the coupled phases never expires.** Even if the follow-up edits had raised no obligations, the four earlier ones would still count, because no transition reset the mark (section 3).
5. **The independent phase is short.** It held one edit per agent, then checks, offers, publication and `ready`, over 81–89 s. Inference: even a split applied at `follow-ups` could have removed at most two obligations per run, both of which ended Keep. It would have cost two `respond` calls and a restart of the moving agent ([`adapters/claude/falinks_arm.rs:691-703`](../../adapters/claude/falinks_arm.rs)). A split would not have changed the #27 verdict.

Agents did not finish one after the other in this fixture. Both offered the same combined revision and published once at the end (one `Published` event per run, Q7).

### 5. Whether the instructions leave agents a reason to ask

These are all the regrouping texts the agents saw.

- **Workflow** ([`evaluation/instructions/falinks.md:3`](../../evaluation/instructions/falinks.md)), in every opening prompt: "Follow engine-proposed regrouping only through the affected agents' agreement mechanism. The task developments do not prescribe group arrangements."
- **Host prompt** ([`adapters/claude/falinks_arm.rs:285-294`](../../adapters/claude/falinks_arm.rs)): no mention of regrouping, joining, splitting or workspaces other than "Your live workspace … shows your peer's unfinished edits as they happen."
- **Tool descriptions**, as recorded in each agent's `controller/tools.json` (Q10) and defined at [`adapters/claude/engine_host.rs:250-261`](../../adapters/claude/engine_host.rs):
  - `falinks_recommend`: "Falinks recommend: {} -> ask the engine whether to join, split or keep the current arrangement."
  - `falinks_propose`: "Falinks propose: {action: Join|Split|Keep, text, failing: bool} -> propose a regrouping with your reasons."
  - `falinks_respond`: "Falinks respond: {id, context, response: "Agree" | {Decline: {reason}}} -> answer a regrouping proposal; repeat its exact context."
- **Engine events**: each Keep proposal reached both agents as a `Regrouping` event inside a "Host notice" ([`src/regroup.rs:152-154`](../../src/regroup.rs), [`src/regroup.rs:663`](../../src/regroup.rs)). Each one carried its full signals and reasoning, for example "uncertain evidence [\"agent 0 registered no scope\", \"agent 1 registered no scope\"]; prefer fewer groups".

The workflow sentence presents regrouping as something the engine proposes and agents agree to. No text says what a join or split changes: a separate live workspace, or no review obligations for the peer's live edits. No text says which evidence the engine reads, or what the `task` and `depends` fields of `register` feed. The engine also answered the question without being asked, 4–5 times per run. Inference: agents had no reason to call `recommend`, and the Keep events gave them a reason not to.

**Transcript evidence.** Agents handled every `Regrouping` event with `falinks_handle`: each agent once per event, all accepted, none deferred (Q8). No agent mentioned regrouping. Q9 searches every assistant text block in the Falinks-arm Claude Code session files for "regroup", "recommend", "falinks_propose", "falinks_respond", "arrangement" and phrases about splitting or joining the work, and finds 0 matches. A wider search of the transcripts finds only unrelated uses such as "strings.Join" and "API split". No agent message (`Plan` or `CheckpointOffer`) mentions regrouping. The recorded thinking blocks are almost all empty, so silent reasoning cannot be checked (see [Limits](#limits)).

This stays within #5's rule. Neither arm received anything that tells agents when to join or split. The milestone triggers do not depend on recommendations ([#24 resolution](https://github.com/danielrispler/Falinks/issues/24#issuecomment-6095496772); [`evaluation/README.md:91-99`](../../evaluation/README.md)).

### 6. Mundane causes ruled out

- **Exposure and names.** Every Falinks-arm session's `init` event lists `mcp__falinks__falinks_recommend`, `mcp__falinks__falinks_propose` and `mcp__falinks__falinks_respond`. Every agent's `tools.json` has 26 tools, 3 of them regrouping tools (Q1, Q10). [`adapters/claude/arm.rs:40-51`](../../adapters/claude/arm.rs) adds the host tools to the full engine list.
- **Relay.** `EngineHost` dispatches all three operations to the engine ([`adapters/claude/engine_host.rs:139-152`](../../adapters/claude/engine_host.rs)). [`adapters/claude/tests/engine_host.rs:267-298`](../../adapters/claude/tests/engine_host.rs) tests them. Each shard's fresh gate (`gate-1.json` in all six evidence directories) passed. Each gate applied an engine-recommended split and an agent-proposed join through real Claude Code workers (Q11; [`adapters/claude/check_integration.rs:607-618`](../../adapters/claude/check_integration.rs), [`adapters/claude/check_integration.rs:691-696`](../../adapters/claude/check_integration.rs)). The gate's steps are host-scripted, so it proves the mechanism works, not that agents would act on their own.
- **Errors.** No regrouping calls were made, so none failed.
- **Provisioning.** Every ledger has two spaces, so a split would not have been blocked as `not provisioned` ([`src/regroup.rs:773-775`](../../src/regroup.rs); Q2). The arm restarts a moved agent in its new root ([`adapters/claude/falinks_arm.rs:691-703`](../../adapters/claude/falinks_arm.rs)).
- **Not used here, but relevant later:** the Codex adapter exposes only `edit`, `review` and `offer` ([`adapters/codex/runtime.rs:141-143`](../../adapters/codex/runtime.rs)). A batch on Codex could not regroup at all.

### Side finding: Keep proposals cost handle calls

Each Keep proposal is an event that both agents must handle. In the scored runs, 78 of the 324 `falinks_handle` calls (24%) handled a `Regrouping` event: 8–10 per run, all accepted (Q8). The pilots added 28 more. Round-trips are lower than calls, because #116 found that handles mostly ride along with other calls. #116 decisions 1 and 4 make this cheaper, but they do not remove the events.

## Recommendation

**Change the policy and fixture 3 together, decided in one grilling ticket. Limit instruction changes to describing the fields and evidence the policy reads. Do not expect a transition from instruction changes alone. Keep #122 and the #123-gated scored batch free of these changes, so the round-trip cuts are measured on their own.**

The alternatives and their consequences:

- **No change.** The next batch will almost certainly report 0 transitions again. The lock-in in section 3 starts at the first related peer live edit, in every fixture, whatever the agents do. Keep was arguably right for go-page and rust-errors, so the cost there is only the Keep events. But the regrouping benefit cannot be measured with these fixtures and this policy. If this option is chosen, a future report should say "not exercisable with the current fixtures and policy" rather than "unestablished".
- **Instructions only.** This is allowed, within the limits below. `recommend` would still return Keep, because the evidence would be unchanged. A transition would need an agent to `propose` a split on its own initiative and the peer to agree. That would measure agent initiative, not #5's design, in which "Falinks identifies useful transitions and proposes them".
- **Fixture only.** This cannot work under the current policy. Any fixture with a coupled phase before an independent phase builds up friction that never resets, so the engine recommends Keep in the independent phase. A fixture with no early coupling would not test a changing relationship.
- **Policy only.** This is not enough for fixture 3. The agents' declared dependencies stay in force for the whole run, and `Render` still consumes `Record`. A looser split rule could also propose splits that cannot apply in go-page (interleaved drafts in `handler.go`). That would add pending proposals and events.
- **Policy plus fixture 3 (recommended).** This is the only combination that could produce an engine-recommended split where fixture 3 intends independent work. It costs a re-freeze of the evaluation sources (see "Freeze and setup budget" in `evaluation/README.md`), setup-budget time, and comparability with #27 on fixture 3.

**Policy questions for the grilling.** These are open; this ticket does not decide them.

1. **Recency.** #14 lists "recent retries, notifications, reconsiderations and waits" as signals and says "Do not require extensive proof of independence". The implementation counts friction since the last applied transition ([`src/regroup.rs:25-26`](../../src/regroup.rs)), and with no transition that means the whole run. Inference: this is a stricter reading of #14 than its text requires. Choosing a recency anchor, such as the last scope change, the last publication or the last few live edits, would remove the lock-in.
2. **Review outcomes.** A reconsideration whose review decided Revise is evidence of collaboration. One that decided Keep may be the overhead a split would remove. In the scored runs, 38 of 43 reviews decided Keep. In fixture 3, the only Revise in each run fell at the contract change. The policy could use that distinction. Today it counts both kinds the same way.
3. **Task labels.** Decide whether two scopes with the same agent-chosen `task` string count as a shared task, or define what the field should hold. Seven of 12 runs had the fixture name on both scopes.
4. **Waits.** Decide whether the `waits` signal should stay. In this arm no code path can write it.

These questions concern the #25 implementation contract ([`docs/engine.md:144`](../engine.md)) and its tests (`tests/regroup.rs`). They do not reopen #14's agreement or transition safety rules.

**Fixture 3 questions for the grilling.** Should the follow-up phase do several steps of work that is independent in code, for example with no shared types? Should the dependency information describe each phase's relationship as task information, delivered identically to both arms? Both arms get the same bytes, so #5's fair-information rule holds. Whether describing code relationships per phase comes too close to steering is a judgment for that grilling.

**Instruction changes that stay within #5.** #5 says: "Do not instruct agents when to join or split." #116 decision 6 allows agent instructions and tool descriptions "limited to describing the Falinks protocol, with no fixture or task hints". Within both rules, the tool descriptions could say:

- what `task` and `depends` mean in `register`, and that an agent may register again with the same id when its intended work changes;
- that `Regrouping` events report the engine's join, split or keep recommendation, and that its evidence is the registered scopes, declared dependencies, task labels, compiler relationships, retries and review obligations;
- what each arrangement means: joined agents share one live workspace and review each other's related live edits; split agents have separate live workspaces, receive the peer's live-edit messages without a review obligation, and receive published work through incorporation ([`docs/engine.md:131`](../engine.md), [`docs/engine.md:161-163`](../engine.md)).

These texts describe what the protocol does. They name no fixture, phase, timing or condition for choosing an arrangement, so they do not tell agents when to join or split. Describing consequences may still make agent-initiated proposals more likely. #5 asks for that to be recorded as part of the treatment ("Record instruction/tool differences as part of the treatment"). Alone, these changes would not produce a transition, because `recommend` would still see a declaration and accumulated friction.

## Limits

- **Agent reasoning is not recorded.** 148 of the 149 thinking blocks in the Falinks-arm Claude Code session files are empty. The one non-empty block is a note about tests. Only visible text and tool calls can be searched. The evidence shows that agents never asked or discussed regrouping, not what they considered silently.
- **Wait durations are unknowable from the records.** The `waits` table has no timestamps, and the evaluation never wrote to it. Agent idle time appears only in host timing (#116's in-turn share).
- **Compiler relationships between scopes are unknown.** `declared` skips the compiler check for the pair, so `compiler: []` in the proposals is not evidence that no edge existed.
- **Proposals have no timestamps.** They are ordered by engine event sequence. Phase boundaries come from the milestone trigger rule and each agent's edit and ack order, not from a shared clock.
- **Counterfactuals are inferences from the code, not executed.** These include what `recommend` would have returned, whether a split would have been blocked in go-page, and what a split at `follow-ups` would have saved. No batch was run for this ticket.
- **One runtime, one model, few runs.** All runs used Claude Code 2.1.287 with `claude-opus-5-5`. Only 3 scored runs and 3 pilots used fixture 3. The first pilot ended in an infrastructure failure at 67 s and is included only for completeness.
- **Evidence paths are local.** The raw evidence lives on Daniel's machine at `~/ev/2026-10-14`. The numbers above are complete without it.

## Sources

**Repository** (worktree at commit `34d681e`; line numbers verified there):

- `src/regroup.rs`: signals, `decide`, reconsideration, suppression, mark and split blockers (lines cited above).
- `src/coordination.rs:20-33` (`Scope`, `relevant`), `src/coordination.rs:953-967` (`wait` writes `waits`).
- `adapters/claude/engine_host.rs:139-152`, `:184-185`, `:207-210`, `:250-261`.
- `adapters/claude/arm.rs:40-51`, `:365-385`.
- `adapters/claude/falinks_arm.rs:285-294`, `:433-439`, `:602-605`, `:691-703`.
- `adapters/claude/check_integration.rs:607-618`, `:691-696`; `adapters/claude/tests/engine_host.rs:267-298`.
- `adapters/codex/runtime.rs:141-143`.
- `evaluation/instructions/falinks.md:3`; `evaluation/fixtures/go-relationships.json:14-36`; `evaluation/fixtures/rust-errors.json:22`; `evaluation/README.md:91-99`.
- `docs/engine.md:129`, `:131`, `:144`, `:161-163`, `:167`, `:171`; `docs/evaluation/first-agent-evaluation.md:27`.
- `tests/regroup.rs:63-68`, `:106-111`: the split test registers scopes with distinct tasks, no `depends` and separate files.

**Issue resolutions** (no later comment supersedes them; checked 14 October 2026):

- [#14 split/join recommendation policy](https://github.com/danielrispler/Falinks/issues/14#issuecomment-5999050658)
- [#5 fair baseline and fixture design](https://github.com/danielrispler/Falinks/issues/5#issuecomment-6045386497)
- [#24 frozen fixtures and baseline](https://github.com/danielrispler/Falinks/issues/24#issuecomment-6095496772)
- [#27 resolution](https://github.com/danielrispler/Falinks/issues/27#issuecomment-6168318968) and [#27 evidence record](https://github.com/danielrispler/Falinks/issues/27#issuecomment-6168534279)
- [#116 resolution](https://github.com/danielrispler/Falinks/issues/116#issuecomment-6169349150)

**Evidence** (`~/ev/2026-10-14`, read-only). Runs are `pilot/pilot-falinks`, `pilot2/pilot-falinks`, `pilot3/pilot-falinks`, `s-rust/rust-errors-N-falinks`, `s-page/go-page-N-falinks` and `s-rel/go-relationships-N-falinks`. `$L` is `sqlite3 "file:$RUN/controller/state/ledger.sqlite?immutable=1"`. The `analysis-116/ledgers` copies are byte-identical to these ledgers (SHA-256 checked).

- **Q1** tool calls, regrouping calls and handle calls: `jq -c '[.[][].events[] | select(.type=="assistant") | .message.content[]? | select(.type=="tool_use") | .name] | [length, (map(select(test("falinks_(recommend|propose|respond)$"))) | length), (map(select(endswith("falinks_handle"))) | length)]' $RUN/controller/transcripts.json`. Exposure: `jq '.[][].events[] | select(.type=="system" and .subtype=="init") | .tools'`.
- **Q2** ledger state: `$L "SELECT (SELECT COUNT(*) FROM waits), (SELECT COUNT(*) FROM proposals), (SELECT COUNT(*) FROM shifts), (SELECT COUNT(*) FROM spaces), (SELECT value FROM meta WHERE key='regroup_enabled'), (SELECT value FROM meta WHERE key='regroup_mark');"`.
- **Q3** proposals: `$L "SELECT seq, json_extract(body,'$.proposer'), json_extract(body,'$.action'), json_extract(body,'$.state'), json_extract(body,'$.reasoning') FROM proposals ORDER BY seq;"`.
- **Q4** scopes and contexts: `$L "SELECT agent, json_extract(body,'$.scope.task'), json_extract(body,'$.scope.nodes'), json_extract(body,'$.scope.depends') FROM scopes;"` and `$L "SELECT COUNT(DISTINCT json_extract(body,'$.context')) FROM proposals;"`. Register calls per agent: Q1's `jq` filtered on `endswith("falinks_register")`.
- **Q5** friction, using the engine's SQL with mark 0: `retries = SELECT COUNT(*) FROM requests WHERE json_extract(body,'$.outcome.Stale') IS NOT NULL OR json_extract(body,'$.outcome.Unreviewed') IS NOT NULL`; `reconsiderations = SELECT COUNT(*) FROM events o JOIN events c ON c.seq = json_extract(o.body,'$.Obligation.cause') WHERE json_extract(c.body,'$.Message') IS NOT NULL`; `waits = SELECT COUNT(*) FROM waits`.
- **Q6** live edits, the obligations they caused and the peer's review: for each `events` row with `$.Message.kind = 'LiveEdit'`, join `Obligation` rows whose `$.Obligation.cause` is its seq, and `Reviewed` rows by the other agent whose `$.Reviewed.review.revision` is its revision. Review totals: `$L "SELECT json_extract(body,'$.Reviewed.review.decision'), COUNT(*) FROM events WHERE json_extract(body,'$.Reviewed') IS NOT NULL GROUP BY 1;"`.
- **Q7** event order: `$L "SELECT seq, json_extract(body,'$.Regrouping.id'), json_extract(body,'$.Obligation.cause') FROM events WHERE json_extract(body,'$.Regrouping') IS NOT NULL OR json_extract(body,'$.Obligation') IS NOT NULL ORDER BY seq;"`. Event kinds: `$L "SELECT (SELECT key FROM json_each(body) LIMIT 1), COUNT(*) FROM events GROUP BY 1;"`.
- **Q8** handle calls on `Regrouping` seqs: Q1's `jq` filtered on `falinks_handle` with `.input.request.seq` in the Q7 `Regrouping` seqs. Each `tool_use_id` was matched to its `tool_result` for acceptance. A deferral check filtered on `.input.request.deferred != null` and found 0.
- **Q9** agent text: `jq -r 'select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text | select(test("(?i)regroup|recommend|falinks_propose|falinks_respond|split (the|our|into)|join (the|our)|arrangement|separate workspace"))'` over `*/*-falinks/controller/claude-projects/*/*.jsonl` found 0 matches. A wider regex over `transcripts.json` text and thinking blocks found only unrelated uses. Message texts: `$L "SELECT seq, json_extract(body,'$.Message.text') FROM events WHERE json_extract(body,'$.Message.kind') IN ('Plan','CheckpointOffer')"` with `LIKE` filters for regroup, split, join and arrangement.
- **Q10** tool descriptions: `jq -r '.[] | select(.name|test("recommend|propose|respond")) | .description' $RUN/agent-{0,1}/controller/tools.json`.
- **Q11** gates: `jq '.passed, .evidence.phases.split.proposal.state, .evidence.phases.join.proposal.state' <dir>/gate-1.json` for all six directories.
- **Q12** milestone times and outcomes: `jq 'select(.elapsed and .kind=="development")' $RUN/controller/events.jsonl`; `jq '.outcome, .timing.elapsed_seconds, .recommendations, .transitions' $RUN/controller/result.json`.
- **Q13** edit and ack order per agent: the ordered `falinks_edit` and `falinks_ack` tool calls in `transcripts.json`, with acceptance and revision from their tool results.
- **Q14** files each agent wrote: `$L "SELECT DISTINCT r.agent, k.key FROM requests r, json_each(json_extract(r.body,'$.request.output')) k WHERE json_extract(r.body,'$.outcome.Applied') IS NOT NULL;"`.
