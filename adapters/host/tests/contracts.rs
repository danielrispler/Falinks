//! The shared adapter contracts: mediation, sandbox probe and pinned-binary verification.
use falinks_host::{
    Boundary, ControlGate, Engine, EngineOutcome, OPERATIONS, mediate, probe_args, probe_controls,
    verify_pinned,
};
use serde_json::{Value, json};
use std::{cell::RefCell, path::Path, rc::Rc};

#[test]
fn mediation_refuses_after_a_failure_and_latches_engine_errors() -> falinks_host::Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().canonicalize()?;
    let mut boundary = Boundary::new(
        &root.join("context.sqlite"),
        &root,
        "session-a".into(),
        OPERATIONS,
    )?;
    boundary.bind("thread-a", "agent-a", false)?;
    boundary.enqueue("E1", &json!({"revision":"R1"}))?;
    boundary.begin("turn-a")?;
    let calls = Rc::new(RefCell::new(0));
    let seen = Rc::clone(&calls);
    let mut engine: Engine = Box::new(move |envelope: &Value| {
        *seen.borrow_mut() += 1;
        match envelope["request"]["action"].as_str() {
            Some("fail") => Err("engine incident".into()),
            _ => Ok(EngineOutcome {
                result: json!({"accepted":true}),
                notice: None,
                ..Default::default()
            }),
        }
    });
    let mut gate = ControlGate::new(&["pin"]);
    let call = |action: &str| {
        json!({"threadId":"thread-a","turnId":"turn-a","callId":"call-a","tool":"falinks_review",
            "arguments":{"event":"E1","action":action}})
    };

    let (envelope, outcome) = mediate(&mut gate, &boundary, &mut engine, &call("defer"))?;
    assert_eq!(envelope["identity"]["agent"], "agent-a");
    assert_eq!(outcome.result, json!({"accepted":true}));
    assert_eq!(boundary.pending()?[0]["status"], "deferred");

    let mut forged = call("keep");
    forged["threadId"] = json!("forged");
    assert!(mediate(&mut gate, &boundary, &mut engine, &forged).is_err());
    assert!(
        !gate.failed,
        "an authentication refusal is not an engine failure"
    );

    assert!(mediate(&mut gate, &boundary, &mut engine, &call("fail")).is_err());
    assert!(gate.failed);
    let Err(error) = mediate(&mut gate, &boundary, &mut engine, &call("keep")) else {
        panic!("a latched gate admitted a call");
    };
    assert!(error.to_string().contains("safety failure"), "{error}");
    assert_eq!(*calls.borrow(), 2, "no engine call after the latch");
    Ok(())
}

#[test]
fn probe_arguments_follow_the_probe_usage() {
    let args = probe_args(
        Path::new("/t/sandbox-probe"),
        Path::new("/s/a.go"),
        "abc",
        Path::new("/x"),
        &[("controller", Path::new("/c/m.txt"))],
    );
    assert_eq!(
        args,
        [
            "/t/sandbox-probe",
            "/s/a.go",
            "abc",
            "/x",
            "controller=/c/m.txt"
        ]
    );
    assert!(probe_controls("no controls line", &args).unwrap().is_none());
}

#[test]
fn probe_controls_need_every_check_and_host_proof_for_protected_writes() {
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "protected fixture\n").unwrap();
    let args = probe_args(
        Path::new("probe"),
        Path::new("source.txt"),
        "hash",
        Path::new("scratch"),
        &[("controller", &secret)],
    );
    let controls: serde_json::Map<String, Value> = [
        "read",
        "scratch",
        "source_write",
        "source_delete",
        "source_rename",
        "source_create",
        "source_replace",
        "alias_write",
        "child_write",
        "controller_read",
        "controller_write",
    ]
    .iter()
    .map(|n| (n.to_string(), json!(true)))
    .collect();
    let report = |c: &serde_json::Map<String, Value>| {
        format!(
            "noise\nFALINKS_CONTROLS={}\nFALINKS_ERRORS={{}}\n",
            json!(c)
        )
    };
    let stdout = report(&controls);
    let checks = probe_controls(&stdout, &args).unwrap().unwrap();
    assert_eq!(checks["controller_write"], json!(true));
    // A write the probe saw succeed fails, even when the real file is unchanged.
    let mut succeeded = controls.clone();
    succeeded.insert("controller_write".into(), json!(false));
    assert!(probe_controls(&report(&succeeded), &args).is_err());
    // A missing check fails even when every reported check passed.
    let mut short = controls.clone();
    short.remove("controller_read");
    assert!(probe_controls(&report(&short), &args).is_err());
    let mut failed = controls.clone();
    failed.insert("source_write".into(), json!(false));
    assert!(probe_controls(&report(&failed), &args).is_err());

    std::fs::write(&secret, "BYPASS").unwrap();
    let error = probe_controls(&stdout, &args).unwrap_err();
    assert!(
        error.to_string().contains("filesystem controls failed"),
        "{error}"
    );

    std::fs::remove_file(&secret).unwrap();
    assert!(probe_controls(&stdout, &args).is_err());
}

#[test]
fn only_the_pinned_binary_runs_and_its_verified_digest_is_returned() -> falinks_host::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let binary = dir.path().join("runtime");
    let ran = dir.path().join("ran");
    std::fs::write(
        &binary,
        format!("#!/bin/sh\ntouch {}\necho \"1.0 $FLAVOR\"\n", ran.display()),
    )?;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))?;
    let platform = (std::env::consts::OS, std::env::consts::ARCH);
    let pins = |digest: String| -> falinks_host::PinMap<&'static str> {
        Box::leak(Box::new([(platform, &*digest.leak())]))
    };
    let pinned = pins(falinks_host::file_hash(&binary)?);
    let flavor = [("FLAVOR", "pinned")];

    let Err(error) = verify_pinned(&binary, pins("0".repeat(64)), "1.0 pinned", &flavor) else {
        panic!("an unpinned binary verified");
    };
    assert!(error.to_string().contains("SHA-256"), "{error}");
    assert!(!ran.exists(), "an unpinned binary must never run");

    assert!(verify_pinned(&binary, pinned, "2.0 pinned", &flavor).is_err());
    assert!(verify_pinned(&binary, pinned, "1.0 pinned", &[]).is_err());
    let (canonical, digest) = verify_pinned(&binary, pinned, "1.0 pinned", &flavor)?;
    assert_eq!(canonical, binary.canonicalize()?);
    assert_eq!(digest, pinned[0].1);
    assert!(verify_pinned(&dir.path().join("missing"), pinned, "1.0 pinned", &flavor).is_err());
    Ok(())
}
