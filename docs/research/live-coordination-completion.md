# Live coordination completion: bounded evidence

2026-10-05. Evidence for [Specify live shared-workspace coordination and safe checkpoints](https://github.com/danielrispler/Falinks/issues/11). This is an isolated research artifact, not a production engine. The canonical human decision belongs in the ticket's resolution; this report separates executed observations from proposed implementation obligations.

## Selected direction and system boundary

Daniel selected retained immutable Git snapshots of completed controlled edits. Checkpoints pin a completed revision; a reusable detached validation worktree materializes the candidate afterward. This separates coherent capture from test-directory preparation and avoids a full live-tree copy for each checkpoint. A checkpoint requested while a multi-file installation is in flight can retain the previous completed revision; it cannot label the partially installed live files as the completed new revision. Clients needing that new edit wait for its explicit successful completion/revision.

The initial integration candidate is the ordinary trusted Codex app-server with host dynamic tools and a source-read-only agent permission profile. The proposed supported platform is the tested macOS setup with CLI 0.160.0, SHA-256 `112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b`. Generated protocol excerpts are retained in the assets. This beta/experimental integration requires pinning and startup capability/permission checks; compatibility with another binary/platform is not implied.

The first runtime trial and the resumed turn used the user's existing default model, observed as `gpt-6.1-sol`, without a model override. A separate restricted trial disabled apps, browser, computer, image-generation and subagent capabilities and reproduced the host-write/steering/gating sequence. A final protected trial additionally denied the controller area, reopened shared source for reads and agent scratch for writes, reproduced the same sequence, denied even opening the host obligation ledger, and verified writable scratch. The final protected profile is the selected runtime candidate; earlier broader profiles are retained as bounded source/tool evidence only. Total new actual model turns in this evidence set: **four**. A separate reconnect inspection started no model turn. No extra agents were spawned.

[Official app-server documentation](https://learn.chatgpt.com/docs/app-server) describes host dynamic calls, active-turn steering with an expected turn ID, and saved-thread resume. [Official permission documentation](https://learn.chatgpt.com/docs/permissions) describes named profiles and source subtree read grants. These APIs supply transport and sandbox mechanisms; Falinks must supply durable obligations, source-version enforcement, retained snapshots and acceptance policy. Documentation alone is not runtime evidence.

## Executed observations

| Probe | Observation | Boundary of the claim |
| --- | --- | --- |
| Completed-revision capture | A checkpoint pinned the old complete snapshot while only the first of two files was installed; after the operation completed it pinned the new snapshot. Unchanged Git blob identity was reused. | Three known text files, Python metadata lock, serialized controlled installation and Git refs. No general source-capture or crash implementation. |
| Reusable validation worktree | Signature-only candidate failed; combined candidate passed real Go checks. Separate live drafts survived. Competing cooperative lock acquisition failed. | Exact candidates were scripted; no ordinary-agent attribution or production acceptance. |
| Ordinary actual-model tools | Initial source read succeeded; direct shell and valid new-session child writes failed with EPERM. One host edit with current expected hash succeeded; ordinary reread saw its bytes. | Finite fixed-fixture cases. Sampling does not exclude transient or arbitrary bypasses. |
| Active notification | `turn/steer` accepted the active turn ID while a host tool call was pending. The model recorded explicit deferral; offer gate rejected; ordinary reread and revision-bound processing preceded eligibility. | Synthetic peer notice to one model, not two collaborating model turns; no proof of reasoning quality. |
| Fresh-server resume | Named profile and all three dynamic tools were restored from the saved thread. A synthetic deferred obligation survived host SQLite close/reopen. | Initial reconnect inspection involved no resumed model action. |
| Deferred real resume | A fresh server resumed the saved thread; the host replayed the deferred notice. Restored tools attempted an offer (rejected), reread source, recorded matching reassessment, then attempted an offer (eligible). | The event is synthetic, stored by the host; no production handled cursor or multi-agent replay implementation. |
| Native helper | An exact path-bearing patch failed in protected source; the same patch succeeded on a writable control. | Direct native helper under the OS sandbox, not a path-bearing event retained from the model's native router. |
| Protected controller state | Actual model ordinary tools could not open the host ledger in read/write mode; a separate scratch control remained writable. | One exact protected controller root with explicitly reopened source/scratch. No arbitrary process containment. |
| Eight sandbox mutation paths | Write, delete, rename, replacement, symlink creation/write, directory rename and detached-child write were denied; readable source and writable controls were verified. | Bounded macOS sandbox cases; no whole-worker containment. |

The model's native patch attempt produced a router rejection, but its path-bearing call was not retained in selected protocol or rollout evidence. Preserve that limitation: the helper check complements the router observation rather than pretending to be the missing model-native event. Unlike the earlier trial, the child program reached the write and was denied rather than failing to parse. This trial's stderr did not contain the earlier Code Mode handshake failure; that does not explain the earlier failure or establish general Code Mode support.

## Coherent capture mechanics demonstrated

The scratch capture probe builds immutable objects from known requested output, reusing unchanged blobs. It serializes actual live installation separately from the short completed-revision pointer lock. Only after all controlled output is installed does it advance that completed pointer. Capture reads and retains the existing completed identity, without scanning changing live files. A candidate based on the older identity is visibly older; it is not automatically upgraded to a later revision.

The two sample pointer reads were below 0.001 ms; retaining the Git checkpoint ref brought each complete pin operation to about 6 ms. These are **two tiny-fixture observations, not a latency benchmark or bound**. Work moves to mutation/object storage and materialization; it does not vanish. A durable engine must preserve a referenced snapshot's reachability before acknowledging capture, bind metadata to that identity, and reconcile source writes and revision records on restart.

Partial installation can be visible to ordinary live reads, which were never promised a fixed multi-file view. It must not become a completed captured revision or an accepted publication. The implementation must preserve before/after work and refuse capture of a claimed incomplete new revision. On a failed apply, restore under the controlled operation where safely possible; if a crash or mismatch prevents reliable reconciliation, retain evidence and stop mutation/publication rather than overwrite drafts or guess. No filesystem-wide atomic replacement is demonstrated.

## Worktree timing and publication race, stated precisely

The 2026-10-04 worktree probe retained known candidate commits and reused a detached test directory. Its 1,004-file fixture included 1,000 unchanged 4-KiB files. Across 15 cycles with two changed source files, median loading was 9.129 ms and restoration to the original base 8.924 ms; an unchanged fixture's inode and modification time remained stable. Initial worktree/candidate creation, checks, new-base synchronization and production metadata are excluded. This is not a controlled speedup comparison with the earlier full-copy fixture.

The scratch published ref advanced **after loading the team candidate and before starting Go checks**, while the lease was held. The test candidate stayed fixed and passed; the script reported changed-base ineligibility and performed no acceptance. Earlier descriptions of publication advancing during compiler execution were imprecise. The experiment did not test a concurrently running publication transaction.

Restoration uses the current published identity before releasing the slot. In production that identity comes from SQLite acceptance authority, not from treating a mutable Git ref as authority. Source-changing tests, ignored/untracked artifacts and dirty-slot crash recovery need implementation controls: verify source identity, allow isolated outputs, clean or quarantine a dirty slot, and never reuse it merely because the OS lock is free.

## Obligations still requiring implementation verification

- Complete snapshot, mutation and event retention/recovery; protocol representations live in the attribution/history decision.
- Atomic freshness plus relevant file/package/version checks across writes and offers; the model-host callback is a fixed synchronous case, not a production concurrency engine.
- Conservative Rust/Go relevance and reconsideration; independent-symbol application remains disabled under the captured-analysis resolution.
- Host session authentication/authorization, path and mode checks, reserved-path protection, symlink/alias handling and rejection of unsupported writers. No unrestricted host filesystem API may be exposed as an agent tool.
- Controlled formatters/generators on captured input, explicit input dependencies and coordinated all-or-none output application; stale output is preserved for reconsideration.
- Durable message/reply and handled-cursor ordering, pending/deferred replay, renewed obligations and final publication transaction. Runtime delivery is not proof that the agent understood a notice.
- Dirty validation-slot detection, isolated build outputs, source-mutating check rejection, interrupted mutation reconciliation, candidate membership and peer/dependency bindings.

These are acceptance requirements for the eventual implementation, not evidence that a production controller exists. Unexpected source changes, failed boundary probes or unsupported capabilities must prevent publication and surface an integration error. Arbitrary unmediated writers, whole-worker OS containment and untested platforms are not supported claims. The trusted host/app-server remains in the trust boundary.

## Reproduction and retained evidence

Run the no-inference check:

```sh
python3 docs/research/coordination-completion-assets/verify_evidence.py
```

It verifies saved bounded observations, including explicit limitations. It does not rerun source enforcement or manufacture production proof. `capture_probe.py` and `worktree_probe.py` run isolated scratch Git fixtures. `native_boundary_probe.py` and `sandbox_probe.py OUTPUT.json` exercise local sandbox paths without inference. Runtime scripts start real model turns and use the existing Codex login/default model; run them only when explicitly requested. `resume_probe.py` reconnects without inference; it expects an existing runtime result and a fresh synthetic-event entry. `deferred_resume_probe.py` replays a synthetic deferred event in a real resumed turn. Each model probe retains only fixture tool evidence and protocol identities, not reasoning/account events.

All native observations, selected stderr, generated protocol excerpts and the pinned binary manifest are adjacent in `coordination-completion-assets/`. These artifacts are research evidence; no scheduler, publication engine, benchmark suite or production Rust module was added.

## Full-ticket learning artifact

The explicitly requested explanation was refreshed at `/Users/danielrispler/code-explanations/Falinks/2026-10-05-live-coordination-completion.html`. It contains eight guided steps, five archived source passages, completed-versus-in-flight and publication-base traces, and four HTML knowledge checks. Mechanical validation and real headless-browser checks at desktop 1440 px and mobile 390 px passed, including all lesson layouts, expanded previews, custom traces and one-question paging; screenshots were visually inspected. Audible narration was not tested. The earlier worktree explanation is preserved. Learning completion is Daniel’s self-directed readiness, not an independently administered chat quiz.
