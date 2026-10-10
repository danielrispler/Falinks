//! Exact checkpoint offers, combined validation and atomic publication (#22).
//! Checks compile and test the exact candidate with the pinned rustc under macOS Seatbelt.
use falinks::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use tempfile::TempDir;

const FILES: &[(&str, &str)] = &[
    (
        "lib.rs",
        "mod a;\nmod b;\n\n#[test]\nfn combined() {\n    assert_eq!(b::b(), 2);\n}\n",
    ),
    ("a.rs", "pub fn a() -> i32 {\n    1\n}\n"),
    ("b.rs", "pub fn b() -> i32 {\n    2\n}\n"),
];
const ENROLLED: &[&str] = &["lib.rs", "a.rs", "b.rs"];

fn fixture() -> (TempDir, Engine, Client, Client) {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    fs::create_dir_all(&live).unwrap();
    for (path, bytes) in FILES {
        fs::write(live.join(path), bytes).unwrap();
    }
    let engine = Engine::open(&live, &dir.path().join("state"), ENROLLED).unwrap();
    engine.configure_checks(checks()).unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let bob = engine.authenticate(1, &engine.credentials()[1]).unwrap();
    (dir, engine, alice, bob)
}
fn reopen(root: &Path) -> (Engine, Client, Client) {
    let engine = Engine::open(&root.join("live"), &root.join("state"), ENROLLED).unwrap();
    engine.configure_checks(checks()).unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let bob = engine.authenticate(1, &engine.credentials()[1]).unwrap();
    (engine, alice, bob)
}
/// Trusted compilation plus the fixed tests, built into the isolated output directory.
fn checks() -> Vec<Check> {
    let rustc = Path::new(env!("CARGO")).parent().unwrap().join("rustc");
    let rustc = rustc.to_str().unwrap();
    let sh = |name: &str, script: String| Check {
        name: name.into(),
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script],
    };
    vec![
        sh(
            "compile",
            format!("'{rustc}' --edition 2024 --crate-type lib lib.rs --out-dir \"$TMPDIR\""),
        ),
        sh(
            "tests",
            format!(
                "'{rustc}' --edition 2024 --test lib.rs -o \"$TMPDIR/tests\" && \"$TMPDIR/tests\""
            ),
        ),
    ]
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
fn applied(engine: &Engine, client: &Client, id: &str, path: &str, bytes: &str) -> Capture {
    let current = engine.capture().unwrap();
    let outcome = engine
        .apply(client, edit(id, &current, path, bytes))
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    engine.capture().unwrap()
}
fn offer(id: &str, revision: u64) -> Offer {
    Offer {
        id: id.into(),
        task: "team".into(),
        revision,
        scope: vec!["a.rs".into(), "b.rs".into()],
        text: format!("offer {id}"),
        supersedes: None,
    }
}
fn members(agents: &[usize]) -> BTreeSet<usize> {
    agents.iter().copied().collect()
}
/// Alice changes `a` to 2; Bob's `b` now needs it. Neither compiles alone as a pair of drafts.
fn team_candidate(engine: &Engine, alice: &Client, bob: &Client) -> Capture {
    applied(
        engine,
        alice,
        "a",
        "a.rs",
        "pub fn a() -> i32 {\n    2\n}\n",
    );
    applied(
        engine,
        bob,
        "b",
        "b.rs",
        "pub fn b() -> i32 {\n    crate::a::a()\n}\n",
    )
}

