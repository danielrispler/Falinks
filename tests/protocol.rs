mod support;

use falinks::*;
use std::fs;
use support::{
    PAIR, edit, fail_commits, fails_with, fixture, halt, incidents, ledger, open, reopen, snapshot,
};

#[test]
fn two_clients_preserve_independent_drafts_and_reject_stale_output_in_full() {
    let (_dir, engine, alice, bob) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let first = engine
        .apply(&alice, edit("a", &base, "a.rs", "fn a( unfinished"))
        .unwrap();
    assert!(matches!(first, Outcome::Applied { .. }));
    engine
        .apply(&bob, edit("b", &base, "b.rs", "fn b() { peer(); }\n"))
        .unwrap();
    let mut stale = edit("stale", &base, "a.rs", "lost draft");
    stale
        .expected
        .insert("b.rs".into(), base.files["b.rs"].version);
    stale.output.insert(
        "b.rs".into(),
        Some(File {
            bytes: b"lost peer".to_vec(),
            executable: false,
        }),
    );
    assert!(matches!(
        engine.apply(&bob, stale.clone()).unwrap(),
        Outcome::Stale { .. }
    ));
    assert_eq!(
        engine.request(&bob, "stale").unwrap().unwrap().request,
        stale
    );
    let current = engine.capture().unwrap();
    assert_eq!(
        current.files["a.rs"].file.as_ref().unwrap().bytes,
        b"fn a( unfinished"
    );
    assert_eq!(
        current.files["b.rs"].file.as_ref().unwrap().bytes,
        b"fn b() { peer(); }\n"
    );
    assert_eq!(
        engine
            .history()
            .unwrap()
            .iter()
            .filter(|r| matches!(r.outcome, Some(Outcome::Applied { .. })))
            .map(|r| r.agent)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn capture_during_installation_keeps_the_previous_completed_revision() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let mut request = edit("multi", &base, "a.rs", "new a");
    request.expected.insert("b.rs".into(), 0);
    request.output.insert(
        "b.rs".into(),
        Some(File {
            bytes: b"new b".to_vec(),
            executable: false,
        }),
    );
    let halfway = std::sync::Barrier::new(2);
    let resume = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            engine
                .apply_observed(&alice, request, |phase| {
                    if phase == Phase::Installed("a.rs".into()) {
                        halfway.wait();
                        resume.wait();
                    }
                    Ok(())
                })
                .unwrap()
        });
        halfway.wait();
        assert_eq!(engine.capture().unwrap(), base);
        resume.wait();
        assert!(matches!(worker.join().unwrap(), Outcome::Applied { .. }));
    });
    let current = engine.capture().unwrap();
    assert_ne!(current.revision, base.revision);
    assert_eq!(current.files["b.rs"].file.as_ref().unwrap().bytes, b"new b");
    assert_eq!(engine.published().unwrap(), base);
}

#[test]
fn restored_bytes_are_a_new_occurrence_and_duplicate_requests_do_not_apply_twice() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let request = edit("change", &base, "a.rs", "different");
    let outcome = engine.apply(&alice, request.clone()).unwrap();
    assert_eq!(engine.apply(&alice, request.clone()).unwrap(), outcome);
    fails_with(
        engine.apply(&alice, edit("change", &base, "a.rs", "changed ID contents")),
        "request ID reused with changed contents",
    );
    let changed = engine.capture().unwrap();
    engine
        .apply(&alice, edit("restore", &changed, "a.rs", "fn a() {}\n"))
        .unwrap();
    let restored = engine.capture().unwrap();
    assert_eq!(restored.tree, base.tree);
    assert_ne!(restored.files["a.rs"].version, base.files["a.rs"].version);
    assert_eq!(changed.files["b.rs"].blob, base.files["b.rs"].blob);
    drop(engine);
    let reopened = open(dir.path(), PAIR).unwrap();
    let alice = reopened
        .authenticate(0, reopened.credentials()[0].as_str())
        .unwrap();
    assert_eq!(reopened.apply(&alice, request).unwrap(), outcome);
    assert_eq!(reopened.capture().unwrap(), restored);
}

#[test]
fn interrupted_installation_retains_evidence_and_requires_explicit_retry() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let mut request = edit("crash", &base, "a.rs", "draft a");
    request.expected.insert("b.rs".into(), 0);
    request.output.insert(
        "b.rs".into(),
        Some(File {
            bytes: b"draft b".to_vec(),
            executable: false,
        }),
    );
    let outcome = engine
        .apply_observed(&alice, request.clone(), |phase| {
            if matches!(phase, Phase::Installed(_)) {
                return Err("controlled interruption".into());
            }
            Ok(())
        })
        .unwrap();
    assert!(matches!(outcome, Outcome::Interrupted { .. }));
    assert_eq!(engine.capture().unwrap(), base);
    assert!(
        engine
            .request(&alice, "crash")
            .unwrap()
            .unwrap()
            .proposed
            .is_some()
    );
    drop(engine);
    let engine = open(dir.path(), PAIR).unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    assert_eq!(engine.capture().unwrap(), base);
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"fn a() {}\n"
    );
    assert!(matches!(
        engine.apply(&alice, request.clone()).unwrap(),
        Outcome::Interrupted { .. }
    ));
    assert!(matches!(
        engine.retry(&alice, request).unwrap(),
        Outcome::Applied { .. }
    ));
}

