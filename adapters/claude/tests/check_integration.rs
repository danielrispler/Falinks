//! Gate setup failures name their cause before any runtime launch.
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn missing_sibling_fails_before_launch_and_names_the_path() {
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("launched");
    // A runtime that would leave a marker if the gate ever launched it.
    let runtime = tmp.path().join("claude");
    std::fs::write(&runtime, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    let missing = tmp.path().join("no-such-probe");
    let output = Command::new(env!("CARGO_BIN_EXE_check-integration"))
        .arg("--binary")
        .arg(&runtime)
        .arg("--output")
        .arg(tmp.path().join("report.json"))
        .arg("--probe")
        .arg(&missing)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains(&missing.display().to_string()), "{stderr}");
    assert!(!marker.exists());
}
