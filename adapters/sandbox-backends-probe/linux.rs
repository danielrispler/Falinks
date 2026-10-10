//! Linux candidates. `wrap` installs a mechanism inside its own single-threaded process
//! and then execs the child, as an in-process Rust backend would; `bwrap` is the external
//! binary candidate. The host keeps the engine's process-group launch and kill path.
use crate::{
    Fixture, Launch, Loopback, Result, Scenario, child_args, descendant_alive, wait_until,
};
use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr,
    RulesetCreatedAttr, Scope, path_beneath_rules,
};
use serde_json::{Value, json};
use std::{
    env, fs,
    os::unix::process::{CommandExt, ExitStatusExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const SYSTEM_READS: &[&str] = &["/usr", "/bin", "/lib", "/lib64", "/sbin", "/etc", "/proc"];

fn read(path: &str) -> Value {
    fs::read_to_string(path).map_or(Value::Null, |text| json!(text.trim()))
}

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

pub fn facts() -> Value {
    let bwrap = Command::new("bwrap").arg("--version").output();
    // cgroup v2 delegation route for process-tree kill: needs a systemd user manager.
    let scope = Command::new("systemd-run")
        .args(["--user", "--scope", "--quiet", "true"])
        .output()
        .map(|o| format!("{} {}", o.status, String::from_utf8_lossy(&o.stderr).trim()))
        .unwrap_or_else(|e| format!("error: {e}"));
    json!({
        "kernel": read("/proc/sys/kernel/osrelease"),
        "os_release": fs::read_to_string("/etc/os-release").ok()
            .and_then(|t| t.lines().find(|l| l.starts_with("PRETTY_NAME=")).map(str::to_owned)),
        "arch": env::consts::ARCH,
        // SAFETY: getuid cannot fail and takes no arguments.
        "uid": unsafe { libc::getuid() },
        "lsm": read("/sys/kernel/security/lsm"),
        "landlock_abi": landlock_abi(),
        "apparmor_restrict_unprivileged_userns": read("/proc/sys/kernel/apparmor_restrict_unprivileged_userns"),
        "max_user_namespaces": read("/proc/sys/user/max_user_namespaces"),
        "unprivileged_userns_clone": read("/proc/sys/kernel/unprivileged_userns_clone"),
        "cgroup": read("/proc/self/cgroup"),
        "systemd_run_user_scope": scope,
        "bwrap": bwrap.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_else(|e| format!("error: {e}")),
    })
}

pub fn plan() -> Vec<(&'static str, Scenario)> {
    use Scenario::*;
    vec![
        ("none", Normal),
        ("landlock", Normal),
        ("landlock", Hang),
        ("landlock", Unavailable),
        ("landlock-v6", Normal),
        ("landlock-seccomp", Normal),
        ("userns", Normal),
        ("userns", Hang),
        ("userns", Unavailable),
        ("bwrap", Normal),
        ("bwrap", Hang),
    ]
}

fn bwrap_args(fixture: &Fixture, exe: &Path) -> Vec<String> {
    let output = fixture.output.display().to_string();
    let mut args: Vec<String> = [
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--clearenv",
        "--setenv",
        "PATH",
        "/usr/bin:/bin",
        "--setenv",
        "HOME",
        &output,
        "--setenv",
        "TMPDIR",
        &output,
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/etc",
        "/etc",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
    ]
    .map(String::from)
    .into();
    for link in ["/bin", "/lib", "/lib64", "/sbin"] {
        match fs::read_link(link) {
            Ok(target) => args.extend([
                "--symlink".into(),
                target.display().to_string(),
                link.into(),
            ]),
            Err(_) if Path::new(link).is_dir() => {
                args.extend(["--ro-bind".into(), link.into(), link.into()])
            }
            Err(_) => {}
        }
    }
    for (flag, path) in [
        ("--ro-bind", exe),
        ("--ro-bind", &fixture.input),
        ("--bind", &fixture.output),
    ] {
        args.extend([
            flag.into(),
            path.display().to_string(),
            path.display().to_string(),
        ]);
    }
    args.extend(["--chdir".into(), output]);
    args
}

pub fn launch(
    mechanism: &str,
    scenario: Scenario,
    fixture: &Fixture,
    loopback: &Loopback,
    timeout: Duration,
) -> Result<Launch> {
    // Orphans reparent to the host instead of init, so the host can see escapes.
    // SAFETY: prctl with integer arguments only.
    unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) };
    let exe = env::current_exe()?;
    let child = child_args(fixture, loopback, scenario);
    let mut command = match mechanism {
        "none" => Command::new(&exe),
        "bwrap" => {
            let mut command = Command::new("bwrap");
            command.args(bwrap_args(fixture, &exe)).arg("--").arg(&exe);
            command
        }
        _ => {
            let mut command = Command::new(&exe);
            let simulate = if scenario == Scenario::Unavailable {
                "1"
            } else {
                "0"
            };
            command
                .args(["wrap", mechanism, simulate])
                .arg(&fixture.root)
                .arg("--")
                .arg(&exe);
            command
        }
    };
    let mut child = command
        .args(&child)
        .current_dir(&fixture.output)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &fixture.output)
        .env("TMPDIR", &fixture.output)
        .stdin(Stdio::null())
        .stdout(fs::File::create(fixture.evidence.join("stdout"))?)
        .stderr(fs::File::create(fixture.evidence.join("stderr"))?)
        .process_group(0)
        .spawn()?;
    let group = child.id() as i32;
    let (exit, timed_out) = wait_until(timeout, || {
        Ok(child.try_wait()?.map(|status| match status.code() {
            Some(code) => format!("exit {code}"),
            None => format!("signal {:?}", status.signal()),
        }))
    })?;
    if timed_out {
        // SAFETY: kill takes no pointers; the target is this run's dedicated process group.
        unsafe { libc::kill(-group, libc::SIGKILL) };
        child.wait()?;
    }
    // Dead processes adopted by this subreaper host stay in the group as zombies until reaped.
    std::thread::sleep(Duration::from_millis(50));
    reap();
    // Same completion check as the engine: anything left in the group is a survivor.
    // SAFETY: signal 0 only probes whether the process group still exists.
    let survivors = unsafe { libc::kill(-group, 0) } == 0;
    // SAFETY: as above; kills the dedicated group.
    unsafe { libc::kill(-group, libc::SIGKILL) };
    let descendant = descendant_pid(fixture);
    let alive_after_group_kill = descendant_alive(&fixture.output);
    let parent = descendant.and_then(|pid| {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Fields after the parenthesised command: state, ppid, pgrp, session.
        let rest = stat
            .rsplit_once(')')?
            .1
            .split_whitespace()
            .take(4)
            .collect::<Vec<_>>();
        Some(json!({ "state": rest[0], "ppid": rest[1], "pgrp": rest[2], "session": rest[3] }))
    });
    // Subreaper sweep: every remaining child of the host escaped the group; kill them all.
    let swept = adopted();
    for pid in &swept {
        // SAFETY: kill takes no pointers; pid is a direct child of this host.
        unsafe { libc::kill(*pid, libc::SIGKILL) };
    }
    std::thread::sleep(Duration::from_millis(50));
    reap();
    Ok(Launch {
        started: true,
        launch_error: None,
        exit,
        timed_out,
        tree_survivors: Some(survivors),
        extra: json!({
            "process_group": group,
            "host_pid": std::process::id(),
            "descendant_proc_stat_in_host_view": parent,
            "descendant_alive_after_group_kill": alive_after_group_kill,
            "subreaper_swept_pids": swept,
        }),
    })
}