#[test]
fn recovered_interruption_is_not_replayed_over_later_work_and_keeps_attempt_evidence() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let request = edit("interrupted", &base, "a.rs", "abandoned output");
    engine
        .apply_observed(&alice, request.clone(), |phase| {
            if matches!(phase, Phase::Installed(_)) {
                return Err("stop".into());
            }
            Ok(())
        })
        .unwrap();
    drop(engine);
    let engine = open(dir.path(), PAIR).unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    engine
        .apply(&alice, edit("later", &base, "a.rs", "later draft"))
        .unwrap();
    let current = engine.capture().unwrap();
    assert!(matches!(
        engine.retry(&alice, request).unwrap(),
        Outcome::Stale { .. }
    ));
    assert!(
        !engine
            .request(&alice, "interrupted")
            .unwrap()
            .unwrap()
            .attempts
            .is_empty()
    );
    drop(engine);
    let engine = open(dir.path(), PAIR).unwrap();
    assert_eq!(engine.capture().unwrap(), current);
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"later draft"
    );
}

#[test]
fn captured_job_applies_coherent_output_with_invoker_attribution() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let spec = JobSpec {
        id: "generator".into(),
        revision: base.revision,
        inputs: vec!["a.rs".into()],
        outputs: vec!["b.rs".into()],
        program: "/bin/sh".into(),
        args: vec!["-ec".into(), "cat ../input/a.rs > b.rs".into()],
    };
    let job = engine.run_job(&alice, spec).unwrap();
    assert!(
        job.error.is_none(),
        "{:?}: {}",
        job.error,
        String::from_utf8_lossy(&job.stderr)
    );
    assert!(matches!(
        engine.apply_job(&alice, "generator").unwrap(),
        Outcome::Applied { .. }
    ));
    assert_eq!(
        engine.capture().unwrap().files["b.rs"]
            .file
            .as_ref()
            .unwrap()
            .bytes,
        b"fn a() {}\n"
    );
    assert_eq!(engine.history().unwrap().last().unwrap().agent, 0);
}

#[test]
fn stale_captured_job_retains_all_output_and_rejects_the_whole_application() {
    let (_dir, engine, alice, bob) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let spec = JobSpec {
        id: "stale-job".into(),
        revision: 0,
        inputs: vec!["a.rs".into()],
        outputs: vec!["a.rs".into(), "b.rs".into()],
        program: "/bin/sh".into(),
        args: vec![
            "-ec".into(),
            "printf generated > a.rs; printf generated > b.rs".into(),
        ],
    };
    let job = engine.run_job(&alice, spec.clone()).unwrap();
    assert!(
        job.error.is_none(),
        "{:?}: {}",
        job.error,
        String::from_utf8_lossy(&job.stderr)
    );
    engine
        .apply(&bob, edit("peer", &base, "a.rs", "peer draft"))
        .unwrap();
    let current = engine.capture().unwrap();
    assert!(matches!(
        engine.apply_job(&alice, "stale-job").unwrap(),
        Outcome::Stale { .. }
    ));
    assert_eq!(engine.capture().unwrap(), current);
    assert_eq!(
        engine
            .job(&alice, "stale-job")
            .unwrap()
            .unwrap()
            .request
            .output["b.rs"]
            .as_ref()
            .unwrap()
            .bytes,
        b"generated"
    );
    assert_eq!(
        engine.run_job(&alice, spec).unwrap().directory,
        job.directory
    );
}

#[test]
fn unmediated_writes_halt_mutations_without_overwriting_evidence() {
    let (dir, engine, alice, bob) = fixture(PAIR);
    let base = engine.capture().unwrap();
    fs::write(dir.path().join("live/a.rs"), "unknown author").unwrap();
    assert!(matches!(
        engine
            .apply(&alice, edit("blocked", &base, "b.rs", "output"))
            .unwrap(),
        Outcome::Rejected { .. }
    ));
    assert!(matches!(
        engine
            .apply(&bob, edit("also-blocked", &base, "b.rs", "other"))
            .unwrap(),
        Outcome::Rejected { .. }
    ));
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"unknown author"
    );
    assert_eq!(
        fs::read(dir.path().join("live/b.rs")).unwrap(),
        b"fn b() {}\n"
    );
    assert!(engine.request(&alice, "blocked").unwrap().is_some());
    drop(engine);
    fails_with(open(dir.path(), PAIR), "unexplained source write");
}

