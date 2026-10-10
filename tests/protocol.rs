use falinks::*;
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;

fn fixture() -> (TempDir, Engine, Client, Client) {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    fs::create_dir(&live).unwrap();
    fs::write(live.join("a.rs"), "fn a() {}\n").unwrap();
    fs::write(live.join("b.rs"), "fn b() {}\n").unwrap();
    let engine = Engine::open(&live, &dir.path().join("state"), &["a.rs", "b.rs"]).unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let bob = engine.authenticate(1, &engine.credentials()[1]).unwrap();
    (dir, engine, alice, bob)
}
fn edit(id: &str, capture: &Capture, path: &str, bytes: &str) -> Request {
    Request {
        id: id.into(),
        expected: BTreeMap::from([(path.into(), capture.files[path].version)]),
        output: BTreeMap::from([(
            path.into(),
            Some(File {
                bytes: bytes.as_bytes().to_vec(),
                executable: false,
            }),
        )]),
    }
}

#[test]
fn two_clients_preserve_independent_drafts_and_reject_stale_output_in_full() {
    let (_dir, engine, alice, bob) = fixture();
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
    let (_dir, engine, alice, _) = fixture();
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
    let (dir, engine, alice, _) = fixture();
    let base = engine.capture().unwrap();
    let request = edit("change", &base, "a.rs", "different");
    let outcome = engine.apply(&alice, request.clone()).unwrap();
    assert_eq!(engine.apply(&alice, request.clone()).unwrap(), outcome);
    assert!(
        engine
            .apply(&alice, edit("change", &base, "a.rs", "changed ID contents"))
            .is_err()
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
    let reopened = Engine::open(
        &dir.path().join("live"),
        &dir.path().join("state"),
        &["a.rs", "b.rs"],
    )
    .unwrap();
    let alice = reopened
        .authenticate(0, reopened.credentials()[0].as_str())
        .unwrap();
    assert_eq!(reopened.apply(&alice, request).unwrap(), outcome);
    assert_eq!(reopened.capture().unwrap(), restored);
}

#[test]
fn interrupted_installation_retains_evidence_and_requires_explicit_retry() {
    let (dir, engine, alice, _) = fixture();
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
    let engine = Engine::open(
        &dir.path().join("live"),
        &dir.path().join("state"),
        &["a.rs", "b.rs"],
    )
    .unwrap();
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
    let (dir, engine, alice, _) = fixture();
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
    let engine = Engine::open(
        &dir.path().join("live"),
        &dir.path().join("state"),
        &["a.rs", "b.rs"],
    )
    .unwrap();
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
    let engine = Engine::open(
        &dir.path().join("live"),
        &dir.path().join("state"),
        &["a.rs", "b.rs"],
    )
    .unwrap();
    assert_eq!(engine.capture().unwrap(), current);
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"later draft"
    );
}

#[test]
fn captured_job_applies_coherent_output_with_invoker_attribution() {
    let (_dir, engine, alice, _) = fixture();
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
    let (_dir, engine, alice, bob) = fixture();
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
    let (dir, engine, alice, bob) = fixture();
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
    assert!(
        Engine::open(
            &dir.path().join("live"),
            &dir.path().join("state"),
            &["a.rs", "b.rs"]
        )
        .is_err()
    );
}

#[test]
fn ownership_authentication_path_aliases_and_modes_fail_closed() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (dir, engine, alice, _) = fixture();
    assert!(engine.authenticate(1, &engine.credentials()[0]).is_err());
    assert!(
        Engine::open(
            &dir.path().join("live"),
            &dir.path().join("state"),
            &["a.rs", "b.rs"]
        )
        .is_err()
    );
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
    assert!(Engine::open(&d.path().join("live"), &d.path().join("state"), &["a.rs"]).is_err());
}

#[test]
fn duplicate_inflight_submissions_join_the_recorded_operation() {
    let (_dir, engine, alice, _) = fixture();
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
    let (dir, engine, alice, _) = fixture();
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
    assert!(engine.capture().is_err());
    drop(engine);
    assert!(
        Engine::open(
            &dir.path().join("live"),
            &dir.path().join("state"),
            &["a.rs", "b.rs"]
        )
        .is_err()
    );
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"unfinished draft"
    );
}

#[test]
fn job_boundary_denies_source_and_controller_writes_and_detects_surviving_descendants() {
    let (dir, engine, alice, _) = fixture();
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
        assert!(engine.apply_job(&alice, id).is_err());
    }
    assert_eq!(engine.capture().unwrap(), base);
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"fn a() {}\n"
    );
}

#[test]
#[ignore = "subprocess crash worker; invoked by crash_boundaries"]
fn crash_worker() {
    let root = std::path::PathBuf::from(std::env::var("FALINKS_CRASH_ROOT").unwrap());
    let phase = std::env::var("FALINKS_CRASH_PHASE").unwrap();
    let engine = Engine::open(&root.join("live"), &root.join("state"), &["a.rs", "b.rs"]).unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let base = engine.capture().unwrap();
    let mut request = edit("process-crash", &base, "a.rs", "new a");
    request.expected.insert("b.rs".into(), 0);
    request.output.insert(
        "b.rs".into(),
        Some(File {
            bytes: b"new b".to_vec(),
            executable: false,
        }),
    );
    engine
        .apply_observed(&alice, request, |observed| {
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
        let (dir, engine, _, _) = fixture();
        let base = engine.capture().unwrap();
        drop(engine);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "crash_worker"])
            .env("FALINKS_CRASH_ROOT", dir.path())
            .env("FALINKS_CRASH_PHASE", phase)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        let engine = Engine::open(
            &dir.path().join("live"),
            &dir.path().join("state"),
            &["a.rs", "b.rs"],
        )
        .unwrap();
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
    let (dir, engine, alice, _) = fixture();
    let base = engine.capture().unwrap();
    let mut request = edit("ambiguous", &base, "a.rs", "partial a");
    request.expected.insert("b.rs".into(), 0);
    request.output.insert(
        "b.rs".into(),
        Some(File {
            bytes: b"new b".to_vec(),
            executable: false,
        }),
    );
    engine
        .apply_observed(&alice, request, |phase| {
            if phase == Phase::Installed("a.rs".into()) {
                return Err("interrupt".into());
            }
            Ok(())
        })
        .unwrap();
    drop(engine);
    fs::write(dir.path().join("live/b.rs"), "external draft").unwrap();
    assert!(
        Engine::open(
            &dir.path().join("live"),
            &dir.path().join("state"),
            &["a.rs", "b.rs"]
        )
        .is_err()
    );
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"partial a"
    );
    assert_eq!(
        fs::read(dir.path().join("live/b.rs")).unwrap(),
        b"external draft"
    );
}

#[test]
fn unknown_write_evidence_survives_even_if_live_bytes_are_later_changed() {
    let (dir, engine, alice, _) = fixture();
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
    let (dir, engine, alice, _) = fixture();
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
    let (_dir, engine, alice, _) = fixture();
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
