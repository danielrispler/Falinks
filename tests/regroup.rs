//! Join/split/keep recommendations, agreement and work-preserving transitions (#25).
//! Relevance uses the real pinned gopls/Go toolchain; checks are a trivial trusted command.
mod support;

use falinks::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::Duration,
};
use support::{GO, edit, fail_commits, fails_with, go_toolchain, halt, incidents, snapshot};
use tempfile::TempDir;

const LEFT: &str = "package probe\n\nfunc Left() int {\n\treturn 2\n}\n\nfunc Right() int {\n\treturn Callee()\n}\n";
const CALLEE: &str = "package probe\n\nfunc Callee() int {\n\treturn 8\n}\n";

fn reopen(root: &Path) -> (Engine, Client, Client) {
    let (engine, alice, bob) = support::reopen(root, GO);
    configure(&engine);
    (engine, alice, bob)
}
fn fixture() -> (TempDir, Engine, Client, Client) {
    let (dir, engine, alice, bob) = support::fixture(GO);
    fs::create_dir_all(dir.path().join("second")).unwrap();
    configure(&engine);
    (dir, engine, alice, bob)
}
fn configure(engine: &Engine) {
    engine.configure_analysis(vec![go_toolchain()]).unwrap();
    engine
        .configure_checks(vec![Check {
            name: "trusted".into(),
            program: "/usr/bin/true".into(),
            args: Vec::new(),
        }])
        .unwrap();
}
fn register(engine: &Engine, client: &Client, id: &str, task: &str, nodes: &[&str]) {
    declare(engine, client, id, task, nodes, &[]);
}
fn declare(
    engine: &Engine,
    client: &Client,
    id: &str,
    task: &str,
    nodes: &[&str],
    depends: &[&str],
) {
    let set = |s: &[&str]| s.iter().map(|n| n.to_string()).collect();
    engine
        .register(
            client,
            Scope {
                id: id.into(),
                task: task.into(),
                nodes: set(nodes),
                depends: set(depends),
            },
        )
        .unwrap();
}
/// Alice drafts `Left`, Bob drafts `Callee`: independent scopes and unfinished work.
fn independent(engine: &Engine, alice: &Client, bob: &Client) {
    register(engine, alice, "left", "left task", &["siblings.go#Left"]);
    register(engine, bob, "callee", "callee task", &["callee.go#Callee"]);
    applied(engine, alice, "left", "siblings.go", LEFT);
    applied(engine, bob, "callee", "callee.go", CALLEE);
}
fn applied(engine: &Engine, client: &Client, id: &str, path: &str, bytes: &str) -> Capture {
    let current = engine.capture_for(client).unwrap();
    let outcome = engine
        .apply(client, edit(id, &current, path, bytes))
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    engine.capture_for(client).unwrap()
}
fn agree(engine: &Engine, client: &Client, proposal: &RegroupingProposal) -> RegroupingProposal {
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
                reviews: Vec::new(),
            },
        )
        .unwrap()
        .checkpoint
}
fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).unwrap()
}
fn agents(list: &[usize]) -> BTreeSet<usize> {
    list.iter().copied().collect()
}
/// Splits the independent drafts into the provisioned second root.
fn split(dir: &TempDir, engine: &Engine, alice: &Client, bob: &Client) -> RegroupingProposal {
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

    // Recommended with concise evidence and no prior failure; both must agree.
    let proposal = engine.reconsider().unwrap();
    // Offered after the proposal: a pending offer keeps the pair (#305).
    let old = offer(&engine, &alice, "team");
    assert_eq!(old.members, agents(&[0, 1]));
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
    let (engine, alice, bob) = reopen(dir.path());
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    assert_eq!(engine.root(&bob).unwrap(), second);
    assert_eq!(engine.root(&alice).unwrap(), live);
    let proposals = engine.proposals().unwrap();
    assert!(matches!(proposals[0].state, ProposalState::Applied { .. }));
    assert!(
        proposals[1..].iter().all(|p| p.action == Action::Keep),
        "the engine reconsiders by itself but never reverses: {proposals:?}"
    );
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

    // Observed waiting on the peer is reported but decides nothing.
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
    let waited = engine.reconsider().unwrap();
    assert_eq!(waited.id, keep.id, "{}", waited.reasoning);

    // Bob's work now names Right too: overlap joins.
    register(&engine, &bob, "both", "callee task", &["siblings.go#Right"]);
    let join = engine.reconsider().unwrap();
    assert_eq!(join.action, Action::Join, "{}", join.reasoning);
    assert_eq!(join.signals.overlap, vec!["0:right 1:both".to_string()]);
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
    fails_with(
        engine.respond(&alice, &proposal.id, &proposal.context, Response::Agree),
        "proposal is not open",
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
    fails_with(
        engine.respond(&bob, &proposal.id, &proposal.context, Response::Agree),
        "proposal is not open",
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
    applied(&engine, &alice, "left", "siblings.go", LEFT);
    let both = "package probe\n\nfunc Left() int {\n\treturn 2\n}\n\nfunc Right() int {\n\treturn Callee() + 1\n}\n";
    applied(&engine, &bob, "right", "siblings.go", both);
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
    let mine = applied(&engine, &bob, "right", "siblings.go", right);
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
    // An overlapping publication is evidence to join.
    let join = engine.reconsider().unwrap();
    assert_eq!(join.action, Action::Join, "{}", join.reasoning);
    assert_eq!(join.signals.behind, vec!["siblings.go".to_string()]);
    // Offers from the stale workspace cannot publish over the newer state.
    offer(&engine, &bob, "stale");
    assert!(engine.validate().unwrap().is_none());

    let merged = "package probe\n\nfunc Left() int {\n\treturn 2\n}\n\nfunc Right() int {\n\treturn Callee() + 1\n}\n";
    let unresolved = engine
        .incorporate(&bob, edit("nothing", &mine, "callee.go", CALLEE))
        .unwrap();
    assert!(
        matches!(unresolved, Outcome::Rejected { .. }),
        "{unresolved:?}"
    );
    let outcome = engine
        .incorporate(&bob, edit("merge", &mine, "siblings.go", merged))
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
    let (dir, engine, alice, bob) = fixture();
    independent(&engine, &alice, &bob);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let proposal = engine.reconsider().unwrap();
    agree(&engine, &alice, &proposal);
    drop(engine);
    // The new root installs first; the transition stops before it reaches the shared root.
    let live = dir.path().join("live");
    let hook: FaultHook = Box::new(|_, fault| match fault {
        Fault::Audited { space: 1 } => Err("interrupt".into()),
        _ => Ok(()),
    });
    let enrolled: Vec<_> = GO.iter().map(|(p, _)| *p).collect();
    let engine =
        Engine::open_with_faults(&live, &dir.path().join("state"), &enrolled, hook).unwrap();
    configure(&engine);
    let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
    let bob = engine.authenticate(1, &engine.credentials()[1]).unwrap();
    let failed = engine
        .respond(&bob, &proposal.id, &proposal.context, Response::Agree)
        .unwrap();
    assert!(
        matches!(&failed.state, ProposalState::Blocked { blocker } if blocker.starts_with("transition failed")),
        "{failed:?}"
    );
    assert_eq!(read(&dir.path().join("second"), "callee.go"), CALLEE);
    let halted = engine
        .apply(
            &alice,
            Request {
                id: "halted".into(),
                expected: BTreeMap::from([("go.mod".into(), 0)]),
                output: BTreeMap::from([("go.mod".into(), Output::Delete)]),
                note: "delete go.mod".into(),
                reviews: Vec::new(),
            },
        )
        .unwrap();
    assert!(
        matches!(&halted, Outcome::Rejected { reason } if reason.starts_with("interrupted transition")),
        "{halted:?}"
    );

    drop(engine);
    let (engine, alice, bob) = reopen(dir.path());
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

/// A statement failing inside an agreed split's commit transaction, after both roots are
/// installed, is an interruption (#158).
fn split_commit_failure_is_an_interruption(operation_between: bool) {
    let (dir, engine, alice, bob) = fixture();
    independent(&engine, &alice, &bob);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let proposal = engine.reconsider().unwrap();
    agree(&engine, &alice, &proposal);
    let (live, second) = (dir.path().join("live"), dir.path().join("second"));
    let roots = [snapshot(&live), snapshot(&second)];
    fail_commits(dir.path(), true);
    let failed = engine
        .respond(&bob, &proposal.id, &proposal.context, Response::Agree)
        .unwrap();
    fail_commits(dir.path(), false);
    assert!(
        matches!(&failed.state, ProposalState::Blocked { blocker } if blocker.starts_with("transition failed")),
        "{failed:?}"
    );
    let halted = halt(dir.path()).unwrap();
    assert!(
        matches!(
            halted,
            Halt::Interruption {
                installation: Interrupted::Transition(_),
                ..
            }
        ),
        "{halted:?}"
    );
    let reason = halted.to_string();
    if operation_between {
        let current = engine.capture_for(&alice).unwrap();
        let later = engine
            .apply(&alice, edit("later", &current, "siblings.go", GO[1].1))
            .unwrap();
        assert_eq!(later, Outcome::Rejected { reason });
    }
    assert_eq!(incidents(dir.path()), 0);
    drop(engine);

    let (engine, alice, bob) = reopen(dir.path());
    assert_eq!([snapshot(&live), snapshot(&second)], roots);
    assert_eq!(incidents(dir.path()), 0);
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    // Agreement and context are unchanged, so the next boundary applies it.
    register(&engine, &alice, "left", "left task", &["siblings.go#Left"]);
    assert!(matches!(
        engine.proposal(&proposal.id).unwrap().unwrap().state,
        ProposalState::Applied { .. }
    ));
    assert_eq!(read(&engine.root(&bob).unwrap(), "callee.go"), CALLEE);
    assert_eq!(read(&live, "callee.go"), GO[2].1);
}
#[test]
fn split_commit_failure_with_an_operation_before_restart_is_an_interruption() {
    split_commit_failure_is_an_interruption(true);
}
#[test]
fn split_commit_failure_with_no_operation_before_restart_is_an_interruption() {
    split_commit_failure_is_an_interruption(false);
}

/// Restart recovery checks every root before it restores any (#159): an interrupted split,
/// and an unexplained file in a root the split does not touch.
#[test]
fn interrupted_transition_with_a_tampered_unaffected_root_refuses_without_change() {
    let (dir, engine, alice, bob) = fixture();
    independent(&engine, &alice, &bob);
    let (live, second, third) = (
        dir.path().join("live"),
        dir.path().join("second"),
        dir.path().join("third"),
    );
    engine.add_workspace(&second).unwrap();
    fs::create_dir(&third).unwrap();
    engine.add_workspace(&third).unwrap();
    let proposal = engine.reconsider().unwrap();
    agree(&engine, &alice, &proposal);
    fail_commits(dir.path(), true);
    engine
        .respond(&bob, &proposal.id, &proposal.context, Response::Agree)
        .unwrap();
    fail_commits(dir.path(), false);
    assert_eq!(
        read(&second, "callee.go"),
        CALLEE,
        "the split reached second"
    );
    drop(engine);
    fs::write(third.join("x.go"), "package probe\n").unwrap();
    let roots = [snapshot(&live), snapshot(&second), snapshot(&third)];

    fails_with(support::open(dir.path(), GO), "unexplained source write");
    assert_eq!(
        [snapshot(&live), snapshot(&second), snapshot(&third)],
        roots
    );
    let installing: bool = support::ledger(dir.path())
        .query_row(
            "SELECT json_extract(body, '$.installing') FROM shifts ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(installing);
    assert_eq!(incidents(dir.path()), 1);
    let halted = halt(dir.path()).unwrap();
    assert!(
        matches!(&halted, Halt::Incident { reason } if reason.contains("unexplained source write")),
        "{halted:?}"
    );
}

/// A statement failing inside the commit transaction that incorporates a publication into a
/// split workspace, after that root is installed, is an interruption (#158).
fn incorporation_commit_failure_is_an_interruption(operation_between: bool) {
    let (dir, engine, alice, bob) = fixture();
    split(&dir, &engine, &alice, &bob);
    offer(&engine, &alice, "alone");
    let second = dir.path().join("second");
    let before = snapshot(&second);
    fail_commits(dir.path(), true);
    engine.validate().unwrap();
    fail_commits(dir.path(), false);
    let halted = halt(dir.path()).unwrap();
    assert!(
        matches!(
            halted,
            Halt::Interruption {
                installation: Interrupted::Transition(_),
                ..
            }
        ),
        "{halted:?}"
    );
    let reason = halted.to_string();
    if operation_between {
        let current = engine.capture_for(&bob).unwrap();
        let later = engine
            .apply(&bob, edit("later", &current, "callee.go", GO[2].1))
            .unwrap();
        assert_eq!(later, Outcome::Rejected { reason });
    }
    assert_eq!(incidents(dir.path()), 0);
    drop(engine);

    let (engine, _, bob) = reopen(dir.path());
    assert_eq!(snapshot(&second), before);
    assert_eq!(incidents(dir.path()), 0);
    assert_eq!(read(&second, "siblings.go"), GO[1].1);
    // The next boundary brings the publication in.
    register(
        &engine,
        &bob,
        "callee",
        "callee task",
        &["callee.go#Callee"],
    );
    assert_eq!(read(&second, "siblings.go"), LEFT);
    assert_eq!(read(&second, "callee.go"), CALLEE, "Bob's draft untouched");
}
#[test]
fn incorporation_commit_failure_with_an_operation_before_restart_is_an_interruption() {
    incorporation_commit_failure_is_an_interruption(true);
}
#[test]
fn incorporation_commit_failure_with_no_operation_before_restart_is_an_interruption() {
    incorporation_commit_failure_is_an_interruption(false);
}

#[test]
fn join_waits_while_both_workspaces_drafted_the_same_file() {
    let (dir, engine, alice, bob) = fixture();
    split(&dir, &engine, &alice, &bob);
    let right = "package probe\n\nfunc Left() int {\n\treturn 1\n}\n\nfunc Right() int {\n\treturn Callee() + 1\n}\n";
    applied(&engine, &bob, "right", "siblings.go", right);
    let join = engine
        .propose(
            &bob,
            Propose {
                action: Action::Join,
                text: "split is not working".into(),
                failing: true,
            },
        )
        .unwrap();
    agree(&engine, &alice, &join);
    let blocked = agree(&engine, &bob, &join);
    assert_eq!(
        blocked.state,
        ProposalState::Blocked {
            blocker: "both workspaces hold unpublished drafts of [\"siblings.go\"]".into()
        }
    );
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    assert_eq!(read(&engine.root(&alice).unwrap(), "siblings.go"), LEFT);
    assert_eq!(read(&engine.root(&bob).unwrap(), "siblings.go"), right);
}

#[test]
fn retained_candidate_keeps_its_membership_across_a_split() {
    let (dir, engine, alice, bob) = fixture();
    engine.require_members(agents(&[0, 1])).unwrap();
    applied(&engine, &alice, "left", "siblings.go", LEFT);
    let held = engine.capture().unwrap();
    let alices = offer(&engine, &alice, "held");
    assert_eq!(alices.members, agents(&[0, 1]), "Bob is a required member");
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = engine
        .propose(
            &bob,
            Propose {
                action: Action::Split,
                text: "separate work".into(),
                failing: false,
            },
        )
        .unwrap();
    agree(&engine, &alice, &split);
    agree(&engine, &bob, &split);
    assert_eq!(engine.placement().unwrap(), [0, 1]);

    // Same identity, same members: Bob still has to offer the exact held candidate.
    let candidate = engine.candidate(held.revision).unwrap();
    assert_eq!(candidate.binding.members, agents(&[0, 1]));
    assert_eq!(candidate.missing, agents(&[1]));
    assert_eq!(engine.checkpoint("0/held").unwrap().unwrap(), alices);
    let bobs = engine
        .offer(
            &bob,
            Offer {
                id: "held".into(),
                task: "task".into(),
                revision: held.revision,
                scope: Vec::new(),
                text: "held".into(),
                supersedes: None,
                reviews: Vec::new(),
            },
        )
        .unwrap()
        .checkpoint;
    assert_eq!(bobs.members, agents(&[0, 1]));
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    assert_eq!(read(&engine.root(&bob).unwrap(), "siblings.go"), LEFT);
}

#[test]
fn obligations_a_transition_raised_are_not_evidence_to_reverse_it() {
    let (dir, engine, alice, bob) = fixture();
    // Right calls Callee: a compiler edge, so the engine itself would not split.
    register(
        &engine,
        &alice,
        "right",
        "right task",
        &["siblings.go#Right"],
    );
    register(
        &engine,
        &bob,
        "callee",
        "callee task",
        &["callee.go#Callee"],
    );
    applied(&engine, &bob, "callee", "callee.go", CALLEE);
    engine
        .review(
            &alice,
            Review {
                scope: "right".into(),
                revision: engine.capture_for(&alice).unwrap().revision,
                decision: Decision::Keep,
                note: "read Callee".into(),
            },
        )
        .unwrap();
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = engine
        .propose(
            &alice,
            Propose {
                action: Action::Split,
                text: "Right is done for now".into(),
                failing: false,
            },
        )
        .unwrap();
    agree(&engine, &alice, &split);
    agree(&engine, &bob, &split);
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    // Bob's Callee draft left Alice's view: a real obligation, caused by the transition.
    assert!(
        engine
            .obligations(&alice)
            .unwrap()
            .iter()
            .any(|o| o.pending())
    );

    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert_eq!(keep.signals.reconsiderations, 0);
    assert!(
        !keep.signals.uncertain.is_empty(),
        "uncertainty alone keeps a split"
    );
    assert_eq!(keep.signals.compiler, vec!["0:right 1:callee".to_string()]);

    // A declared dependency on Bob's work is new collaboration evidence.
    declare(
        &engine,
        &alice,
        "right",
        "right task",
        &["siblings.go#Right"],
        &["callee.go#Callee"],
    );
    let join = engine.reconsider().unwrap();
    assert_eq!(join.action, Action::Join, "{}", join.reasoning);
    assert_eq!(join.signals.declared, vec!["0:right 1:callee".to_string()]);
}

/// A producer/consumer Go module: `Consume` uses `Produce`.
const PAIRED: &[(&str, &str)] = &[
    ("go.mod", "module example.com/probe\n\ngo 1.22\n"),
    (
        "producer.go",
        "package probe\n\nfunc Produce() []string {\n\treturn []string{\"a\"}\n}\n",
    ),
    (
        "consumer.go",
        "package probe\n\nfunc Consume() int {\n\treturn len(Produce())\n}\n",
    ),
];
const RECORD: &str = "package probe\n\ntype Record struct {\n\tName string\n}\n\nfunc Produce() []Record {\n\treturn []Record{{Name: \"a\"}}\n}\n";
const CONSUME_RECORD: &str = "package probe\n\nfunc Consume() int {\n\tn := 0\n\tfor _, r := range Produce() {\n\t\tn += len(r.Name)\n\t}\n\treturn n\n}\n";

/// Whole-file registration with mutual declarations, as in every #27 run.
fn paired() -> (TempDir, Engine, Client, Client) {
    let (dir, engine, alice, bob) = support::fixture(PAIRED);
    fs::create_dir_all(dir.path().join("second")).unwrap();
    configure(&engine);
    declare(
        &engine,
        &alice,
        "producer",
        "produce",
        &["producer.go"],
        &["consumer.go"],
    );
    declare(
        &engine,
        &bob,
        "consumer",
        "consume",
        &["consumer.go"],
        &["producer.go"],
    );
    (dir, engine, alice, bob)
}
/// Settles the client's pending obligation on its only scope.
fn settle(engine: &Engine, client: &Client, scope: &str, decision: Decision) {
    let pending = engine.obligations(client).unwrap();
    assert!(pending.iter().any(|o| o.pending()), "{pending:?}");
    engine
        .review(
            client,
            Review {
                scope: scope.into(),
                revision: engine.capture_for(client).unwrap().revision,
                decision,
                note: format!("{decision:?}"),
            },
        )
        .unwrap();
}
/// Appends `bytes` to the client's current copy of `path`.
fn append(engine: &Engine, client: &Client, id: &str, path: &str, bytes: &str) {
    let current = engine.capture_for(client).unwrap();
    let file = current.files[path].file.as_ref().unwrap();
    let text = String::from_utf8(file.bytes.clone()).unwrap() + bytes;
    applied(engine, client, id, path, &text);
}

#[test]
fn independent_follow_ups_after_a_contract_change_propose_a_split() {
    let (_dir, engine, alice, bob) = paired();
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert!(!keep.signals.compiler.is_empty(), "Consume uses Produce");

    // The contract change: Bob revises, Alice keeps.
    applied(&engine, &alice, "record", "producer.go", RECORD);
    settle(&engine, &bob, "consumer", Decision::Revise);
    applied(&engine, &bob, "consume", "consumer.go", CONSUME_RECORD);
    settle(&engine, &alice, "producer", Decision::Keep);
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert!(!keep.signals.revised.is_empty(), "{:?}", keep.signals);

    // Follow-ups in new functions, each reviewed Keep.
    append(
        &engine,
        &bob,
        "summary",
        "consumer.go",
        "\nfunc Summary() string {\n\treturn \"ok\"\n}\n",
    );
    let pending = engine.reconsider().unwrap();
    assert_eq!(pending.action, Action::Keep, "{}", pending.reasoning);
    assert!(
        !pending.signals.uncertain.is_empty(),
        "unreviewed obligation"
    );
    settle(&engine, &alice, "producer", Decision::Keep);
    let one_way = engine.reconsider().unwrap();
    assert_eq!(
        one_way.action,
        Action::Keep,
        "Bob's latest review is Revise"
    );
    append(
        &engine,
        &alice,
        "stats",
        "producer.go",
        "\nfunc Stats() int {\n\treturn 3\n}\n",
    );
    settle(&engine, &bob, "consumer", Decision::Keep);

    let split = engine.reconsider().unwrap();
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    assert!(split.signals.compiler.is_empty(), "{:?}", split.signals);
    assert!(split.signals.overlap.is_empty() && split.signals.revised.is_empty());
    assert!(
        !split.signals.declared.is_empty(),
        "declarations no longer veto"
    );
}

#[test]
fn consumer_edits_calling_the_producer_keep_the_pair() {
    let (_dir, engine, alice, bob) = paired();
    let produce =
        "package probe\n\nfunc Produce() []string {\n\treturn []string{\"a\", \"b\"}\n}\n";
    applied(&engine, &alice, "produce", "producer.go", produce);
    settle(&engine, &bob, "consumer", Decision::Keep);
    let consume = "package probe\n\nfunc Consume() int {\n\treturn len(Produce()) + 1\n}\n";
    applied(&engine, &bob, "consume", "consumer.go", consume);
    settle(&engine, &alice, "producer", Decision::Keep);
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert_eq!(keep.signals.compiler, vec!["0:edits 1:edits".to_string()]);
    assert!(keep.signals.revised.is_empty() && keep.signals.overlap.is_empty());
}

#[test]
fn both_agents_editing_the_same_symbol_keep_the_pair() {
    let (_dir, engine, alice, bob) = paired();
    let first = "package probe\n\nfunc Produce() []string {\n\treturn []string{\"a\", \"b\"}\n}\n";
    applied(&engine, &alice, "first", "producer.go", first);
    settle(&engine, &bob, "consumer", Decision::Keep);
    let second = "package probe\n\nfunc Produce() []string {\n\treturn []string{\"c\"}\n}\n";
    applied(&engine, &bob, "second", "producer.go", second);
    settle(&engine, &alice, "producer", Decision::Keep);
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert_eq!(keep.signals.overlap, vec!["0:edits 1:edits".to_string()]);
}

#[test]
fn an_unreviewed_obligation_keeps_the_pair_as_uncertain() {
    let (_dir, engine, alice, bob) = paired();
    append(
        &engine,
        &alice,
        "stats",
        "producer.go",
        "\nfunc Stats() int {\n\treturn 3\n}\n",
    );
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert!(
        keep.signals
            .uncertain
            .iter()
            .any(|u| u.starts_with("agent 1 has not reviewed consumer")),
        "{:?}",
        keep.signals
    );
    // Once reviewed Keep, nothing current ties Stats to Bob's registered scope.
    settle(&engine, &bob, "consumer", Decision::Keep);
    assert_eq!(engine.reconsider().unwrap().action, Action::Split);
}

#[test]
fn after_a_split_only_a_new_declaration_joins() {
    let (dir, engine, alice, bob) = fixture();
    independent(&engine, &alice, &bob);
    // A declaration on Bob's work does not veto the split.
    declare(
        &engine,
        &alice,
        "left",
        "left task",
        &["siblings.go#Left"],
        &["callee.go#Callee"],
    );
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = engine.reconsider().unwrap();
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    assert_eq!(split.signals.declared, vec!["0:left 1:callee".to_string()]);
    agree(&engine, &alice, &split);
    agree(&engine, &bob, &split);
    assert_eq!(engine.placement().unwrap(), [0, 1]);

    declare(
        &engine,
        &alice,
        "left",
        "left task",
        &["siblings.go#Left"],
        &["callee.go#Callee"],
    );
    let unchanged = engine.reconsider().unwrap();
    assert_eq!(unchanged.action, Action::Keep, "{}", unchanged.reasoning);
    assert!(unchanged.signals.redeclared.is_empty());

    declare(
        &engine,
        &alice,
        "left",
        "left task",
        &["siblings.go#Left"],
        &["callee.go#Callee", "siblings.go#Right"],
    );
    let join = engine.reconsider().unwrap();
    assert_eq!(join.action, Action::Join, "{}", join.reasoning);
    assert_eq!(join.signals.redeclared, vec!["0:left".to_string()]);
}

#[test]
fn engine_keeps_are_silent_but_recommend_returns_them() {
    let (_dir, engine, alice, bob) = paired();
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep);
    assert_eq!(engine.recommend(&bob).unwrap().id, keep.id);
    for client in [&alice, &bob] {
        assert!(
            !engine
                .events(client, 0)
                .unwrap()
                .iter()
                .any(|e| matches!(e.body, Body::Regrouping(_))),
            "no Regrouping event for an engine-made Keep"
        );
    }
    // An agent's own proposal still reaches both agents.
    let own = engine
        .propose(
            &alice,
            Propose {
                action: Action::Keep,
                text: "stay".into(),
                failing: false,
            },
        )
        .unwrap();
    for client in [&alice, &bob] {
        assert!(
            engine
                .events(client, 0)
                .unwrap()
                .iter()
                .any(|e| matches!(&e.body, Body::Regrouping(p) if p.id == own.id))
        );
    }
}

#[test]
fn a_pending_offer_keeps_the_pair_until_the_revision_advances() {
    let (_dir, engine, alice, bob) = paired();
    applied(&engine, &alice, "record", "producer.go", RECORD);
    settle(&engine, &bob, "consumer", Decision::Revise);
    applied(&engine, &bob, "consume", "consumer.go", CONSUME_RECORD);
    settle(&engine, &alice, "producer", Decision::Keep);
    let follow_up = |client: &Client, id: &str, path: &str, scope: &str, other: &Client| {
        append(
            &engine,
            client,
            id,
            path,
            &format!("\nfunc {id}() int {{\n\treturn 1\n}}\n"),
        );
        settle(&engine, other, scope, Decision::Keep);
    };
    follow_up(&bob, "Summary", "consumer.go", "producer", &alice);
    follow_up(&alice, "Stats", "producer.go", "consumer", &bob);
    assert_eq!(engine.reconsider().unwrap().action, Action::Split);

    // Withdrawn and superseded offers do not suppress.
    offer(&engine, &alice, "gone");
    engine.withdraw(&alice, "gone").unwrap();
    offer(&engine, &bob, "old");
    let revision = engine.capture_for(&bob).unwrap().revision;
    let replacement = Offer {
        id: "new".into(),
        task: "task".into(),
        revision,
        scope: Vec::new(),
        text: "new".into(),
        supersedes: Some("old".into()),
        reviews: Vec::new(),
    };
    engine.offer(&bob, replacement).unwrap();
    engine.withdraw(&bob, "new").unwrap();
    let split = engine.reconsider().unwrap();
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    assert!(split.signals.pending.is_empty(), "{:?}", split.signals);

    offer(&engine, &alice, "final");
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert!(keep.reasoning.contains("0/final"), "{}", keep.reasoning);

    // The next accepted edit advances the revision past the offer.
    follow_up(&bob, "Total", "consumer.go", "producer", &alice);
    let split = engine.reconsider().unwrap();
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);

    // A failure report with a pending offer still splits.
    decline(&engine, &alice, &split);
    offer(&engine, &bob, "again");
    let failing = engine
        .propose(
            &bob,
            Propose {
                action: Action::Split,
                text: "not working".into(),
                failing: true,
            },
        )
        .unwrap();
    decline(&engine, &alice, &failing);
    let split = engine.reconsider().unwrap();
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    assert!(split.signals.failing && !split.signals.pending.is_empty());
}
fn decline(engine: &Engine, client: &Client, proposal: &RegroupingProposal) {
    let reason = "stay".into();
    engine
        .respond(
            client,
            &proposal.id,
            &proposal.context,
            Response::Decline { reason },
        )
        .unwrap();
}

/// Bob's consumer depends on `Produce` and `Record` by symbol, as in #249's go-relationships.
fn symbolic() -> (TempDir, Engine, Client, Client) {
    let (dir, engine, alice, bob) = support::fixture(&[
        PAIRED[0],
        ("producer.go", RECORD),
        ("consumer.go", CONSUME_RECORD),
    ]);
    fs::create_dir_all(dir.path().join("second")).unwrap();
    configure(&engine);
    register(&engine, &alice, "producer", "produce", &["producer.go"]);
    declare(
        &engine,
        &bob,
        "consumer",
        "consume",
        &["consumer.go"],
        &["producer.go#Produce", "producer.go#Record"],
    );
    (dir, engine, alice, bob)
}
fn unreviewed(signals: &Signals) -> bool {
    signals
        .uncertain
        .iter()
        .any(|u| u.starts_with("agent 1 has not reviewed consumer"))
}
const STATS: &str = "\nfunc Stats() int {\n\treturn 3\n}\n";

#[test]
fn an_insertion_unrelated_by_owner_is_not_uncertain() {
    let (_dir, engine, alice, bob) = symbolic();
    append(&engine, &alice, "stats", "producer.go", STATS);
    assert!(
        engine
            .obligations(&bob)
            .unwrap()
            .iter()
            .any(|o| o.pending())
    );
    let signals = engine.recommend(&alice).unwrap().signals;
    assert!(!unreviewed(&signals), "{signals:?}");
}

#[test]
fn an_edit_to_a_depended_symbol_is_uncertain_until_reviewed() {
    let (_dir, engine, alice, bob) = symbolic();
    let widened = RECORD.replace("Name string", "Name string\n\tSize int");
    applied(&engine, &alice, "size", "producer.go", &widened);
    assert!(unreviewed(&engine.recommend(&alice).unwrap().signals));
    settle(&engine, &bob, "consumer", Decision::Keep);
    assert!(!unreviewed(&engine.recommend(&alice).unwrap().signals));
}

#[test]
fn a_file_level_edit_without_owners_is_uncertain() {
    let (_dir, engine, alice, _bob) = symbolic();
    let doc = format!("// Package probe produces records.\n{RECORD}");
    applied(&engine, &alice, "doc", "producer.go", &doc);
    let signals = engine.recommend(&alice).unwrap().signals;
    assert!(unreviewed(&signals), "{signals:?}");
}

#[test]
fn a_mixed_edit_reads_as_its_new_owner_only() {
    // Accepted limitation (#306): the out-of-owner change loses its file-level coupling.
    let (_dir, engine, alice, _bob) = symbolic();
    let mixed = format!("{RECORD}\n// Records end here.\n{STATS}");
    applied(&engine, &alice, "mixed", "producer.go", &mixed);
    let signals = engine.recommend(&alice).unwrap().signals;
    assert!(!unreviewed(&signals), "{signals:?}");
}

#[test]
fn an_insertion_only_obligation_does_not_hold_a_split() {
    let (_dir, engine, alice, bob) = symbolic();
    let follow_up = |client: &Client, id: &str, path: &str, scope: &str, other: &Client| {
        append(
            &engine,
            client,
            id,
            path,
            &format!("\nfunc {id}() int {{\n\treturn 1\n}}\n"),
        );
        settle(&engine, other, scope, Decision::Keep);
    };
    // Two review rounds turn the window over.
    follow_up(&bob, "Summary", "consumer.go", "producer", &alice);
    follow_up(&alice, "Stats", "producer.go", "consumer", &bob);
    follow_up(&bob, "Total", "consumer.go", "producer", &alice);
    append(
        &engine,
        &alice,
        "Duplicates",
        "producer.go",
        STATS.replace("Stats", "Duplicates").as_str(),
    );
    assert!(
        engine
            .obligations(&bob)
            .unwrap()
            .iter()
            .any(|o| o.pending())
    );
    let split = engine.reconsider().unwrap();
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    assert!(split.signals.overlap.is_empty() && split.signals.compiler.is_empty());
}

fn offer_at(engine: &Engine, client: &Client, id: &str, revision: u64) -> Offered {
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
                reviews: Vec::new(),
            },
        )
        .unwrap()
}