#[test]
fn ownership_authentication_path_aliases_and_modes_fail_closed() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (dir, engine, alice, _) = fixture(PAIR);
    fails_with(
        engine.authenticate(1, &engine.credentials()[0]),
        "authentication failed",
    );
    fails_with(open(dir.path(), PAIR), "engine writer already owned");
    let base = engine.capture().unwrap();
    let mut bad = edit("traversal", &base, "a.rs", "bad");
    bad.expected.insert("../outside".into(), 0);
    bad.output.insert("../outside".into(), None);
    assert!(matches!(
        engine.apply(&alice, bad).unwrap(),
        Outcome::Rejected { .. }
    ));
    fs::remove_file(dir.path().join("live/a.rs")).unwrap();
    symlink("b.rs", dir.path().join("live/a.rs")).unwrap();
    assert!(matches!(
        engine
            .apply(&alice, edit("symlink", &base, "a.rs", "bad"))
            .unwrap(),
        Outcome::Rejected { .. }
    ));
    assert_eq!(
        fs::read(dir.path().join("live/b.rs")).unwrap(),
        b"fn b() {}\n"
    );
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join("live")).unwrap();
    fs::write(d.path().join("live/a.rs"), "source").unwrap();
    fs::set_permissions(
        d.path().join("live/a.rs"),
        fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    fails_with(
        Engine::open(&d.path().join("live"), &d.path().join("state"), &["a.rs"]),
        "unsupported source mode",
    );
}

#[test]
fn duplicate_inflight_submissions_join_the_recorded_operation() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let request = edit("duplicate", &base, "a.rs", "one occurrence");
    let started = std::sync::Barrier::new(2);
    let release = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            engine
                .apply_observed(&alice, request.clone(), |phase| {
                    if phase == Phase::Retained {
                        started.wait();
                        release.wait();
                    }
                    Ok(())
                })
                .unwrap()
        });
        started.wait();
        let second = scope.spawn(|| {
            release.wait();
            engine.apply(&alice, request.clone()).unwrap()
        });
        assert_eq!(first.join().unwrap(), second.join().unwrap());
    });
    assert_eq!(engine.history().unwrap().len(), 1);
}

#[test]
fn missing_objects_fail_capture_and_restart_without_resetting_drafts() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    engine
        .apply(&alice, edit("draft", &base, "a.rs", "unfinished draft"))
        .unwrap();
    let blob = base.files["b.rs"].blob.as_ref().unwrap();
    fs::remove_file(
        dir.path()
            .join("state/objects.git/objects")
            .join(&blob[..2])
            .join(&blob[2..]),
    )
    .unwrap();
    fails_with(engine.capture(), "missing snapshot blob");
    drop(engine);
    fails_with(open(dir.path(), PAIR), "missing snapshot blob");
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"unfinished draft"
    );
}

#[test]
fn job_boundary_denies_source_and_controller_writes_and_detects_surviving_descendants() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    for (id, script) in [
        (
            "source-write",
            format!("printf bad > '{}'", dir.path().join("live/a.rs").display()),
        ),
        (
            "controller-read",
            format!(
                "cat '{}' > b.rs",
                dir.path().join("state/ledger.sqlite").display()
            ),
        ),
        ("input-write", "printf bad > ../input/a.rs".into()),
        ("extra-output", "printf unexpected > extra.rs".into()),
        ("background", "sleep 10 &".into()),
    ] {
        let job = engine
            .run_job(
                &alice,
                JobSpec {
                    id: id.into(),
                    revision: 0,
                    inputs: vec!["a.rs".into()],
                    outputs: vec!["b.rs".into()],
                    program: "/bin/sh".into(),
                    args: vec!["-ec".into(), script],
                },
            )
            .unwrap();
        assert!(job.error.is_some(), "unsupported job {id} acknowledged");
        assert!(job.directory.exists());
        fails_with(engine.apply_job(&alice, id), "job unavailable");
    }
    assert_eq!(engine.capture().unwrap(), base);
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"fn a() {}\n"
    );
}

fn shell_job(engine: &Engine, client: &Client, id: &str, program: &str, script: &str) -> Job {
    engine
        .run_job(
            client,
            JobSpec {
                id: id.into(),
                revision: 0,
                inputs: vec!["a.rs".into()],
                outputs: vec!["b.rs".into()],
                program: program.into(),
                args: vec!["-ec".into(), script.into()],
            },
        )
        .unwrap()
}

#[test]
fn job_network_attempt_is_denied() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let job = shell_job(
        &engine,
        &alice,
        "network",
        "/bin/bash",
        &format!("exec 3<>/dev/tcp/127.0.0.1/{port}; printf connected > b.rs"),
    );
    assert!(job.error.is_some(), "network job acknowledged");
    assert!(listener.accept().is_err(), "job reached the host network");
    fails_with(engine.apply_job(&alice, "network"), "job unavailable");
}

#[test]
fn job_udp_attempt_is_denied() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let job = shell_job(
        &engine,
        &alice,
        "udp",
        "/bin/bash",
        &format!("printf leak > /dev/udp/127.0.0.1/{port}; printf sent > b.rs"),
    );
    assert!(job.error.is_some(), "UDP job acknowledged");
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        listener.recv(&mut [0; 16]).is_err(),
        "job reached the host network"
    );
}