/// Pid reported by the child, valid only when it ran in the host PID namespace.
fn descendant_pid(fixture: &Fixture) -> Option<i32> {
    let text = fs::read_to_string(fixture.output.join("child.json")).ok()?;
    let child: Value = serde_json::from_str(&text).ok()?;
    let pid = child["descendant"]
        .as_str()?
        .strip_prefix("setsid pid ")?
        .parse()
        .ok()?;
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    String::from_utf8_lossy(&cmdline)
        .contains("descendant")
        .then_some(pid)
}

pub fn cleanup(fixture: &Fixture) {
    if let Some(pid) = descendant_pid(fixture) {
        // SAFETY: kill takes no pointers; pid was checked to be the probe descendant.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    reap();
}

/// Reap adopted orphans so they do not linger as zombies of the subreaper host.
fn reap() {
    // SAFETY: waitpid with a null status pointer is allowed.
    while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
}

/// Live children of this host: after the job is waited, only adopted escapes remain.
fn adopted() -> Vec<i32> {
    let mut pids = Vec::new();
    for task in fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
    {
        if let Ok(text) = fs::read_to_string(task.path().join("children")) {
            pids.extend(
                text.split_whitespace()
                    .filter_map(|p| p.parse::<i32>().ok()),
            );
        }
    }
    pids
}

// ---- wrap: in-process mechanisms ----

/// One seccomp rule; the filter allows everything not matched.
enum Rule {
    Deny(libc::c_long, i32),
    /// Deny the syscall unless its first argument equals the value.
    DenyUnlessArg0(libc::c_long, u32, i32),
}

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7;

fn seccomp(rules: &[Rule]) -> Result<()> {
    use libc::{BPF_ABS, BPF_JEQ, BPF_JGE, BPF_JMP, BPF_K, BPF_LD, BPF_RET, BPF_W, sock_filter};
    let stmt = |code: u32, k: u32| sock_filter {
        code: code as u16,
        jt: 0,
        jf: 0,
        k,
    };
    let jump = |code: u32, k: u32, jt: u8, jf: u8| sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    };
    let errno = |e: i32| libc::SECCOMP_RET_ERRNO | e as u32;
    let mut program = vec![
        stmt(BPF_LD | BPF_W | BPF_ABS, 4), // seccomp_data.arch
        jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH, 1, 0),
        stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_KILL_PROCESS),
        stmt(BPF_LD | BPF_W | BPF_ABS, 0), // seccomp_data.nr
    ];
    if cfg!(target_arch = "x86_64") {
        // x32 syscalls share the x86_64 audit arch; refuse them so rules cannot be bypassed.
        program.push(jump(BPF_JMP | BPF_JGE | BPF_K, 0x4000_0000, 0, 1));
        program.push(stmt(BPF_RET | BPF_K, errno(libc::ENOSYS)));
    }
    for rule in rules {
        match *rule {
            Rule::Deny(nr, e) => {
                program.push(jump(BPF_JMP | BPF_JEQ | BPF_K, nr as u32, 0, 1));
                program.push(stmt(BPF_RET | BPF_K, errno(e)));
            }
            Rule::DenyUnlessArg0(nr, value, e) => {
                program.push(jump(BPF_JMP | BPF_JEQ | BPF_K, nr as u32, 0, 4));
                program.push(stmt(BPF_LD | BPF_W | BPF_ABS, 16)); // low half of args[0]
                program.push(jump(BPF_JMP | BPF_JEQ | BPF_K, value, 0, 1));
                program.push(stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ALLOW));
                program.push(stmt(BPF_RET | BPF_K, errno(e)));
            }
        }
    }
    program.push(stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ALLOW));
    let fprog = libc::sock_fprog {
        len: program.len() as u16,
        filter: program.as_mut_ptr(),
    };
    // SAFETY: fprog points at a live, correctly sized filter; no_new_privs is already set.
    if unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &fprog) } != 0 {
        return Err(format!("seccomp: {}", std::io::Error::last_os_error()).into());
    }
    Ok(())
}

