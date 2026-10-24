//! `CLAUDE_CODE_SHELL_PREFIX` for per-command Bash confinement (#236 spike). Claude Code
//! 2.1.308 runs each Bash command as `PREFIX 'COMMAND'`, and launches each stdio MCP
//! server as `PREFIX 'SERVER ARGS'`. This confines itself, then execs `SHELL -c COMMAND`
//! with the shell Claude Code uses: reads outside the denied roots, writes only beneath
//! scratch and `/dev/null`, no network. The `falinks` relay's exact command line is the one
//! exception: it runs unconfined, as it does under Claude Code's sandbox, so it can read
//! the token. A confined command that names that line stays confined: a profile is
//! inherited and cannot be loosened. The adapter's hook is exec form, which the prefix
//! skips. The shell, paths and relay come from `FALINKS_SHELL`. Any failure exits 126
//! before COMMAND runs.
use falinks_host::Result;
use serde_json::Value;
use std::{
    convert::Infallible,
    env,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{self, Command},
};

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
    let text = |value: &Value| -> Result<String> {
        Ok(value
            .as_str()
            .ok_or("FALINKS_SHELL entry is not a string")?
            .into())
    };
    // Every entry or none: a dropped denied root would widen the confinement.
    let list = |value: &Value| -> Result<Vec<String>> {
        value
            .as_array()
            .ok_or("FALINKS_SHELL list missing")?
            .iter()
            .map(text)
            .collect()
    };
    let relay = &spec["relay"];
    let mut line = vec![text(&relay["command"])?];
    line.extend(list(&relay["args"])?);
    if shlex::split(command).as_ref() == Some(&line) {
        return Err(Command::new(&line[0]).args(&line[1..]).exec().into());
    }
    let scratch = Path::new(&text(&spec["scratch"])?).canonicalize()?;
    let denied = list(&spec["denied"])?
        .iter()
        .map(|p| Path::new(p).canonicalize())
        .collect::<std::io::Result<Vec<PathBuf>>>()?;
    Err(confined(&scratch, &denied, &text(&spec["shell"])?)?
        .arg("-c")
        .arg(command)
        .exec()
        .into())
}

#[cfg(target_os = "macos")]
fn confined(scratch: &Path, denied: &[PathBuf], shell: &str) -> Result<Command> {
    let quote = |p: &Path| -> Result<String> {
        let s = p.to_str().ok_or("non-UTF-8 path")?;
        falinks_host::require(!s.contains(['\n', '\r']), "unsupported path")?;
        Ok(serde_json::to_string(s)?)
    };
    let mut profile = format!(
        "(version 1) (allow default) (deny network*) (deny file-write*) (allow file-write* (literal \"/dev/null\") (subpath {}))",
        quote(scratch)?
    );
    // Denied roots hide existence too, as the engine's do (#106).
    for root in denied {
        let root = quote(root)?;
        profile.push_str(&format!(
            " (deny file-read* file-write* file-test-existence (subpath {root}))"
        ));
    }
    profile.push_str(PROCESSES);
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command.arg("-p").arg(profile).arg(shell);
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
fn confined(scratch: &Path, denied: &[PathBuf], shell: &str) -> Result<Command> {
    falinks::confine_self(denied, scratch)?;
    Ok(Command::new(shell))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn confined(_: &Path, _: &[PathBuf], _: &str) -> Result<Command> {
    Err("per-command confinement needs macOS or Linux".into())
}