#[test]
fn job_timeout_kills_the_command_and_retains_evidence() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    engine
        .configure_job_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let job = shell_job(&engine, &alice, "timeout", "/bin/sh", "sleep 60");
    assert!(job.error.as_deref().unwrap().contains("timed out"));
    assert!(job.directory.join("stdout").exists());
    fails_with(engine.apply_job(&alice, "timeout"), "timed out");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_missing_containment_helper_fails_closed() {
    let (dir, engine, alice, _) = fixture(PAIR);
    engine
        .configure_containment(dir.path().join("missing-helper"))
        .unwrap();
    let job = shell_job(
        &engine,
        &alice,
        "no-helper",
        "/bin/sh",
        "cat ../input/a.rs > b.rs",
    );
    assert!(
        job.error
            .as_deref()
            .unwrap()
            .contains("containment unavailable")
    );
    fails_with(engine.apply_job(&alice, "no-helper"), "job unavailable");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_subreaper_sweep_kills_setsid_descendants() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let job = shell_job(
        &engine,
        &alice,
        "setsid",
        "/bin/sh",
        "setsid -f /bin/sh -c 'echo $$ > pid.tmp; mv pid.tmp pid; exec sleep 60'; \
         while [ ! -f pid ]; do sleep 0.1; done",
    );
    assert!(
        job.error
            .as_deref()
            .unwrap()
            .contains("ambiguous completion")
    );
    let pid = fs::read_to_string(job.directory.join("output/pid")).unwrap();
    assert!(
        !std::path::Path::new(&format!("/proc/{}", pid.trim())).exists(),
        "setsid descendant survived the sweep"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_sweep_kills_setsid_descendants() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let job = shell_job(
        &engine,
        &alice,
        "setsid",
        "/bin/sh",
        "/usr/bin/perl -MPOSIX -e 'exit if fork; POSIX::setsid(); \
         open F, q(>pid.tmp); print F $$; close F; rename q(pid.tmp), q(pid); sleep 60'; \
         while [ ! -f pid ]; do sleep 0.1; done",
    );
    assert!(
        job.error
            .as_deref()
            .unwrap()
            .contains("ambiguous completion")
    );
    let pid = fs::read_to_string(job.directory.join("output/pid")).unwrap();
    let alive = || {
        std::process::Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    // launchd reaps the killed escapee asynchronously.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while alive() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!alive(), "setsid descendant survived the sweep");
}

#[test]
fn engine_open_fails_visibly_without_git_2_32() {
    let bin = tempfile::tempdir().unwrap();
    let old = bin.path().join("old");
    fs::create_dir(&old).unwrap();
    fs::write(old.join("git"), "#!/bin/sh\necho 'git version 2.31.8'\n").unwrap();
    fs::set_permissions(
        old.join("git"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    for (path, expected) in [
        (bin.path().join("empty"), "Git not found on PATH"),
        (old, "Git 2.32 or newer is required"),
    ] {
        fs::create_dir_all(&path).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "open_worker", "--nocapture"])
            .env("PATH", &path)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(expected), "{expected}: {stdout}");
    }
}

#[test]
#[ignore = "subprocess Engine::open worker; invoked by engine_open_fails_visibly_without_git_2_32"]
fn open_worker() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("live")).unwrap();
    fs::write(dir.path().join("live/a.rs"), "fn a() {}\n").unwrap();
    let error = Engine::open(
        &dir.path().join("live"),
        &dir.path().join("state"),
        &["a.rs"],
    )
    .err()
    .expect("open succeeded");
    println!("{error}");
}

/// Replaces `a.rs` with `new a` and `b.rs` with `new b`, installing `a.rs` first.
fn two_files(id: &str, base: &Capture) -> Request {
    let mut request = edit(id, base, "a.rs", "new a");
    request.expected.insert("b.rs".into(), 0);
    request.output.insert(
        "b.rs".into(),
        Some(File {
            bytes: b"new b".to_vec(),
            executable: false,
        }),
    );
    request
}
/// Runs `crash_worker` over a fixture root; the process exits at `phase`.
fn crash(root: &std::path::Path, phase: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "crash_worker"])
        .env("FALINKS_CRASH_ROOT", root)
        .env("FALINKS_CRASH_PHASE", phase)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(73));
}

#[test]
#[ignore = "subprocess crash worker; invoked by crash_boundaries"]
fn crash_worker() {
    let root = std::path::PathBuf::from(std::env::var("FALINKS_CRASH_ROOT").unwrap());
    let phase = std::env::var("FALINKS_CRASH_PHASE").unwrap();
    let engine = open(&root, PAIR).unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let base = engine.capture().unwrap();
    engine
        .apply_observed(&alice, two_files("process-crash", &base), |observed| {
            let selected = match phase.as_str() {
                "before-retention" => observed == Phase::BeforeRetention,
                "retained" => observed == Phase::Retained,
                "halfway" => observed == Phase::Installed("a.rs".into()),
                "before-commit" => observed == Phase::BeforeCommit,
                "committed" => observed == Phase::Committed,
                _ => panic!("unknown crash phase"),
            };
            if selected {
                std::process::exit(73);
            }
            Ok(())
        })
        .unwrap();
    panic!("crash barrier missed");
}

