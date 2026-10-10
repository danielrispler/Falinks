use falinks_host::{Boundary, ControlGate, REQUIRED};
use serde_json::json;

#[test]
#[ignore = "subprocess fixture invoked by cleanup regression"]
fn output_holding_child() -> falinks_host::Result<()> {
    use std::{process::Command, time::Duration};
    if std::env::var_os("FALINKS_SLEEP_CHILD").is_some() {
        std::thread::sleep(Duration::from_secs(60));
    } else {
        let child = Command::new(std::env::current_exe()?)
            .args(["--ignored", "--exact", "output_holding_child"])
            .env("FALINKS_SLEEP_CHILD", "1")
            .spawn()?;
        std::fs::write(
            std::env::var_os("FALINKS_CHILD_PID_FILE").ok_or("missing fixture path")?,
            child.id().to_string(),
        )?;
    }
    Ok(())
}

#[test]
fn exited_command_cannot_leave_descendants_holding_output() -> falinks_host::Result<()> {
    use std::{process::Command, time::Duration};
    let dir = tempfile::tempdir()?;
    let pid_file = dir.path().join("descendant.pid");
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--ignored", "--exact", "output_holding_child"])
        .env("FALINKS_CHILD_PID_FILE", &pid_file);
    assert!(falinks_host::command_output(command, Duration::from_secs(2)).is_err());
    let pid: i32 = std::fs::read_to_string(pid_file)?.trim().parse()?;
    // A killed orphan can remain a zombie until reaped; it must no longer run.
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()?;
    let state = String::from_utf8(output.stdout)?;
    assert!(
        state.trim().is_empty() || state.trim().starts_with('Z'),
        "descendant still running: {state}"
    );
    Ok(())
}

#[test]
fn only_registered_runtime_identity_reaches_engine() -> falinks_host::Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().canonicalize()?;
    let mut boundary = Boundary::new(&root.join("context.sqlite"), &root, "session-a".into())?;
    boundary.bind("thread-a", "agent-a", false)?;
    boundary.begin("turn-a")?;
    let call = json!({"threadId":"thread-a","turnId":"turn-a","callId":"call-a","tool":"falinks_edit",
        "arguments":{"workspace":root,"request":{"request_id":"edit-a","content":"new"}}});
    let envelope = boundary.authenticate(&call)?;
    assert_eq!(
        envelope["identity"],
        json!({"agent":"agent-a","session":"session-a","thread":"thread-a","turn":"turn-a","workspace":root})
    );
    for (key, value) in [
        ("threadId", "forged"),
        ("turnId", "old-turn"),
        ("tool", "shell"),
    ] {
        let mut forged = call.clone();
        forged[key] = json!(value);
        assert!(boundary.authenticate(&forged).is_err());
    }
    for request in [
        json!({"author":"agent-b"}),
        json!({"identity":{"agent":"agent-b"}}),
    ] {
        let mut forged = call.clone();
        forged["arguments"]["request"] = request;
        assert!(boundary.authenticate(&forged).is_err());
    }
    let mut forged = call.clone();
    forged["arguments"]["workspace"] = json!("/unauthorized");
    assert!(boundary.authenticate(&forged).is_err());
    boundary.end("turn-a");
    assert!(boundary.authenticate(&call).is_err());
    Ok(())
}

#[test]
fn context_survives_presentation_deferral_and_resume() -> falinks_host::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let mut boundary = Boundary::new(&root.join("context.sqlite"), &root, "session-a".into())?;
    boundary.bind("thread-a", "agent-a", false)?;
    boundary.enqueue("E1", &json!({"revision":"R1","text":"reread source"}))?;
    boundary.enqueue("E1", &json!({"revision":"R1","text":"reread source"}))?;
    boundary.begin("turn-a")?;
    let mut call = json!({"threadId":"thread-a","turnId":"turn-a","callId":"call-a","tool":"falinks_review","arguments":{"workspace":root,"request":{"event":"E1","action":"acknowledge"}}});
    boundary.record_result(&boundary.authenticate(&call)?, &json!({"accepted":true}))?;
    assert_eq!(boundary.pending()?[0]["status"], "pending");
    call["arguments"]["request"]["action"] = json!("defer");
    boundary.record_result(&boundary.authenticate(&call)?, &json!({"accepted":true}))?;
    drop(boundary);
    let mut resumed = Boundary::new(&root.join("context.sqlite"), &root, "session-b".into())?;
    assert!(resumed.bind("thread-a", "agent-b", true).is_err());
    resumed.bind("thread-a", "agent-a", true)?;
    assert_eq!(
        resumed.pending()?,
        json!([{"event":"E1","context":{"revision":"R1","text":"reread source"},"status":"deferred"}])
    );
    resumed.begin("turn-b")?;
    call["turnId"] = json!("turn-b");
    call["arguments"]["request"]["action"] = json!("keep");
    resumed.record_result(&resumed.authenticate(&call)?, &json!({"accepted":true}))?;
    assert_eq!(resumed.pending()?, json!([]));
    resumed.enqueue("E2", &json!({"revision":"R2"}))?;
    resumed.enqueue("E1", &json!({"revision":"R1","text":"reread source"}))?;
    assert_eq!(resumed.pending()?.as_array().unwrap().len(), 1);
    assert_eq!(resumed.pending()?[0]["event"], "E2");
    assert!(resumed.enqueue("E2", &json!({"revision":"R3"})).is_err());
    Ok(())
}

