//! Join/split/keep recommendations, agreement and work-preserving transitions (#25).
//! Relevance uses the real pinned gopls/Go toolchain; checks are a trivial trusted command.
use falinks::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempDir;

const GO: &[(&str, &str)] = &[
    ("go.mod", "module example.com/probe\n\ngo 1.22\n"),
    (
        "siblings.go",
        "package probe\n\nfunc Left() int {\n\treturn 1\n}\n\nfunc Right() int {\n\treturn Callee()\n}\n",
    ),
    (
        "callee.go",
        "package probe\n\nfunc Callee() int {\n\treturn 7\n}\n",
    ),
];
const LEFT: &str = "package probe\n\nfunc Left() int {\n\treturn 2\n}\n\nfunc Right() int {\n\treturn Callee()\n}\n";
const CALLEE: &str = "package probe\n\nfunc Callee() int {\n\treturn 8\n}\n";

fn pin(path: PathBuf) -> Pin {
    assert!(path.is_file(), "missing pinned tool {}", path.display());
    Pin {
        sha256: sha256_file(&path).unwrap(),
        path,
    }
}
fn go_toolchain() -> Toolchain {
    let env = |key: &str| {
        let out = Command::new("go").args(["env", key]).output().unwrap();
        PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
    };
    let gopls = std::env::var_os("FALINKS_GOPLS")
        .map(PathBuf::from)
        .unwrap_or_else(|| env("GOPATH").join("bin/gopls"));
    Toolchain {
        language: Language::Go,
        executables: vec![pin(gopls), pin(env("GOROOT").join("bin/go"))],
        features: Vec::new(),
    }
}
fn open(root: &Path) -> (Engine, Client, Client) {
    let enrolled: Vec<_> = GO.iter().map(|(p, _)| *p).collect();
    let engine = Engine::open(&root.join("live"), &root.join("state"), &enrolled).unwrap();
    engine.configure_analysis(vec![go_toolchain()]).unwrap();
    engine
        .configure_checks(vec![Check {
            name: "trusted".into(),
            program: "/usr/bin/true".into(),
            args: Vec::new(),
        }])
        .unwrap();
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let bob = engine.authenticate(1, &engine.credentials()[1]).unwrap();
    (engine, alice, bob)
}
fn fixture() -> (TempDir, Engine, Client, Client) {
    let dir = tempfile::tempdir().unwrap();
    for (path, bytes) in GO {
        fs::create_dir_all(dir.path().join("live").join(path).parent().unwrap()).unwrap();
        fs::write(dir.path().join("live").join(path), bytes).unwrap();
    }
    fs::create_dir_all(dir.path().join("second")).unwrap();
    let (engine, alice, bob) = open(dir.path());
    (dir, engine, alice, bob)
}
fn register(engine: &Engine, client: &Client, id: &str, task: &str, nodes: &[&str]) {
    engine
        .register(
            client,
            Scope {
                id: id.into(),
                task: task.into(),
                nodes: nodes.iter().map(|n| n.to_string()).collect(),
            },
        )
        .unwrap();
}
/// Alice drafts `Left`, Bob drafts `Callee`: independent scopes and unfinished work.
fn independent(engine: &Engine, alice: &Client, bob: &Client) {
    register(engine, alice, "left", "left task", &["siblings.go#Left"]);
    register(engine, bob, "callee", "callee task", &["callee.go#Callee"]);
    edit(engine, alice, "left", "siblings.go", LEFT);
    edit(engine, bob, "callee", "callee.go", CALLEE);
}
fn edit(engine: &Engine, client: &Client, id: &str, path: &str, bytes: &str) -> Capture {
    let current = engine.capture_for(client).unwrap();
    let outcome = engine
        .apply(
            client,
            Request {
                id: id.into(),
                expected: BTreeMap::from([(path.into(), current.files[path].version)]),
                output: BTreeMap::from([(
                    path.into(),
                    Some(File {
                        bytes: bytes.as_bytes().to_vec(),
                        executable: false,
                    }),
                )]),
            },
        )
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    engine.capture_for(client).unwrap()
}
fn agree(engine: &Engine, client: &Client, proposal: &Proposal) -> Proposal {
    engine
        .respond(client, &proposal.id, &proposal.context, Response::Agree)
        .unwrap()
}
fn offer(engine: &Engine, client: &Client, id: &str) -> Checkpoint {
    let revision = engine.capture_for(client).unwrap().revision;
    engine
        .offer(
            client,
            Offer {
                id: id.into(),
                task: "task".into(),
                revision,
                scope: Vec::new(),
                text: id.into(),
                supersedes: None,
            },
        )
        .unwrap()
}
fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).unwrap()
}
fn agents(list: &[usize]) -> BTreeSet<usize> {
    list.iter().copied().collect()
}
/// Splits the independent drafts into the provisioned second root.
fn split(dir: &TempDir, engine: &Engine, alice: &Client, bob: &Client) -> Proposal {
    independent(engine, alice, bob);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let proposal = engine.reconsider().unwrap();
    assert_eq!(proposal.action, Action::Split, "{:?}", proposal.reasoning);
    agree(engine, alice, &proposal);
    let applied = agree(engine, bob, &proposal);
    assert!(
        matches!(applied.state, ProposalState::Applied { .. }),
        "{:?}",
        applied.state
    );
    applied
}