#[test]
fn crash_boundaries_preserve_drafts_and_never_promote_incomplete_installations() {
    for phase in [
        "before-retention",
        "retained",
        "halfway",
        "before-commit",
        "committed",
    ] {
        let (dir, engine, _, _) = fixture(PAIR);
        let base = engine.capture().unwrap();
        drop(engine);
        crash(dir.path(), phase);
        let engine = open(dir.path(), PAIR).unwrap();
        let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
        let record = engine.request(&alice, "process-crash").unwrap().unwrap();
        if phase == "committed" {
            assert!(matches!(record.outcome, Some(Outcome::Applied { .. })));
            assert_eq!(
                engine.capture().unwrap().files["b.rs"]
                    .file
                    .as_ref()
                    .unwrap()
                    .bytes,
                b"new b"
            );
            assert_eq!(
                engine.apply(&alice, record.request).unwrap(),
                record.outcome.unwrap()
            );
        } else {
            assert!(matches!(record.outcome, Some(Outcome::Interrupted { .. })));
            assert_eq!(engine.capture().unwrap(), base, "{phase}");
            assert_eq!(
                fs::read(dir.path().join("live/a.rs")).unwrap(),
                b"fn a() {}\n"
            );
        }
        assert_eq!(engine.published().unwrap(), base);
    }
}

#[test]
fn ambiguous_restart_preserves_all_files_instead_of_partially_restoring() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    engine
        .apply_observed(&alice, two_files("ambiguous", &base), |phase| {
            if phase == Phase::Installed("a.rs".into()) {
                return Err("interrupt".into());
            }
            Ok(())
        })
        .unwrap();
    drop(engine);
    fs::write(dir.path().join("live/b.rs"), "external draft").unwrap();
    fails_with(open(dir.path(), PAIR), "ambiguous interrupted source");
    assert_eq!(fs::read(dir.path().join("live/a.rs")).unwrap(), b"new a");
    assert_eq!(
        fs::read(dir.path().join("live/b.rs")).unwrap(),
        b"external draft"
    );
}

#[test]
fn unknown_write_evidence_survives_even_if_live_bytes_are_later_changed() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    fs::write(dir.path().join("live/a.rs"), "unattributed evidence").unwrap();
    engine
        .apply(&alice, edit("incident", &base, "b.rs", "retained proposal"))
        .unwrap();
    fs::write(dir.path().join("live/a.rs"), "later external change").unwrap();
    let incident = engine.incident().unwrap().unwrap();
    assert_eq!(
        incident.files["a.rs"].bytes.as_deref(),
        Some(b"unattributed evidence".as_slice())
    );
    assert!(incident.reason.contains("unexplained"));
}

#[test]
fn retention_failure_never_installs_or_acknowledges_the_proposal() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let request = edit("retention-failure", &base, "a.rs", "retained draft");
    let git = dir.path().join("state/objects.git");
    let unavailable = dir.path().join("state/objects.unavailable");
    let result = engine
        .apply_observed(&alice, request.clone(), |phase| {
            if phase == Phase::BeforeRetention {
                fs::rename(&git, &unavailable)?;
            }
            Ok(())
        })
        .unwrap();
    assert!(matches!(result, Outcome::Interrupted { .. }));
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"fn a() {}\n"
    );
    fs::rename(unavailable, git).unwrap();
    assert_eq!(engine.capture().unwrap(), base);
    assert_eq!(
        engine
            .request(&alice, "retention-failure")
            .unwrap()
            .unwrap()
            .request,
        request
    );
}

#[test]
fn lost_committed_response_recovers_the_same_outcome() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let request = edit("lost-response", &base, "a.rs", "committed draft");
    assert!(
        engine
            .apply_observed(&alice, request.clone(), |phase| {
                if phase == Phase::Committed {
                    return Err("response lost".into());
                }
                Ok(())
            })
            .is_err()
    );
    let recovered = engine.apply(&alice, request).unwrap();
    assert_eq!(
        Some(recovered),
        engine
            .request(&alice, "lost-response")
            .unwrap()
            .unwrap()
            .outcome
    );
    assert_eq!(engine.history().unwrap().len(), 1);
}

#[test]
fn repository_writer_ownership_cannot_be_bypassed_with_a_second_state_directory() {
    let (dir, _engine, _, _) = fixture(PAIR);
    fails_with(
        Engine::open(
            &dir.path().join("live"),
            &dir.path().join("other-state"),
            &["a.rs", "b.rs"],
        ),
        "engine writer already owned",
    );
}

#[test]
fn startup_incidents_remain_halted_after_external_bytes_are_put_back() {
    let (dir, engine, _, _) = fixture(PAIR);
    drop(engine);
    fs::write(dir.path().join("live/a.rs"), "startup external evidence").unwrap();
    fails_with(open(dir.path(), PAIR), "unexplained source write");
    let incident = Engine::incident_at(&dir.path().join("state"))
        .unwrap()
        .unwrap();
    assert_eq!(
        incident.files["a.rs"].bytes.as_deref(),
        Some(b"startup external evidence".as_slice())
    );
    fs::write(dir.path().join("live/a.rs"), "fn a() {}\n").unwrap();
    fails_with(open(dir.path(), PAIR), "engine halted for reconciliation");
}