#[test]
fn complete_offer_coverage_runs_checks_and_publishes_only_the_exact_candidate() {
    let (dir, engine, alice, bob) = fixture();
    let candidate = team_candidate(&engine, &alice, &bob);
    let status = engine.candidate(candidate.revision).unwrap();
    assert_eq!(status.binding.members, members(&[0, 1]));
    assert_eq!(status.missing, members(&[0, 1]));
    engine
        .offer(&alice, offer("alice", candidate.revision))
        .unwrap();
    assert!(engine.validate().unwrap().is_none(), "Bob's offer missing");
    let bobs = engine
        .offer(&bob, offer("bob", candidate.revision))
        .unwrap();
    assert_eq!(bobs.base, 0);
    assert_eq!(bobs.members, members(&[0, 1]));
    // Live drafting continues, including broken code, after the offers.
    applied(&engine, &alice, "draft", "a.rs", "pub fn a( unfinished");
    let run = engine.validate().unwrap().expect("coverage queued a run");
    assert_eq!(run.binding.tree, candidate.tree);
    assert_eq!(run.binding.base, 0);
    let attempt = run.attempts.last().unwrap();
    assert_eq!(attempt.results.len(), 2);
    assert!(attempt.results.iter().all(|r| r.passed), "{attempt:?}");
    let Some(RunOutcome::Published { event }) = run.outcome else {
        panic!("{run:?}");
    };
    assert_eq!(engine.published().unwrap(), candidate);
    for id in ["0/alice", "1/bob"] {
        assert_eq!(
            engine.checkpoint(id).unwrap().unwrap().state,
            Availability::Accepted { event }
        );
    }
    assert!(matches!(
        engine
            .wait(&alice, Condition::Checkpoint { id: "1/bob".into() }, None, None)
            .unwrap(),
        WaitOutcome::Met(Event { seq, body: Body::Published { .. }, .. }) if seq == event
    ));
    assert_eq!(
        fs::read(dir.path().join("live/a.rs")).unwrap(),
        b"pub fn a( unfinished",
        "publication never checks out over drafts"
    );
    assert!(engine.validate().unwrap().is_none());
}

#[test]
fn coverage_counts_overwritten_authors_and_required_members_but_not_notified_peers() {
    let (_dir, engine, alice, bob) = fixture();
    // Bob is notified about a.rs but contributes nothing: he is not an approver.
    engine
        .register(
            &bob,
            Scope {
                id: "watch".into(),
                task: "consumer".into(),
                nodes: ["a.rs".to_string()].into(),
                depends: Default::default(),
            },
        )
        .unwrap();
    let solo = applied(
        &engine,
        &alice,
        "a",
        "a.rs",
        "pub fn a() -> i32 {\n    2\n}\n",
    );
    assert!(engine.obligations(&bob).unwrap()[0].pending());
    assert_eq!(
        engine.candidate(solo.revision).unwrap().binding.members,
        members(&[0])
    );
    // Bob's later edit is overwritten by Alice, yet Bob remains a required offerer.
    let current = engine.capture().unwrap();
    engine
        .review(
            &bob,
            Review {
                scope: "watch".into(),
                revision: current.revision,
                decision: Decision::Revise,
                note: "reread a".into(),
            },
        )
        .unwrap();
    applied(
        &engine,
        &bob,
        "bob-a",
        "a.rs",
        "pub fn a() -> i32 {\n    3\n}\n",
    );
    let overwritten = applied(
        &engine,
        &alice,
        "a-again",
        "a.rs",
        "pub fn a() -> i32 {\n    2\n}\n",
    );
    let status = engine.candidate(overwritten.revision).unwrap();
    assert_eq!(status.binding.members, members(&[0, 1]));
    assert_eq!(status.binding.contributions.len(), 3);
    engine
        .offer(&alice, offer("alice", overwritten.revision))
        .unwrap();
    // A disconnected contributor's unoffered work blocks the candidate; nothing is taken over.
    engine.disconnect(&bob).unwrap();
    assert_eq!(
        engine.candidate(overwritten.revision).unwrap().missing,
        members(&[1])
    );
    assert!(engine.validate().unwrap().is_none());
    assert_eq!(engine.published().unwrap().revision, 0);

    // An explicitly required member must offer even without included work.
    let (_dir, engine, alice, bob) = fixture();
    engine.require_members(members(&[1])).unwrap();
    let solo = applied(
        &engine,
        &alice,
        "a",
        "a.rs",
        "pub fn a() -> i32 {\n    2\n}\n",
    );
    engine.offer(&alice, offer("alice", solo.revision)).unwrap();
    assert!(engine.validate().unwrap().is_none(), "Bob is required");
    engine.offer(&bob, offer("bob", solo.revision)).unwrap();
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    assert_eq!(run.binding.members, members(&[0, 1]));
}