/// #331's E4: a candidate held from before a split holds Bob's own earlier draft. Its
/// publication after the split is no overlap with Bob's later draft of the same file.
#[test]
fn pre_split_candidate_publication_is_not_an_overlap_with_own_later_draft() {
    let (dir, engine, alice, bob) = fixture();
    applied(&engine, &alice, "left", "siblings.go", LEFT);
    let held = applied(&engine, &bob, "callee", "callee.go", CALLEE);
    offer_at(&engine, &alice, "held", held.revision);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = engine
        .propose(
            &bob,
            Propose {
                action: Action::Split,
                text: "separate".into(),
                failing: false,
            },
        )
        .unwrap();
    agree(&engine, &alice, &split);
    agree(&engine, &bob, &split);
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    let later = CALLEE.replace('8', "9");
    applied(&engine, &bob, "callee2", "callee.go", &later);
    let offered = offer_at(&engine, &bob, "held", held.revision);
    assert!(offered.coverage.queued, "{:?}", offered.coverage);
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    let events = engine.events(&bob, 0).unwrap();
    assert!(
        !events.iter().any(|e| matches!(e.body, Body::Behind { .. })),
        "{events:?}"
    );
    let second = engine.root(&bob).unwrap();
    assert_eq!(read(&second, "callee.go"), later);
    assert_eq!(read(&second, "siblings.go"), LEFT);
}