#[test]
fn explicit_retry_after_independent_progress_creates_a_later_completed_occurrence() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let request = edit("retry-later", &base, "a.rs", "alice draft");
    let interrupted = engine
        .apply_observed(&alice, request.clone(), |phase| {
            if phase == Phase::Retained {
                return Err("interrupted before installation".into());
            }
            Ok(())
        })
        .unwrap();
    fails_with(
        engine.retry(&alice, request.clone()),
        "restart reconciliation required",
    );
    drop(engine);
    let (engine, alice, bob) = reopen(dir.path(), PAIR);
    assert_eq!(engine.apply(&alice, request.clone()).unwrap(), interrupted);
    engine
        .apply(&bob, edit("independent", &base, "b.rs", "bob draft"))
        .unwrap();
    let preceding = engine.capture().unwrap();
    engine.retry(&alice, request).unwrap();
    let latest = engine.capture().unwrap();
    assert!(latest.revision > preceding.revision);
    assert_eq!(
        latest.files["b.rs"].file.as_ref().unwrap().bytes,
        b"bob draft"
    );
    assert_eq!(
        latest.files["a.rs"].file.as_ref().unwrap().bytes,
        b"alice draft"
    );
}

#[test]
fn dropping_an_engine_releases_ownership_even_while_a_fork_copy_exists() {
    let (dir, engine, _, _) = fixture(PAIR);
    let mut pipe = [0; 2];
    // SAFETY: `pipe` points to a writable array of two c_ints.
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    // SAFETY: the child only makes async-signal-safe calls before `_exit`.
    let child = unsafe { libc::fork() };
    assert!(child >= 0);
    if child == 0 {
        // SAFETY: only async-signal-safe libc calls between fork and exit; `byte` is a valid one-byte buffer.
        unsafe {
            libc::close(pipe[1]);
            let mut byte = 0u8;
            libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
            libc::_exit(0);
        }
    }
    // SAFETY: closes the read end owned by this process; no pointers involved.
    unsafe {
        libc::close(pipe[0]);
    }
    drop(engine);
    let reopened = open(dir.path(), PAIR);
    // SAFETY: the buffer is a valid one-byte static; the fds and child pid were created above.
    unsafe {
        libc::write(pipe[1], b"x".as_ptr().cast(), 1);
        libc::close(pipe[1]);
        libc::waitpid(child, std::ptr::null_mut(), 0);
    }
    assert!(reopened.is_ok());
}

#[test]
fn different_functions_in_one_file_both_survive_an_explicit_reread_and_rebuild() {
    let (_dir, engine, alice, bob) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let two = "fn a() {}\nfn c() {}\n";
    let bytes = |capture: &Capture| capture.files["a.rs"].file.clone().unwrap().bytes;
    // Both start from the same capture; each changes a different function of a.rs.
    engine
        .apply(&alice, edit("alice", &base, "a.rs", "fn a() { 1; }\n"))
        .unwrap();
    let stale = engine.apply(&bob, edit("bob", &base, "a.rs", two)).unwrap();
    let Outcome::Stale { conflicts } = stale else {
        panic!("{stale:?}");
    };
    // Bob rereads the current bytes from the conflict, rebuilds on them, and resubmits.
    let current = conflicts["a.rs"].current.file.clone().unwrap().bytes;
    assert_eq!(current, b"fn a() { 1; }\n");
    let reread = engine.capture().unwrap();
    let rebuilt = format!("{}fn c() {{}}\n", String::from_utf8(current).unwrap());
    let outcome = engine
        .apply(&bob, edit("bob-rebuilt", &reread, "a.rs", &rebuilt))
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(
        bytes(&engine.capture().unwrap()),
        b"fn a() { 1; }\nfn c() {}\n"
    );
    // The rejected attempt stays retained beside the applied rebuild.
    assert_eq!(
        engine.request(&bob, "bob").unwrap().unwrap().request.output["a.rs"]
            .as_ref()
            .unwrap()
            .bytes,
        two.as_bytes()
    );
}

