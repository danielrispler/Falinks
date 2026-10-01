# Coupled Codex workspace experiment

2026-10-01. Resolves [Compare coupled-agent shared workspaces with isolated development and joint validation](https://github.com/danielrispler/Falinks/issues/10), within [Map: Specify the smallest agent-first collaborative editing experiment](https://github.com/danielrispler/Falinks/issues/1). Evidence for [Specify the minimum proposal and publication protocol](https://github.com/danielrispler/Falinks/issues/4), **not an architecture selection**.

## Finding

Both conditions produced exact, draft-free early candidates and passing combined candidates. In the shared condition, safe early capture required a cooperative backup, withdrawal and restoration of B's unfinished function while both writers were parked. Isolated worktrees needed no draft withdrawal; ordinary Git merged the final same-file revisions without conflicts or manual repair. This fixture exposed no manual integration work for sharing to eliminate. It did expose an extra shared interruption and capture operation.

The measured shared pair completed in 53.36 seconds and the isolated pair in 62.66 seconds. Model latency, deliberately forced checkpoints, sequential condition order and one measured pair dominate this comparison. These numbers do not establish a general speedup or answer the product hypothesis. Shared capture also does not meet the existing fixed-read requirement by itself: shared reads can see the other client's unoffered changes.

## Scope, contracts and method

The controller and fixture are [trial.py](coupled-workspace-assets/trial.py). Two actual `codex exec` processes overlap **within each condition**. Conditions run sequentially, shared first, on separate fresh repositories with equivalent fixture files, prompts, compiler, lint rules and tests. One initial pilot pair and one corrected measured pair were run; no production engine was built.

The behavioral contract was fixed in the prompts and test fixture before inference:

- Normalize a SKU by trimming whitespace, preserving casing and Unicode, and rejecting a blank result with `TypeError`. Initially normalization returns a string; presentation returns `SKU:<normalized-value>`.
- The coupled final contract changes normalization to `{key: string}` and presentation to accept that object. The unchanged `checkout()` caller composes both functions. Both final revisions are required together.
- A owns normalization; B owns presentation, in different functions of **the same file**. B also generates two TypeScript modules. Ownership is prescribed for this experiment; there is no automatic owner or dependency discovery.

Both agents receive the whole contract, the role allocation and the same available tools. A first offers an independent string-returning improvement and requests full validation. B deliberately leaves `return undefined; // UNFINISHED_B`, declines the optional early check and supplies no estimate. The measured runs confirm B's draft checkpoint preceded A's request. These are assigned scenarios, not spontaneous readiness negotiations or a test of delivery to an already-thinking agent.

After early capture, both agents resume, reread their development file and prepare their coupled final revisions. The controller validates the early fixed copy while development resumes, then verifies its complete manifest again after both final checkpoints. Both final members remain required irrespective of B's earlier optional decline.

## Actual capture boundaries

**Shared:** both agents park in a cooperative checkpoint command, with no detached writers allowed. B's running checkpoint helper saves its own unfinished function outside the candidate, replaces that function with its retained-base version, and signals withdrawal. A remains parked. The controller copies the whole fixture; B then restores its saved draft. Byte equality against the pre-withdrawal file is checked before either agent resumes. This is a narrow, explicitly programmed withdrawal protocol using known function boundaries, not inferred authorship from a shared before/after diff. A whole-file copy before withdrawal contains the draft and fails the development compiler check.

Final shared capture waits until both offered members are parked and the synchronous generator has returned. The entire fixture is then copied. This avoids a known active writer in the tested path; it does not enforce quiescence against an arbitrary agent, unknown descendant or bypass writer. The helper has no crash-safe withdrawal transaction: interruption between withdrawal and restoration would require recovery from the saved backup. That remains a protocol obligation, not a production capability demonstrated here.

**Isolated:** each agent has a Git worktree from the same retained base. A's parked worktree is copied for early validation without touching B. At final checkpoints, the researcher commits only the offered source/generated files in each worktree, creates a complete validation worktree at the base, and runs ordinary `git merge --no-edit A` followed by `git merge --no-edit B`. Both succeed; the second uses Git's `ort` strategy. The commits are researcher actions after capture, not writes attributed to the agent's native Git tools. Worktrees separate files and indexes but share repository administration. [Git worktree documentation](https://git-scm.com/docs/git-worktree).

The known-base function replacement in shared capture is a deliberately narrow fixture technique. It is not available for arbitrary mixed edits, formatter rewrites or unknown child writes. Both conditions rely on exclusive, cooperative writers and fixed copies under one controller; the copies are checked for stability, not enforced filesystem immutability. Readiness messages alone authorize none of this.

## Validation and observations

| Observation | Shared workspace | Isolated worktrees |
| --- | --- | --- |
| Concurrent sessions at both early checkpoints | Observed | Observed |
| Early candidate excludes B | Passed; after withdrawal | Passed; directly from A |
| B's unfinished draft preserved | Exact restoration | Exact untouched bytes |
| Early candidate stays fixed through later development | Complete manifest unchanged | Complete manifest unchanged |
| Shared dirty development check | Fails on unfinished presentation | Not applicable; no shared dirty tree |
| A-final focused check | Passes | Passes |
| B-final focused check | Passes | Passes |
| A-only final candidate | Whole-module compilation rejects unchanged caller | Same |
| B-only final candidate | Whole-module compilation rejects unchanged caller | Same |
| Both offered final members | Build/type check, lint, tests pass | Same |
| Manual merge/recovery | None in successful run | None; normal Git merges cleanly |
| Draft withdrawal/restoration interruption | One pair | None |

The early candidate manifests are identical across conditions. Final candidates differ in harmless formatting chosen by the agents; each exact manifest is retained separately. All affected TypeScript files, including the untouched caller and both generated modules, are compiled with `strict` and `noEmitOnError`. One TypeScript invocation performs full type checking and JavaScript emission. ESLint parses **all fixture TypeScript modules**, with `no-unreachable`, `no-debugger`, `eqeqeq`, `no-var` and `prefer-const`. Node's built-in assertions run on the emitted complete module graph. These are modest declared lint rules, not a claim of exhaustive static analysis. [TypeScript compiler options](https://www.typescriptlang.org/tsconfig/), [ESLint configuration](https://eslint.org/docs/latest/use/configure/configuration-files).

The fixed assertions check trimming, rejection, casing/Unicode, presentation, unchanged-caller behavior and generated values. Their predeclared `base`, `early` and `final` modes reflect the three contracts; they were not changed after observing agent output. Focused checks transpile the edited module and exercise only the role's behavior, intentionally omitting full type/module validation. Both focused final checks pass even though either revision alone breaks the untouched caller:

```text
A only: src/caller.ts(4,21): TS2345: { key: string; } is not assignable to string.
B only: src/caller.ts(4,21): TS2345: string is not assignable to { key: string; }.
```

Failed builds do not emit artifacts, and their Node tests are explicitly skipped. No failed candidate is treated as publication-authorized. The B-only counterfactual checks were run after the live comparison and are saved separately; shared omitted-member candidates are researcher-created diagnostics, not separately attributable shared offers.

## Timing and effort

Measured pair only, in seconds. Readiness and release observations use **one parent/controller monotonic clock**, polled approximately every 20–50 ms. Do not subtract helper clocks across processes.

| Measurement | Shared | Isolated |
| --- | ---: | ---: |
| Setup, including baseline checks | 0.517 | 0.583 |
| Engine: A early request to capture start | 0.578 | 0.025 |
| Included shared dirty-tree diagnostic | 0.497 | — |
| B interruption: withdrawn to restored | 0.058 | 0 |
| Engine: A final ready to B final ready | 5.435 | 8.851 |
| Final assembly, excluding missing-member checks | 0.004 | 0.090 |
| Early candidate full checks | 0.485 | 0.485 |
| Combined final full checks | 0.472 | 0.467 |
| Total setup through both process exits | 53.363 | 62.663 |

Capture-start waiting includes polling and, for shared, the diagnostic plus cooperative withdrawal. The withdrawal/restoration interval measures only the actual withdrawn state; B's earlier deliberate checkpoint wait is separate. Final readiness waiting measures the required-member gap, not a deadline imposed on B. Final assembly includes shared copying, or isolated source commits, worktree creation and merges. It excludes the intentionally failing A-only validation. Automated Git work is nonzero effort even though no manual repair was necessary.

The measured live loop ran four full check phases per condition: baseline, early, A-only final counterfactual and combined final. Shared also ran the dirty-tree diagnostic. Afterwards, one B-only counterfactual was checked per condition. Each pair ran three agent-focused checks. These are deliberate distinct candidates/check scopes, not recovery retries; no post-failure agent repair or publication revalidation cycle was exercised. The extra shared diagnostic is disclosed rather than presented as an equivalent fixed-check budget.

The pilot's capture-wait numbers are invalid: helper `time.monotonic()` values in this runtime were not comparable to the controller's. Its successful capture/build evidence is retained under [pilot](coupled-workspace-assets/evidence/pilot), but those timing fields must not be used. The measured repeat observes markers in the controller instead. The shared pilot request also preceded B's draft marker; the controller waited for both before capture. The measured pair has the intended draft-before-request ordering. There were no retry runs beyond this corrected repeat.

No agent completion deadline or scheduling policy was adopted. Checkpoint barriers deliberately create this scenario and wait for release. Model latency and final-response work are included in total elapsed; active model time is not separately instrumented. The controller waits for A's exit and then B's, so saved end observations can lag an earlier B exit. They establish completion through the last observed exit, not exact per-agent lifetimes or overlap duration. Concurrency is demonstrated by both live parked sessions before release, not by subtracting those end timestamps. One measured pair cannot support throughput, interruption-frequency or recovery-cost estimates for real projects.

## Shell and child-write coverage

Each real B session invokes a shell command that launches a synchronous Python generator, writes two valid TypeScript modules, waits for child exit and only then offers its final revision. Both generated files enter the same final candidate and are compiled, linted and tested.

The separate runnable [child_probe.py](coupled-workspace-assets/child_probe.py) launches a **detached known child** from a parent that exits successfully. The child writes one file, waits, then changes that file and creates a second. The controller observes parent exit 0 while the child is alive. A diagnostic copy is fixed but contains only the partial group; after the registered child signals completion, the complete copy contains both matching revisions. The diagnostic remains unchanged. This is a researcher-controlled counterexample, not a real-agent detached-generator enforcement test.

Accordingly, completed tool events, `--ephemeral`, hooks and a readiness label are insufficient evidence of descendant quiescence. JSONL records commands and native patch items; it is not an authoritative filesystem write journal. Official hooks documentation also describes coverage exceptions. [Non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode), [hooks](https://learn.chatgpt.com/docs/hooks), [app-server](https://learn.chatgpt.com/docs/app-server).

Unsupported: unknown detached descendants, arbitrary mixed shared shell writes, out-of-bound writes, process-tree termination, crash-safe restoration, immutable snapshots under hostile writers, mid-turn delivery/refresh, owner/dependency version validation, publication races, durable events and restart recovery. Inner sandbox access to temporary siblings was not denied/tested. The worktrees are a cooperative storage control, not a security isolation claim. Fixed candidate testing here is distinct from testing a shared mutable development directory.

## Interface, configuration, identities and replay

Scratch root: `/private/tmp/falinks-coupled-nnb2534u`; research checkout: `research/`, branch `research/coupled-workspace-trial`, cloned with `--no-hardlinks` from main revision `caabadc245e05d8444c4e5c63a2356710e20bcd0`. Codex CLI **0.159.3**, Node **v22.23.1**, Python **3.9.6**, Git **2.50.1 (Apple Git-155)**. Temporary tool installation: TypeScript **5.9.3**, ESLint **9.39.1**, TypeScript ESLint parser **8.48.1**, with a saved complete package lock. Installation used `--ignore-scripts`; initial offline installation lacked cached metadata, so only scratch dependencies were downloaded. ESLint emitted its version-support warning; this historical fixture version is pinned for replay, not recommended for adoption.

Exact per-session shape, with prompt supplied on stdin:

```sh
codex exec --ignore-user-config --ephemeral -C <development-root> \
  -s workspace-write --add-dir <condition-coordination-directory> --json -
```

Each launch sets `TRIAL_COORD`, `TRIAL_CONDITION`, `TRIAL_TOOLS` and the retained-base presentation function in `TRIAL_BASE_B`. Saved authentication is reused; it is not copied into assets. User configuration is skipped. No model/reasoning override is passed; the installed/account default is used, and the emitted JSONL did not identify its exact model. That is an uncontrolled performance variable. Outer launch approval enabled inference; the inner CLI still requested `workspace-write`. This records configuration, not a proof of complete sandbox enforcement. [Official command reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli), [sandbox documentation](https://learn.chatgpt.com/docs/sandboxing).

All prompts, argv, outputs, compiler/lint/test results, fixed copies, manifests, draft backups and offered isolated diffs are in [measured evidence](coupled-workspace-assets/evidence/measured). Reasoning items are excluded from saved JSONL; task tool events remain. [Isolated identities](coupled-workspace-assets/evidence/measured/isolated/identities.json) contain full base/A/B/combined commit and tree OIDs. Shared and isolated candidates also have canonical full-manifest SHA-256 identities. The immutable source assets plus manifests identify exact candidates; branch names alone do not. [Git revision syntax](https://git-scm.com/docs/gitrevisions).

```sh
# From this research checkout: no inference or installation needed.
python3 docs/research/coupled-workspace-assets/verify.py

# Replay the live comparison into a NEW scratch output directory.
# Requires saved Codex authentication, network inference and CLI 0.159.3.
npm ci --ignore-scripts --no-audit --no-fund \
  --prefix docs/research/coupled-workspace-assets/tools
python3 docs/research/coupled-workspace-assets/trial.py \
  /private/tmp/falinks-coupled-replay \
  docs/research/coupled-workspace-assets/tools
python3 docs/research/coupled-workspace-assets/child_probe.py \
  /private/tmp/falinks-child-replay
```

Replay rejects an existing output directory. Cooperative barriers are for this experiment and have no agent completion timeout; an operator can stop a hung replay and retain its partial evidence. The saved-evidence verifier passed, checking manifests, draft preservation/exclusion, real overlap, required-member failures, successful complete checks, child lifecycle and main-workspace preservation. Main's HEAD, branch, status and all seven tracked file hashes matched the pre-trial fingerprint. No research code, dependency or Git metadata was installed into main.

## Recommendation for the protocol discussion

Carry forward explicit offered revision identities, explicit required membership, optional decline/no-estimate behavior, a separately stated child-writer boundary, draft exclusion/preservation, and full checks on the exact complete candidate. Keep engine waiting observations separate from agent completion and from required-member validation. Compilation caught a concrete untouched-caller failure that both focused checks missed.

If the human wants shared mutable development, the next specification must address **both** stable client reads and capture of unoffered mixed work, including recovery during withdrawal and unknown descendants. Cooperative parking alone fixes neither authorship nor draft exclusion. If the human wants isolated development, this small baseline already combines coupled same-file work without manual integration repair; any proposed advantage should be demonstrated on harder tasks rather than assumed.

Compared with [Verify Codex capture of ready proposals alongside unfinished drafts](https://github.com/danielrispler/Falinks/issues/9), this closes bounded gaps in actual session overlap, whole-module compiler/lint/tests, required coupled revisions, generated valid TypeScript and measured shared interruption versus ordinary Git integration. It does not establish an exhaustive integrated capture boundary or resolve the workspace architecture. The smallest useful next step is the existing human protocol discussion, using these observed costs and unsupported boundaries. No additional architecture or scheduling decision has been made.
