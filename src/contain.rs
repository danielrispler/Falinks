//! Contained commands: captured jobs and required checks run under the containment
//! guarantee. One OS-blind entry point; macOS uses Seatbelt, Linux the `falinks-contain`
//! helper executable (ADR 2). Every other platform fails closed.
use crate::{Result, fail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::process::{CommandExt, ExitStatusExt},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// System runtime trees a captured job may read besides its input and output.
#[cfg(target_os = "macos")]
pub(crate) const SYSTEM_READS: &[&str] = &[
    "/usr",
    "/bin",
    "/System",
    "/Library",
    "/opt",
    "/private/etc",
    "/private/var/db/dyld",
    "/dev",
];
#[cfg(not(target_os = "macos"))]
pub(crate) const SYSTEM_READS: &[&str] = &[
    "/usr", "/bin", "/lib", "/lib64", "/sbin", "/etc", "/proc", "/opt", "/dev",
];

/// Bumped whenever `Launch` or `Report` change; a mismatched helper fails closed.
const PROTOCOL: u32 = 1;
/// The helper's file name, looked up beside the host executable by default.
pub const HELPER: &str = "falinks-contain";

/// What a contained command may read besides system runtime files.
pub(crate) enum Reads {
    /// Only these trees (captured jobs).
    Only(Vec<PathBuf>),
    /// Everything except `denied`, of which `allowed` stays readable (required checks).
    Except {
        denied: Vec<PathBuf>,
        allowed: Vec<PathBuf>,
    },
}

/// A trusted host-enrolled command; `output` is the only writable tree besides `/dev/null`.
pub(crate) struct Contained<'a> {
    pub evidence: &'a Path,
    pub program: &'a str,
    pub args: &'a [String],
    pub cwd: &'a Path,
    pub output: &'a Path,
    pub env: &'a [(&'a str, &'a Path)],
    pub reads: Reads,
    pub timeout: Duration,
}

pub(crate) fn supported() -> Result<()> {
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        Ok(())
    } else {
        fail("contained commands need macOS or Linux; unsupported platform")
    }
}

/// Runs `command` with a cleared environment, retaining stdout, stderr and the
/// containment record in its evidence directory.
pub(crate) fn run(helper: &Path, command: &Contained) -> Result<ExitStatus> {
    supported()?;
    if cfg!(target_os = "macos") {
        seatbelt(command)
    } else {
        linux(helper, command)
    }
}

/// Serialize paths as quoted strings; prevent Seatbelt profile injection from storage paths.
fn quote(p: &Path) -> Result<String> {
    let s = p.to_str().ok_or("non-UTF8 sandbox path")?;
    if s.contains(['\n', '\r']) {
        return fail("unsupported sandbox path");
    }
    Ok(serde_json::to_string(s)?)
}

fn subpaths(paths: impl IntoIterator<Item = impl AsRef<Path>>) -> Result<String> {
    Ok(paths
        .into_iter()
        .map(|p| Ok(format!("(subpath {})", quote(p.as_ref())?)))
        .collect::<Result<Vec<_>>>()?
        .join(" "))
}

fn seatbelt(command: &Contained) -> Result<ExitStatus> {
    let output = quote(command.output)?;
    let writes = format!("(allow file-write* (subpath {output}) (literal \"/dev/null\"))");
    let profile = match &command.reads {
        Reads::Only(trees) => format!(
            "(version 1) (allow default) (deny network*) (deny file-write*) (deny file-read*) (allow file-read* (literal \"/\") {} {}) {writes}",
            subpaths(SYSTEM_READS)?,
            subpaths(trees)?,
        ),
        Reads::Except { denied, allowed } => format!(
            "(version 1) (allow default) (deny network*) (deny file-write*) {writes} (deny file-read-data {}) (allow file-read-data {})",
            subpaths(denied)?,
            subpaths(allowed)?,
        ),
    };
    fs::write(command.evidence.join("sandbox.sb"), &profile)?;
    let mut child = Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &profile, command.program])
        .args(command.args)
        .current_dir(command.cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .envs(command.env.iter().copied())
        .stdin(Stdio::null())
        .stdout(fs::File::create(command.evidence.join("stdout"))?)
        .stderr(fs::File::create(command.evidence.join("stderr"))?)
        .process_group(0)
        .spawn()?;
    let group = child.id() as i32;
    // Persist running process identity as evidence; restart never adopts its output.
    fs::write(command.evidence.join("process-group"), group.to_string())?;
    let deadline = Instant::now() + command.timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            kill_group(group);
            child.wait()?;
            return fail("timed out; output retained");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // Trusted tools must keep descendants in this group and join them before exit.
    // A surviving descendant makes completion ambiguous; kill the group, retain output.
    if group_alive(group) {
        kill_group(group);
        return fail("ambiguous completion: surviving descendants");
    }
    Ok(status)
}

