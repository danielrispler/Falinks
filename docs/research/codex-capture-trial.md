# Codex capture trial — issue #9

2026-10-01. [Research question](https://github.com/danielrispler/Falinks/issues/9); informs [the human protocol decision](https://github.com/danielrispler/Falinks/issues/4). Bounded investigation, no production engine, publication, or tracker mutation. The report and assets are committed only in a scratch clone on `research/codex-capture-trial`. Parent repository revision: `d2594311c812ebc2761731d220584b2d33d897b7`. Followed research, OpenAI Docs, and ponytail skills; this investigation was delegated by main. Earlier [primitive findings](https://github.com/danielrispler/Falinks/blob/3080a5f61e3167c71e22577c586ef265beb6b6a0/docs/research/workspace-capture.md) were read through issue #8's resolution, not locally present on main.

## Answer

**An isolated ordinary-tool repository copy produced the exact ready candidate while a different Codex run left an unfinished edit in the other function of the same fixture. Shared live writes did not establish safe attributable capture.** Native patch hooks expose a narrower attributable operation, but shell hooks expose commands rather than an exhaustive write set. A shared whole-file candidate included the unfinished function. No workspace architecture is selected here.

Candidate stability was observed after further draft edits; filesystem immutability, concurrent repository snapshot consistency, exhaustive process attribution, and stable whole-repository reads under shared writers were not established. Agent runs were sequential, with deliberately unfinished content retained between runs; this is not a concurrent two-agent stress test. Subsequent draft changes and the active writer were researcher-controlled, not a second running model turn.

## Interface, configuration, and replay

Scratch clone: `/tmp/falinks-codex-trial.Fpn1OF/repo` (canonical `/private/tmp/…`). Minimal fixture repos and raw run products are siblings, not production files. Installed executable `/opt/homebrew/bin/codex`: **`codex-cli 0.159.3`**, rather than the ticket's previously observed `0.159.2`; no older binary was available or installed. Python `3.9.6`, Git `2.50.1 (Apple Git-155)`, Node `v22.23.1`; app-server handshake reports macOS `26.6.2`, arm64. Hook input reported default model **`gpt-6.1-sol`**. Saved account authentication was reused without copying or exposing credentials.

Exact prompts, argv, emitted events, results, and captured fixture bytes are in [evidence](codex-capture-assets/evidence/). The trial is [runnable Python](codex-capture-assets/trial.py), using only stdlib plus installed Git/Codex. The interface and hooks checks are [interfaces.py](codex-capture-assets/interfaces.py), [hook_trial.py](codex-capture-assets/hook_trial.py), and its vetted [logging hook](codex-capture-assets/hooks.py).

```sh
# Setup actually used (main worktree remains untouched):
git clone --no-hardlinks <MAIN_WORKTREE> /tmp/falinks-codex-trial.Fpn1OF/repo
git -C /tmp/falinks-codex-trial.Fpn1OF/repo switch -c research/codex-capture-trial
codex --version
codex exec --help
codex app-server --help
codex features list
codex app-server generate-json-schema --out /tmp/falinks-codex-trial.Fpn1OF/schema

# Replay from this report's checkout; use fresh output directories:
python3 docs/research/codex-capture-assets/verify.py
python3 docs/research/codex-capture-assets/trial.py /tmp/codex-capture-replay
python3 docs/research/codex-capture-assets/interfaces.py /tmp/codex-interface-replay
python3 docs/research/codex-capture-assets/interfaces.py /tmp/codex-turn-replay --turn
python3 docs/research/codex-capture-assets/hook_trial.py /tmp/codex-hook-replay
```

Inference command shape was `codex exec --ignore-user-config --ephemeral -C <fixture-repo> -s workspace-write --json <prompt> </dev/null`, with stdout/stderr retained. This suppresses user config for CLI inference runs, not all possible system/project instructions or account defaults. The tested fixture repos have no project instructions. App-server was launched as `codex app-server --stdio -c analytics.enabled=false`; it loads installed defaults. JSON-RPC requests are explicit in `interfaces.py` and responses in evidence.

The outer agent sandbox initially returned `failed to initialize in-process app-server client: Operation not permitted`; approved outer escalation allowed launch while CLI `workspace-write` remained selected. An initial explicit `-m gpt-5.4` run failed with account/model unsupported HTTP 400; retry omitted `-m`. No inference result is attributed to that failed run. Hook test additionally used invocation-local `--dangerously-bypass-hook-trust` for the reviewed logging-only script and inline `hooks.PreToolUse`/`hooks.PostToolUse` TOML; no global trust file was changed. Hook payload's `permission_mode: bypassPermissions` is recorded verbatim: it must not be mistaken for proof of a sandbox boundary.

The runnable check has a 240-second researcher subprocess bound to keep this investigation finite. This is **not an agent completion deadline, readiness rule, or engine scheduling policy**. Replay uses fresh directories; rerunning against existing outputs is unsupported.

## Observed results

| Check | Actual result | What it establishes / limits |
|---|---|---|
| Ordinary repository-copy control | Real agent A patched `ready()` from `1` to `2`; real agent B used shell redirection to leave `unfinished()` as `return ; // UNFINISHED_B` in its own copy | Ordinary native editing and shell tools work in separate fixed-base scratch repositories. Attribution is by exclusive workspace assumption, not a complete kernel audit. |
| Exact candidate | Candidate bytes equal retained base with only A's edit; SHA-256 `1349d1832345445f11583e8345203747ba09f160c6d745be1b1cdab219509fab` | B's draft absent from candidate and preserved in B's workspace. Further researcher edits to B leave candidate hash unchanged. No enforced immutability. |
| Temporary file plus retained base | `git merge-file -p <shared-draft> <retained-base> <ready-copy>` returned 0 and retained both separated edits | Incorporation preserved B in the live draft. That combined output is not a ready candidate: it still contains B. Retained-base A copy alone excludes B. This is text merging, not owner/dependency revalidation. |
| Shared native patch | Real Codex patched A in a file seeded with B's deliberately unfinished content, preserving B exactly | A simple whole-file capture contains B; preserving another edit is different from excluding it from publication. Shared readiness messages do not fix this. |
| Native JSONL | `file_change` started/completed events named path and operation | CLI events here did not carry full patch text. Hook payload did. A before/after Git diff of the shared file still includes B. |
| Shell and generation | Real Codex used `printf … > shell.txt` and a Python command writing two `.ts` files; both members were copied together after the command completed | Generated files contain literal `\n`, a model quoting mistake. Existence/group capture passed; valid generated TypeScript was not established. No `file_change` events appeared for these shell writes. |
| Pre/Post tool hooks | Four events covered native `apply_patch` and shell `Bash`, with session/turn/tool-use identifiers | Patch text is available for native operation mediation. Bash supplies command text and response, no authoritative file set or child-write journal. Logging did not rewrite or deny calls. |
| Watch notification | `fs/watch` followed by external write produced `fs/changed` with `watchId` and `changedPaths` | No writer, thread, turn, or process attribution in the observed event. Useful invalidation only. |
| Writer at capture | External process alive after writing `phase-one`; capture retained it while live file became `phase-two` | A fixed copy can be a partial process result; fixed bytes do not imply ready or quiescent bytes. |
| Process termination | Actual app-server `command/exec` process ID was terminated through `command/exec/terminate`, exit 137; second write absent | Standalone managed process control worked. No model turn interruption, descendant-process quiescence, crash recovery, or atomic rollback proof. |
| Group consistency counterexample | Read A old; change A and B; read B new → captured `[old,new]` | Deterministic interleaving, not an OS snapshot test: sequential copying of mutable files is not a consistent change-group guarantee. |

The two generated files' exact bytes are retained so the mistake is reviewable, rather than silently repaired. `tsc` was absent from PATH and no installed global TypeScript compiler was found in the checked Node installation. No compiler, formatter, package installation, or application integration check was attempted. Minimal fixtures use no dependencies.

Measured one-run setup for two fixture Git repositories: **60.725 ms**; completed A-copy capture: **1.417 ms**. Model runs: unfinished 10.518 s, ready 18.475 s, shared native 19.500 s, shell group 12.111 s. These are tiny local-fixture measurements, exclude clone/auth/startup variability, and establish no general speedup.

## Official documentation versus observed interfaces

[Non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode) documents `--json` event output and saved authentication. Both worked after outer escalation; emitted tool events did not constitute a comprehensive filesystem change journal.

[Hooks](https://learn.chatgpt.com/docs/hooks) documents Pre/Post coverage for native patching and shell execution, aliases, input rewriting/denial, trust review, and special paths outside hook coverage. Our logging test observed patch and Bash callbacks only. We did not verify rewrite/deny, `write_stdin` completion timing, or interception of every tool. A child's individual writes are not independent tool calls. Concurrent matching hooks are not a serialized publication lock.

[App-server](https://learn.chatgpt.com/docs/app-server) documents thread/turn item events, file-change diffs and approvals, turn diffs, filesystem watching, and turn interruption. Installed schema generation additionally exposes `item/fileChange/patchUpdated`, `command/exec/terminate`, and `hooks/list`; schema presence alone is not runtime verification. Actual handshake, watcher, standalone command launch/termination, and the native turn/retained-base replay below are distinguished from untested approvals and interruption.

A known-path native patch can plausibly be logged or mediated with retained base identity. That is a restricted integration hypothesis, not support for arbitrary shared shell editing. Temporary file copies also leave ordinary searches, imports, generators, and tests needing a complete stable view; their existence does not freeze other repository reads.

## Observed app-server turn and retained-base replay

The actual app-server turn completed in 50.436 seconds. It first made four unsuccessful patch guesses (prompt disallowed shell reads), then applied a minimal hunk successfully. Its `fileChange` item carried a diff plus thread/turn identifiers; `turn/diff/updated` carried a ready-only unified diff. Neither included B's unfinished body as an addition. Failed guesses are not successful captures.

The final [observed diff](codex-capture-assets/evidence/ready-turn.diff) was applied with `git apply <ready-turn.diff>` in a fresh retained-base directory. The [result](codex-capture-assets/evidence/retained-base-candidate.ts) exactly equals the A-only ready bytes, excludes B, and stays unchanged after further researcher changes to B in the live original. This is a credible narrow native-patch route for this fixture; it does **not** establish complete attribution when other writers race, or handling of shell changes. The diff's Git index hashes identify the dirty shared before/after file, not Falinks' retained snapshot; ordinary non-index `git apply` succeeded by context. A real adapter must separately bind/check snapshot and owner/dependency versions rather than adopting those hashes as the expected base.

Evidence is intentionally small and sanitized: no credentials, account quotas, editor identity, home directories, unrelated config, reasoning streams, or unrelated prompts are retained. App-server streams retain only relevant patch/diff/turn/process/watch messages. Initialization results retain platform fields only. Absolute scratch paths and task-specific prompts remain to make the runs reproducible; main worktree setup path is written as `<MAIN_WORKTREE>`. Five small relevant generated schemas are retained, not the full generated bundle.

## Recommendations and unresolved boundaries

For ordinary editing and shell tools, keep the isolated repository-copy control as the measured baseline; its single-fixture test does not prove whole-repository stable reads. Separately, ask the human whether a native-only patch adapter warrants another bounded trial. The native-only result does not support shell editing in a shared workspace. The smallest further check is to exercise a detached generator while another writer changes the same shared file, and verify writer ownership/quiescence before treating either change group as ready. The retained-base native-patch replay passed above, without concurrent model writers. Do not generalize command completion or turn interruption to descendants without that check. No supported comprehensive controlled-shared-write mechanism was demonstrated here.

Do not silently publish drafts; required change groups cannot lose a member. Changed edited owners or declared dependencies require revalidation. Exact-candidate checks and expected-base acceptance remain requirements, not engine capabilities this experiment implements. Readiness and estimates remain advisory scheduling input, without pressure to complete. Joint-testing benefit and engine waiting policy are separate open hypotheses. Same-owner conflicts, dependency tracking, refresh/revalidation, sandbox escape coverage, process-tree capture, publication coordination, stable whole-repository reads under external writes, and recovery remain gaps.