/// Fail closed: any Landlock feature the kernel lacks is an error, never a weaker ruleset.
fn landlock(abi: ABI, input: &Path, output: &Path, exe: &Path) -> Result<()> {
    let mut reads = SYSTEM_READS.iter().map(PathBuf::from).collect::<Vec<_>>();
    reads.extend([input.to_owned(), exe.to_owned()]);
    reads.retain(|path| path.exists());
    let ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(abi))?
        // Handling TCP with no allow rules denies every TCP bind and connect.
        .handle_access(AccessNet::from_all(abi))?;
    // ABI 6 scopes abstract Unix sockets and signals to the sandbox domain.
    let ruleset = if abi >= ABI::V6 {
        ruleset.scope(Scope::from_all(abi))?
    } else {
        ruleset
    };
    let status = ruleset
        .create()?
        .add_rules(path_beneath_rules(&reads, AccessFs::from_read(abi)))?
        .add_rules(path_beneath_rules(
            ["/dev/null"],
            AccessFs::ReadFile | AccessFs::WriteFile,
        ))?
        .add_rules(path_beneath_rules([output], AccessFs::from_all(abi)))?
        .restrict_self()?;
    eprintln!(
        "landlock: {:?} no_new_privs={}",
        status.ruleset, status.no_new_privs
    );
    Ok(())
}