fn kill_group(group: i32) {
    // SAFETY: kill takes no pointers; the target is a dedicated process group.
    unsafe { libc::kill(-group, libc::SIGKILL) };
}
fn group_alive(group: i32) -> bool {
    // SAFETY: signal 0 only probes whether the process group still exists.
    unsafe { libc::kill(-group, 0) == 0 }
}

/// The helper's input, retained as `launch.json` evidence.
#[derive(Serialize, Deserialize)]
struct Launch {
    protocol: u32,
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, PathBuf)>,
    reads: Vec<PathBuf>,
    output: PathBuf,
    timeout_ms: u64,
    report: PathBuf,
}
/// The helper's outcome, retained as `containment.json` evidence.
#[derive(Default, Serialize, Deserialize)]
struct Report {
    /// Raw wait status of the command; absent when it never ran or timed out.
    status: Option<i32>,
    timed_out: bool,
    survivors: bool,
    error: Option<String>,
}

fn linux(helper: &Path, command: &Contained) -> Result<ExitStatus> {
    let reads = match &command.reads {
        Reads::Only(trees) => SYSTEM_READS
            .iter()
            .map(PathBuf::from)
            .chain(trees.iter().cloned())
            .collect(),
        Reads::Except { denied, allowed } => {
            let mut reads = complement(denied)?;
            reads.extend(allowed.iter().cloned());
            reads
        }
    };
    let launch = Launch {
        protocol: PROTOCOL,
        program: command.program.into(),
        args: command.args.to_vec(),
        cwd: command.cwd.into(),
        env: std::iter::once(("PATH", Path::new("/usr/bin:/bin")))
            .chain(command.env.iter().copied())
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        reads,
        output: command.output.into(),
        timeout_ms: command.timeout.as_millis() as u64,
        report: command.evidence.join("containment.json"),
    };
    let request = command.evidence.join("launch.json");
    fs::write(&request, serde_json::to_vec_pretty(&launch)?)?;
    let mut child = Command::new(helper)
        .arg(&request)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(fs::File::create(command.evidence.join("stdout"))?)
        .stderr(fs::File::create(command.evidence.join("stderr"))?)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("containment unavailable: helper {}: {e}", helper.display()))?;
    let group = child.id() as i32;
    fs::write(command.evidence.join("process-group"), group.to_string())?;
    // The helper enforces the bound; this backstop only catches a stuck helper.
    let deadline = Instant::now() + command.timeout + Duration::from_secs(10);
    let helper_status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            kill_group(group);
            child.wait()?;
            return fail("containment helper overran its bound; output retained");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let report: Report = match fs::read(&launch.report) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(_) => {
            return fail(format!(
                "containment unavailable: helper exited {helper_status} without a report"
            ));
        }
    };
    if let Some(error) = report.error {
        return fail(format!("containment unavailable: {error}"));
    }
    if report.timed_out {
        return fail("timed out; output retained");
    }
    if report.survivors {
        return fail("ambiguous completion: surviving descendants");
    }
    Ok(ExitStatus::from_raw(
        report.status.ok_or("containment report has no status")?,
    ))
}

/// Every tree outside `denied`, found by listing siblings along the denied roots'
/// ancestors only. Symlinks are skipped: their targets are listed elsewhere or denied.
/// `denied` must be canonical.
fn complement(denied: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut allowed = Vec::new();
    let mut pending = vec![PathBuf::from("/")];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_symlink() || denied.contains(&path) {
                continue;
            }
            if denied.iter().any(|d| d.starts_with(&path)) {
                pending.push(path);
            } else {
                allowed.push(path);
            }
        }
    }
    Ok(allowed)
}

