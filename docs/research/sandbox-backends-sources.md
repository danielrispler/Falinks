# Sandbox backends: what primary sources say

Issue [#34](https://github.com/danielrispler/Falinks/issues/34), 10 October 2026. This file covers documentation only. Probe evidence from real CI runners is recorded separately and takes precedence where the two disagree. A claim with no primary source is marked **unverified**.

## Question

Which in-kernel or OS mechanisms on Linux and native Windows can uphold the six-property sandbox contract that the Seatbelt captured-job path uses today (`execute_job` and `sandboxed()` in `src/lib.rs`)? What do their owners document about versions, gaps, privilege requirements and failure modes?

The current macOS contract: a `(deny network*)` / `(deny file-read*)` / `(deny file-write*)` profile, with reads allowed under `/`, system paths, `input/` and `output/`, and writes allowed only under `output/`. The child also runs with `env_clear()` plus a fixed `PATH`/`HOME`/`TMPDIR`. It gets its own process group, which receives `SIGKILL` at a 30 s timeout or when any member survives the leader. Any other OS fails closed.

## Landlock (Linux LSM)

**ABI versions and kernel releases.** The kernel version for each ABI was checked by looking for each flag in `include/uapi/linux/landlock.h` at the [torvalds/linux](https://github.com/torvalds/linux/blob/master/include/uapi/linux/landlock.h) release tags. The `landlock` crate's [`ABI` enum](https://docs.rs/landlock/latest/landlock/enum.ABI.html) agrees up to V9.

| ABI | Kernel | Adds |
|---|---|---|
| 1 | 5.13 | Filesystem access rights |
| 2 | 5.19 | `ACCESS_FS_REFER` (rename and link across directories) |
| 3 | 6.2 | `ACCESS_FS_TRUNCATE` |
| 4 | 6.7 | `ACCESS_NET_BIND_TCP`, `ACCESS_NET_CONNECT_TCP` |
| 5 | 6.10 | `ACCESS_FS_IOCTL_DEV` |
| 6 | 6.12 | `SCOPE_ABSTRACT_UNIX_SOCKET`, `SCOPE_SIGNAL` |
| 7 | 6.15 | Audit logging flags for `landlock_restrict_self` |
| 8 | 7.0 | `RESTRICT_SELF_TSYNC` (multithreaded enforcement) |
| 9 | 7.1 | `ACCESS_FS_RESOLVE_UNIX` (connecting to pathname UNIX sockets) |
| 10 | 7.2 | `ACCESS_NET_BIND_UDP`, `ACCESS_NET_CONNECT_SEND_UDP` |
| 11 | in `master`, not yet tagged | `RESTRICT_SELF_NO_NEW_PRIVS` |

The feature list for each ABI is in the [kernel userspace-api doc](https://docs.kernel.org/userspace-api/landlock.html) ([source](https://github.com/torvalds/linux/blob/master/Documentation/userspace-api/landlock.rst)).

**What Landlock does not cover:**
- **Network:** only TCP and UDP ports. The [uapi `net_access` doc](https://github.com/torvalds/linux/blob/v7.2/include/uapi/linux/landlock.h) names those two protocols and nothing else.
  - UDP was unrestricted until 7.2, so DNS over UDP passes on every kernel before 7.2.
  - The docs do not mention raw, ICMP, SCTP, netlink, vsock or other socket families, so those are **outside Landlock's network scope** (inferred from that absence).
- **Pathname UNIX sockets:** before ABI 9, a job could connect to them. That includes `systemd-resolved`, D-Bus and other host daemons. This is an inference from ABI 9 being the one that introduces the right.
- **Abstract UNIX sockets and signals** can be restricted only from ABI 6.
- **Syscall families it cannot restrict:** `chdir`, `stat`, `flock`, `chmod`, `chown`, `setxattr`, `utime`, `fcntl` and `access`. Pipes and sockets reached through `/proc/<pid>/fd/*`, and nsfs, are not explicitly restricted; ptrace domain rules protect them instead. `chroot` is not denied. Source: the "Current limitations" section of the [kernel doc](https://docs.kernel.org/userspace-api/landlock.html).
- **Reads of `/proc`:** these are ordinary path opens and fall under the filesystem rules. The contract's read allow-list must therefore grant whatever the tools need from `/proc`, `/sys` and `/dev`.
  - **Unverified**, inferred because those paths are not listed as exceptions.

**Privileges.** A process without privileges must set `no_new_privs`. Without `CAP_SYS_ADMIN`, `landlock_restrict_self` requires it ([kernel doc](https://docs.kernel.org/userspace-api/landlock.html)). Restrictions are inherited by children and cannot be undone ("only adding more restrictions is allowed").

**Detection.**
- `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)` returns the ABI, or an error when Landlock is not built or not enabled.
- Landlock must be listed in `CONFIG_LSM` or the `lsm=` boot parameter.
- Source: [kernel doc](https://docs.kernel.org/userspace-api/landlock.html).

**The Rust crate is best-effort by default.** In [`landlock` 0.4.7](https://docs.rs/landlock/latest/landlock/), the ruleset quietly uses "the intersection of the currently running kernel's features and those required by the caller".
- [`CompatLevel::HardRequirement`](https://docs.rs/landlock/latest/landlock/enum.CompatLevel.html) "returns a compatibility error" when a requested right is unsupported.
- The crate docs say to assert `RestrictionStatus { ruleset: FullyEnforced, no_new_privs: true }`.
- Fail-closed design: `set_compatibility(HardRequirement)`, plus an explicit check of that status.
- The crate stops at ABI V9 and does not yet expose the UDP rights.

**Enabled by default in distribution kernels** (whether `landlock` is in `CONFIG_LSM`):

| Distribution | Landlock in `CONFIG_LSM` | Source |
|---|---|---|
| Ubuntu 22.04 and later | Yes | Prepended for jammy in [LP #1953192](https://launchpad.net/bugs/1953192). The current Ubuntu config line is `"landlock,lockdown,yama,integrity,apparmor"` ([kernel-team patch, Sept 2026](https://lists.ubuntu.com/archives/kernel-team/2026-September/171490.html)). |
| Debian 12 | Yes | [salsa `debian/6.1/bookworm` config](https://salsa.debian.org/kernel-team/linux/-/raw/debian/6.1/bookworm/debian/config/config) |
| Fedora | Yes | [rawhide `kernel-x86_64-fedora.config`](https://src.fedoraproject.org/rpms/kernel/raw/rawhide/f/kernel-x86_64-fedora.config) |
| RHEL 9 | Yes in CentOS Stream 9; RHEL itself **unverified** | [CentOS Stream 9 `CONFIG_LSM`](https://gitlab.com/redhat/centos-stream/src/kernel/centos-stream-9/-/raw/main/redhat/configs/common/generic/CONFIG_LSM) |

RHEL 9 runs a 5.14-based kernel, so even with Landlock enabled it may have no network rights. Which features Red Hat backported is **unverified**.

## User namespaces and an empty network namespace

- **A new network namespace isolates the network stack.** It gets its own devices, protocol stacks, routes, ports, and its own abstract UNIX socket namespace ([network_namespaces(7)](https://git.kernel.org/pub/scm/docs/man-pages/man-pages.git/plain/man/man7/network_namespaces.7)).
  - In an empty namespace, TCP, UDP and DNS to any outside address have no route.
  - The new namespace has its own loopback, so the host's `127.0.0.53` resolver is unreachable. bwrap docs: "only a loopback device" ([bubblewrap README](https://github.com/containers/bubblewrap/blob/main/README.md)).
  - **Pathname** UNIX sockets are filesystem objects, and a network namespace does not isolate them. A job that can open `/run/systemd/resolve/io.systemd.Resolve` or `/run/dbus/system_bus_socket` can still ask a host daemon to use the network for it.
    - This is inferred from the man page: only the abstract namespace is listed as isolated. The filesystem rules must close this gap.
- **Creating a network namespace without privileges** needs `CAP_SYS_ADMIN` in the owning user namespace. A process that creates a user namespace "gains a full set of capabilities in that namespace" ([user_namespaces(7)](https://git.kernel.org/pub/scm/docs/man-pages/man-pages.git/plain/man/man7/user_namespaces.7)).
- **Ubuntu 23.10 and later:**
  - `kernel.apparmor_restrict_unprivileged_userns` allows unprivileged user namespaces only for processes whose AppArmor profile has a `userns,` rule ([Ubuntu blog](https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces), [spec](https://discourse.ubuntu.com/t/spec-unprivileged-user-namespace-restrictions-via-apparmor-in-ubuntu-23-10/37626)).
  - The exact mechanism is in AppArmor's [`unprivileged_userns`](https://gitlab.com/apparmor/apparmor/-/raw/master/profiles/apparmor.d/unprivileged_userns) profile. Its own comment says it is "transitioned to by unconfined when creating an unprivileged user namespace", and it contains `audit deny capability`.
  - So `unshare(CLONE_NEWUSER)` itself can succeed while the new namespace has **no capabilities**. A following `CLONE_NEWNET`, mount or `pivot_root` then fails.
    - This combines the profile text with user_namespaces(7). The exact errno is **unverified** and needs a probe.
  - Ubuntu 24.04 ships `unprivileged_userns` in `/etc/apparmor.d` ([noble apparmor filelist](https://packages.ubuntu.com/noble/amd64/apparmor/filelist)).
  - That this restriction is on by default in 24.04 comes from the Canonical blog and the [Claude Code docs](https://code.claude.com/docs/en/sandboxing). No Canonical release note was fetched; **probe it**.
- **Debian:** `kernel.unprivileged_userns_clone` is a Debian-only sysctl. "The default is 1 for all modern Debian and Ubuntu kernels" ([Debian bubblewrap README.Debian](https://metadata-backend.ftp-master.debian.org/changelogs/main/b/bubblewrap/stable_README.Debian)).
- **RHEL:** `user.max_user_namespaces` limits creation. A value of 0 disables it. That RHEL 8 and 9 default to non-zero is **unverified** (no Red Hat doc fetched).

## seccomp-bpf as the network blocker

- **Installing a filter without privileges** requires `prctl(PR_SET_NO_NEW_PRIVS, 1)` or `CAP_SYS_ADMIN`; otherwise the call returns `-EACCES` ([seccomp_filter.rst](https://github.com/torvalds/linux/blob/master/Documentation/userspace-api/seccomp_filter.rst)).
- **Denying `socket()` by family works** because the family is the integer first argument, so a classic BPF filter can match on it. A filter that allows only `AF_UNIX` then blocks TCP, UDP, raw, netlink and so on.
  - Not covered: inherited file descriptors, `socketpair`, and `AF_UNIX` sockets, which can still reach host daemons through pathname sockets.
  - On 32-bit x86, `socketcall(2)` multiplexes socket calls and needs separate handling (**unverified**; not relevant to x86_64 or aarch64).
- **io_uring bypass.** `IORING_OP_SOCKET` "Issue[s] the equivalent of a socket(2) system call. Available since 5.19" ([io_uring_enter(2)](https://git.kernel.org/pub/scm/linux/kernel/git/axboe/liburing.git/plain/man/io_uring_enter.2)).
  - seccomp sees only the `io_uring_enter` syscall, not the operations it submits. A filter must therefore deny `io_uring_setup`, `io_uring_enter` and `io_uring_register` entirely.
  - Codex does exactly this: "All filter modes block `io_uring`" ([codex linux-sandbox README](https://github.com/openai/codex/blob/main/codex-rs/linux-sandbox/README.md)).
  - The system-wide alternative is the `kernel.io_uring_disabled` sysctl ([sysctl/kernel.rst](https://github.com/torvalds/linux/blob/master/Documentation/admin-guide/sysctl/kernel.rst)), which needs root.
- **Rust:** seccomp can be installed with raw `prctl`/`seccomp` syscalls through `libc`. Whether to use a crate such as `seccompiler` is a later choice, and no crate docs were checked (**unverified**).

## bubblewrap

- **Needs unprivileged user namespaces.** "Historically, bubblewrap also supported a setuid mode … However, this has been removed" ([README](https://github.com/containers/bubblewrap/blob/main/README.md)). Debian says the same ([README.Debian](https://metadata-backend.ftp-master.debian.org/changelogs/main/b/bubblewrap/stable_README.Debian)).
- **Options** (from the [bwrap(1) source](https://github.com/containers/bubblewrap/blob/main/bwrap.xml)):
  - `--unshare-net`: a new network namespace.
  - `--unshare-pid`: also runs a reaping PID 1.
  - `--die-with-parent`: uses `PR_SET_PDEATHSIG`, and SIGKILLs every sandbox process when bwrap or its parent dies.
  - `--new-session`: calls `setsid()`. The README requires it unless TIOCSTI is filtered (CVE-2017-5226).
  - `--clearenv`: clears everything except `PWD`.
  - `--disable-userns`: stops the job from creating further user namespaces.
- **Ubuntu AppArmor.** AppArmor's [`bwrap-userns-restrict`](https://gitlab.com/apparmor/apparmor/-/raw/master/profiles/apparmor/profiles/extras/bwrap-userns-restrict) profile lets `/usr/bin/bwrap` use user namespaces and capabilities, and its "children do not have capabilities". Which Ubuntu releases ship it:

| Ubuntu release | Ships `/etc/apparmor.d/bwrap-userns-restrict`? | Source |
|---|---|---|
| 24.04 | No | [noble filelist](https://packages.ubuntu.com/noble/amd64/apparmor/filelist) |
| 25.10 | Yes | [questing filelist](https://packages.ubuntu.com/questing/amd64/apparmor/filelist) |
| 26.04 | Yes | [resolute filelist](https://packages.ubuntu.com/resolute/amd64/apparmor/filelist) |

  - So on stock 24.04, bwrap without root is expected to fail. Claude Code documents this and gives a root-installed workaround ([docs](https://code.claude.com/docs/en/sandboxing)).
  - Whether the 24.04 `bubblewrap` package adds its own profile is **unverified**; probe it.
- **Availability:** bubblewrap is packaged everywhere ([Ubuntu noble](https://packages.ubuntu.com/noble/bubblewrap)). Desktop installs pull it in through Flatpak and glycin (README.Debian). Whether it is present on server/cloud images, or on the `ubuntu-latest` runner, is **unverified**.
- **Rust-only rule:** bubblewrap is an external C binary. Using it adds a runtime prerequisite. Codex works around this by bundling its own `bwrap` ([codex README](https://github.com/openai/codex/blob/main/codex-rs/linux-sandbox/README.md)).

## Linux process-tree containment

| Mechanism | Since | What it gives | Limit |
|---|---|---|---|
| Process group (today's macOS model) | always | `kill(-pgid)` | A descendant can leave with `setsid`/`setpgid` (setsid(2)). Not a containment boundary. |
| `PR_SET_CHILD_SUBREAPER` | 3.4 (**unverified** version) | Orphans are reparented to the nearest subreaper, which can `wait` for them ([PR_SET_CHILD_SUBREAPER(2const)](https://git.kernel.org/pub/scm/docs/man-pages/man-pages.git/plain/man/man2const/PR_SET_CHILD_SUBREAPER.2const)) | Lets the parent see survivors. Killing them still needs a walk over the reaped children, which races with forks. |
| PID namespace | 3.8 | "If the 'init' process of a PID namespace terminates, the kernel terminates all of the processes in the namespace via a SIGKILL" ([pid_namespaces(7)](https://git.kernel.org/pub/scm/docs/man-pages/man-pages.git/plain/man/man7/pid_namespaces.7)) | Needs a user namespace when unprivileged, so the same AppArmor caveat applies. |
| cgroup v2 `cgroup.kill` | 5.14 (doc present at the [v5.14 tag](https://github.com/torvalds/linux/blob/v5.14/Documentation/admin-guide/cgroup-v2.rst), absent at v5.13) | SIGKILLs the whole subtree; "deal[s] with concurrent forks appropriately and is protected against migrations" | Needs a delegated, writable cgroup: systemd `Delegate=` plus the leaf-only rule ([systemd cgroup delegation](https://systemd.io/CGROUP_DELEGATION/)). Without root, this depends on a systemd user session. |
| pidfd (`pidfd_open`) | 5.3 (syscall table at tag) | Signals a process without PID-reuse races | Covers one process, not a tree. |

## Native Windows

**AppContainer**
- **Launch:** pass `SECURITY_CAPABILITIES` through `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`. The profile comes from `CreateAppContainerProfile`. Available from Windows 8 ([Launch an AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer), [UpdateProcThreadAttribute](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)).
- **Network:** "without the network capability, an AppContainer cannot access the network" (Launch an AppContainer).
  - The capabilities are `internetClient`, `internetClientServer` and `privateNetworkClientServer` ([capability declarations](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations)).
  - The docs describe network isolation by direction and network type, never by protocol. UDP, and DNS through the Dnscache service, are **unverified** without a probe.
- **Loopback** between different apps is blocked by network isolation. `CheckNetIsolation LoopbackExempt` is documented for development only ([archived doc](https://learn.microsoft.com/en-us/previous-versions/windows/apps/hh780593(v=win.10))).
- **Files:** access is the intersection of the user's normal permissions and the AppContainer or ALL APPLICATION PACKAGES SID. Each path must explicitly grant that SID ([AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)).
  - That is a persistent permission change on `input/` and `output/`, unlike a Seatbelt profile, which applies to one launch only.
  - System files that already grant ALL APPLICATION PACKAGES stay readable.
- **LPAC** (`PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT`) drops those default grants. It reads less of the system and breaks more tools.
- **`PROCESS_CREATION_CHILD_PROCESS_RESTRICTED`** blocks child processes, but only inside a sandbox such as AppContainer (UpdateProcThreadAttribute).
- **Profile environment:** the profile redirects `TEMP`/`LOCALAPPDATA` to `%LOCALAPPDATA%\Packages\<name>\AC`.

**Restricted tokens and WFP**
- `CreateRestrictedToken` performs a two-pass access check and documents **no network control** ([doc](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken)).
- Only local administrators can add Windows Filtering Platform filters ([About WFP](https://learn.microsoft.com/en-us/windows/win32/fwp/about-windows-filtering-platform)). WFP is therefore not an unprivileged option.

**Job Objects**
- **Membership:** children inherit the job. Breakaway needs `JOB_OBJECT_LIMIT_BREAKAWAY_OK` or `SILENT_BREAKAWAY_OK`, so set neither. A process cannot leave a job ([Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)).
  - Documented escape: a process started through WMI `Win32_Process.Create` lands outside the job.
- **Killing:** `TerminateJobObject` kills every process. `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` kills on the last handle close, including child jobs. Jobs nest since Windows 8 (same page).
- **Starting inside the job:** `PROC_THREAD_ATTRIBUTE_JOB_LIST` (Windows 10) assigns the job when the process is created. Otherwise use `CREATE_SUSPENDED`, then `AssignProcessToJobObject`, then resume ([AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject)).
- **Survivor check:** `JOBOBJECT_BASIC_ACCOUNTING_INFORMATION.ActiveProcesses` counts live members ([doc](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information)). It replaces `kill(-pgid, 0)`.
  - Completion-port messages are "not guaranteed", so poll the count instead.
- **`CREATE_NEW_PROCESS_GROUP` is not containment.** It only routes console Ctrl events ([creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)).

**Mapping the Unix file model**

| Engine assumption | Windows | Rust |
|---|---|---|
| Modes 0644/0755 | No exec bit; only `FILE_ATTRIBUTE_READONLY` | [`Permissions`](https://doc.rust-lang.org/std/fs/struct.Permissions.html) exposes `readonly` only. `PermissionsExt::mode` is Unix-only. Enrollment needs another rule for "executable", such as a mode stored as metadata. |
| Single-link regular files | `BY_HANDLE_FILE_INFORMATION.nNumberOfLinks`; always 1 on FAT; file identity is volume serial plus file index, but ReFS needs `FILE_ID_INFO` ([doc](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information)) | `MetadataExt::number_of_links` is nightly-only (`windows_by_handle`, [docs](https://doc.rust-lang.org/std/os/windows/fs/trait.MetadataExt.html)). Stable code needs `GetFileInformationByHandle` through `windows-sys`. |
| `flock` writer leases (advisory) | `LockFileEx` is **mandatory** byte-range locking. An exclusive lock "denies all other processes both read and write access", even to other handles in the same process. Release after process exit can be delayed ([LockFileEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex)). | [`File::lock`](https://doc.rust-lang.org/std/fs/struct.File.html), stable since 1.89, uses `flock` on Unix and `LockFileEx` on Windows ("may be advisory or mandatory"). Lock a separate lock file, never the data file. |
| Process groups | Job Objects (above) | `CommandExt::creation_flags` is stable. Attribute lists (`spawn_with_attributes`) are unstable ([CommandExt](https://doc.rust-lang.org/std/os/windows/process/trait.CommandExt.html)), so an AppContainer plus job-list launch must call `CreateProcessW` through `windows-sys`. Its feature names are **unverified**. |

## Seatbelt and what other projects use

- **Seatbelt is deprecated but still ships.**
  - `sandbox-exec(1)`, as shipped on macOS 26.6.2: "The sandbox-exec command is DEPRECATED. Developers who wish to sandbox an app should instead adopt the App Sandbox feature".
  - `sandbox_init(3)` uses the same wording.
  - The SDK's `sandbox.h` says "This header is deprecated and may be removed in a future release", with `API_DEPRECATED(..., macos(10.5, 10.8))`.
  - These are local man pages and the local SDK header. No Apple web page was fetched.
  - No removal date has been published (**unverified**: absence of evidence).
- **Chromium:** a Seatbelt profile through the private `sandbox_init_with_parameters` ([mac design](https://chromium.googlesource.com/chromium/src/+/HEAD/sandbox/mac/seatbelt_sandbox_design.md)).
  - On Linux: namespaces where supported, a setuid helper, seccomp-bpf. It does not use Landlock: "We currently don't use Landlock, but we'd like to" ([linux README](https://chromium.googlesource.com/chromium/src/+/HEAD/sandbox/linux/README.md)).
  - On Windows: restricted token, Job Object, alternate desktop, integrity levels, AppContainer/LPAC ([design](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/design/sandbox.md)).
- **Bazel** ([sandboxing](https://bazel.build/docs/sandboxing), [flags](https://bazel.build/reference/command-line-reference)):
  - macOS: `darwin-sandbox`, which uses `sandbox-exec`.
  - Linux: `linux-sandbox`, built on user, mount, PID, network and IPC namespaces. Users are told to set `kernel.unprivileged_userns_clone=1`.
  - Network is allowed by default.
  - Windows: an experimental `BazelSandbox.exe`.
- **Codex CLI:**
  - macOS: hard-coded `/usr/bin/sandbox-exec` ([seatbelt.rs](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt.rs)).
  - **Linux: bubblewrap is now the default.** Landlock is "legacy" and rejected for filesystem-restricted policies "because it cannot isolate app-server Unix sockets".
    - Setup: `--ro-bind / /` plus writable roots, then in-process `PR_SET_NO_NEW_PRIVS` and seccomp, with io_uring blocked.
    - It uses the system `bwrap`, or a bundled copy ([linux-sandbox README](https://github.com/openai/codex/blob/main/codex-rs/linux-sandbox/README.md), [docs](https://learn.chatgpt.com/docs/sandboxing)).
  - **Windows:** an elevated backend (sandbox users, ACLs, firewall), an unelevated one (restricted token plus ACLs, documented as weaker on network), and an MXC backend: suspended start, kill-on-close job, direct DNS denied ([Windows sandbox](https://learn.chatgpt.com/docs/windows/windows-sandbox), [mxc-sandbox README](https://github.com/openai/codex/blob/main/codex-rs/mxc-sandbox/README.md)).
  - Policies a backend cannot enforce "still fail closed" ([core README](https://github.com/openai/codex/blob/main/codex-rs/core/README.md)).
- **Claude Code** ([sandboxing docs](https://code.claude.com/docs/en/sandboxing)):
  - macOS: Seatbelt.
  - Linux and WSL2: bubblewrap, plus `socat` and an optional seccomp filter for UNIX sockets, with a separate network namespace.
  - Native Windows: "runs commands unsandboxed".
  - **Not fail-closed by default:** strict behaviour needs `sandbox.failIfUnavailable: true`.
  - [sandbox-runtime](https://github.com/anthropic-experimental/sandbox-runtime) Windows alpha uses a dedicated local user, WFP and NTFS ACLs, not AppContainer.

## Implications for the six contract properties (documentation only)

Labels: **Y** means documented to meet the property. **P** means partial or with conditions. **N** means does not meet it. **?** means the docs are silent and a probe is needed. Probe results replace any cell they contradict.

| Candidate | 1 No network | 2 Reads limited | 3 Writes limited | 4 Clean env/layout | 5 Process tree + kill | 6 Fail closed | No root | Rust-only |
|---|---|---|---|---|---|---|---|---|
| Landlock alone | P: TCP only from 6.7, UDP only from 7.2. No other families. Pathname UNIX sockets open before 7.1. | Y (ABI 1); must allow-list `/proc` etc. | Y; truncate from 6.2 | Y (host-side `env_clear`) | N: needs a separate mechanism | Y with `HardRequirement` plus status check | Y | Y |
| Landlock + seccomp | Y on any 5.13+ kernel: seccomp allows only `AF_UNIX` and denies io_uring. Pathname UNIX sockets still need fs rules (ABI 9) or the job cannot reach them. | Y | Y | Y | N: needs subreaper or cgroup | Y if both installs are checked | Y | Y |
| userns + netns | Y for IP, incl. UDP/DNS/loopback; abstract sockets isolated; pathname sockets not | N (needs mount namespace or Landlock) | N (same) | Y | P: PID namespace kills on init exit | P: fails on Ubuntu 23.10+ AppArmor and where userns is disabled. Must be detected, never ignored. | P (blocked by Ubuntu default) | Y |
| bubblewrap | Y (`--unshare-net`) | Y (bind-mount view) | Y | Y (`--clearenv`) | Y (`--unshare-pid`, `--die-with-parent`) | P: fails without userns. Needs no root on Ubuntu 25.10+; needs a root-installed profile on 24.04. | P | N: external C binary |
| AppContainer + Job Object | ? Documented "no network capability, no network"; UDP and DNS not documented | P: ACL grants per path; default ALL APPLICATION PACKAGES reads | Y via ACL grant on `output/` only | Y | Y: no breakaway, kill-on-close, `ActiveProcesses` | ? Must check `CreateAppContainerProfile` and job assignment results | Y | Y (`windows-sys`) |
