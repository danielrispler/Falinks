use falinks_codex::REQUIRED;
use falinks_host::{ControlGate, Result};

#[test]
fn missing_or_failed_fresh_controls_block_capabilities() -> Result<()> {
    let mut gate = ControlGate::new(REQUIRED);
    assert!(gate.require_supported().is_err());
    for name in REQUIRED {
        gate.record(name, true)?;
    }
    gate.require_supported()?;
    gate.record("source_write_denial", false)?;
    assert!(gate.require_supported().is_err());
    gate.record("source_write_denial", true)?;
    assert!(gate.require_supported().is_err());
    assert!(ControlGate::new(REQUIRED).require_supported().is_err());
    Ok(())
}

#[test]
fn only_declared_platforms_have_a_codex_pin() {
    let pin = falinks_codex::pin("macos", "aarch64").unwrap();
    assert_eq!(
        pin.codex,
        "112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b"
    );
    for (os, arch) in [
        ("macos", "x86_64"),
        ("linux", "x86_64"),
        ("windows", "x86_64"),
    ] {
        let error = falinks_codex::pin(os, arch).unwrap_err().to_string();
        assert!(error.contains("unsupported platform"), "{error}");
    }
}

#[test]
fn platforms_doc_lists_the_codex_pins() {
    let doc = include_str!("../../../docs/platforms.md");
    let row = falinks_host::platforms_row("Codex adapter", falinks_codex::PINS);
    assert!(doc.lines().any(|line| line == row), "missing row: {row}");
}
