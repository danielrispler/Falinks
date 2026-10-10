# Frozen evaluation setup (issue #24)

This is an engine-independent evaluation harness, not an engine or performance result.
It is a standalone Rust crate (independent of the engine crate) and needs Git,
macOS `sandbox-exec`, Rust/Cargo and Go on PATH. The fixtures have no third-party
dependencies. Verification used Rust 1.99.0 and Go 1.27.2 on macOS arm64.
Unsupported/missing sandbox execution fails closed.

Binaries: `falinks-eval` (the host CLI), `codex-worker` (the baseline Codex
bridge) and `scripted-worker` (test-only scripted workers and fake runtime).

## Run the checks

```sh
cd evaluation
cargo test --offline
cargo run --offline -- verify
cargo run --offline -- schedule
cargo run --offline -- prepare go-page --output /tmp/fresh-go-page
cargo run --offline -- oracle go-page --repo /tmp/fresh-go-page --evidence /tmp/fresh-check-evidence
```

`verify` reconstructs every initial Git commit using frozen author/committer
metadata, checks its identity, confirms the initial repository compiles and its
oracle fails, then checks a known-correct reference against both trusted visible
commands and the independent oracle. The public CLI tests also reject compiling
regressions in error propagation, kind selection and record IDs. Initial failure
may be a missing required API compilation error; reference success exercises the
full behavioral oracle. Checks capture raw Git tree blobs, so export-ignore/export-subst cannot hide or rewrite files. Checks require exact **committed** source, never live
workspace files. Visible tests cannot change the oracle's captured source.

`fixtures/*.json` contain public initial source, allocation, dependency
information, commands, requirements and host milestone definitions.
`protected/*.json` contain host-only oracle and reference bytes. Do **not** give
this checkout or a clone of it to task agents. `prepare` exports only the initial
fixture repository. A future Falinks runner must use the same export, freeze,
instructions, developments and `oracle` seam, with its engine/adapter controls and repeated `--deny-root` arguments for all prior runs or reference copies.

## Baseline

Copy `config.json` to a host-only run configuration. Fill in the actual shared
model, resolved model version, reasoning setting, runtime version, absolute
runtime binary and its SHA-256, worker script path, and auth file. Use an absolute
path to the `codex-worker` binary (`cargo build --release` puts it in
`evaluation/target/release/`); credentials are copied into each isolated
fresh CODEX_HOME rather than reusing previous session storage. Set `denied_roots`
to all previous run directories and any additional solution/reference/transcript
copies. Never put a run beneath a denied root or beneath this checkout.

```sh
cargo run --release --offline -- baseline go-relationships \
  --config /tmp/batch-config.json --output /tmp/new-unique-run \
  --pair go-relationships-1 --order 0
```

The run directory must not exist. The clock starts before workspace preparation.
Two worker **processes run concurrently**, each with a fresh context and its own
Git worktree. Peers receive messages and full unfinished diffs. Neither a shared
draft nor an individual contribution needs to compile. The JSON-line protocol
is documented in `instructions/git.md`; an alternative runtime can supply a
worker program and argv with `{worker}`. The host copies the worker into each
scratch directory before sandbox launch. Runtime identity is checked against
the configured binary hash; model/version/reasoning are supplied identically to
both workers and recorded, rather than guessed from a marketing alias.