#[test]
fn feedback_and_failed_checks_preserve_drafts_candidates_and_published_state() {
    let (dir, engine, alice, bob) = fixture();
    // Unfinished team code: Bob relies on `a` returning 2 before Alice has changed it.
    let unfinished = applied(
        &engine,
        &bob,
        "b",
        "b.rs",
        "pub fn b() -> i32 {\n    crate::a::a()\n}\n",
    );
    let early = engine
        .feedback(&alice, "early", unfinished.revision)
        .unwrap();
    assert!(
        engine.feedback(&alice, "early", 0).is_err(),
        "changed contents under an ID"
    );
    assert_eq!(
        engine
            .feedback(&alice, "early", unfinished.revision)
            .unwrap(),
        early
    );
    let run = engine.validate().unwrap().unwrap();
    assert_eq!(
        run.outcome,
        Some(RunOutcome::Failed {
            check: "tests".into()
        })
    );
    assert!(
        run.attempts[0].results[0].passed,
        "the combined candidate compiles"
    );
    assert_eq!(engine.published().unwrap().revision, 0);
    // A failing publication candidate stays retained, available and unpublished.
    engine
        .offer(&bob, offer("bob", unfinished.revision))
        .unwrap();
    applied(
        &engine,
        &bob,
        "draft",
        "b.rs",
        "pub fn b() -> i32 {\n    2\n}\n",
    );
    let run = engine.validate().unwrap().unwrap();
    assert_eq!(
        run.binding.tree, unfinished.tree,
        "later drafts leave the held candidate fixed"
    );
    assert_eq!(
        run.outcome,
        Some(RunOutcome::Failed {
            check: "tests".into()
        })
    );
    assert_eq!(engine.published().unwrap().revision, 0);
    assert_eq!(
        engine.checkpoint("1/bob").unwrap().unwrap().state,
        Availability::Available
    );
    assert_eq!(
        fs::read(dir.path().join("live/b.rs")).unwrap(),
        b"pub fn b() -> i32 {\n    2\n}\n"
    );
    let failed = engine.events(&alice, 0).unwrap().into_iter().filter(|e| {
        matches!(
            &e.body,
            Body::Validated {
                outcome: RunOutcome::Failed { .. },
                ..
            }
        )
    });
    assert_eq!(failed.count(), 2, "exact-version results reach peers");
}

fn superseding(id: &str, revision: u64, old: &str) -> Offer {
    Offer {
        supersedes: Some(old.into()),
        ..offer(id, revision)
    }
}

