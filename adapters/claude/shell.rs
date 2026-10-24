//! `CLAUDE_CODE_SHELL_PREFIX` for per-command Bash confinement (#236 spike). Claude Code
//! 2.1.308 runs each Bash command as `PREFIX 'COMMAND'`, and launches each stdio MCP
//! server as `PREFIX 'SERVER ARGS'`. This confines itself, then execs `/bin/bash -c
//! COMMAND`: reads outside the denied roots, writes only beneath scratch and `/dev/null`,
//! no network. The `falinks` relay's exact command line is the one exception: it runs
//! unconfined, as it does under Claude Code's sandbox, so it can read the token. A confined
//! command that names that line stays confined: a profile is inherited and cannot be
//! loosened. The adapter's hook is exec form, which the prefix skips. Paths come from
//! `FALINKS_SHELL`. Any failure exits 126 before COMMAND runs.
use falinks_host::Result;
use serde_json::Value;
use std::{
    convert::Infallible,
    env,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{self, Command},
};

/// The shell Claude Code is told to use (`CLAUDE_CODE_SHELL`), so its snapshot matches.
const SHELL: &str = "/bin/bash";

fn main() {
    let Err(error) = run();
    eprintln!("falinks-claude-shell: {error}");
    process::exit(126);
}

/// Returns only on failure; COMMAND never starts unconfined.
fn run() -> Result<Infallible> {
    let args: Vec<String> = env::args().skip(1).collect();
    let [command] = args.as_slice() else {
        return Err("usage: falinks-claude-shell COMMAND".into());
    };
    let spec: Value = serde_json::from_str(&env::var("FALINKS_SHELL")?)?;
    let strings = |key: &str| -> Vec<String> {
        spec[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    };
    let relay = strings("relay");
    if !relay.is_empty() && shlex::split(command).as_ref() == Some(&relay) {
        return Err(Command::new(&relay[0]).args(&relay[1..]).exec().into());
    }
    let scratch = Path::new(spec["scratch"].as_str().ok_or("no scratch")?).canonicalize()?;
    let denied = strings("denied")
        .iter()
        .map(|p| Path::new(p).canonicalize())
        .collect::<std::io::Result<Vec<PathBuf>>>()?;
    Err(confined(&scratch, &denied)?
        .arg("-c")
        .arg(command)
        .exec()
        .into())
}

#[cfg(target_os = "macos")]
fn confined(scratch: &Path, denied: &[PathBuf]) -> Result<Command> {
    let quote = |p: &Path| -> Result<String> {
        let s = p.to_str().ok_or("non-UTF-8 path")?;
        falinks_host::require(!s.contains(['\n', '\r']), "unsupported path")?;
        Ok(serde_json::to_string(s)?)
    };
    let mut profile = format!(
        "(version 1) (allow default) (deny network*) (deny file-write*) (allow file-write* (literal \"/dev/null\") (subpath {}))",
        quote(scratch)?
    );
    for root in denied {
        let root = quote(root)?;
        profile.push_str(&format!(" (deny file-read* file-write* (subpath {root}))"));
    }
    profile.push_str(PROCESSES);
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command.arg("-p").arg(profile).arg(SHELL);
    Ok(command)
}

/// What Claude Code's own Seatbelt profile allows other processes, so a confined command
/// cannot write through one: no Launch Services or Apple Events, Mach lookups only to
/// the same 14 services, signals and process information only within the sandbox.
#[cfg(target_os = "macos")]
const PROCESSES: &str = r#"
(deny lsopen) (deny appleevent-send)
(deny mach-lookup)
(allow mach-lookup
  (global-name "com.apple.audio.systemsoundserver")
  (global-name "com.apple.distributed_notifications@Uv3")
  (global-name "com.apple.FontObjectsServer")
  (global-name "com.apple.fonts")
  (global-name "com.apple.logd")
  (global-name "com.apple.lsd.mapdb")
  (global-name "com.apple.PowerManagement.control")
  (global-name "com.apple.system.logger")
  (global-name "com.apple.system.notification_center")
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.opendirectoryd.membership")
  (global-name "com.apple.bsd.dirhelper")
  (global-name "com.apple.securityd.xpc")
  (global-name "com.apple.coreservices.launchservicesd"))
(deny signal) (allow signal (target same-sandbox))
(deny process-info*) (allow process-info* (target same-sandbox))"#;

/// Landlock and seccomp from the engine's containment, installed in this
/// single-threaded process and inherited by the shell (ADR 2).
#[cfg(target_os = "linux")]
fn confined(scratch: &Path, denied: &[PathBuf]) -> Result<Command> {
    falinks::confine_self(denied, scratch)?;
    Ok(Command::new(SHELL))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn confined(_: &Path, _: &[PathBuf]) -> Result<Command> {
    Err("per-command confinement needs macOS or Linux".into())
}
