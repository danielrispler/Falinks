# Frozen evaluation setup (issue #24)

This is an engine-independent evaluation harness, not an engine or performance result.
It is a standalone Rust crate (independent of the engine crate) and needs Git,
Rust/Cargo and Go on PATH, and a sandbox backend: macOS `sandbox-exec`, or on Linux
Landlock ABI 3 with the `falinks-eval-contain` helper beside `falinks-eval` (#56; the
kernel requirements are those of the engine in `docs/platforms.md`). The fixtures have
no third-party dependencies. Verification used Rust 1.99.0 and Go 1.27.2 on macOS arm64;
the Linux backend rests on the `ubuntu-latest` shards of `evaluation.yml`.
Unsupported/missing sandbox execution fails closed.

Each batch records its platform (`os-arch`) on every invocation in `batch.json`.
Resuming a batch, scoring after a pilot or merging shards recorded on another
platform is refused: one platform per batch.

Binaries: `falinks-eval` (the host CLI), `claude-worker` (the Git-arm Claude Code
bridge, #27), `codex-worker` (the historical baseline Codex bridge) and
`scripted-worker` (test-only scripted workers, fake runtimes and a fake Falinks runner).
The Falinks arm runner is `falinks-arm` in `adapters/claude` (it needs the engine).
It launches its workers through the adapter's own settings, so since #295 their Bash
commands run under `falinks-claude-shell`'s per-command confinement instead of Claude
Code's sandbox. That is a run-configuration change: Falinks-arm runs before #295 ran
under Claude Code's sandbox, and a batch spanning both is not one configuration.

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
producer Stats/Duplicates and consumer Parse/IDs work in separate files. Deliver
the same events using the same observable triggers in the Falinks arm; a
checkpoint must not require compilation.

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
test descendants run in the host sandbox (Seatbelt on macOS; Landlock, plus seccomp
socket denial when networking is off, on Linux). Worker writes are restricted to their
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
not a subscription dollar-cost estimate. Tokens and cost come from the archived
session JSONL instead (#202, below). Rewrite/discard quantities are also
unavailable unless supplied by a runtime; do not invent them. Baseline engine
recommendation/transition counts are zero by construction. Concurrent timeline
intervals overlap and must not be summed into elapsed time.

Success stops the completion clock after exact final checks pass; cleanup time
is separate. Then both arms drain usage (#179): the host refuses every request
while it waits up to `usage_drain_seconds` (60) for each worker's final turn
result, its first after an accepted `ready`, and then stops the workers. Usage
is `complete` only when every final result arrived within that window;
otherwise it is `partial` (a result flushed while the worker stopped is kept
with `late: true`, a worker stopped before its result has none) or
`unavailable` when no turn reported. `usage.final_results` says which. Partial
usage is never used for cost comparisons: the report counts tokens from the
archived Claude Code session JSONL (`controller/claude-projects`), one usage
per `message.id`, so an agent's last turn counts even without its `result`.
Cost is those tokens at the `prices` table frozen in `config.json`, an
API-equivalent list price, not billed cost. Each run cross-checks against the
drained `result` usage and Claude Code's `cost-state` total, and reports
mismatches; a run without session JSONL has `missing` usage. The Codex bridge reports its last turn
before `ready`, so its usage is never complete. Failures/timeouts/usage interruptions retain evidence and their
reason; no automatic provider switch or silent replacement. The 30-minute
budget includes preparation, communication, checks and integration. Every
candidate check also has a bounded deadline. `schedule` emits the unscored
producer/consumer pilot and nine pairs / 18 scored slots: three repetitions per
fixture, alternating arm order and reversing the starting order by fixture.
Infrastructure replacements need separate IDs linked to the failed run; `batch`
gives them `-rN` IDs and a `replaces` field.

## Paired evaluation (#27)

Claude Code `2.1.308` with `claude-opus-5-5` is the evaluated runtime in both arms
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
  the host sandbox profile with Claude Code's sandbox off and tools
  `Bash,Read,Glob,Grep,Edit,Write`. The agent reaches the host by running
  `"$FALINKS_HOST" message|share|ack|check|ready` through Bash. A Unix socket relays those
  commands as the JSON-line requests above, and `ready` fills in the last shared commit.
  `runtime_home` and `runtime_writable` give the runtime its login and session state.
- **Falinks arm** (`falinks-arm FIXTURE --config FILE --output DIR`): `prepare` exports the
  fixture into the live root. Its dot-paths (`.gitignore`) are removed, because the engine
  reserves them; the host adds their frozen bytes back to every candidate. The engine
  enrolls the fixture files plus `x_test.go` / `tests/x.rs`. Engine checks are the Git
  arm's visible commands with the pinned toolchain. Like the Git arm's checks, they cannot
  read the contents of the run directory (except their slot and output) or of any denied
  root; the run records these in `check_denied_roots`. The host `check` and the final
  oracle deny agent tests the same roots, apart from the oracle's own directory. A
  `denied_roots` entry that does not resolve fails the run. Like the Git arm, this arm also
  denies the checkout holding `evaluation_root`, its Git common directory and every worktree
  of it, without listing them. Workers use the #26 launch profile and
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
  - **Shards**: `--fixtures NAME[,NAME]` runs only those fixtures' slots, keeping each
    slot's pair ID and arm order. Run one shard per fixture in parallel sessions, in
    sibling directories under one parent, so each shard denies the others. While a run is
    active, its directory holds `active.json`. Each run records in `concurrency` the sibling
    runs active at its start and stop: any sibling directory's lock counts, the pilot's too. A
    shard killed mid-run leaves a stale lock: delete it, or the others over-report concurrency.
  - **Batch checkout** (#228 decision 29a): every invocation of the scored batch,
    including resumes and shards, runs from a checkout of #188's freeze commit.
  - **Inspection rule** (#228 decisions 30–31): lifted. #228's stage 2 is frozen (#233),
    so a scored run's final code may be inspected; the lifted rules stay on record in
    `code-quality.json`, `rules`.
  - **Report** (#204): `report --runs DIR [--runs DIR]...` puts every dimension of both
    arms side by side, per fixture. **It computes no winner**: the decision is written on
    the ticket once the evidence is in ([#5 amendment](https://github.com/danielrispler/Falinks/issues/5#issuecomment-6322468250)).
    - **Runs and pairs.** Every run's outcome is listed. Each fixture also gives its
      successes per arm and its complete pairs. Its `median_seconds` and
      `median_reduction` cover the jointly successful pairs: the time difference, as a
      number only.
    - **`metrics`**, for both arms:
      - completion time (`completion_seconds`), which a failed run lacks;
      - `model_seconds` and `round_trips` from `timing`;
      - `cost_usd`, `cold_start_cost_usd` (see **Cost**) and `tokens_<class>` per run;
      - `success`;
      - `refused_operations` (`rejected_operations`);
      - `failed_checks_before_pass`, the checks in `validation_attempts` before the first
        pass, or all of them when none passed;
      - `safety_incidents`.

      `retries` is not compared, because the Git arm fixes it at 0.
    - **`code_quality`** (#228, #230), from `quality --runs DIR [--runs DIR]...`, run over
      the finished batch directories before `report`. It writes `quality.json` into each
      batch directory, never into a run directory, and records its own commit and the
      batch's (`batch.json` records each invocation's `commit` and `go`).
      - Only runs whose final oracle passed are measured; every other repetition is
        missing, with its outcome in `missing_reasons`.
      - Each final snapshot is exported from its arm's own run storage: the Git arm's
        `agents/repo`, or the Falinks arm's `controller/oracle/candidate-N` holding it.
      - The pinned `gofmt` or `rustfmt` runs on copies of the initial tree, the final
        snapshot and the reference; every metric uses those copies. Formatters, `go vet`
        and `cargo clippy` run in the oracle's sandbox, environment and denied roots.
      - Per arm: `production_lines_changed`, `test_lines_added`, `test_functions`, and
        `vet_findings` (Go) or `clippy_findings` (Rust), each with the usual statistics, its
        `labels` entry, and the per-lint breakdown in `lints`. Lint findings count only on
        lines the formatted diff added or changed, de-duplicated by lint, file and line.
        Test code is `_test.go`, `tests/*.rs` and Rust `#[cfg(test)]` items.
      - `reference_production_lines_changed` is the formatted reference's figure, with no
        ratio. `treatment_properties` states the Falinks arm's fixed file set and the
        formatter-access gap (#228 decision 5).
      - A tool failure makes that metric missing with the tool's error. Each run's
        `versions` gives `go version`, `rustc -V`, `clippy-driver -V` and `rustfmt -V`, and
        `go_note` says when the tools' Go is not the Go the batch's checks resolved.
      - `failing_properties` (#231): the number of the fixture's differential properties
        whose output differs from the formatted reference's, with the failing IDs in
        `properties`. The suites (`protected/properties/<fixture>.rs` or `_test.go`) print
        one line per seeded input, with the frozen seed and input count; they use only the
        API the oracle imports, and run without the snapshot's own tests through the
        oracle's contained path. A suite that does not compile gives missing, with the
        compiler's error.
      - Before any property value, `quality` checks two gates per fixture (#228 decision
        13): the reference gives every input of every property without a panic, and each
        property fails on its broken copy (`protected/properties/broken.json`, one edit of
        the reference per property). When a gate fails, every run of that fixture lacks
        `failing_properties`, and the reason names the property and the gate.
        `quality.json`'s `property_gates` gives each fixture's result. Tests point
        `FALINKS_PROPERTY_SUITES` at other suites; `measurement.property_suites` then
        records it.
      - **Judge** (#232), with `quality ... --judge CONFIG`, where CONFIG is a run
        configuration whose `runtime_version` and `model` must equal `code-quality.json`'s
        judge pin and whose `runtime_binary` must match `runtime_sha256`. Each call is a
        fresh `claude -p --output-format json` session from an empty directory, with
        `--tools ""` and the workers' isolation flags. The material is the fixture's
        requirement text, the formatted initial production code, the formatted change as a
        whole-file diff per changed source file, and all test code after the change as one
        block (Rust inline test modules moved out); file names and the reference are not
        shown. The frozen prompt and anchors are filled verbatim.
        - Per measured snapshot, `k` rating calls; each failed or unparseable call is retried
          once. `quality.json` gives each dimension's `value` (median), `spread`, ratings and
          reasons, or `missing` with the reason when any rating is still missing. Every call's
          `usage` is in `calls`.
        - One forced-choice blinding call per snapshot, not retried. `report` prints
          `judge.blinding.arm_guessed` (`arm guessed k/n`, n counting parsed guesses), the
          missing guesses and the frozen note.
        - A dimension without a passing calibration record
          (`judge.calibration.<dimension>.passed` in `code-quality.json`) prints
          `Not yet calibrated` instead of its values. The calibration ticket adds the record.
        - Tests use the scripted worker's fake judge only; real calls need a session outside
          auto mode.
    - **Values and labels.** Each metric gives every repetition's value per arm, its
      `median`, `range`, `n` and `missing`. A repetition lacks a value when its run is
      absent, interrupted or lacks the metric; missing data is never imputed. The `label`
      beside it is `consistent` (every repetition of one arm below every repetition of the
      other, no data missing), `mixed` (the ranges overlap) or `incomplete` (a repetition
      lacks the metric). `label_note` states that chance alone gives `consistent` 2/20 of
      the time per cell at n = 3, and that it is not proof of a meaningful or
      generalizable advantage.
    - **Cost.** Each fixture's `cost` gives, per arm, tokens by class over every scored
      run (failed and replaced runs included) and runs `missing` usage. Its cost lines say
      "API-equivalent list price, not billed cost" (`label`). They follow the
      [#5 amendment of 2026-10-22](https://github.com/danielrispler/Falinks/issues/5#issuecomment-6341068066):
      - **Measured**: the cost per successful run, which is null when usage is missing.
      - **Cold-start**, beside it, for both arms: the measured cost with the cache read of
        each agent session's first API call repriced at the 1-hour cache-write price.
        - A process resumed later in the same session, after a regrouping transition, is
          not repriced: its read is the session's own context.
        - Why: Git's first call reads a prefix that other Claude Code users keep warm, while
          Falinks's prefix is warm only after our own runs within the hour. Measured cost
          would otherwise depend on run order in one arm only.
        - Its per-run values are also the metric `cold_start_cost_usd`, with the same
          statistics and label as `cost_usd`.
      - **`first_calls`**: each agent session's first-call `cache_read` and `cache_write`,
        per run.
      - **Without `sandbox_instructions`** (`sandbox_excluded`), Falinks arm only, labelled
        "estimate ±15%".
        - Why: Claude Code adds that attachment, about 3.1k tokens per call, because the
          Falinks arm runs its sandbox. The Git arm is confined at host level and gets none.
        - Its tokens are its characters in the session JSONL divided by the first request's
          characters per token. Those are the characters of that request's tools, CLI
          prefix, system text, first message and attachments, over its input, cache-write
          and cache-read tokens.
        - Their cache write, and their reads on every later call of the session, are
          removed.
        - A run with a session that lacks the attachment, or a prompt snapshot listing its
          tools, has no value: missing, never zero.
    - **`falinks_request_key_refusals`**: per op and in `total`, with the same statistics
      per repetition, the Falinks requests the adapter refused because of unknown or missing
      request keys (reasons starting `request keys:`, #220). `timing` counts them from the
      transcripts; a run without transcripts is missing.
    - **`falinks_edits`**, per output form (`content`, `replace`, `from`), with the same
      statistics per repetition:
      - accepted, `stale`, `unreviewed`, `rejected` and `interrupted` edits, from the
        run's ledger;
      - `host_refused`: calls refused before they reached the ledger, such as a scratch
        reference the host refused;
      - `resends`: edits sent after the same agent's previous edit was refused;
      - `recovery_refusals`, `recovery_unresolved`, `recovery_round_trips` and
        `recovery_model_seconds`, from `timing`'s `refusal_recovery`;
      - the `reasons` of each refusal, such as hunk mismatches.

      `falinks-arm` records these counts in `result.json` as `edits`.
      `reverted_peer_text` lists accepted edits that bring back lines a peer's earlier
      edit removed, for a manual check: each entry's agent, id, path, peer id and
      `line_count`. No report field prints source text by default; `--show-code` adds
      the lines.
    - **Merging and gaps.** Several directories are merged as one batch: they must share
      one configuration and one pilot, and no run may appear twice. `concurrent_runs`
      lists the runs whose times overlapped a run in a sibling directory. `gaps` names
      the fixtures with fewer than three complete pairs or no jointly successful pair,
      and every unreplaced interruption and invalid run.
    - **Interruptions.** An infrastructure failure or usage limit on a scored run is an
      interruption, not a task outcome: until replaced, its repetition lacks every metric.
      A recorded run without a result reads as an infrastructure failure. A run still in
      progress is not recorded yet, so its pair is only incomplete. An interrupted pilot
      run never enters the report's metrics.
  - **Agent mistakes** in either arm are refusals counted in `rejected_operations`. They
    are never infrastructure failures.
  - **Timing**: `timing RUN_DIR` reports, per agent of one run of either arm, busy, model,
    tool, launch and waiting-on-peer seconds, round-trips and their model seconds by
    category (edit, refused edit, native, coordination), output bytes against changed bytes,
    and the slowest agent, by #116's method. Its `refusal_recovery` gives, per Falinks
    output form, the refusals, the round-trips and model seconds from a refused edit to the
    agent's next accepted one, and the refusals never followed by one (`unresolved`). Its
    `request_key_refusals` counts, per Falinks op, the results refused with `request keys:`.
    The definitions and heuristics are in
    `src/timing.rs`; `tests/timing/` holds synthetic transcripts with hand-computed values.
    Runs before #179 record their start to the second only (`start_precision`), so their
    launch and waiting times are good to about a second. Its `results` and `usage_reports`
    per agent compare the transcript's turn results with those recorded in `result.json`.

Run the pilot and batch in a session **outside auto mode**, because they launch nested
agents:

```sh
cargo build --release --bins                       # repository root: falinks-arm, check-integration
(cd evaluation && cargo build --release --offline) # claude-worker, falinks-eval
cp evaluation/config.json /private/tmp/eval-config.json  # fill every REQUIRED field
evaluation/target/release/falinks-eval batch --config /private/tmp/eval-config.json --runs /private/tmp/ev/pilot
evaluation/target/release/falinks-eval batch --config /private/tmp/eval-config.json --runs /private/tmp/ev/scored --after-pilot /private/tmp/ev/pilot
```

Or shard the scored batch across three sessions, then report over all shards:

```sh
evaluation/target/release/falinks-eval batch --config /private/tmp/eval-config.json --runs /private/tmp/ev/s-rust --after-pilot /private/tmp/ev/pilot --fixtures rust-errors
evaluation/target/release/falinks-eval batch --config /private/tmp/eval-config.json --runs /private/tmp/ev/s-page --after-pilot /private/tmp/ev/pilot --fixtures go-page
evaluation/target/release/falinks-eval batch --config /private/tmp/eval-config.json --runs /private/tmp/ev/s-rel --after-pilot /private/tmp/ev/pilot --fixtures go-relationships
evaluation/target/release/falinks-eval quality --runs /private/tmp/ev/s-rust --runs /private/tmp/ev/s-page --runs /private/tmp/ev/s-rel
evaluation/target/release/falinks-eval report --runs /private/tmp/ev/s-rust --runs /private/tmp/ev/s-page --runs /private/tmp/ev/s-rel
```

`/private/tmp` does not survive a reboot. When a batch is finished, move its directory into
`local-evidence/` and index it (`AGENTS.md`, "Local evidence").

Track T (#178) compares one agent with two Git agents, unscored, in its own batch directory:

```sh
evaluation/target/release/falinks-eval batch --config /private/tmp/eval-config.json --runs /private/tmp/ev/track-t --schedule track-t
evaluation/target/release/falinks-eval report --runs /private/tmp/ev/track-t --schedule track-t
```

- **Schedule**: every fixture, five repetitions per team size (#182), each repetition a
  pair that alternates which size runs first. There is no pilot; every invocation renews
  the gate.
- **Report** (`report --schedule track-t`, also the batch's `report.json`): per fixture and
  metric, each repetition's value per team size with the median, range, n and missing
  count, the `consistent` / `mixed` / `incomplete` label, and the paired difference (two
  agents minus one) per repetition. The metrics are the scored report's plus coordination
  model seconds, tool seconds, output and changed bytes (summed over agents), load balance
  (the busiest agent's busy seconds over the mean), the first development's arrival and the
  drafts shared after it (from `controller/events.jsonl`). Completion time also gets its
  sample coefficient of variation per team size (standard deviation with n − 1, over the
  mean). Each pair lists the per-agent `timing` figures, the slowest agent, the usage status
  and the developments; nested-agent hits are counted per team size. No winner is computed.

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
the setup ledger is `setup.json`. The shared setup ceiling is eight hours
(`config.json`'s `setup_limit_seconds`, 28,800 s) and covers this preparation
plus remaining pilot/evaluation setup, **not engine implementation**; the ledger
test checks it against `setup.json`'s limit and amendments. #123's 40,517 s was
the limit before #250's two amendments. Add timed setup sessions to that ledger; it does not enforce
or estimate engine work. See #19 and `docs/evaluation/first-agent-evaluation.md`
for safety gates and run interpretation. The [#5 amendment](https://github.com/danielrispler/Falinks/issues/5#issuecomment-6322468250)
replaced its continuation criteria with the report above, which computes no winner. Track T (#178) has
its own `team_size_research` section with a 43,200 s limit. It charges only
run time (including the usage drain) and harness preparation time, with
complete-only usage; implementation time is not charged (Daniel, 2026-10-22,
on #180). A session may still note `implementation_seconds` for the record. The ledger test fails when either section is
overdrawn: stop and ask before that happens. Each `limit_seconds` already
includes its amendments, so the remaining balance is the limit minus the charged
sessions. From `evaluation/`:

```sh
jq -c '{shared: (.limit_seconds - ([.sessions[].charged_seconds] | add)), team_size_research: (.team_size_research | .limit_seconds - ([.sessions[].charged_seconds] | add))}' setup.json
```

For the time to charge, `report --runs` gives each run's outcome and paired
timings, and `timing RUN_DIR` gives per-agent figures (see "Paired evaluation (#27)" above).
`code-quality.json` holds #228's frozen code-quality definitions (stage 1): the
normalization and deterministic metric rules, the thirteen differential properties
with their requirement sentences, generator domains, seed and input count, the judge
pin, prompt, anchors and rules, and the inspection and diagnosis rules.
`code_quality_definitions_trace_to_fixture_requirements` fails when a quoted sentence
no longer appears verbatim in its fixture. Stage 2 changes the file only by a versioned
amendment: bump its `version` and add an `amendments` entry with the reason, then
re-freeze.

The judge's known-answer set (#233) is `protected/known-answer.json`: per fixture, the
test file that joins the reference to make the known-answer base, and one degraded copy per
dimension as a declaration edit of the formatted base. `known-answer` checks the base and
every copy: the formatter leaves it unchanged, the oracle and its tests pass, no property
fails, and `vet` or `clippy` report nothing on changed lines. With `--judge CONFIG`, from a
session outside auto mode, it then rates each tree three times (45 calls) and writes
`calibration/v<definitions version>.json`, which names the manifest version; re-freeze to
freeze it. After an amendment, the next record gets the new version's name. `--judge` with
`--fixtures` is refused unless every fixture named is in `judge.uncalibrated_fixtures`: their
judged inputs changed, so they are calibrated alone (15 calls each) and the other fixtures keep
their record (#263). The 54 information calls on the #27 runs are `quality --runs DIR --judge CONFIG` over those batches. A record's outcome reaches the report by an amendment that sets `judge.calibration.<dimension>` to `{"passed": true}`, or to `{"dropped": true}` after a second failure. A dimension dropped on one fixture alone (#263) sets `judge.calibration.<dimension>.<fixture>` to `{"dropped": true}`.

`config.json`'s `protocol` names the protocol version a batch measures, and each
batch invocation records it with its configuration. Track P's is manifest v34
(#247). Every pilot and scored invocation runs from its freeze commit, recorded
on #188 (#228 decision 29a); the batch records its `HEAD` but does not compare it,
so the operator checks it.

`freeze` lists each changed frozen path and refuses unstaged changes to them, so
stage your repair first. Re-freeze before observing a red harness test, or the
freeze guard fails first and masks the real failure.

## Verification record

`verification.json` records the six passing integration checks, fixture identities,
compiler/runtime versions and the explicit limits of the evidence. `review.md`
records the independent Standards/Spec review and resolved findings. The setup
ledger `setup.json` holds the charged total and the remaining balance of the shared
setup allowance (eight hours plus amendments); `setup_ledger_totals_match_its_sessions` checks the total
against its sessions and the balance against the limit.