fn unshare_namespaces() -> Result<()> {
    // SAFETY: getuid/getgid cannot fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    // SAFETY: unshare takes only flags.
    if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET | libc::CLONE_NEWPID) } != 0
    {
        return Err(format!("unshare: {}", std::io::Error::last_os_error()).into());
    }
    fs::write("/proc/self/setgroups", "deny").map_err(|e| format!("setgroups: {e}"))?;
    fs::write("/proc/self/uid_map", format!("{uid} {uid} 1"))
        .map_err(|e| format!("uid_map: {e}"))?;
    fs::write("/proc/self/gid_map", format!("{gid} {gid} 1"))
        .map_err(|e| format!("gid_map: {e}"))?;
    Ok(())
}

/// `wrap MECHANISM SIMULATE_UNAVAILABLE ROOT -- PROGRAM ARGS...`
pub fn wrap(args: &[String]) -> Result<()> {
    let [mechanism, simulate, root, dash, target @ ..] = args else {
        return Err("usage: wrap MECHANISM 0|1 ROOT -- PROGRAM ARGS".into());
    };
    if dash != "--" || target.is_empty() {
        return Err("missing -- PROGRAM".into());
    }
    let root = Path::new(root);
    let simulate = simulate == "1";
    // SAFETY: prctl with integer arguments only.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err("no_new_privs failed".into());
    }
    let mut rules = Vec::new();
    match mechanism.as_str() {
        // Simulates a kernel without Landlock: the syscall reports ENOSYS.
        "landlock" | "landlock-v6" | "landlock-seccomp" if simulate => {
            rules.push(Rule::Deny(libc::SYS_landlock_create_ruleset, libc::ENOSYS))
        }
        // Simulates user namespaces being disabled by policy.
        "userns" if simulate => rules.push(Rule::Deny(libc::SYS_unshare, libc::EPERM)),
        _ => {}
    }
    if mechanism == "landlock-seccomp" {
        // Landlock covers TCP only; refuse every non-Unix socket and io_uring (IORING_OP_SOCKET).
        rules.push(Rule::DenyUnlessArg0(
            libc::SYS_socket,
            libc::AF_UNIX as u32,
            libc::EPERM,
        ));
        rules.push(Rule::Deny(libc::SYS_io_uring_setup, libc::EPERM));
    }
    if !rules.is_empty() {
        seccomp(&rules)?;
    }
    let exe = Path::new(&target[0]);
    let abi = if mechanism == "landlock-v6" {
        ABI::V6
    } else {
        ABI::V4
    };
    let contain = || landlock(abi, &root.join("input"), &root.join("output"), exe);
    match mechanism.as_str() {
        "landlock" | "landlock-v6" | "landlock-seccomp" => contain()?,
        "userns" => {
            unshare_namespaces()?;
            // The first child is PID 1 of the new namespace; when it exits the kernel
            // kills every process left in the namespace, including setsid escapes.
            // SAFETY: this process is single-threaded, so fork is sound.
            match unsafe { libc::fork() } {
                -1 => return Err(std::io::Error::last_os_error().into()),
                0 => {
                    if let Err(error) = contain() {
                        eprintln!("{error}");
                        std::process::exit(126);
                    }
                }
                pid => {
                    let mut status = 0;
                    // SAFETY: status points at a live c_int.
                    unsafe { libc::waitpid(pid, &mut status, 0) };
                    std::process::exit(if libc::WIFEXITED(status) {
                        libc::WEXITSTATUS(status)
                    } else {
                        128 + libc::WTERMSIG(status)
                    });
                }
            }
        }
        other => return Err(format!("unknown mechanism {other}").into()),
    }
    Err(format!("exec: {}", Command::new(exe).args(&target[1..]).exec()).into())
}