/// Default helper location: beside the host executable, or beside Cargo's `deps`
/// directory for integration tests.
pub(crate) fn default_helper() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let mut dir = exe.parent().ok_or("host executable has no directory")?;
    if !dir.join(HELPER).exists() && dir.ends_with("deps") {
        dir = dir.parent().unwrap_or(dir);
    }
    Ok(dir.join(HELPER))
}

/// Entry point of the `falinks-contain` executable: runs one launch request and
/// writes its report. The exit code only says whether a report was written.
pub fn helper_main() -> i32 {
    let Some(request) = std::env::args_os().nth(1) else {
        eprintln!("usage: {HELPER} LAUNCH.json");
        return 2;
    };
    let launch: Launch = match fs::read(&request)
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
    {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{HELPER}: unreadable launch request: {error}");
            return 2;
        }
    };
    let report = supervise(&launch).unwrap_or_else(|error| Report {
        error: Some(error.to_string()),
        ..Report::default()
    });
    match serde_json::to_vec(&report)
        .map_err(|e| e.to_string())
        .and_then(|b| fs::write(&launch.report, b).map_err(|e| e.to_string()))
    {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{HELPER}: cannot write report: {error}");
            1
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn supervise(_: &Launch) -> Result<Report> {
    fail("the containment helper runs only on Linux")
}

/// Fail closed below the floor: ABI 3 is the first that controls truncation.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn require_landlock(abi: i64) -> Result<()> {
    match abi {
        ..=0 => fail("Landlock unavailable: not in the kernel's LSM list or disabled"),
        1..=2 => fail(format!("Landlock ABI {abi} is below the required ABI 3")),
        _ => Ok(()),
    }
}

