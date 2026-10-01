# Coupled-workspace trial: primary-source boundaries

2026-10-01. Companion reading for the bounded coupled-workspace experiment. Sources were opened directly; this note distinguishes documented behavior, earlier observations, and experimental questions. It makes no workspace architecture decision. All local actions for this reading were confined to the isolated research checkout or were read-only.

## Codex execution and evidence

`codex exec --json` emits a JSONL stream containing thread, turn, item, and error events. Saved CLI authentication is reused by default. `--output-last-message` saves the final response, and `--output-schema` constrains that response; neither makes a readiness statement authoritative. The documented automation default is read-only; allow edits explicitly with `--sandbox workspace-write`. [Official non-interactive documentation](https://learn.chatgpt.com/docs/non-interactive-mode).

`-C` selects the workspace root. `--ephemeral` suppresses persisted session rollout files; it does not document filesystem immutability, descendant termination, or exhaustive write attribution. `--ignore-user-config` skips the user configuration file while retaining authentication. [Official command reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli).

Local read-only checks returned `codex-cli 0.159.3`. Its `codex exec --help` exposes `--json`, `--ephemeral`, `--ignore-user-config`, `--ignore-rules`, `-C`, `--worktree`, and the three sandbox modes. Help describes `--worktree` as creating a managed Git worktree; this note does not experimentally verify that option. The installed version matches the earlier capture trial, allowing comparison without claiming CLI versions are interchangeable.

The configuration reference exposes `sandbox_workspace_write.exclude_slash_tmp`, `exclude_tmpdir_env_var`, and additional `writable_roots`, as well as `network_access`. `approval_policy = "never"` is supported for non-interactive work and prevents approval prompts; it does not remove the selected sandbox. [Official configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference).

For fixture worktrees under `/private/tmp`, sibling-write denial is informative only after broad temporary-directory grants have been excluded. Otherwise a write can be allowed by the temporary root even though it lies outside the selected checkout. This is an inference from the documented grants, to verify with a harmless sibling canary; it is not proof that a particular run escaped its sandbox.

The workspace-write policy protects `.git` recursively, including a linked worktree's pointer file and resolved Git directory. `.agents` and `.codex` directories are also protected. Temporary directories are normally in the workspace, and sandboxing and approvals are separate controls. [Official approval and security documentation](https://learn.chatgpt.com/docs/agent-approvals-security#protected-paths-in-writable-roots).

Consequently, an inner agent may edit fixture source while being unable to stage or commit it. Researcher-created commits after capture are a distinct harness action and should be recorded as such. An outer launch escalation does not, by itself, establish which inner sandbox boundary actually applied.

Sandbox restrictions apply to spawned commands such as Git and test runners, using platform enforcement; macOS uses Seatbelt. This documents an access boundary, not a snapshot or a capture protocol. [Official sandbox documentation](https://learn.chatgpt.com/docs/sandboxing).

## Child writes, hooks, and cancellation

Tool hooks cover shell/unified exec and native patch operations. `write_stdin` can deliver the original command's post-hook on completion but does not rerun the pre-hook. Some specialized paths opt out; the documentation explicitly declines to describe hooks as a complete enforcement boundary. Hook inputs contain tool-call identity and command data, and a post-hook cannot undo side effects. [Official hooks documentation](https://learn.chatgpt.com/docs/hooks#tool-coverage).

Inference: one shell event can encompass a generator and its descendants, so command event identity alone is insufficient to assign every resulting write or prove that writers are quiescent. A subprocess can continue after its launcher returns; the trial must observe the specific lifecycle rather than infer it from a completed tool event.

App-server documents `turn/interrupt` cancellation with final status `interrupted`. Filesystem watching emits `watchId` and changed paths and is described for state invalidation. Its documented watch payload does not identify the writer. Neither description establishes atomic rollback or descendant-process quiescence. [Official app-server documentation](https://learn.chatgpt.com/docs/app-server).

The bounded child-process check can therefore answer a narrow question: after an observed launcher/tool/turn boundary, did a known child write later, and did candidate bytes change? Preserve PID/lifecycle evidence and a later file observation. A successful termination of one process is not an exhaustive test of all descendant behaviors.

## Git views and exact candidates

Git linked worktrees separate working files, `HEAD`, and index, while sharing most repository data. Ordinary branch refs and default configuration are shared; `git rev-parse --git-path` resolves the appropriate metadata paths. `git worktree lock` prevents pruning/moving/removing the worktree; it does not lock its contents against editing. [Official worktree documentation](https://git-scm.com/docs/git-worktree).

Inference: one agent's source edits in its own worktree need not appear in another's ordinary file reads, but worktrees alone are not a security boundary or independent repository administration. A fixed checkout can support the stable-read baseline only under the stated exclusive-writer assumption; file searches, imports, generators, and tests should all use that checkout.

For a separate scratch clone, `--no-hardlinks` copies local Git objects instead of hardlinking them. The Git manual also warns that a local clone can race concurrent source-repository changes. [Official clone documentation](https://git-scm.com/docs/git-clone).

Git's `<rev>:<path>` syntax addresses a blob or tree in a named revision. Use captured full object IDs when comparing candidate content; a branch name can move between commands. [Official revision documentation](https://git-scm.com/docs/gitrevisions).

`git merge-tree --write-tree` computes a merge using ordinary merge features without reading/writing working files or index, and returns a tree object. In ordinary invocation, exit 0 means clean, 1 means conflicts, and other codes mean errors. An empty conflicted-file list does not prove a clean merge; check the exit code. [Official merge-tree documentation](https://git-scm.com/docs/git-merge-tree).

Inference: merge success establishes syntactic Git reconciliation, not owning-symbol independence, declared dependency validity, or test correctness. In the trial, retain A-only, B-only, and joint tree/object identities and test each exact candidate. Excluding an unfinished B draft should be checked against complete expected bytes and required group membership, rather than merely checking that a selected A hunk is present.

`git update-ref <ref> <new-oid> <old-oid>` changes a ref only when its current value equals the expected old value. Reference transactions lock refs, but concurrent readers can observe a subset of multi-ref changes. [Official update-ref documentation](https://git-scm.com/docs/git-update-ref).

A scratch expected-base rejection check can demonstrate this primitive independently of merge/test success. It does not implement the Falinks publication workflow or choose its architecture.

## What the earlier capture trial leaves for this experiment

The immutable [Codex capture trial](https://github.com/danielrispler/Falinks/blob/5c524b33d965a8cfc4e5d336d4fec4b6b44273d3/docs/research/codex-capture-trial.md) was read through GitHub's contents API at the linked commit. It reported:

- Exact A-only capture in isolated repository copies and a narrow native-diff replay, with unfinished B content excluded from the captured candidate.
- Sequential model runs; later draft changes and the active writer were researcher-controlled.
- Tool events/hooks that did not establish exhaustive shell or child-write attribution.
- No proof of stable whole-repository reads, snapshot consistency during arbitrary concurrent writes, descendant quiescence, or filesystem immutability.

The coupled-workspace trial can close bounded gaps by measuring actual overlapping Codex sessions, recording ordinary searches/imports/tests in their views, retaining complete generated groups, comparing single and joint test outcomes, and observing a detached child through and beyond the candidate-capture boundary. State separately which properties passed in the fixture and which remained untested. A fast result in a tiny fixture is a measurement, not a general throughput claim. Joint success demonstrates a coordination opportunity; any willingness to wait or choice of workspace architecture remains the human's decision.
