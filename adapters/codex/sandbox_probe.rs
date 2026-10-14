//! The actual agent-visible control program is Rust, including its child process.
use falinks_host::{Result, file_hash, require};
use serde_json::{Map, Value, json};
use std::{
    env, fs, io,
    os::unix::process::CommandExt,
    path::Path,
    process::{self, Command},
};
// Seatbelt denies with EPERM. On Linux, bubblewrap's read-only binds fail writes with
// EROFS, and a rename between source and scratch (separate binds) fails with EXDEV.
fn denied(result: io::Result<impl Sized>) -> bool {
    result.err().is_some_and(|error| {
        matches!(
            error.raw_os_error(),
            Some(libc::EPERM) | Some(libc::EACCES) | Some(libc::EROFS) | Some(libc::EXDEV)
        )
    })
}
fn run() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|x| x == "--child-write") {
        let target = args.get(2).ok_or("missing child target")?;
        require(
            denied(fs::write(target, b"CHILD")),
            "child write was not denied",
        )?;
        println!("CHILD_DENIED");
        return Ok(());
    }
    require(
        args.len() >= 4,
        "usage: sandbox-probe SOURCE_FILE EXPECTED_HASH SCRATCH [NAME=PROTECTED_FILE]...",
    )?;
    let source = Path::new(&args[1]);
    let directory = source.parent().ok_or("source file needs a directory")?;
    let scratch_dir = Path::new(&args[3]);
    let scratch = scratch_dir.join("control.txt");
    let mut checks = Map::new();
    let mut errors = Map::new();
    checks.insert("read".into(), json!(file_hash(source)? == args[2]));
    fs::write(&scratch, b"scratch\n")?;
    checks.insert("scratch".into(), json!(fs::read(&scratch)? == b"scratch\n"));
    let mut record = |name: String, result: io::Result<()>| {
        errors.insert(
            name.clone(),
            json!(
                result
                    .as_ref()
                    .map_or_else(|e| e.to_string(), |_| "ok".into())
            ),
        );
        checks.insert(name, json!(denied(result)));
    };
    record("source_write".into(), fs::write(source, b"BYPASS"));
    record("source_delete".into(), fs::remove_file(source));
    record(
        "source_rename".into(),
        fs::rename(source, scratch_dir.join("renamed.txt")),
    );
    record(
        "source_create".into(),
        fs::write(directory.join("new.txt"), b"BYPASS"),
    );
    record("source_replace".into(), fs::rename(&scratch, source));
    // The host links `SCRATCH/alias` to the source file before the probe runs.
    record(
        "alias_write".into(),
        fs::write(scratch_dir.join("alias"), b"BYPASS"),
    );
    for protected in &args[4..] {
        let (name, secret) = protected
            .split_once('=')
            .ok_or("protected storage is NAME=FILE")?;
        record(format!("{name}_read"), fs::read(secret).map(drop));
        record(format!("{name}_write"), fs::write(secret, b"BYPASS"));
    }
    let mut child = Command::new(env::current_exe()?);
    child.arg("--child-write").arg(source);
    // SAFETY: setsid is async-signal-safe and called before exec, with no allocations.
    unsafe {
        child.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    // command_output creates a process group, which would prevent setsid. Run this
    // short fixed child directly; it performs one filesystem write and exits.
    let child = child.output()?;
    checks.insert(
        "child_write".into(),
        json!(child.status.success() && child.stdout == b"CHILD_DENIED\n"),
    );
    println!("FALINKS_CONTROLS={}", Value::Object(checks));
    // Evidence only: how each check was denied, or "ok" where the operation succeeded.
    println!("FALINKS_ERRORS={}", Value::Object(errors));
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}