#[test]
fn useful_split_moves_unfinished_drafts_and_preserves_history_and_offers() {
    let (dir, engine, alice, bob) = fixture();
    independent(&engine, &alice, &bob);
    let before = engine.capture().unwrap();
    let old = offer(&engine, &alice, "team");
    assert_eq!(old.members, agents(&[0, 1]));

    // Recommended with concise evidence and no prior failure; both must agree.
    let proposal = engine.reconsider().unwrap();
    assert_eq!(proposal.action, Action::Split);
    assert_eq!(proposal.state, ProposalState::Open);
    assert_eq!(proposal.from, [0, 0]);
    assert_eq!(proposal.target, [0, 1]);
    assert!(proposal.signals.compiler.is_empty() && proposal.signals.declared.is_empty());
    assert_eq!(proposal.scopes[1][0].id, "callee");
    let waiting = agree(&engine, &alice, &proposal);
    assert_eq!(
        waiting.state,
        ProposalState::Open,
        "silence is not agreement"
    );
    assert_eq!(engine.placement().unwrap(), [0, 0]);

    // Agreed but unsafe: no provisioned root yet. Work continues in place.
    let blocked = agree(&engine, &bob, &proposal);
    assert_eq!(
        blocked.state,
        ProposalState::Blocked {
            blocker: "workspace root 1 is not provisioned".into()
        }
    );
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    assert!(matches!(
        engine.proposal(&proposal.id).unwrap().unwrap().state,
        ProposalState::Applied { .. }
    ));

    // Bob's draft moved with its exact occurrence; Alice's stayed; nothing published first.
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    let (live, second) = (engine.root(&alice).unwrap(), engine.root(&bob).unwrap());
    assert_eq!(second, fs::canonicalize(dir.path().join("second")).unwrap());
    assert_eq!(read(&live, "siblings.go"), LEFT);
    assert_eq!(read(&live, "callee.go"), GO[2].1);
    assert_eq!(read(&second, "callee.go"), CALLEE);
    assert_eq!(read(&second, "siblings.go"), GO[1].1);
    let bobs = engine.capture_for(&bob).unwrap();
    assert_eq!(bobs.files["callee.go"], before.files["callee.go"]);
    assert_eq!(engine.published().unwrap().revision, 0);
    let history = engine.history().unwrap();
    assert_eq!(
        history.iter().map(|r| r.agent).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(engine.checkpoint("0/team").unwrap().unwrap(), old);
    assert!(
        engine
            .obligations(&alice)
            .unwrap()
            .iter()
            .all(|o| !o.pending())
    );

    // Separate progress: each group publishes alone, and publications reach the other.
    let alices = offer(&engine, &alice, "alone");
    assert_eq!(alices.members, agents(&[0]));
    engine.validate().unwrap();
    assert_eq!(
        read(&second, "siblings.go"),
        LEFT,
        "published into Bob's root"
    );
    assert_eq!(read(&second, "callee.go"), CALLEE, "Bob's draft untouched");
    let bobs = offer(&engine, &bob, "alone");
    assert_eq!(bobs.members, agents(&[1]));
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    assert_eq!(read(&live, "callee.go"), CALLEE);
    let published = engine.published().unwrap();
    assert_eq!(
        published.files["siblings.go"].file.as_ref().unwrap().bytes,
        LEFT.as_bytes()
    );
    let pending = engine.pending(&bob).unwrap();
    assert!(
        pending
            .unhandled
            .iter()
            .any(|e| matches!(e.body, Body::Incorporated { space: 1, .. }))
    );

    // Restart keeps the arrangement, roots and proposals.
    drop(engine);
    let (engine, alice, bob) = open(dir.path());
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    assert_eq!(engine.root(&bob).unwrap(), second);
    assert_eq!(engine.root(&alice).unwrap(), live);
    assert_eq!(engine.proposals().unwrap().len(), 1);
}

#[test]
fn compiler_edge_alone_keeps_a_split_and_collaboration_evidence_joins_it() {
    let (dir, engine, alice, bob) = fixture();
    split(&dir, &engine, &alice, &bob);
    // Right calls Callee: a compiler edge between the scopes, nothing else.
    register(
        &engine,
        &alice,
        "right",
        "right task",
        &["siblings.go#Right"],
    );
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert_eq!(keep.signals.compiler, vec!["0:right 1:callee".to_string()]);
    assert!(keep.reasoning.contains("compiler relationship alone"));
    assert_eq!(engine.reconsider().unwrap().id, keep.id, "unchanged repeat");

    // Observed waiting on the peer is collaboration evidence.
    let after = engine.events(&alice, 0).unwrap().last().unwrap().seq;
    let outcome = engine
        .wait(
            &alice,
            Condition::Message { from: 1, after },
            Some(Duration::from_millis(10)),
            None,
        )
        .unwrap();
    assert_eq!(outcome, WaitOutcome::TimedOut);
    let join = engine.reconsider().unwrap();
    assert_eq!(join.action, Action::Join, "{}", join.reasoning);
    assert_eq!(join.signals.waits, 1);
    assert_eq!(join.target, [0, 0]);
    agree(&engine, &bob, &join);
    let applied = agree(&engine, &alice, &join);
    assert!(
        matches!(applied.state, ProposalState::Applied { .. }),
        "{applied:?}"
    );

    // Both unfinished drafts share the live root; Bob's root no longer applies.
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    let live = engine.root(&bob).unwrap();
    assert_eq!(read(&live, "siblings.go"), LEFT);
    assert_eq!(read(&live, "callee.go"), CALLEE);
    let candidate = engine
        .candidate(engine.capture().unwrap().revision)
        .unwrap();
    assert_eq!(candidate.binding.members, agents(&[0, 1]));
    // Bob's Callee change is now in Alice's view and relevant to Right.
    assert!(
        engine
            .obligations(&alice)
            .unwrap()
            .iter()
            .any(|o| o.scope.id == "right" && o.pending())
    );
}

#[test]
fn disagreement_keeps_the_arrangement_and_unchanged_repeats_are_suppressed() {
    let (dir, engine, alice, bob) = fixture();
    independent(&engine, &alice, &bob);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let proposal = engine.reconsider().unwrap();
    agree(&engine, &alice, &proposal);
    let declined = engine
        .respond(
            &bob,
            &proposal.id,
            &proposal.context,
            Response::Decline {
                reason: "about to touch Right".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        declined.state,
        ProposalState::Declined { agent: 1, .. }
    ));
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    assert_eq!(engine.reconsider().unwrap().id, proposal.id);
    assert!(
        engine
            .respond(&alice, &proposal.id, &proposal.context, Response::Agree)
            .is_err()
    );
    assert_eq!(engine.proposals().unwrap().len(), 1);
}

#[test]
fn changed_task_context_invalidates_agreement() {
    let (dir, engine, alice, bob) = fixture();
    independent(&engine, &alice, &bob);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let proposal = engine.reconsider().unwrap();
    agree(&engine, &alice, &proposal);
    register(&engine, &bob, "more", "callee task", &["callee.go"]);
    let stale = engine.proposal(&proposal.id).unwrap().unwrap();
    assert!(
        matches!(stale.state, ProposalState::Stale { .. }),
        "{stale:?}"
    );
    assert!(
        engine
            .respond(&bob, &proposal.id, &proposal.context, Response::Agree)
            .is_err()
    );
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    let fresh = engine.reconsider().unwrap();
    assert_ne!(fresh.id, proposal.id);
    assert_ne!(fresh.context, proposal.context);
    assert!(fresh.agreed.is_empty());
}

#[test]
fn interleaved_drafts_stay_pending_until_published_then_split_applies() {
    let (dir, engine, alice, bob) = fixture();
    engine.add_workspace(&dir.path().join("second")).unwrap();
    edit(&engine, &alice, "left", "siblings.go", LEFT);
    let both = "package probe\n\nfunc Left() int {\n\treturn 2\n}\n\nfunc Right() int {\n\treturn Callee() + 1\n}\n";
    edit(&engine, &bob, "right", "siblings.go", both);
    let proposal = engine
        .propose(
            &alice,
            Propose {
                action: Action::Split,
                text: "Callee work next".into(),
                failing: false,
            },
        )
        .unwrap();
    agree(&engine, &alice, &proposal);
    let blocked = agree(&engine, &bob, &proposal);
    assert_eq!(
        blocked.state,
        ProposalState::Blocked {
            blocker: "interleaved unpublished drafts in [\"siblings.go\"]".into()
        }
    );
    // Work continues in the current arrangement.
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    assert_eq!(read(&engine.root(&bob).unwrap(), "siblings.go"), both);

    offer(&engine, &alice, "a");
    offer(&engine, &bob, "b");
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    assert!(matches!(
        engine.proposal(&proposal.id).unwrap().unwrap().state,
        ProposalState::Applied { .. }
    ));
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    assert_eq!(read(&engine.root(&bob).unwrap(), "siblings.go"), both);
}

#[test]
fn reversal_needs_new_evidence_or_a_failure_report() {
    let (dir, engine, alice, bob) = fixture();
    let split = split(&dir, &engine, &alice, &bob);
    let suppressed = engine
        .propose(
            &bob,
            Propose {
                action: Action::Join,
                text: "rejoin".into(),
                failing: false,
            },
        )
        .unwrap();
    assert_eq!(suppressed.action, Action::Keep);
    assert!(
        suppressed.reasoning.contains(&split.id),
        "{}",
        suppressed.reasoning
    );
    let failing = engine
        .propose(
            &bob,
            Propose {
                action: Action::Join,
                text: "I keep needing Left".into(),
                failing: true,
            },
        )
        .unwrap();
    assert_eq!(failing.action, Action::Join);
    assert_eq!(failing.state, ProposalState::Open);
    assert!(failing.signals.failing);
}

#[test]
fn overlapping_publication_waits_for_explicit_incorporation() {
    let (dir, engine, alice, bob) = fixture();
    split(&dir, &engine, &alice, &bob);
    let right = "package probe\n\nfunc Left() int {\n\treturn 1\n}\n\nfunc Right() int {\n\treturn Callee() + 1\n}\n";
    let mine = edit(&engine, &bob, "right", "siblings.go", right);
    offer(&engine, &alice, "left");
    engine.validate().unwrap();
    let second = engine.root(&bob).unwrap();
    assert_eq!(
        read(&second, "siblings.go"),
        right,
        "draft never overwritten"
    );
    let behind = engine.pending(&bob).unwrap();
    assert!(behind.unhandled.iter().any(|e| e.body
        == Body::Behind {
            space: 1,
            publication: engine.published().unwrap().revision,
            overlaps: vec!["siblings.go".into()],
        }));
    // Offers from the stale workspace cannot publish over the newer state.
    offer(&engine, &bob, "stale");
    assert!(engine.validate().unwrap().is_none());

    let merged = "package probe\n\nfunc Left() int {\n\treturn 2\n}\n\nfunc Right() int {\n\treturn Callee() + 1\n}\n";
    let unresolved = engine
        .incorporate(
            &bob,
            Request {
                id: "nothing".into(),
                expected: BTreeMap::from([("callee.go".into(), mine.files["callee.go"].version)]),
                output: BTreeMap::from([(
                    "callee.go".into(),
                    Some(File {
                        bytes: CALLEE.as_bytes().to_vec(),
                        executable: false,
                    }),
                )]),
            },
        )
        .unwrap();
    assert!(
        matches!(unresolved, Outcome::Rejected { .. }),
        "{unresolved:?}"
    );
    let outcome = engine
        .incorporate(
            &bob,
            Request {
                id: "merge".into(),
                expected: BTreeMap::from([(
                    "siblings.go".into(),
                    mine.files["siblings.go"].version,
                )]),
                output: BTreeMap::from([(
                    "siblings.go".into(),
                    Some(File {
                        bytes: merged.as_bytes().to_vec(),
                        executable: false,
                    }),
                )]),
            },
        )
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(read(&second, "siblings.go"), merged);
    assert_eq!(
        engine.request(&bob, "merge").unwrap().unwrap().incorporates,
        Some(engine.published().unwrap().revision)
    );
    let bobs = offer(&engine, &bob, "merged");
    assert_eq!(bobs.members, agents(&[1]));
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    assert_eq!(read(&engine.root(&alice).unwrap(), "siblings.go"), merged);
}

#[test]
fn interrupted_transition_restores_both_roots_and_retries_after_restart() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, engine, alice, bob) = fixture();
    independent(&engine, &alice, &bob);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let proposal = engine.reconsider().unwrap();
    agree(&engine, &alice, &proposal);
    // The new root installs first; restoring Bob's moved path in the shared root then fails.
    let live = dir.path().join("live");
    fs::set_permissions(&live, fs::Permissions::from_mode(0o555)).unwrap();
    let failed = engine.respond(&bob, &proposal.id, &proposal.context, Response::Agree);
    fs::set_permissions(&live, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(failed.is_err());
    assert_eq!(read(&dir.path().join("second"), "callee.go"), CALLEE);
    let halted = engine
        .apply(
            &alice,
            Request {
                id: "halted".into(),
                expected: BTreeMap::from([("go.mod".into(), 0)]),
                output: BTreeMap::from([("go.mod".into(), None)]),
            },
        )
        .unwrap();
    assert!(
        matches!(&halted, Outcome::Rejected { reason } if reason.starts_with("interrupted transition")),
        "{halted:?}"
    );

    drop(engine);
    let (engine, alice, bob) = open(dir.path());
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    assert!(
        !dir.path().join("second/callee.go").exists(),
        "restored to empty"
    );
    assert_eq!(read(&live, "callee.go"), CALLEE, "draft kept in place");
    assert_eq!(read(&live, "siblings.go"), LEFT);
    // Agreement and context are unchanged, so the next boundary applies it.
    register(&engine, &alice, "left", "left task", &["siblings.go#Left"]);
    assert!(matches!(
        engine.proposal(&proposal.id).unwrap().unwrap().state,
        ProposalState::Applied { .. }
    ));
    assert_eq!(read(&engine.root(&bob).unwrap(), "callee.go"), CALLEE);
    assert_eq!(read(&live, "callee.go"), GO[2].1);
}