`codex-worker` uses `codex exec --json`, a structured action response and
explicit session-ID resume within a run. No `--last` or cross-run resume is used.
It queues peer context at turn boundaries and waits for share capture replies
before allowing more edits. Raw runtime events remain in scratch; usage is
reported as retained per-turn records when exposed; telemetry acknowledgments never wake a waiting model. These CLI primitives are described in [official Codex
noninteractive documentation](https://learn.chatgpt.com/docs/non-interactive-mode)
and the [structured-loop example](https://developers.openai.com/cookbook/examples/codex/build_iterative_repair_loops_with_codex).
This bridge has not been authenticated against a model in this ticket; it is
not a replacement for the pinned app-server adapter's capability tests.

Both agents can request checks. The host ordinarily merges A and B in the
integration worktree. If Git reports a conflict, agents may resolve it there,
commit the ordinary merge and request `check` with `resolved` naming that exact
commit. Both shared tips must be ancestors. A successful resolved candidate is
retained for final checks unless a new draft is shared. Final readiness names
only each agent's last shared tip, without a Falinks ledger, review gate or
checkpoint-offer protocol. The host retains the selected candidate in private
Git storage and checks the raw tree blobs of that commit. Later worktree writes
cannot change checked bytes. Failure/conflict evidence and drafts remain intact.

## Developments and fairness

The runner sends allocation and initial behavioral/dependency information to
both workers. Future development contents are withheld until their trigger.
The first trigger is each worker's first nonempty draft, regardless of compile
status. The second producer/consumer trigger is a new nonempty draft from each
worker after that worker acknowledges the contract development. These triggers
are based on observed task activity, independent of recommendations, and do not
say when to join or split. They expose unfinished coupling then separate
producer Stats and consumer Render work. Deliver the same events using the same
observable triggers in the Falinks arm; a checkpoint must not require compilation.

Freeze an actual batch configuration **before** pilots/scoring. The checked-in
configuration is an intentionally incomplete template; placeholder identities
cannot launch a run. The historical decision pins Codex 0.160.0 / SHA-256
`112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b`.
The initial setup installation was 0.161.0, so **no scored runs were attempted**. Reverify
the pinned adapter or explicitly record/version renewed controls for another
runtime; do not silently score with a new one. Deterministic engine and adapter
safety gates from #19 remain prerequisites. This baseline CLI records unscored
runs only. Scored-batch orchestration belongs with the downstream engine gates.

Record all arm instruction/tool differences. The baseline has separate writable
worktrees and ordinary Git integration; Falinks has live grouped source and
mediated edits/revisions/offers. The baseline exec bridge delivers context at
turn boundaries; the pinned Falinks app-server adapter may steer active turns.
That transport difference must be resolved or reported before interpreting
paired timings. Both arms still receive the same task information, agent
allocation, model/version/reasoning, peer feedback opportunities and trusted
check set. Do not impose the engine ledger on Git or claim an engine is present.

## Protection, reset and evidence

The host process, binaries and material checkout are trusted. Worker and compiler/
test descendants run in macOS sandboxes. Worker writes are restricted to their
own worktree, scratch, shared fixture Git metadata and integration worktree.
Workers and host Git subprocesses cannot read/write controller state, this checkout, other worktrees of
this setup repository or its shared Git object storage (which contains oracle
bytes), or configured denied roots. Host Git filters and subprocesses inherit the sandbox rather than privileged host access. Checks inherit those same exclusions, have networking disabled and can
write only isolated build/cache/temp outputs; captured source and oracle files are read-only. Protected check logs are not sent to
workers; workers get pass/fail and ordinary merge diagnostics. Runtime credentials
are not included in retained evidence. The scripted test observes a denied
controller read for both workers. Finite filesystem controls are not a claim
of containment against arbitrary privileged processes or all side channels.

Each run uses a unique output directory and fresh contexts, initial objects,
worktrees, scratch, check copies and controller state. Do not supply prior
solutions or transcripts. Deny previous runs in both arms. Retain run directories
for diagnosis; remove them only with an explicit experiment reset after recording
evidence. Source symlinks/submodules/nonregular tree entries and new build
hooks/dependency configuration are rejected. Independent oracles ignore agent
integration tests and always use the frozen build inputs; focused tests are
additional feedback. Added helpers within the fixture package are allowed.

`controller/result.json` follows `result-schema.json`; `events.jsonl` records
observed starts, messages, drafts, developments, checks and failures with elapsed
timestamps. Retained Git objects identify exact candidates; `check-N.json`
contains private visible/oracle command outputs. Preserve worker stderr and raw
runtime events. Token usage is explicitly unavailable if not reported; it is
not a subscription dollar-cost estimate. Rewrite/discard quantities are also
unavailable unless supplied by a runtime; do not invent them. Baseline engine
recommendation/transition counts are zero by construction. Concurrent timeline
intervals overlap and must not be summed into elapsed time.

Success stops the completion clock after exact final checks pass; cleanup time
is separate. Failures/timeouts/usage interruptions retain evidence and their
reason; no automatic provider switch or silent replacement. The 30-minute
budget includes preparation, communication, checks and integration. Every
candidate check also has a bounded deadline. `schedule` emits the unscored
producer/consumer pilot and nine pairs / 18 scored slots: three repetitions per
fixture, alternating arm order and reversing the starting order by fixture.
Infrastructure replacements need separate IDs linked to the failed run. No
performance analysis or scored-batch execution is implemented here.

## Freeze and setup budget

`manifest.json` pins hashes and reproducible initial Git commits. The harness
rejects changed materials. Repairs must be versioned before a new batch; then
run `freeze`, retain the previous manifest and record why it changed. Never tune
against hidden oracle cases after scoring starts. Public and protected material
hashes, instructions, worker/harness bytes and run configuration are evidence;
the setup ledger is `setup.json`. The shared setup ceiling is eight hours and
covers this preparation plus remaining pilot/evaluation setup, **not engine
implementation**. Add timed setup sessions to that ledger; it does not enforce
or estimate engine work. See #19 and `docs/evaluation/first-agent-evaluation.md`
for safety gates, run interpretation and continuation criteria.

## Verification record

`verification.json` records the six passing integration checks, fixture identities,
compiler/runtime versions and the explicit limits of the evidence. `review.md`
records the independent Standards/Spec review and resolved findings. The setup
ledger conservatively charges 3,789 seconds (about 63 minutes), leaving 25,011
seconds of the shared eight-hour setup allowance for subsequent setup.
