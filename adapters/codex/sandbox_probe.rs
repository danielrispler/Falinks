//! The actual agent-visible control program is Rust, including its child process.
use falinks_host::{Result, file_hash, require};
use serde_json::{Map, Value, json};
use std::{
    env, fs, io,
    os::unix::process::CommandExt,
    path::Path,
    process::{self, Command},
};
fn denied(result: io::Result<impl Sized>) -> bool {
    result
        .err()
        .is_some_and(|error| matches!(error.raw_os_error(), Some(libc::EPERM) | Some(libc::EACCES)))
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
    require(args.len() == 3, "usage: sandbox-probe ROOT EXPECTED_HASH")?;
    let root = Path::new(&args[1]);
    let source = root.join("source/source.txt");
    let scratch = root.join("scratch/control.txt");
    let mut checks = Map::new();
    checks.insert("read".into(), json!(file_hash(&source)? == args[2]));
    fs::write(&scratch, b"scratch\n")?;
    checks.insert("scratch".into(), json!(fs::read(&scratch)? == b"scratch\n"));
    checks.insert(
        "source_write".into(),
        json!(denied(fs::write(&source, b"BYPASS"))),
    );
    checks.insert(
        "source_delete".into(),
        json!(denied(fs::remove_file(&source))),
    );
    checks.insert(
        "source_rename".into(),
        json!(denied(fs::rename(
            &source,
            root.join("scratch/renamed.txt")
        ))),
    );
    checks.insert(
        "source_create".into(),
        json!(denied(fs::write(root.join("source/new.txt"), b"BYPASS"))),
    );
    checks.insert(
        "source_replace".into(),
        json!(denied(fs::rename(&scratch, &source))),
    );
    checks.insert(
        "alias_write".into(),
        json!(denied(fs::write(root.join("scratch/alias"), b"BYPASS"))),
    );
    for name in ["controller", "snapshots", "validation"] {
        let secret = root.join(name).join("secret.txt");
        checks.insert(format!("{name}_read"), json!(denied(fs::read(&secret))));
        checks.insert(
            format!("{name}_write"),
            json!(denied(fs::write(&secret, b"BYPASS"))),
        );
    }
    let mut child = Command::new(env::current_exe()?);
    child.arg("--child-write").arg(&source);
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
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}