// The ledger schema is additive: every table is `CREATE TABLE IF NOT EXISTS`, run when the
// engine opens, with no `ALTER TABLE` and no `user_version`. So an older ledger is today's
// minus some tables, and dropping the ones this run left empty reproduces it without
// copying old setup code. Not covered: a change to how stored rows serialize. If the ledger
// format becomes a contract, add `PRAGMA user_version` and a committed fixture per version.
#[test]
fn a_ledger_missing_later_tables_reopens_unchanged_and_regains_them() {
    let (dir, engine, alice, bob) = fixture(PAIR);
    let base = engine.capture().unwrap();
    engine
        .apply(&alice, edit("a", &base, "a.rs", "fn a() { 1; }\n"))
        .unwrap();
    let message = Plan {
        task: "b".into(),
        scope: vec!["b.rs".into()],
        work: Work::None,
        text: "taking b".into(),
        reply_to: None,
    };
    engine.post(&bob, message).unwrap();
    let capture = engine.capture().unwrap();
    let events = [
        engine.events(&alice, 0).unwrap(),
        engine.events(&bob, 0).unwrap(),
    ];
    let pending = [
        engine.pending(&alice).unwrap(),
        engine.pending(&bob).unwrap(),
    ];
    let record = format!("{:?}", engine.request(&alice, "a").unwrap());
    assert!(events.iter().all(|stream| !stream.is_empty()), "{events:?}");
    drop(engine);

    let ledger = dir.path().join("state/ledger.sqlite");
    let db = rusqlite::Connection::open(&ledger).unwrap();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    let mut dropped = Vec::new();
    for table in tables {
        let rows: i64 = db
            .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
                row.get(0)
            })
            .unwrap();
        if rows == 0 {
            db.execute_batch(&format!("DROP TABLE \"{table}\""))
                .unwrap();
            dropped.push(table);
        }
    }
    assert!(dropped.contains(&"waits".to_string()), "{dropped:?}");
    db.execute_batch("PRAGMA journal_mode=DELETE;").unwrap();
    drop(db);

    let (engine, alice, bob) = reopen(dir.path(), PAIR);
    assert_eq!(engine.capture().unwrap(), capture);
    assert_eq!(
        [
            engine.events(&alice, 0).unwrap(),
            engine.events(&bob, 0).unwrap()
        ],
        events
    );
    assert_eq!(
        [
            engine.pending(&alice).unwrap(),
            engine.pending(&bob).unwrap()
        ],
        pending
    );
    assert_eq!(
        format!("{:?}", engine.request(&alice, "a").unwrap()),
        record
    );

    let db = rusqlite::Connection::open(&ledger).unwrap();
    for table in &dropped {
        let present: i64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(present, 1, "{table} not recreated");
    }
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    drop(db);

    let outcome = engine
        .apply(&bob, edit("b", &capture, "b.rs", "fn b() { 2; }\n"))
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
}

/// A statement failing inside the commit transaction after installation (#158).
fn commit_failure_after_installation_is_an_interruption(operation_between: bool) {
    let (dir, engine, alice, bob) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let live = snapshot(&dir.path().join("live"));
    let request = edit("a", &base, "a.rs", "fn a() { 1; }\n");
    fail_commits(dir.path(), true);
    let outcome = engine.apply(&alice, request.clone());
    fail_commits(dir.path(), false);
    assert!(
        matches!(outcome, Ok(Outcome::Interrupted { .. })),
        "{outcome:?}"
    );
    let reason = halt(dir.path()).unwrap();
    assert!(reason.starts_with("interrupted operation "), "{reason}");
    if operation_between {
        let later = engine
            .apply(&bob, edit("b", &base, "b.rs", "fn b() { 2; }\n"))
            .unwrap();
        assert_eq!(later, Outcome::Rejected { reason });
    }
    assert_eq!(incidents(dir.path()), 0);
    drop(engine);

    let (engine, alice, _) = reopen(dir.path(), PAIR);
    assert_eq!(snapshot(&dir.path().join("live")), live);
    assert_eq!(incidents(dir.path()), 0);
    assert!(matches!(
        engine.apply(&alice, request.clone()).unwrap(),
        Outcome::Interrupted { .. }
    ));
    assert!(matches!(
        engine.retry(&alice, request).unwrap(),
        Outcome::Applied { .. }
    ));
}
#[test]
fn edit_commit_failure_with_an_operation_before_restart_is_an_interruption() {
    commit_failure_after_installation_is_an_interruption(true);
}
#[test]
fn edit_commit_failure_with_no_operation_before_restart_is_an_interruption() {
    commit_failure_after_installation_is_an_interruption(false);
}

