//! Linux evaluation sandbox (#56): restricts itself with Landlock (and, without network,
//! seccomp), then execs the command, which inherits both. A separate single-threaded
//! executable because the harness is multithreaded (ADR 2). Arguments: the JSON spec
//! `Sandbox::wrap` writes, then the command's argv.
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: falinks-eval-contain SPEC COMMAND [ARG]...");
        exit(2);
    }
    let error = linux::contain_and_exec(&args[0], &args[1..]);
    eprintln!("falinks-eval-contain: {error}");
    exit(126);
}

#[cfg(not(target_os = "linux"))]
mod linux {
    pub fn contain_and_exec(_: &str, _: &[String]) -> String {
        "the evaluation containment helper runs only on Linux".into()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr,
        RulesetStatus, path_beneath_rules,
    };
    use serde_json::Value;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    /// Exactly the ABI 3 rights on every kernel, as the engine pins them (#35).
    const PINNED: ABI = ABI::V3;

    /// Returns only on failure; the command never starts unconfined.
    pub fn contain_and_exec(spec: &str, argv: &[String]) -> String {
        match contain(spec) {
            Ok(()) => Command::new(&argv[0]).args(&argv[1..]).exec().to_string(),
            Err(error) => format!("containment unavailable: {error}"),
        }
    }

    fn contain(spec: &str) -> Result<()> {
        let spec: Value = serde_json::from_str(spec)?;
        let paths = |key: &str| -> Vec<PathBuf> {
            spec[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_str().map(PathBuf::from))
                .collect()
        };
        let status = Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessFs::from_all(PINNED))?
            .create()?
            .add_rules(path_beneath_rules(
                paths("reads"),
                AccessFs::from_read(PINNED),
            ))?
            .add_rules(path_beneath_rules(
                ["/dev/null"],
                AccessFs::ReadFile | AccessFs::WriteFile | AccessFs::Truncate,
            ))?
            .add_rules(path_beneath_rules(
                paths("writable"),
                AccessFs::from_all(PINNED),
            ))?
            .restrict_self()?;
        if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
            return Err("Landlock ruleset not fully enforced".into());
        }
        if spec["network"] == true {
            return Ok(());
        }
        // No socket of any family: Landlock does not cover UDP or pathname Unix sockets (#34).
        seccomp(&[libc::SYS_socket, libc::SYS_io_uring_setup])
    }

    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 0xC000_00B7;

    /// Denies each syscall with EPERM and kills any foreign-architecture syscall; copied
    /// from the engine's helper, since this crate is standalone.
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
            return Err(format!("seccomp: {}", std::io::Error::last_os_error()).into());
        }
        Ok(())
    }
}