#[cfg(target_os = "linux")]
use linux_helper::supervise;
#[cfg(target_os = "linux")]
mod linux_helper {
    //! Runs inside the single-threaded helper process, so `pre_exec` may allocate.
    use super::{Launch, Report, require_landlock};
    use crate::{Result, fail};
    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr,
        RulesetStatus, path_beneath_rules,
    };
    use std::{
        fs,
        os::unix::process::{CommandExt, ExitStatusExt},
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    /// The pinned ruleset: exactly the ABI 3 rights on every kernel, so the rules
    /// enforced do not depend on how new the kernel is.
    const PINNED: ABI = ABI::V3;

    fn landlock_abi() -> i64 {
        const LANDLOCK_CREATE_RULESET_VERSION: libc::c_uint = 1;
        // SAFETY: the version query takes a null attribute pointer and size 0 by contract.
        unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<libc::c_void>(),
                0usize,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        }
    }

    pub(super) fn supervise(launch: &Launch) -> Result<Report> {
        if launch.protocol != super::PROTOCOL {
            return fail(format!(
                "helper protocol {} does not match engine protocol {}",
                super::PROTOCOL,
                launch.protocol
            ));
        }
        require_landlock(landlock_abi())?;
        // Orphaned descendants reparent to this helper instead of init, so it can sweep them.
        // SAFETY: prctl with integer arguments only.
        if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) } != 0 {
            return fail(format!("subreaper: {}", std::io::Error::last_os_error()));
        }
        let (reads, output) = (launch.reads.clone(), launch.output.clone());
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .current_dir(&launch.cwd)
            .env_clear()
            .envs(launch.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .process_group(0);
        // SAFETY: the helper is single-threaded, so the forked child may allocate before exec.
        unsafe {
            command.pre_exec(move || {
                contain(&reads, &output).map_err(|e| std::io::Error::other(e.to_string()))
            });
        }
        let mut child = command.spawn()?;
        let group = child.id() as i32;
        let deadline = Instant::now() + Duration::from_millis(launch.timeout_ms);
        let (status, timed_out) = loop {
            if let Some(status) = child.try_wait()? {
                break (Some(status.into_raw()), false);
            }
            if Instant::now() >= deadline {
                super::kill_group(group);
                child.wait()?;
                break (None, true);
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // Escaped descendants that already exited are reaped, not counted.
        std::thread::sleep(Duration::from_millis(50));
        reap();
        let survivors = super::group_alive(group) || !adopted().is_empty();
        sweep(group)?;
        Ok(Report {
            status,
            timed_out,
            survivors,
            error: None,
        })
    }

    /// Kills the command's group and every adopted descendant, including `setsid` escapes.
    fn sweep(group: i32) -> Result<()> {
        for _ in 0..100 {
            super::kill_group(group);
            let pids = adopted();
            if pids.is_empty() && !super::group_alive(group) {
                return Ok(());
            }
            for pid in pids {
                // SAFETY: kill takes no pointers; pid is a child of this helper.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
            std::thread::sleep(Duration::from_millis(10));
            reap();
        }
        fail("descendants survived the sweep")
    }

    fn reap() {
        // SAFETY: waitpid with a null status pointer is allowed.
        while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
    }

    /// Live children of this helper: after the command is waited, only adopted escapes.
    fn adopted() -> Vec<i32> {
        fs::read_to_string("/proc/thread-self/children")
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect()
    }

    /// Runs in the forked child before exec: Landlock for files, seccomp for sockets.
    fn contain(reads: &[PathBuf], output: &PathBuf) -> Result<()> {
        let status = Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessFs::from_all(PINNED))?
            .create()?
            .add_rules(path_beneath_rules(reads, AccessFs::from_read(PINNED)))?
            .add_rules(path_beneath_rules(
                ["/dev/null"],
                AccessFs::ReadFile | AccessFs::WriteFile | AccessFs::Truncate,
            ))?
            .add_rules(path_beneath_rules([output], AccessFs::from_all(PINNED)))?
            .restrict_self()?;
        if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
            return fail("Landlock ruleset not fully enforced");
        }
        // No socket of any family: Landlock does not cover UDP or pathname Unix sockets (#34).
        seccomp(&[libc::SYS_socket, libc::SYS_io_uring_setup])
    }

    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 0xC000_00B7;

    /// Denies each syscall with EPERM and kills any foreign-architecture syscall.
    fn seccomp(denied: &[libc::c_long]) -> Result<()> {
        use libc::{
            BPF_ABS, BPF_JEQ, BPF_JGE, BPF_JMP, BPF_K, BPF_LD, BPF_RET, BPF_W, sock_filter,
        };
        let stmt = |code: u32, k: u32| sock_filter {
            code: code as u16,
            jt: 0,
            jf: 0,
            k,
        };
        let jump = |k: u32, jt: u8, jf: u8, code: u32| sock_filter {
            code: code as u16,
            jt,
            jf,
            k,
        };
        let mut program = vec![
            stmt(BPF_LD | BPF_W | BPF_ABS, 4), // seccomp_data.arch
            jump(AUDIT_ARCH, 1, 0, BPF_JMP | BPF_JEQ | BPF_K),
            stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_KILL_PROCESS),
            stmt(BPF_LD | BPF_W | BPF_ABS, 0), // seccomp_data.nr
        ];
        if cfg!(target_arch = "x86_64") {
            // x32 syscalls share the x86_64 audit arch; refuse them so rules cannot be bypassed.
            program.push(jump(0x4000_0000, 0, 1, BPF_JMP | BPF_JGE | BPF_K));
            program.push(stmt(
                BPF_RET | BPF_K,
                libc::SECCOMP_RET_ERRNO | libc::ENOSYS as u32,
            ));
        }
        for nr in denied {
            program.push(jump(*nr as u32, 0, 1, BPF_JMP | BPF_JEQ | BPF_K));
            program.push(stmt(
                BPF_RET | BPF_K,
                libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
            ));
        }
        program.push(stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ALLOW));
        let filter = libc::sock_fprog {
            len: program.len() as u16,
            filter: program.as_mut_ptr(),
        };
        // SAFETY: filter points at a live, correctly sized program; no_new_privs is set.
        if unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &filter) } != 0 {
            return fail(format!("seccomp: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landlock_below_abi_3_fails_closed() {
        assert!(require_landlock(-1).is_err());
        assert!(require_landlock(2).is_err());
        assert!(require_landlock(3).is_ok());
    }

    #[test]
    fn complement_excludes_denied_roots_and_keeps_their_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        for name in ["state", "live", "other"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        std::os::unix::fs::symlink(root.join("state"), root.join("link")).unwrap();
        let denied = [root.join("state"), root.join("live")];
        let allowed = complement(&denied).unwrap();
        assert!(allowed.contains(&root.join("other")));
        assert!(!allowed.contains(&root.join("link")));
        assert!(allowed.iter().all(|a| {
            denied
                .iter()
                .all(|d| !d.starts_with(a) && !a.starts_with(d))
        }));
    }
}
