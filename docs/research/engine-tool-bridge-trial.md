# Engine tool bridge: one actual-model trial

2026-10-02. Bounded evidence for [Specify live shared-workspace coordination and safe checkpoints](https://github.com/danielrispler/Falinks/issues/11): a live team source tree with ordinary reads and engine-mediated writes. This tests an interface path, not a production engine, publication checkpoint, or performance claim.

## Finding

**The ordinary app-server path registered a source-read-only thread, denied an actual model's direct shell write, accepted one host dynamic-tool edit with an expected hash, and allowed the model to read the resulting contents.** Exactly one model turn ran on the user's default `gpt-6.1-sol`, provider `openai`; it completed and the server exited 0.

Coverage is partial. A native patch-router rejection appeared in stderr, but no path-bearing native file-change event was retained. The child-write program had a quoting error and never reached its write. Neither observation proves that every native edit or descendant is constrained in this integration.

This ordinary trusted server ran outside the command sandbox. Its thread permissions constrain applicable agent tools; this is **not whole-worker OS containment**. Earlier restrictive whole-worker startup failures are preserved separately in [the boundary report](engine-write-boundary-feasibility.md).

## Pinned interface and setup

Executable `/opt/homebrew/Caskroom/codex/0.160.0/bin/codex`, `codex-cli 0.160.0`, SHA-256 `112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b`. Installed `app-server --help` documents `--stdio`. The trial inspected locally generated experimental schemas under `inspection/ts/` and `inspection/schema/` in the original scratch checkout; these generated files are not included in this review branch. Its `ThreadStartParams` exposes `ephemeral`, `dynamicTools`, named `permissions` and runtime workspace roots; `DynamicToolCallParams` supplies thread, turn and call identifiers. The official client-tool flow is experimental. [App-server dynamic calls](https://learn.chatgpt.com/docs/app-server#dynamic-tool-calls-experimental).

Trial root: `/private/tmp/falinks-tool-bridge-u5m40wha`; source fixture: `source/source.txt`, initially `original` followed by a newline. Invocation-local settings directed runtime SQLite and logs to `worker-state` and `worker-logs`. These are documented configuration paths; `HOME`, `CODEX_HOME` and global settings were unchanged. [Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference).

```text
codex app-server --stdio
  -c analytics.enabled=false
  -c sqlite_home="<root>/worker-state"
  -c log_dir="<root>/worker-logs"
  -c default_permissions="falinks-bridge"
  -c 'permissions.falinks-bridge={extends=":workspace",filesystem={"<root>/source"="read"},network={enabled=false}}'
```

An initial launch omitted `default_permissions` and exited before initialization with an explicit configuration validation error. Adding the required default selected the same restrictive profile; grants were unchanged. Both attempts are preserved. Official permission documentation describes required default selection and specific subtree rules overriding inherited workspace/tmp grants. [Permission profiles](https://learn.chatgpt.com/docs/permissions).

The host initialized experimental API support, registered one ephemeral thread with `cwd=<root>/source`, `runtimeWorkspaceRoots=[<root>]`, `permissions="falinks-bridge"`, `approvalPolicy="never"`, and one dynamic function, `engine_edit_probe`. The single `turn/start` retained the profile and approval policy. No model override was supplied. The returned active profile was `falinks-bridge`, extending `:workspace`.

## Observed evidence

| Requested path | Observation |
| --- | --- |
| Initial ordinary read | No initial-read tool evidence retained; do not claim exercised |
| Native `apply_patch` | Router logged `patch rejected: writing outside of the project; rejected by user approval settings`; exact fixture path/payload not independently established |
| Ordinary shell write | Python attempted `source.txt` mutation; output `SHELL_DENIED 1` (`EPERM`) |
| New-session child write | Child exited 1 with `SyntaxError: EOL while scanning string literal`; write enforcement untested in this turn |
| Host dynamic edit | One accepted `engine_edit_probe`, with matching protocol thread/turn/call identity and current expected SHA-256 |
| Ordinary read after edit | Shell output `b'engine_updated\n'`, exit 0 |
| Final source | Exactly `engine_updated` followed by one newline |

The callback accepted only one fixed target, exact intended contents and the original expected hash. It wrote a temporary file outside source and replaced the fixture from the trusted host. The protocol call ID was `exec-80c57d3d-b28e-4233-b926-b6ef528a574e`; pre-edit hash `25718360e05d3c2d0963d1381e9dd4dae5fca789244ee4b9f861adcc0cc96218`; post-edit hash `75a85dcb3d59ec80a5b757e167b580f2d90a0c6d93e028fa7334f1f619fda370`.

Source content and directory entries were sampled before handling each protocol event. No unauthorized change was observed. Sampling is not an authoritative write audit and cannot exclude transient writes between observations. No MCP calls or approval requests were observed; the handler would decline approvals and grant no additional permissions. Observed protocol identity is useful session attribution, not a production authorization design.

Stderr also logged `code-mode host exited during handshake`. The turn subsequently completed, but this is a recorded integration limitation with an undiagnosed cause.

## Runnable asset and remaining boundaries

Stdlib-only [bridge.py](engine-write-boundary-assets/bridge.py) retains the small host handler and assertion-based evidence verifier. [Successful evidence](engine-write-boundary-assets/bridge-default-profile.json), [stderr](engine-write-boundary-assets/bridge-default-profile.stderr.log), and [initial configuration failure](engine-write-boundary-assets/bridge.json) preserve the observations. Selected fixture/tool events are retained; reasoning/account content was excluded. Saved-evidence verification passed:

```sh
python3 docs/research/engine-write-boundary-assets/bridge.py --verify docs/research/engine-write-boundary-assets/bridge-default-profile.json
```

For a separately authorized replay, `python3 .../bridge.py <fresh-output.json>` starts one new model turn. The runnable prompt's child quoting was simplified after this trial; that correction has **not** been exercised by another model turn. Original evidence remains unchanged.

The separate native sandbox check already observed a valid child write denied, plus rename/delete/symlink attempts. It does not fill this actual-model coverage gap. Dynamic tools add a host call path; they do not remove built-ins or prove that all built-ins honor this profile. [Sandboxing scope](https://learn.chatgpt.com/docs/sandboxing).

Still unverified: path-specific native edit denial, a valid actual-model child attempt, exhaustive descendants/aliases, other tool and platform paths, crash recovery, concurrent atomic check/write, durable attribution, privileged formatter/generator jobs, feedback snapshots and coherent publication checkpoints. The ordinary server and host filesystem APIs remain trusted; they must not be exposed as generic agent source-writing escape paths. No production controller, benchmark, second model turn, credential inspection, tracker write, merge, or main-worktree change was performed by this research agent.