#[test]
fn missing_or_failed_fresh_controls_block_capabilities() -> falinks_host::Result<()> {
    let mut gate = ControlGate::default();
    assert!(gate.require_supported().is_err());
    for name in REQUIRED {
        gate.record(name, true)?;
    }
    gate.require_supported()?;
    gate.record("source_write_denial", false)?;
    assert!(gate.require_supported().is_err());
    gate.record("source_write_denial", true)?;
    assert!(gate.require_supported().is_err());
    assert!(ControlGate::default().require_supported().is_err());
    Ok(())
}

#[test]
fn unknown_change_preserves_work_and_stops_even_when_old_bytes_return() -> falinks_host::Result<()>
{
    use falinks_host::controlled_host::ControlledHost;
    let dir = tempfile::tempdir()?;
    let root = dir.path().canonicalize()?;
    std::fs::create_dir(root.join("source"))?;
    std::fs::create_dir(root.join("controller"))?;
    let target = root.join("source/source.txt");
    std::fs::write(&target, b"original\n")?;
    let mut host = ControlledHost::new(&root)?;
    let mut boundary = Boundary::new(
        &root.join("context.sqlite"),
        &root.join("source"),
        "session".into(),
    )?;
    boundary.bind("thread", "agent", false)?;
    boundary.begin("turn")?;
    std::fs::write(&target, b"external work\n")?;
    let call = json!({"threadId":"thread","turnId":"turn","callId":"call","tool":"falinks_offer","arguments":{"workspace":root.join("source"),"request":{"expected_hash":"anything"}}});
    assert!(host.execute(&boundary.authenticate(&call)?).is_err());
    assert_eq!(std::fs::read_to_string(&target)?, "external work\n");
    std::fs::write(target, b"original\n")?;
    assert!(host.execute(&boundary.authenticate(&call)?).is_err());
    Ok(())
}

#[test]
fn unknown_directories_modes_and_root_aliases_stop_the_host() -> falinks_host::Result<()> {
    use falinks_host::controlled_host::ControlledHost;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    for change in ["empty_directory", "directory_mode", "root_alias"] {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        let source = root.join("source");
        fs::create_dir(&source)?;
        fs::create_dir(root.join("controller"))?;
        fs::write(source.join("source.txt"), b"original\n")?;
        let host = ControlledHost::new(&root)?;
        match change {
            "empty_directory" => fs::create_dir(source.join("unexpected"))?,
            "directory_mode" => fs::set_permissions(
                &source,
                fs::Permissions::from_mode(fs::metadata(&source)?.permissions().mode() ^ 0o100),
            )?,
            _ => {
                fs::rename(&source, root.join("original-source"))?;
                symlink(root.join("original-source"), &source)?;
            }
        }
        assert!(host.integrity().is_err(), "{change}");
    }
    Ok(())
}

#[test]
fn relocation_rebinds_only_an_existing_thread_for_resume() -> falinks_host::Result<()> {
    let dir = tempfile::tempdir()?;
    let (first, second) = (dir.path().join("first"), dir.path().join("second"));
    std::fs::create_dir(&first)?;
    std::fs::create_dir(&second)?;
    let db = dir.path().join("context.sqlite");
    Boundary::new(&db, &first, "c1".into())?.bind("thread", "agent", false)?;
    let mut moved = Boundary::new(&db, &second, "c2".into())?;
    assert!(moved.bind("thread", "agent", true).is_err());
    assert!(moved.relocate("thread", "other-agent").is_err());
    moved.relocate("thread", "agent")?;
    moved.bind("thread", "agent", true)?;
    Ok(())
}
