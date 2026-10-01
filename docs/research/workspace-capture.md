# Temporary edit copies and shared-workspace capture

Research for [Investigate temporary edit copies and shared-workspace capture](https://github.com/danielrispler/Falinks/issues/8), informing [Specify the minimum proposal and publication protocol](https://github.com/danielrispler/Falinks/issues/4). Investigated 2026-09-30, completed 2026-10-01 after a session interruption. This report records documented primitives and a small local check, not a selected architecture or a verified agent integration.

## Answer and recommendation

**Small temporary edit copies are a viable proposal representation. They do not require a long-lived Git worktree.** An agent can edit a copy, retain the original base, and submit the difference for engine-controlled combination. Copying the entire edited file over the current file is unsafe when another edit has intervened; use a base-aware change and freshness checks instead.

There are two separate requirements: preserving concurrent changes when applying an edit, and providing stable repository reads while preparing it. Copying `price.ts` addresses neither reads of its changing imports nor a test runner's reads of the rest of the repository. Falinks' existing agreement keeps reads tied to a stable base until explicit refresh, so a raw shared mutable directory is not a sufficient implementation. Reopening that agreement is a human decision, not a consequence of this research. The canonical requirements are in [Define the first experiment’s correctness guarantees and failure outcomes](https://github.com/danielrispler/Falinks/issues/3#issuecomment-5913442940).

**Recommended next step:** a bounded trial of one harness with a fixed repository view and small captured proposals, using an ordinary temporary directory or temporary worktree as the control. Compare per-file mediated edits against ordinary shell formatting/generation in that same trial. Choose the cheapest mechanism that passes the agreed cases. Do not build a virtual filesystem before demonstrating a need for it, and do not promise that hook configuration alone makes a shared directory safe.

This recommendation is an engineering assessment. No performance, conflict-frequency, novel-architecture, or implementation-hours claim was established.

## What the mechanisms actually buy

| Candidate | Ordinary tools | What it solves | What still needs a mechanism |
| --- | --- | --- | --- |
| Direct shared-file writes with hooks | Tools run against the same changing tree | Very little directory setup; intentions and tool calls can be recorded | Stable reads, complete attribution, exclusion of unfinished edits, multi-file capture, and bypass enforcement. Per-tool before/after diffs can include another agent's simultaneous edits. |
| Temporary per-file copies and base-aware incorporation | Natural for file-specific edit tools; less natural for shell tools that discover paths | Draft privacy and provenance for those captured files; safe rejection or combination against a retained base | Reads of other files remain unstable without a fixed view; a formatter/generator may touch unlisted files; copy-back must not replace an intervening edit. |
| Short-lived repository materialization for a proposal/check | Existing shell, formatter, compiler and test commands can use normal relative paths | A complete filesystem view and before/after change set at a controlled boundary | Correct base selection, private writable outputs, lifetime/cleanup, refresh, capture consistency, and publication authority. Short-lived storage does not by itself let an active agent keep stable reads between invocations. |
| Per-agent Git worktree | Normal repository tools work in a separate directory | Separate working files and indexes, with shared Git object storage | Engine proposal/dependency rules, safe publication, unmanaged writes, setup artifacts, and recovery. A worktree is an implementation baseline, not a semantic requirement. |
| Fixed lower tree plus per-agent copy-on-write view | Tools may use normal paths inside their own namespace | Potentially cheaper separate repository views | Platform-specific setup, a genuinely fixed lower layer, upper-layer change extraction, permissions, and adapter validation. |

Git supports multiple working trees associated with a repository, and linked worktrees have private Git administrative state alongside shared refs/objects. Detached and temporary worktrees are supported; long-lived task branches are not mandatory. Worktree isolation does not sandbox absolute paths or external processes. The storage behavior is documented by [Git worktree](https://git-scm.com/docs/git-worktree); the implications for Falinks are this report's inference.

## Copy, merge, capture, and publication are different operations

1. **Prepare:** retain a base snapshot identity and create the agent's edit target. Its later edits have an agent/proposal identity. Small chunks are possible; chunk size is not a consistency guarantee.
2. **Capture:** freeze specific proposal revisions, including all members of an explicit change group. Later draft writes belong to another revision. A copy performed while writers are active can collect files from different instants; quiesce the writers or capture through an authoritative revisioned mechanism.
3. **Combine:** compare the retained base, current published state, and proposed content. If the whole-file base still matches, replacing that file can be allowed under exclusive publication authority. Otherwise reject conservatively or perform a base-aware combination. A check followed by replacement has a race unless acceptance is serialized or guarded by an equivalent version comparison.
4. **Validate:** run the required compilation/tests on a materialized exact candidate. Source content must stay fixed throughout the checks. Build outputs and caches need their own allowed writable locations; a validator that rewrites source creates a different candidate and requires recapture/rechecking.
5. **Accept:** ensure the expected published base is still current and accept exactly the checked candidate. If it advanced, rebuild/recheck or require agent revalidation as prescribed by the existing rules.

The process above is a candidate design outline, not an implemented protocol. It does not adopt symbol reservations. A brief exclusive acceptance step is different from preventing agents from preparing overlapping proposals.

Python documents `os.replace` as atomic when successful, with possible cross-filesystem failure and silent replacement of an existing file. Atomic replacement avoids exposing an intermediate partial file; it does not merge its old and new contents. This distinction is demonstrated by the scratch lost-update case. Neither this primitive nor a succession of file replacements establishes crash-atomic multi-file publication. [Python os.replace](https://docs.python.org/3/library/os.html#os.replace)

Existing Git primitives can avoid inventing a text merge algorithm. `git merge-file current base proposed` performs a three-way file merge, and `-p` returns its result without rewriting the input. `git apply --3way` needs base blob identities and locally available blobs; with `--cached`, conflicts stay in index stages rather than the shared working tree. These are alternatives to investigate in a private candidate, not commands to run against an actively edited shared checkout. [Git merge-file](https://git-scm.com/docs/git-merge-file), [Git apply](https://git-scm.com/docs/git-apply)

**Text combination is not Falinks' freshness proof.** Two distant edits inside the same function can merge cleanly; the agreed owning-symbol rule still requires agent revalidation after an intervening publication to that function. Changed declared dependencies, same-owner edits, and uncertain ownership must go through their existing rules. Whole-file fallback is conservative; whole-file base rejection is safe but blocks independently edited symbols unless combination and symbol checks provide the finer path. Group atomicity is an engine publication rule, not a property of copying several files.

SQLite can record proposal/base/revision/group identities and coordinate an authoritative acceptance step. SQLite knowledge, messages, and symbol versions do not alone intercept arbitrary operating-system reads/writes or make a directory snapshot. That is the boundary this investigation needs to expose, rather than a reason to discard the coordination design.

## Plausible first-harness integration

The following are documented capabilities and explicit gaps. No real Claude Code or Codex integration was built or tested.

### Claude Code

- `PreToolUse` can deny calls or replace complete input arguments; native Edit/Write and Bash have documented inputs. Hook context can reach the model. This makes file-path redirection and proposal-status delivery plausible, but rewriting arbitrary shell command text is not transparent filesystem interception.
- `PostToolUse` observes completed calls; failure hooks and batch boundaries exist. Watched-file events identify changes but are asynchronous observations, not a transaction or a complete writer-attribution journal. Safe model context can be delivered at supported hook/turn boundaries; a notification does not silently refresh a snapshot. [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- Bash sandbox filesystem rules are enforced by the operating system for shell child processes. The documented sandbox scope is Bash/PowerShell/Monitor, while other tools use permission rules. Unsandboxed retries/exclusions and unprotected native or MCP writes must be accounted for; configuration options include refusing unsandboxed fallback. [Claude Code sandboxing](https://code.claude.com/docs/en/sandboxing)
- An SDK wrapper has in-process hook callbacks and model-visible `additionalContext`. This is a concrete orchestration surface to investigate instead of assuming an agent's prompt will obey every capture rule. [Agent SDK hooks](https://code.claude.com/docs/en/agent-sdk/hooks)

### Codex

- Current official hooks documentation covers Bash, unified exec, `apply_patch`, MCP, and other local tools; supported calls can be denied or rewritten. `additionalContext` supports model-visible coordination. It explicitly warns that specialized paths can opt out, that hooks are not a complete enforcement boundary, and that `write_stdin` does not invoke `PreToolUse` again for an existing exec session. Post-tool hooks cannot undo completed writes. [Official OpenAI Hooks documentation](https://learn.chatgpt.com/docs/hooks)
- App server supplies thread/command working directories, tool events, approval flows, and `turn/interrupt`. Dynamic tools are experimental. These are useful wrapper surfaces; file-change notifications are not documented as an exhaustive journal of every shell child's filesystem writes, and interruption is not evidence that every detached descendant has stopped. [Codex App Server](https://learn.chatgpt.com/docs/app-server)
- Sandbox modes distinguish read-only and workspace-write access. A writable workspace confines where supported execution can write; it does not attribute concurrent writes to proposals or provide immutable repository versions. A trial must pin the actual CLI version and inspect the supported permission configuration. [Official OpenAI sandbox documentation](https://learn.chatgpt.com/docs/sandboxing)

### The capture boundary to test

For either harness, putting commands in a private materialized view is a plausible way to capture shell side effects by comparing controlled before/after trees. A shared-directory pre/post diff cannot reliably assign a change to A if B writes during the same interval. These are engineering inferences, not vendor guarantees.

At minimum, a trial must exercise native edit, shell redirection, a formatter that rewrites several files, a generator that creates/deletes files, compilation/tests, and a long-running child process. Include relative paths, absolute paths, parent-directory traversal, symlinks, ignored/untracked files, and executable-mode/rename/delete changes. Define which source/configuration paths are managed and which generated outputs are disposable. An unexpected managed-path write must pause publication and produce an integration error; the check must not silently adopt the write.

Interruption must be followed by a known-safe capture/refresh boundary. Stop launching new writes; wait for or terminate owned running commands and verify quiescence before capturing. Resuming an interrupted agent must tell it which proposal revision and base it now has. How to establish complete process ownership and preserve tool continuity remains an unverified harness task.

## Platform options, bounded

Linux OverlayFS offers a merged lower/upper view and copy-up semantics. Its documentation states that modifying underlying filesystems while the overlay is mounted is unsupported/undefined. A lower layer pointing at a changing shared checkout therefore does **not** implement the agreed stable base; use a fixed lower tree. Extracting proposals also has to represent deletions/whiteouts and renames, not only copied-up files. No OverlayFS mount or capture integration was tested here. [Linux OverlayFS](https://docs.kernel.org/filesystems/overlayfs.html)

Apple's official APFS overview was found, but its JavaScript-only page did not expose enough retrievable content in this investigation to verify a usable repository capture API or privilege requirements. APFS cloning/snapshotting remains a storage optimization to verify on the target platform, not a chosen cross-platform dependency. [Apple APFS overview](https://developer.apple.com/documentation/foundation/about-apple-file-system)

Hardlinks are not private edit copies: they create another link to the file rather than independent contents. They therefore cannot provide draft isolation for in-place writes. A simple directory copy is the portable control for a later trial; performance measurements can justify a platform optimization afterward. [Python os.link](https://docs.python.org/3/library/os.html#os.link)

## Runnable primitive check

[workspace_capture_check.py](workspace_capture_check.py) uses Python's standard library and installed Git, creates disposable temporary files, and does not mutate the repository. Run from this research branch:

```sh
python3 docs/research/workspace_capture_check.py
```

The check was designed and run on macOS with Python 3.9.6 and `git version 2.50.1 (Apple Git-155)`. Its individual schedules are deterministic demonstrations, not load tests. Internally it invokes `git merge-file -p A BASE B`; the separated cases return zero and the competing-line case returns a positive conflict count. The run above exited zero with this exact output:

```text
PASS: naive copy-back loses A's independent edit
PASS: stale whole-file base is detectable (it also rejects harmless sibling edits)
PASS: three-way merge preserves separated edits without changing the drafts
PASS: competing edits to the same line return a merge conflict
PASS: same-owner disjoint edits can merge cleanly (text merge alone cannot enforce our owner rule)
PASS: completed directory capture stays fixed while drafts change
PASS: copying one edited file does not freeze other dependency files
PASS: copying files across an intervening draft change can produce a mixed capture
```

The check uses ordinary copy-back, not an atomic replacement test; the `os.replace` property above comes from its documentation. It does not prove symbol parsing/identity, version history, dependency completeness, real-agent attribution, sandbox enforcement, stable reads across all tools, compilation/test success, engine restarts, or durable/atomic multi-file publication. A completed quiescent copy staying fixed under subsequent draft edits is a narrower claim than consistent capture while writers are active or an enforced immutable candidate.

## Switching cost and the next human decision

**Before implementation:** this is a cheap time to investigate. There is little code to migrate, and a small capture trial can settle concrete tool boundaries. The expensive assumption to avoid is committing the protocol to raw shared pathnames before understanding the read/capture contract.

**After implementation:** swapping ordinary temporary copies for worktrees or a verified copy-on-write materialization is likely a narrower change if proposals already carry stable base identities, captured revisions, and independent candidate validation. Switching between uncontrolled shared live reads and fixed per-agent views is broader: it changes read/refresh behavior, cwd/path mappings, tool continuation, attribution, snapshot storage, and recovery. These relative costs are architectural judgments; no measured migration estimate is available.

**During active work:** a design switch needs a safe checkpoint. Preserve captured and uncaptured drafts separately, stop/finish owned writers, assign a fixed base, resume agents explicitly, and regenerate validation candidates. Simply changing directories while a shell/process is still writing is not a safe migration.

The next decision is not necessarily “worktrees or shared workspace.” It is **which first harness and capture boundary can support ordinary tools while preserving the already agreed fixed reads?** A small first trial should compare:

1. File-specific mediated editing on temporary copies, submitting retained-base changes.
2. Ordinary shell/formatter/generator work in a fixed temporary repository directory, captured at a safe boundary.
3. A temporary Git worktree as the conventional tool-compatibility control, rather than a product commitment.

Acceptance cases: independent functions in one file combine; same-owner publication requests revalidation despite a clean textual merge; whole-file fallback is symmetric; declared-dependency changes request revalidation; a multi-file group validates frozen revisions while newer drafts stay separate; and bypass writes stop publication. Record setup time, captured/unhandled writes, and workspace costs, but do not infer a general speedup from one trial.

Unknown facts that need that trial are exact adapter/tool coverage on pinned harness versions, shell descendants and background-process lifecycle, supported notification/refresh behavior during long commands, ignored/untracked path capture policy, consistent capture under ongoing editing, and recovery/publication reconciliation. These are explicit remaining investigations, not silently satisfied requirements.
