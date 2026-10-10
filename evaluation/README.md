# Frozen evaluation setup (issue #24)

This is an engine-independent evaluation harness, not an engine or performance result.
It is a standalone Rust crate (independent of the engine crate) and needs Git,
macOS `sandbox-exec`, Rust/Cargo and Go on PATH. The fixtures have no third-party
dependencies. Verification used Rust 1.99.0 and Go 1.27.2 on macOS arm64.
Unsupported/missing sandbox execution fails closed.

Binaries: `falinks-eval` (the host CLI), `claude-worker` (the Git-arm Claude Code
bridge, #27), `codex-worker` (the historical baseline Codex bridge) and
`scripted-worker` (test-only scripted workers, fake runtimes and a fake Falinks runner).
The Falinks arm runner is `falinks-arm` in `adapters/claude` (it needs the engine).

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
runtime binary and its SHA-256, worker script path, and runtime login fields. Use an
absolute path to the worker binary (`cargo build --release` puts it in
`evaluation/target/release/`). For the historical Codex bridge, `auth_file` is copied
into each isolated fresh CODEX_HOME. For Claude Code, see
[Paired evaluation](#paired-evaluation-27). Set `denied_roots`
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
safety gates from #19 remain prerequisites. `baseline` records unscored runs unless
`batch` passes `--scored true`; see [Paired evaluation](#paired-evaluation-27).

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
Infrastructure replacements need separate IDs linked to the failed run; `batch`
gives them `-rN` IDs and a `replaces` field.

## Paired evaluation (#27)

Claude Code `2.1.287` with `claude-opus-5-5` is the evaluated runtime in both arms
([#32](https://github.com/danielrispler/Falinks/issues/32#issuecomment-6096677771)).
Both arms launch one long-lived `claude -p` stream-json session per agent with the same
isolation flags (`--setting-sources ""`, `--strict-mcp-config`, `--disable-slash-commands`,
`dontAsk`), default reasoning and `--max-turns 12`. When a message hits that tool-call cap,
the host sends a continuation. Host events are written at once and presented at the next
tool-result boundary or as a new turn. An idle, unfinished agent with no new events gets a
reminder after two minutes. Any `system/init` mismatch, model refusal fallback or other
model in `modelUsage` invalidates the run. A `rate_limit_event` with status `rejected` ends
it as `usage_limit`.

- **Git arm** (`baseline` with `worker_script` = `claude-worker`): the worker runs in
  the host sandbox-exec profile with Claude Code's sandbox off and tools
  `Bash,Read,Glob,Grep,Edit,Write`. The agent reaches the host by running
  `"$FALINKS_HOST" message|share|ack|check|ready` through Bash. A Unix socket relays those
  commands as the JSON-line requests above, and `ready` fills in the last shared commit.
  `runtime_home` and `runtime_writable` give the runtime its login and session state.
- **Falinks arm** (`falinks-arm FIXTURE --config FILE --output DIR`): `prepare` exports the
  fixture into the live root. Its dot-paths (`.gitignore`) are removed, because the engine
  reserves them; the host adds their frozen bytes back to every candidate. The engine
  enrolls the fixture files plus `x_test.go` / `tests/x.rs`. Engine checks are the Git
  arm's visible commands with the pinned toolchain. Workers use the #26 launch profile and
  every `engine_host` tool, plus host `ack`, `check` (visible checks and oracle on an exact
  capture, pass/fail only) and `ready`. `ready` succeeds once every development is
  acknowledged and the agent's workspace equals the published state. When both agents are
  ready, the oracle runs on the published bytes. An applied split or join resumes the
  moved worker in its new root. An engine incident is a `safety_failure`.
- **Batch** (`batch --config FILE --runs DIR [--after-pilot PILOT_DIR]`): without
  `--after-pilot` it runs the unscored pilot pair. Scoring is refused until that pilot
  pair is complete.
  - **Before any run**, the batch checks the frozen identities with `verify`. It then
    runs `gate_command` once per invocation. That command must be a fresh
    `check-integration` on the same `runtime_binary`.
  - **Denied roots**: slots run in schedule order. Every other entry in the batch
    directory is a denied root, as are sibling batch directories such as the pilot and
    `<runtime_home>/.claude/projects`. Git-arm workers re-open only their own session
    folder.
  - **After each run**, the batch moves the run's `~/.claude/projects` session folders
    into its private `controller/claude-projects`.
  - **Pauses** (exit 3) happen on `usage_limit`, `safety_failure`,
    `infrastructure_failure`, or a usage window at 90%.
  - **Resuming**: rerunning resumes the batch. An infrastructure failure or a
    usage-limited run gets an identified `-rN` replacement, and both records stay in
    the evidence. After a `safety_failure`, the directory refuses to resume: repair,
    renew the gates and start a new batch directory.
  - **Configuration**: one batch directory keeps one configuration, so limits change
    only between batches.
  - **Report**: `report --runs DIR` lists every run's outcome, paired successes, medians
    over jointly successful pairs and the continuation signal.
  - **Agent mistakes** in either arm are refusals counted in `rejected_operations`. They
    are never infrastructure failures.

Run the pilot and batch in a session **outside auto mode**, because they launch nested
agents:

```sh
cargo build --release --bins                       # repository root: falinks-arm, check-integration
(cd evaluation && cargo build --release --offline) # claude-worker, falinks-eval
cp evaluation/config.json /private/tmp/eval-config.json  # fill every REQUIRED field
evaluation/target/release/falinks-eval batch --config /private/tmp/eval-config.json --runs /private/tmp/ev/pilot
evaluation/target/release/falinks-eval batch --config /private/tmp/eval-config.json --runs /private/tmp/ev/scored --after-pilot /private/tmp/ev/pilot
```

Keep run roots short: the adapter's host socket lives in each run directory, and macOS
limits socket paths to 104 bytes. The first real Git-arm runs will show whether login and
session state need more `runtime_writable` paths under the host sandbox. Record any such
repair as a versioned pilot repair. A repair that changes frozen material needs
`freeze`, which keeps the superseded manifest under `manifests/`, and a new
configuration, so the scored batch uses a new directory.

## Freeze and setup budget

`manifest.json` pins hashes and reproducible initial Git commits. The harness
rejects changed materials. Repairs must be versioned before a new batch; then
run `freeze`, which bumps `version` and retains the previous manifest under
`manifests/`, and record why it changed in `setup.json`. Never tune
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
ledger charges 6,491 seconds (about 108 minutes) through the #27 harness session,
leaving 22,309 seconds of the shared eight-hour setup allowance for the pilot and any
pilot repairs.