#[test]
fn withdrawal_during_passing_checks_blocks_and_supersession_transfers_nothing() {
    let (_dir, engine, alice, bob) = fixture();
    let candidate = team_candidate(&engine, &alice, &bob);
    engine
        .offer(&alice, offer("alice", candidate.revision))
        .unwrap();
    engine
        .offer(&bob, offer("bob", candidate.revision))
        .unwrap();
    let run = engine
        .validate_observed(|stage| {
            if stage == Stage::Checked("compile".into()) {
                engine.withdraw(&bob, "bob")?;
            }
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert!(run.attempts[0].results.iter().all(|r| r.passed), "feedback");
    assert!(
        matches!(&run.outcome, Some(RunOutcome::Blocked { reason }) if reason.contains("missing offers from {1}")),
        "{run:?}"
    );
    assert_eq!(engine.published().unwrap().revision, 0);

    // A fresh offer forms a new candidate; superseding Alice's offer invalidates it again.
    engine
        .offer(&bob, offer("bob-again", candidate.revision))
        .unwrap();
    engine
        .offer(&alice, superseding("alice-2", candidate.revision, "alice"))
        .unwrap();
    let stale = engine.validate().unwrap().unwrap();
    assert_eq!(stale.binding.checkpoints[&0], "0/alice");
    assert!(
        matches!(stale.outcome, Some(RunOutcome::Blocked { .. })),
        "{stale:?}"
    );
    let fresh = engine.validate().unwrap().unwrap();
    assert_eq!(
        fresh.binding.checkpoints,
        BTreeMap::from([(0, "0/alice-2".into()), (1, "1/bob-again".into())])
    );
    assert_eq!(fresh.attempts.len(), 1, "fresh checks, no carried results");
    assert!(matches!(fresh.outcome, Some(RunOutcome::Published { .. })));
    assert!(matches!(
        engine.checkpoint("0/alice").unwrap().unwrap().state,
        Availability::Superseded { .. }
    ));
    assert_eq!(
        engine.checkpoint("1/bob").unwrap().unwrap().state,
        Availability::Withdrawn
    );
}

#[test]
fn base_advances_return_candidates_and_newer_publications_are_never_overwritten() {
    let (_dir, engine, alice, bob) = fixture();
    let first = applied(
        &engine,
        &alice,
        "a",
        "a.rs",
        "pub fn a() -> i32 {\n    2\n}\n",
    );
    let team = applied(
        &engine,
        &bob,
        "b",
        "b.rs",
        "pub fn b() -> i32 {\n    crate::a::a()\n}\n",
    );
    engine
        .offer(&alice, offer("first", first.revision))
        .unwrap();
    engine.offer(&alice, offer("team", team.revision)).unwrap();
    engine.offer(&bob, offer("team", team.revision)).unwrap();
    let accepted = engine.validate().unwrap().unwrap();
    assert!(matches!(
        accepted.outcome,
        Some(RunOutcome::Published { .. })
    ));
    assert_eq!(engine.published().unwrap(), first);
    let returned = engine.validate().unwrap().unwrap();
    assert!(
        matches!(&returned.outcome, Some(RunOutcome::Blocked { reason }) if reason.contains("base advanced")),
        "{returned:?}"
    );
    assert!(returned.attempts[0].results.is_empty(), "no stale checks");
    // Against the new base only Bob's work is unpublished: he alone re-offers.
    let status = engine.candidate(team.revision).unwrap();
    assert_eq!((status.binding.base, status.missing), (1, members(&[1])));
    engine
        .offer(&bob, superseding("rebased", team.revision, "team"))
        .unwrap();
    let rebased = engine.validate().unwrap().unwrap();
    assert!(
        matches!(rebased.outcome, Some(RunOutcome::Published { .. })),
        "{rebased:?}"
    );
    assert_eq!(rebased.binding.base, 1);
    assert_eq!(engine.published().unwrap(), team);

    // A run for older work queued behind a newer publication can never move the pointer back.
    let (_dir, engine, alice, bob) = fixture();
    let first = applied(
        &engine,
        &alice,
        "a",
        "a.rs",
        "pub fn a() -> i32 {\n    2\n}\n",
    );
    let team = applied(
        &engine,
        &bob,
        "b",
        "b.rs",
        "pub fn b() -> i32 {\n    crate::a::a()\n}\n",
    );
    engine.offer(&alice, offer("team", team.revision)).unwrap();
    engine.offer(&bob, offer("team", team.revision)).unwrap();
    engine
        .offer(&alice, offer("first", first.revision))
        .unwrap();
    engine.validate().unwrap();
    let older = engine.validate().unwrap().unwrap();
    assert!(
        matches!(&older.outcome, Some(RunOutcome::Blocked { reason }) if reason.contains("never overwritten")),
        "{older:?}"
    );
    assert_eq!(engine.published().unwrap(), team);
}

#[test]
fn the_reusable_slot_holds_source_fixed_and_quarantines_dirty_or_mutated_state() {
    let (dir, engine, alice, _) = fixture();
    let slot = dir.path().join("state/validation");
    let live = fs::canonicalize(dir.path().join("live")).unwrap();
    let escape = |name: &str, script: String| Check {
        name: name.into(),
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script],
    };
    let mut attempts = checks();
    attempts.push(escape("write-source", "echo x > a.rs".into()));
    attempts.push(escape(
        "read-live",
        format!("cat '{}'", live.join("a.rs").display()),
    ));
    engine.configure_checks(attempts).unwrap();
    engine.capture().unwrap();
    engine.feedback(&alice, "escape", 0).unwrap();
    let run = engine.validate().unwrap().unwrap();
    let results = &run.attempts[0].results;
    assert!(results[0].passed && results[1].passed);
    assert!(!results[2].passed && !results[3].passed, "{results:?}");
    assert_eq!(
        run.outcome,
        Some(RunOutcome::Failed {
            check: "write-source".into()
        })
    );
    assert_eq!(fs::read(slot.join("a.rs")).unwrap(), FILES[1].1.as_bytes());

    // A mutation that escapes the profile is still detected and refused.
    engine.configure_checks(checks()).unwrap();
    engine.feedback(&alice, "mutated", 0).unwrap();
    let run = engine
        .validate_observed(|stage| {
            if stage == Stage::Checked("compile".into()) {
                fs::write(slot.join("a.rs"), "tampered")?;
            }
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert!(
        matches!(&run.outcome, Some(RunOutcome::Refused { reason }) if reason.contains("quarantined")),
        "{run:?}"
    );
    let quarantined = || {
        fs::read_dir(dir.path().join("state/quarantine"))
            .unwrap()
            .count()
    };
    assert_eq!(quarantined(), 1);
    engine.feedback(&alice, "rebuilt", 0).unwrap();
    let run = engine.validate().unwrap().unwrap();
    assert_eq!(run.attempts[0].slot, "constructed");
    assert_eq!(run.outcome, Some(RunOutcome::Passed));
    engine.feedback(&alice, "reused", 0).unwrap();
    assert_eq!(
        engine.validate().unwrap().unwrap().attempts[0].slot,
        "reused"
    );

    // A clean record does not vouch for files changed afterwards.
    fs::write(slot.join("stray.rs"), "generated").unwrap();
    engine.feedback(&alice, "stray", 0).unwrap();
    let run = engine.validate().unwrap().unwrap();
    assert!(run.attempts[0].slot.starts_with("quarantined"), "{run:?}");
    assert_eq!(run.outcome, Some(RunOutcome::Passed));
    assert_eq!(quarantined(), 2);
    assert!(!slot.join("stray.rs").exists());
}

fn mirror(dir: &TempDir) -> String {
    let output = std::process::Command::new("/usr/bin/git")
        .arg("--git-dir")
        .arg(dir.path().join("state/objects.git"))
        .args(["show-ref", "--verify", "--hash", "refs/falinks/published"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
#[ignore = "subprocess crash worker; invoked by publication_crash_boundaries"]
fn publication_crash_worker() {
    let root = std::path::PathBuf::from(std::env::var("FALINKS_CRASH_ROOT").unwrap());
    let stage = std::env::var("FALINKS_CRASH_STAGE").unwrap();
    let (engine, _, _) = reopen(&root);
    engine
        .validate_observed(|observed| {
            let selected = match stage.as_str() {
                "checking" => observed == Stage::Checked("compile".into()),
                "before-accept" => observed == Stage::BeforeAccept,
                "committing" => observed == Stage::Committing,
                "accepted" => observed == Stage::Accepted,
                _ => panic!("unknown crash stage"),
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
fn publication_crash_boundaries_accept_all_or_none_and_recover_committed_outcomes() {
    for stage in ["checking", "before-accept", "committing", "accepted"] {
        let (dir, engine, alice, bob) = fixture();
        let initial = engine.published().unwrap();
        let candidate = team_candidate(&engine, &alice, &bob);
        let alices = offer("alice", candidate.revision);
        engine.offer(&alice, alices.clone()).unwrap();
        engine
            .offer(&bob, offer("bob", candidate.revision))
            .unwrap();
        let id = engine.runs().unwrap()[0].id.clone();
        applied(&engine, &alice, "draft", "a.rs", "pub fn a( unfinished");
        drop(engine);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "publication_crash_worker"])
            .env("FALINKS_CRASH_ROOT", dir.path())
            .env("FALINKS_CRASH_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73), "{stage}");
        assert_eq!(
            mirror(&dir),
            initial.tree,
            "{stage}: mirror not yet updated"
        );
        let (engine, alice, bob) = reopen(dir.path());
        assert_eq!(
            fs::read(dir.path().join("live/a.rs")).unwrap(),
            b"pub fn a( unfinished",
            "{stage}: drafts survive restart"
        );
        let run = engine.run(&id).unwrap().unwrap();
        if stage == "accepted" {
            let Some(RunOutcome::Published { event }) = run.outcome else {
                panic!("{stage}: {run:?}");
            };
            assert_eq!(engine.published().unwrap(), candidate);
            assert_eq!(mirror(&dir), candidate.tree, "restart repairs the mirror");
            // The notification the crash prevented is replayed from the committed event.
            for client in [&alice, &bob] {
                assert!(
                    engine
                        .events(client, 0)
                        .unwrap()
                        .iter()
                        .any(|e| matches!(&e.body, Body::Published { run, .. } if *run == id)),
                    "Published event not replayed after restart"
                );
            }
            // Duplicate requests recover the committed outcome without another publication.
            assert_eq!(engine.retry_run(&alice, &id).unwrap(), run);
            let duplicate = engine.offer(&alice, alices.clone()).unwrap();
            assert_eq!(duplicate.state, Availability::Accepted { event });
            assert!(
                engine
                    .offer(
                        &alice,
                        Offer {
                            text: "changed".into(),
                            ..alices
                        }
                    )
                    .is_err()
            );
            assert!(engine.validate().unwrap().is_none());
            continue;
        }
        assert!(
            matches!(run.outcome, Some(RunOutcome::Interrupted { .. })),
            "{stage}: {run:?}"
        );
        assert_eq!(engine.published().unwrap(), initial, "{stage}");
        assert_eq!(mirror(&dir), initial.tree);
        for id in ["0/alice", "1/bob"] {
            assert_eq!(
                engine.checkpoint(id).unwrap().unwrap().state,
                Availability::Available,
                "{stage}: no partial group"
            );
        }
        assert!(engine.validate().unwrap().is_none(), "no automatic resume");
        engine.retry_run(&alice, &id).unwrap();
        let retried = engine.validate().unwrap().unwrap();
        assert_eq!(retried.attempts.len(), 2);
        assert!(
            retried.attempts[1].slot.starts_with("quarantined"),
            "{stage}: a dirty slot is never reused: {retried:?}"
        );
        assert!(
            matches!(retried.outcome, Some(RunOutcome::Published { .. })),
            "{stage}: {retried:?}"
        );
        assert_eq!(engine.published().unwrap(), candidate);
        assert_eq!(mirror(&dir), candidate.tree);
    }
}

#[test]
fn unconfigured_checks_are_retryable_and_committed_acceptance_survives_observer_errors() {
    let (dir, engine, alice, bob) = fixture();
    let candidate = team_candidate(&engine, &alice, &bob);
    engine
        .offer(&alice, offer("alice", candidate.revision))
        .unwrap();
    engine
        .offer(&bob, offer("bob", candidate.revision))
        .unwrap();
    drop(engine);
    // Restart without reconfiguring the in-memory check set: a host gap, not a verdict.
    let engine = Engine::open(
        &dir.path().join("live"),
        &dir.path().join("state"),
        ENROLLED,
    )
    .unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Interrupted { .. })),
        "{run:?}"
    );
    engine.configure_checks(checks()).unwrap();
    engine.retry_run(&alice, &run.id).unwrap();
    let run = engine
        .validate_observed(|stage| match stage {
            Stage::Accepted => Err("observer failed after commit".into()),
            _ => Ok(()),
        })
        .unwrap()
        .unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    assert_eq!(engine.run(&run.id).unwrap().unwrap(), run);
    assert_eq!(engine.published().unwrap(), candidate);
    assert!(!dir.path().join("state/validation-runs").exists());
}

#[test]
fn unexplained_source_writes_stop_publication_and_keep_evidence() {
    let (dir, engine, alice, bob) = fixture();
    let candidate = team_candidate(&engine, &alice, &bob);
    engine
        .offer(&alice, offer("alice", candidate.revision))
        .unwrap();
    engine
        .offer(&bob, offer("bob", candidate.revision))
        .unwrap();
    let run = engine
        .validate_observed(|stage| {
            if stage == Stage::BeforeAccept {
                fs::write(dir.path().join("live/a.rs"), "external")?;
            }
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert!(
        matches!(&run.outcome, Some(RunOutcome::Blocked { reason }) if reason.contains("unexplained")),
        "{run:?}"
    );
    assert_eq!(engine.published().unwrap().revision, 0);
    let incident = engine.incident().unwrap().unwrap();
    assert_eq!(
        incident.files["a.rs"].bytes.as_deref(),
        Some(&b"external"[..])
    );
}

#[test]
fn missing_published_evidence_stops_restart_without_replacing_drafts() {
    let (dir, engine, alice, bob) = fixture();
    let candidate = team_candidate(&engine, &alice, &bob);
    engine
        .offer(&alice, offer("alice", candidate.revision))
        .unwrap();
    engine
        .offer(&bob, offer("bob", candidate.revision))
        .unwrap();
    engine.validate().unwrap();
    applied(&engine, &bob, "draft", "b.rs", "draft");
    drop(engine);
    let blob = candidate.files["a.rs"].blob.clone().unwrap();
    fs::remove_file(
        dir.path()
            .join("state/objects.git/objects")
            .join(&blob[..2])
            .join(&blob[2..]),
    )
    .unwrap();
    assert!(
        Engine::open(
            &dir.path().join("live"),
            &dir.path().join("state"),
            ENROLLED
        )
        .is_err()
    );
    assert_eq!(fs::read(dir.path().join("live/b.rs")).unwrap(), b"draft");
}

#[test]
fn a_failing_check_on_a_two_member_candidate_accepts_neither_member() {
    let (_dir, engine, alice, bob) = fixture();
    let initial = engine.published().unwrap();
    applied(
        &engine,
        &alice,
        "a",
        "a.rs",
        "pub fn a() -> i32 {\n    3\n}\n",
    );
    let candidate = applied(
        &engine,
        &bob,
        "b",
        "b.rs",
        "pub fn b() -> i32 {\n    crate::a::a()\n}\n",
    );
    let alices = engine
        .offer(&alice, offer("alice", candidate.revision))
        .unwrap();
    let bobs = engine
        .offer(&bob, offer("bob", candidate.revision))
        .unwrap();
    assert_eq!(bobs.members, members(&[0, 1]));
    // Both compile; the fixed test fails only for the combination.
    let run = engine.validate().unwrap().expect("coverage queued a run");
    assert_eq!(
        run.outcome,
        Some(RunOutcome::Failed {
            check: "tests".into()
        })
    );
    assert_eq!(engine.published().unwrap(), initial);
    for checkpoint in [alices, bobs] {
        assert_eq!(
            engine.checkpoint(&checkpoint.id).unwrap().unwrap().state,
            Availability::Available,
            "a failed group accepts no member"
        );
    }
}