/// Restart recovery checks every root before it restores any (#159): an edit interrupted
/// after its first installed path, and an unenrolled file in another root.
fn recovery_refuses_an_unexplained_root_without_change(process_exit: bool) {
    let (dir, engine, alice, _) = fixture(PAIR);
    let (live, second) = (dir.path().join("live"), dir.path().join("second"));
    fs::create_dir(&second).unwrap();
    engine.add_workspace(&second).unwrap();
    if process_exit {
        drop(engine);
        crash(dir.path(), "halfway");
    } else {
        let base = engine.capture().unwrap();
        engine
            .apply_observed(&alice, two_files("process-crash", &base), |phase| {
                if phase == Phase::Installed("a.rs".into()) {
                    return Err("interrupt".into());
                }
                Ok(())
            })
            .unwrap();
        drop(engine);
    }
    fs::write(second.join("x.rs"), "fn x() {}\n").unwrap();
    let roots = [snapshot(&live), snapshot(&second)];

    fails_with(open(dir.path(), PAIR), "unexplained source write");
    assert_eq!([snapshot(&live), snapshot(&second)], roots);
    let pending: bool = ledger(dir.path())
        .query_row(
            "SELECT json_extract(body, '$.installation_pending') FROM requests WHERE request_id='process-crash'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(pending);
    assert_eq!(incidents(dir.path()), 1);
    let reason = halt(dir.path()).unwrap();
    assert!(reason.contains("unexplained source write"), "{reason}");

    fs::remove_file(second.join("x.rs")).unwrap();
    fails_with(open(dir.path(), PAIR), "engine halted for reconciliation");
}
#[test]
fn recovery_after_an_in_process_interruption_checks_every_root_before_restoring() {
    recovery_refuses_an_unexplained_root_without_change(false);
}
#[test]
fn recovery_after_a_process_exit_checks_every_root_before_restoring() {
    recovery_refuses_an_unexplained_root_without_change(true);
}

/// The host fault hook reaches every durable step of a transition, a restart and a
/// synchronization, in order, and an error there is the failure it stands for (#160).
#[test]
fn fault_hook_reaches_every_durable_step_of_transitions_restart_and_synchronization() {
    let (dir, engine, alice, bob) = fixture(PAIR);
    let (live, second) = (dir.path().join("live"), dir.path().join("second"));
    fs::create_dir(&second).unwrap();
    engine.add_workspace(&second).unwrap();
    let base = engine.capture().unwrap();
    engine
        .apply(&bob, edit("b", &base, "b.rs", "fn b() { 2; }\n"))
        .unwrap();
    drop(engine);
    let faults = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let open = |fail: Option<Fault>| {
        let faults = faults.clone();
        let hook: FaultHook = Box::new(move |_, fault| {
            faults.lock().unwrap().push(fault.clone());
            if Some(fault) == fail {
                return Err("fault".into());
            }
            Ok(())
        });
        Engine::open_with_faults(&live, &dir.path().join("state"), &["a.rs", "b.rs"], hook).unwrap()
    };
    let take = || std::mem::take(&mut *faults.lock().unwrap());
    let installed = |space: usize, path: &str| Fault::Installed {
        space,
        path: path.into(),
    };
    let restored = |space: usize, path: &str| Fault::Restored {
        space,
        path: path.into(),
    };

    // Bob's draft moves to `second`; `live` takes back the published `b.rs`.
    let engine = open(Some(Fault::Commit));
    let split = Propose {
        action: Action::Split,
        text: "split".into(),
        failing: false,
    };
    let proposal = engine.propose(&bob, split).unwrap();
    engine
        .respond(&alice, &proposal.id, &proposal.context, Response::Agree)
        .unwrap();
    let roots = [snapshot(&live), snapshot(&second)];
    let failed = engine
        .respond(&bob, &proposal.id, &proposal.context, Response::Agree)
        .unwrap();
    let transition = vec![
        Fault::Journaled,
        installed(1, "a.rs"),
        installed(1, "b.rs"),
        Fault::Audited { space: 1 },
        installed(0, "b.rs"),
        Fault::Audited { space: 0 },
        Fault::Commit,
    ];
    assert_eq!(take(), transition);
    // As a failing commit statement would (#158).
    assert!(
        matches!(&failed.state, ProposalState::Blocked { blocker } if blocker.starts_with("transition failed")),
        "{failed:?}"
    );
    let reason = halt(dir.path()).unwrap();
    assert!(reason.starts_with("interrupted transition "), "{reason}");
    assert_eq!(incidents(dir.path()), 0);
    drop(engine);

    // An error after each commit leaves the committed result.
    let engine = open(Some(Fault::Committed));
    assert_eq!(
        take(),
        [
            restored(1, "a.rs"),
            restored(1, "b.rs"),
            restored(0, "b.rs")
        ]
    );
    assert_eq!([snapshot(&live), snapshot(&second)], roots);
    // An edit's boundary retries the agreed split.
    let current = engine.capture_for(&alice).unwrap();
    engine
        .apply(&alice, edit("a", &current, "a.rs", "fn a() { 1; }\n"))
        .unwrap();
    assert_eq!(take(), [transition, vec![Fault::Committed]].concat());
    assert!(matches!(
        engine.proposal(&proposal.id).unwrap().unwrap().state,
        ProposalState::Applied { .. }
    ));
    assert_eq!(halt(dir.path()), None);

    // Publishing Alice's edit brings it into Bob's workspace.
    engine
        .configure_checks(vec![Check {
            name: "true".into(),
            program: "/usr/bin/true".into(),
            args: Vec::new(),
        }])
        .unwrap();
    let revision = engine.capture_for(&alice).unwrap().revision;
    engine
        .offer(
            &alice,
            Offer {
                id: "a".into(),
                task: "a".into(),
                revision,
                scope: Vec::new(),
                text: "a".into(),
                supersedes: None,
                reviews: Vec::new(),
            },
        )
        .unwrap();
    engine.validate().unwrap();
    assert_eq!(
        take(),
        [
            Fault::Journaled,
            installed(1, "a.rs"),
            Fault::Audited { space: 1 },
            Fault::Commit,
            Fault::Committed,
        ]
    );
    assert_eq!(
        fs::read_to_string(second.join("a.rs")).unwrap(),
        "fn a() { 1; }\n"
    );
    assert_eq!(halt(dir.path()), None);
}
