# Cross-platform sandbox backends (Linux, native Windows)

Issue [#34](https://github.com/danielrispler/Falinks/issues/34), 10 October 2026. This is research evidence and a recommendation. No production backend is built here.

## Question

Which mechanism per OS can uphold the **sandbox contract** that captured jobs get today from Seatbelt (`execute_job` and `sandboxed()` in `src/lib.rs`)? The six properties are:

1. No network.
2. Reads only from declared inputs plus system tool/runtime reads.
3. Writes only beneath `output/`.
4. Cleared environment and a fixed input/output layout.
5. Descendants kept in an assigned process tree; survivors and timeouts are killed and reported.
6. Fail closed when the sandbox cannot be installed.

The answer must work without root and stay Rust/syscall-level where possible.

## Answer

- **Linux: Landlock + seccomp + subreaper, all in-process Rust.**
  - **Landlock** restricts the filesystem. It runs with `CompatLevel::HardRequirement` at a minimum ABI.
  - **A seccomp filter** denies `socket()` for **every** family and denies `io_uring_setup`. Landlock alone does not stop UDP, and neither Landlock nor a network namespace stops pathname UNIX sockets that lead to host daemons.
  - **The launcher is a `PR_SET_CHILD_SUBREAPER`.** It sweeps and kills adopted descendants, because the existing process-group check misses `setsid` escapes.
  - This combination upheld all six properties on every probed kernel, including Ubuntu 24.04's default AppArmor user-namespace restriction and hosts with user namespaces disabled. It needs no root, no namespaces and no external binary.
- **Native Windows: feasible with AppContainer (zero capabilities) + Job Object, at real cost.** The probes upheld properties 1–6. The cost is hand-written `windows-sys` FFI, persistent ACL grants, environment quirks and a redefined file model (see [Native Windows cost](#native-windows-cost)). This is not committed to.
- **Rejected:**
  - **bubblewrap** fails without root on stock Ubuntu 24.04 and is an external C binary.
  - **User/network/PID namespaces** are blocked by the same AppArmor policy.
  - **Landlock alone** leaks UDP and pathname UNIX sockets.
  - **LPAC** breaks Winsock and child creation.

## Setup

- **Probe:** `adapters/sandbox-backends-probe`, a standalone throwaway Rust crate. One binary plays three roles:
  - `host` builds a fresh fixture per run (`input/`, `output/`, `secret/`, `evidence/`), launches the child under a mechanism, enforces the timeout and records the result.
  - `wrap` (Linux) installs a mechanism in its own single-threaded process, then `exec`s. This is the shape of an in-process Rust backend.
  - `child` attempts each violation and records the exact error.
- **Launch path:** the host keeps the engine's launch path: `env_clear()` with fixed `PATH`/`HOME`/`TMPDIR`, a dedicated process group, `SIGKILL` of the group at the timeout, and the "survivor in the group" check. It also sets a canary environment variable in itself and is a subreaper.
- **Checks:**
  - Read input, the secret, the user's home and the fixture root.
  - Write output, input, the secret and `/tmp`; rename input into output.
  - TCP to `1.1.1.1:443`, TCP loopback to a host listener, TCP bind.
  - Raw UDP DNS to `8.8.8.8:53`, resolver DNS.
  - Pathname UNIX connects to systemd-resolved, D-Bus and journald, plus an abstract UNIX connect to a host listener.
  - Opening the null device.
  - A `setsid`/detached descendant that writes a heartbeat file into `output/`. The heartbeat lets the host detect it across PID namespaces.
  - The hang scenario exceeds the timeout. The probe uses 15 s instead of 30 s; the kill path is the same.
  - The tool scenario runs the real `gofmt -l input/`.
- **Unavailable scenarios:**
  - **Simulated old kernel:** a seccomp filter makes `landlock_create_ruleset` return `ENOSYS`.
  - **Simulated disabled user namespaces:** `unshare` returns `EPERM`.
  - **Real disablement:** the workflow reruns everything with `sudo sysctl user.max_user_namespaces=0` and with `kernel.apparmor_restrict_unprivileged_userns=1`.
  - **Real old kernel:** the 6.8 kernel (ABI 4) runs `landlock-v6`.
- **Runners:** workflow `.github/workflows/sandbox-probes.yml` (not a required check), run [38057671207](https://github.com/danielrispler/Falinks/actions/runs/38057671207). Reports are in `adapters/sandbox-backends-probe/evidence/2026-10-10/`. Earlier runs [38057076705](https://github.com/danielrispler/Falinks/actions/runs/38057076705) and [38057438835](https://github.com/danielrispler/Falinks/actions/runs/38057438835) found the probe bugs and gaps that led to the final checks.

| Runner | OS | Kernel | Landlock ABI | `apparmor_restrict_unprivileged_userns` | bwrap |
|---|---|---|---|---|---|
| `ubuntu-latest` | Ubuntu 24.04.5, x86_64 | 6.17.0-1022-azure | 7 | 1 (default) | 0.9.0 |
| `ubuntu-24.04-arm` | Ubuntu 24.04.5, aarch64 | 6.17.0-1022-azure | 7 | 1 (default) | 0.9.0 |
| `ubuntu-22.04` | Ubuntu 22.04.5, x86_64 | 6.8.0-1064-azure | 4 | 0 (default) | 0.6.1 |
| `windows-latest` | Windows 10.0.26100, elevated `runneradmin` | — | — | — | — |

Documentation findings with primary-source citations are in [sandbox-backends-sources.md](sandbox-backends-sources.md). Where the two disagree, probe results take precedence.

## Linux findings

The mechanisms probed (`linux.rs`):

- `landlock`: ABI 4 hard requirement. Filesystem reads are limited to `/usr /bin /lib /lib64 /sbin /etc /proc /opt`, the executable and `input/`. Writes are limited to `output/`. Handling TCP with no rules denies all TCP.
- `landlock-v6`: the same, plus abstract-socket and signal scoping.
- `landlock-seccomp`: `landlock`, plus `socket()` allowed only for `AF_UNIX`, and `io_uring_setup` denied.
- `landlock-seccomp-strict`: `landlock`, plus every `socket()` denied and `io_uring_setup` denied.
- `userns`: `CLONE_NEWUSER|NEWNET|NEWPID` plus `landlock`. The job is PID 1 of the new namespace.
- `bwrap`: `--unshare-all --die-with-parent --new-session --clearenv`, with read-only binds for system paths and input, and a writable bind for output.

### Per-property results

Results are from `ubuntu-latest` unless noted. Every `ran` row behaved the same on all three Linux runners wherever the mechanism could run.

| Check | none | landlock | landlock-seccomp | landlock-seccomp-strict | userns (22.04 only) | bwrap (22.04 only) |
|---|---|---|---|---|---|---|
| TCP internet / loopback / bind | ok / ok / ok | EACCES ×3 | EPERM ×3 | EPERM ×3 | EACCES ×3 | unreachable / refused / ok (own netns) |
| UDP DNS to 8.8.8.8 | reply | **reply** | EPERM | EPERM | unreachable | unreachable |
| Pathname UNIX: resolved, D-Bus | ok | **ok** | **ok** | EPERM | **ok** | absent (not bound) |
| Abstract UNIX to host | ok | **ok** (`landlock-v6`: EPERM) | **ok** | EPERM | refused | refused |
| Read secret / list home / list fixture root | ok | EACCES | EACCES | EACCES | EACCES | ENOENT / empty dir / skeleton only |
| Write input / secret / `/tmp`; rename input→output | ok | EACCES | EACCES | EACCES | EACCES | EROFS / ENOENT / ok (private tmpfs) / EXDEV |
| Read input, write output | ok | ok | ok | ok | ok | ok |
| Environment | `HOME PATH TMPDIR`, no canary | same | same | same | same | plus `PWD` |
| `setsid` descendant alive after the group kill | **yes** | **yes** | **yes** | **yes** | no (namespace torn down) | no |
| ... after the subreaper sweep | no | no | no | no | — | — |
| Hang: killed at timeout, no survivors | — | yes | — | — | yes | yes |
| Real `gofmt -l input/` | ok | — | — | ok | — | — |

Reading the table:

- **"No network" needs more than Landlock or a network namespace.** Before ABI 10 (7.2), Landlock lets UDP through: DNS got a reply. Before ABI 9 (7.1), it lets pathname UNIX sockets through, and so does a network namespace. A job could ask systemd-resolved or D-Bus to use the network on its behalf. Codex rejected Landlock for the same reason ([sources](sandbox-backends-sources.md#seatbelt-and-what-other-projects-use)).
  - Only the strict seccomp filter blocked every channel on every kernel.
  - Denying all `socket()` calls still lets `socketpair` and pipes through. The real formatter (`gofmt`) ran unaffected.
- **Process groups are not containment.** This is true on every mechanism, including today's model. A descendant that calls `setsid` survives `kill(-pgid)` and is invisible to the `kill(-pgid, 0)` survivor check.
  - A subreaper host adopted it (its `ppid` was the host's pid), found it in `/proc/self/task/*/children`, and killed it.
  - PID namespaces and bwrap also killed it, because the namespace died when its init exited. Both depend on user namespaces.
- **Fail closed.** Every unavailable case refused to start the job and kept the reason in `stderr`:
  - Simulated missing Landlock: `fully incompatible access-rights`.
  - `landlock-v6` on the real 6.8 / ABI 4 kernel: `partially incompatible access-rights … IoctlDev`.
  - Simulated or real user-namespace disablement: `unshare: Operation not permitted` / `No space left on device`.
  - None of them fell back to running unrestricted. For Landlock this depends on `HardRequirement`. The crate defaults to best effort ([sources](sandbox-backends-sources.md#landlock-linux-lsm)).
- **User namespaces are not available by default on Ubuntu 24.04.** With the AppArmor restriction on, which is the default on both 24.04 runners:
  - `unshare(CLONE_NEWUSER|…)` succeeds, then writing `/proc/self/setgroups` fails with `EACCES`.
  - bwrap fails with `loopback: Failed RTM_NEWADDR: Operation not permitted`.
  - On 22.04 with the restriction turned on, `unshare` itself fails with `EACCES`.
  - Both mechanisms work only on 22.04 with the restriction off (its default).
  - So namespaces and bubblewrap would make root (an AppArmor profile) a prerequisite on the most common current distribution.
- **No root needed:** every Landlock/seccomp variant ran as uid 1001 under all three sysctl configurations.

### Requirements of the recommended Linux backend

- **Kernel:**
  - Landlock in the LSM list (default on Ubuntu 22.04+, Debian 12, Fedora, CentOS Stream 9; [sources](sandbox-backends-sources.md#landlock-linux-lsm)).
  - Seccomp filter support (universal on these kernels).
- **Minimum Landlock ABI: 3 (kernel 6.2), recommended.** ABI 3 is the first that controls `truncate`. Below it, a job could truncate files it can only read.
  - TCP rules (ABI 4) are redundant once seccomp denies sockets.
  - This leaves Debian 12 (6.1, ABI 2) and stock RHEL 9 (5.14) failing closed as unsupported.
  - The probes required ABI 4. ABI 3 is a documented inference, not a probed configuration.
- **Signal scoping (ABI 6, 6.12)** also stops a job from signalling other processes of the same user. Seatbelt's `(allow default)` does not deny signals either, so this hardening goes beyond parity. Whether to require it is the supported-platforms decision (#35).
- **Process tree:**
  - A per-job launcher that is a subreaper, `fork`/`exec`s the tool, and on tool exit or timeout sweeps all adopted descendants, reporting any as ambiguous completion.
  - The engine keeps the group kill, and itself as subreaper, as a backstop.
  - The sweep can race with a descendant that is still forking. Production code should loop until no children remain (unprobed).
  - cgroup v2 `cgroup.kill` is the race-free alternative. `systemd-run --user --scope` worked on the runners, but kill semantics through it were not probed.
- **Rust-only:** `landlock` crate 0.4.7 plus raw `libc` seccomp BPF (`linux.rs`, about 50 lines). Filters also refuse x32 syscall numbers on x86_64 so the rules cannot be bypassed that way.

## Native Windows findings

Mechanisms (`windows.rs`):

- `job`: a Job Object with `KILL_ON_JOB_CLOSE` and no breakaway. The process starts inside it through `PROC_THREAD_ATTRIBUTE_JOB_LIST`.
- `appcontainer`: `job` plus an AppContainer profile with zero capabilities, launched with `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`. ACL grants give the container SID read+execute on a private `bin/` copy of the probe, read on `input/`, and full access on `output/`.
- `lpac`: `appcontainer` with the ALL APPLICATION PACKAGES opt-out.

| Check | none | job | appcontainer | lpac |
|---|---|---|---|---|
| TCP internet | ok | ok | `WSAEACCES` (10013) | Winsock init fails |
| TCP loopback to host | ok | ok | timed out | ″ |
| TCP bind | ok | ok | **ok** (inbound needs a capability) | ″ |
| UDP DNS to 8.8.8.8 | reply | reply | send ok, **no reply** (10060) | ″ |
| Resolver DNS | ok | ok | `11001` host not known | ″ |
| Read secret / list home / list fixture root | ok | ok | access denied | access denied |
| Write input / secret / `C:\Users\Public` | ok | ok | access denied | access denied |
| Read input, write output | ok | ok | ok | ok |
| Open `NUL` | ok | ok | **access denied** | access denied |
| Breakaway descendant | detached, survives | breakaway refused; counted in `ActiveProcesses`, killed by the job | same as job | child creation denied |
| Hang: killed at timeout | — | yes | yes | — |
| Profile creation fails | — | — | launch refused, process never created | — |

- **The process-tree contract is cleaner than on Unix.** `CREATE_BREAKAWAY_FROM_JOB` was refused with access denied. The detached descendant stayed in the job, `ActiveProcesses` reported it after the main process exited, and `TerminateJobObject` killed it. This is the direct replacement for `kill(-pgid, 0)` plus `SIGKILL`.
- **AppContainer blocked TCP, loopback and resolver DNS.** UDP is silently dropped rather than refused. Binding a listening socket succeeds, but nothing can reach it without a capability.
- **AppContainer creation needs `LOCALAPPDATA`/`APPDATA` in the environment block.** The minimal block failed `CreateProcessW` with error 203 (`ERROR_ENVVAR_NOT_FOUND`) and the second block succeeded (`extra.env_attempts`). So "cleared environment" on Windows means a fixed, slightly larger set.
- **`NUL` is denied inside the container**, so tools that spawn children with null stdio fail there. Rust's `Stdio::null()` hit exactly this. A backend must pass real handles.
- **LPAC is unusable for general tools:** `WSAStartup` fails with 10107, which makes Rust std panic on the first socket call, and creating child processes is denied.
- **Not verified:** behaviour under a non-elevated user (the runner token is elevated). AppContainer does not need elevation, but the probes ran only as administrator.

### Native Windows cost

| Area | Change needed |
|---|---|
| Launch | Hand-written `windows-sys` FFI (the probe needed about 590 lines with 35 `unsafe` sites): profile, ACLs, attribute list, job. Rust std cannot pass attribute lists on stable ([sources](sandbox-backends-sources.md#native-windows)). |
| File access | Persistent ACL grants to the container SID on `input/`, `output/` and the tool directory, removed after the job. Seatbelt's grants applied to one launch only. Enrolled tools outside system directories need a grant or a copy, as the probe did. |
| Environment | Fixed set must include `LOCALAPPDATA`/`APPDATA` (error 203 otherwise); stdio must be real handles, never `NUL`. |
| Modes 0644/0755 | Windows has no execute bit. Enrollment must record "executable" as metadata rather than a file mode. |
| Single-link regular files | `nNumberOfLinks` through `GetFileInformationByHandle`. `MetadataExt::number_of_links` is nightly-only. |
| `flock` writer leases | `LockFileEx` is mandatory and also blocks reads by other handles. The lease must lock a dedicated lock file, never a data file (`File::lock` maps to it). |
| Process groups | Replaced by Job Objects (probed above). |

## Seatbelt deprecation risk

`sandbox-exec(1)`, `sandbox_init(3)` and `sandbox.h` have all said "deprecated" since macOS 10.8. They still ship and work on macOS 26.6.2, and Chromium, Bazel, Codex and Claude Code all depend on them ([sources](sandbox-backends-sources.md#seatbelt-and-what-other-projects-use)). No removal date is published. The documented replacement, App Sandbox, is an entitlement of a signed app, not a per-launch profile, so there is no drop-in fallback. Recommendation:

- Keep Seatbelt.
- The macOS CI job is the detector: removal fails the existing captured-job tests.
- If it is removed, macOS fails closed until a new backend lands.
- Inference, not probed on macOS: the `setsid` escape found here is POSIX process-group behaviour. It applies to today's macOS backend too. A macOS subreaper equivalent does not exist, so the macOS answer is a follow-up question (for example, `kqueue` `NOTE_EXIT`/`NOTE_TRACK` or tracking the session).

## Worker runtimes

- **Codex** now defaults to bubblewrap on Linux (system or bundled) with seccomp. Its Windows backends do not use plain AppContainer.
- **Claude Code** uses bubblewrap on Linux and WSL2. It runs commands unsandboxed on native Windows and fails closed only with `sandbox.failIfUnavailable: true`.
- Both inherit the Ubuntu 24.04 user-namespace restriction seen here. Adapter pins on Linux must document that prerequisite, or the AppArmor profile, separately from the engine backend, which does not need it.

Sources for this section: [sources](sandbox-backends-sources.md#seatbelt-and-what-other-projects-use).

## Limitations

- Probes ran on GitHub-hosted Azure kernels only. Other distributions' defaults come from documentation.
- The kernels tested (6.8, 6.17) are ABI 4 and 7. ABI 9/10 behaviour (pathname UNIX, UDP in Landlock) is documented, not probed.
- The Windows probes ran elevated.
- cgroup kill and the sweep race were not probed.
- The probe is research code. It is not hardened against hostile programs, consistent with the contract's bounded trusted-tool scope.
