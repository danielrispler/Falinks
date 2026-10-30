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
/// `LEFT` with a draft of Right too.
const RIGHT: &str = "package probe\n\nfunc Left() int {\n\treturn 2\n}\n\nfunc Right() int {\n\treturn Callee() + 1\n}\n";
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
    // Alice's draft of Right calls Bob's draft of Callee: a compiler edge, nothing else.
    register(
        &engine,
        &alice,
        "right",
        "right task",
        &["siblings.go#Right"],
    );
    applied(&engine, &alice, "right", "siblings.go", RIGHT);
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert_eq!(keep.signals.compiler, vec!["0:drafts 1:drafts".to_string()]);
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

    // Bob now declares a dependency on Right: new collaboration evidence joins.
    declare(
        &engine,
        &bob,
        "callee",
        "callee task",
        &["callee.go#Callee"],
        &["siblings.go#Right"],
    );
    let join = engine.reconsider().unwrap();
    assert_eq!(join.action, Action::Join, "{}", join.reasoning);
    assert_eq!(join.signals.redeclared, vec!["1:callee".to_string()]);
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
    assert_eq!(read(&live, "siblings.go"), RIGHT);
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

/// #357 decision 2: a split refused because two authors drafted one file is void, not
/// held. Once a publication makes it safe, a new proposal applies.
#[test]
fn interleaved_drafts_void_an_agreed_split_until_published() {
    let (dir, engine, alice, bob) = fixture();
    engine.add_workspace(&dir.path().join("second")).unwrap();
    applied(&engine, &alice, "left", "siblings.go", LEFT);
    let propose = || {
        let text = "Callee work next".into();
        let split = Propose {
            action: Action::Split,
            text,
            failing: false,
        };
        engine.propose(&alice, split).unwrap()
    };
    let proposal = propose();
    assert_eq!(
        proposal.state,
        ProposalState::Open,
        "{}",
        proposal.reasoning
    );
    agree(&engine, &alice, &proposal);
    applied(&engine, &bob, "right", "siblings.go", RIGHT);
    let stale = agree(&engine, &bob, &proposal);
    assert_eq!(
        stale.state,
        ProposalState::Stale {
            reason: "unsafe until publication: interleaved unpublished drafts in [\"siblings.go\"]"
                .into()
        }
    );
    // Work continues in the current arrangement.
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    assert_eq!(read(&engine.root(&bob).unwrap(), "siblings.go"), RIGHT);

    publish(&engine, &alice, &bob, "both");
    assert_eq!(engine.proposal(&proposal.id).unwrap().unwrap(), stale);
    let again = propose();
    agree(&engine, &alice, &again);
    let applied = agree(&engine, &bob, &again);
    assert!(
        matches!(applied.state, ProposalState::Applied { .. }),
        "{applied:?}"
    );
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    assert_eq!(read(&engine.root(&bob).unwrap(), "siblings.go"), RIGHT);
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
    // After a transition Alice, with no drafts, has no current work: not her scope (#357).
    assert!(keep.signals.compiler.is_empty(), "{:?}", keep.signals);

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
/// Both joined agents offer their current revision, and it publishes.
fn publish(engine: &Engine, alice: &Client, bob: &Client, id: &str) {
    offer(engine, alice, id);
    offer(engine, bob, id);
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
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
    // Unpublished, Consume's draft uses Alice's draft of Record: a split would strand it.
    assert_eq!(keep.signals.compiler, vec!["0:drafts 1:drafts".to_string()]);
    publish(&engine, &alice, &bob, "contract");

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
    assert_eq!(keep.signals.compiler, vec!["0:drafts 1:drafts".to_string()]);
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
    assert_eq!(keep.signals.overlap, vec!["0:drafts 1:drafts".to_string()]);
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
    publish(&engine, &alice, &bob, "contract");
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

    // An unanswered failure report with a pending offer still splits.
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
    assert!(failing.state.pending());
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

/// #358, from #331's probe `probe_declined_failure_report_twice`: a declined failure report
/// is answered and stops driving engine proposals; a new report counts again.
#[test]
fn a_declined_failure_report_stops_driving_engine_proposals() {
    let (dir, engine, alice, bob) = support::fixture(&[
        ("go.mod", "module example.com/probe\n\ngo 1.22\n"),
        (
            "disc.go",
            "package probe\n\nfunc Disc() int {\n\treturn 0\n}\n",
        ),
    ]);
    fs::create_dir_all(dir.path().join("second")).unwrap();
    configure(&engine);
    register(&engine, &alice, "a", "a", &["disc.go#Disc"]);
    register(&engine, &bob, "b", "b", &["disc.go#Disc"]);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    engine.reconsider().unwrap();
    let report = |text: &str| {
        engine
            .propose(
                &bob,
                Propose {
                    action: Action::Split,
                    text: text.into(),
                    failing: true,
                },
            )
            .unwrap()
    };
    let failing = report("this is not working");
    decline(&engine, &alice, &failing);
    for n in 1..=3 {
        let body = format!("package probe\n\nfunc Disc() int {{\n\treturn {n}\n}}\n");
        applied(&engine, &alice, &format!("d{n}"), "disc.go", &body);
        if let Some(o) = engine
            .obligations(&bob)
            .unwrap()
            .iter()
            .find(|o| o.pending())
        {
            let revision = engine.capture_for(&bob).unwrap().revision;
            let decision = if n % 2 == 0 {
                Decision::Keep
            } else {
                Decision::Revise
            };
            let scope = o.scope.id.clone();
            let note = "r".into();
            engine
                .review(
                    &bob,
                    Review {
                        scope,
                        revision,
                        decision,
                        note,
                    },
                )
                .unwrap();
        }
        let recommended = engine.reconsider().unwrap();
        assert!(!recommended.signals.failing, "{}", recommended.reasoning);
        assert_ne!(
            recommended.action,
            Action::Split,
            "{}",
            recommended.reasoning
        );
    }
    let engine_splits = engine
        .proposals()
        .unwrap()
        .into_iter()
        .filter(|p| p.proposer.is_none() && p.action == Action::Split)
        .count();
    assert_eq!(engine_splits, 0);

    // A new report after the decline counts again.
    let again = report("still not working");
    assert!(again.state.pending() && again.signals.failing);
    let recommended = engine.reconsider().unwrap();
    assert!(recommended.signals.failing, "{}", recommended.reasoning);
    assert_eq!(
        recommended.action,
        Action::Split,
        "{}",
        recommended.reasoning
    );
}

/// A layered Go package (#331): core.go and model.go are the foundation (L1), disc.go and
/// fx.go the independent layer (L2), summary.go the shared hot spot (L3).
const LAYERED: &[(&str, &str)] = &[
    ("go.mod", "module example.com/probe\n\ngo 1.22\n"),
    (
        "core.go",
        "package probe\n\nfunc Money() int {\n\treturn 0\n}\n",
    ),
    (
        "model.go",
        "package probe\n\nfunc Item() int {\n\treturn 0\n}\n",
    ),
    (
        "disc.go",
        "package probe\n\nfunc Disc() int {\n\treturn 0\n}\n",
    ),
    ("fx.go", "package probe\n\nfunc Fx() int {\n\treturn 0\n}\n"),
    (
        "summary.go",
        "package probe\n\nfunc Summary() int {\n\treturn 0\n}\n",
    ),
];
fn layered(files: &[(&str, &str)]) -> (TempDir, Engine, Client, Client) {
    let (dir, engine, alice, bob) = support::fixture(files);
    fs::create_dir_all(dir.path().join("second")).unwrap();
    configure(&engine);
    (dir, engine, alice, bob)
}
/// A Go file holding one function `name` that returns `body`.
fn go(name: &str, body: &str) -> String {
    format!("package probe\n\nfunc {name}() int {{\n\treturn {body}\n}}\n")
}
fn split_by(engine: &Engine, client: &Client) -> RegroupingProposal {
    let propose = Propose {
        action: Action::Split,
        text: "separate".into(),
        failing: false,
    };
    engine.propose(client, propose).unwrap()
}

/// #357 decision 2: an agent's `Split` that would strand a draft's use of the peer's
/// unpublished draft is kept, and the reason names the edge.
#[test]
fn an_agent_split_whose_drafts_relate_is_kept_naming_the_edge() {
    let (dir, engine, alice, bob) = layered(LAYERED);
    register(&engine, &alice, "a", "a", &["disc.go#Disc"]);
    register(&engine, &bob, "b", "b", &["fx.go#Fx"]);
    applied(&engine, &alice, "disc", "disc.go", &go("Disc", "1"));
    applied(&engine, &bob, "fx", "fx.go", &go("Fx", "Disc() + 2"));
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let keep = split_by(&engine, &bob);
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert_eq!(keep.state, ProposalState::Kept);
    for named in ["unsafe until publication", "disc.go#Disc", "fx.go#Fx"] {
        assert!(keep.reasoning.contains(named), "{}", keep.reasoning);
    }
    assert_eq!(engine.placement().unwrap(), [0, 0]);
}

/// #357 decision 2: a draft that lands between agreement and application and relates the
/// moving drafts to the staying ones voids the agreed `Split`; the joined root still builds.
#[test]
fn an_agreed_split_that_would_strand_a_draft_becomes_stale() {
    let (dir, engine, alice, bob) = layered(LAYERED);
    register(&engine, &alice, "a", "a", &["disc.go#Disc"]);
    register(&engine, &bob, "b", "b", &["fx.go#Fx"]);
    applied(&engine, &alice, "disc", "disc.go", &go("Disc", "1"));
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = split_by(&engine, &bob);
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    agree(&engine, &alice, &split);
    applied(&engine, &bob, "fx", "fx.go", &go("Fx", "Disc() + 2"));
    let stale = agree(&engine, &bob, &split);
    let ProposalState::Stale { reason } = &stale.state else {
        panic!("{:?}", stale.state);
    };
    assert!(reason.starts_with("unsafe until publication: "), "{reason}");
    assert!(
        reason.contains("disc.go#Disc") && reason.contains("fx.go#Fx"),
        "{reason}"
    );
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    assert!(vets(&engine.root(&bob).unwrap()));
}
/// Whether `go vet` passes in `root`.
fn vets(root: &Path) -> bool {
    std::process::Command::new("go")
        .args(["vet", "."])
        .current_dir(root)
        .env("GOTOOLCHAIN", "local")
        .status()
        .unwrap()
        .success()
}

/// #331's E1 (`split_strands`): L2 drafts use the peer's unpublished L1 drafts. Whether or not
/// the review rounds turned the recency window over, the drafts keep the pair (#357).
fn strands(rounds: usize) {
    let (dir, engine, alice, bob) = layered(LAYERED);
    register(&engine, &alice, "a", "a", &["core.go", "disc.go"]);
    register(&engine, &bob, "b", "b", &["model.go", "fx.go"]);
    // Each agent adds an L1 function after the foundation's own.
    let added = |file: &str, name: &str| format!("{file}\nfunc {name}() int {{\n\treturn 2\n}}\n");
    let rate = added(LAYERED[1].1, "Rate");
    let price = added(LAYERED[2].1, "Price");
    applied(&engine, &alice, "core", "core.go", &rate);
    applied(&engine, &bob, "model", "model.go", &price);
    for n in 1..=rounds {
        let disc = go("Disc", &format!("Price() * {n}"));
        applied(&engine, &alice, &format!("disc{n}"), "disc.go", &disc);
        settle(&engine, &bob, "b", Decision::Keep);
        let fx = go("Fx", &format!("Rate() * {n}"));
        applied(&engine, &bob, &format!("fx{n}"), "fx.go", &fx);
        settle(&engine, &alice, "a", Decision::Keep);
    }
    assert!(vets(&engine.root(&alice).unwrap()));
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert_eq!(keep.signals.compiler, vec!["0:drafts 1:drafts".to_string()]);
    let own = split_by(&engine, &bob);
    assert_eq!(own.action, Action::Keep, "{}", own.reasoning);
    assert!(
        own.reasoning
            .contains("unpublished drafts core.go#Rate (agent 0) and fx.go#Fx (agent 1)"),
        "{}",
        own.reasoning
    );
}
#[test]
fn drafts_using_the_peers_unpublished_foundation_keep_the_pair_after_two_rounds() {
    strands(2);
}
#[test]
fn drafts_using_the_peers_unpublished_foundation_keep_the_pair_after_one_round() {
    strands(1);
}

/// L1, the foundation L2 uses, before the L2 drafts.
#[derive(PartialEq)]
enum L1 {
    Untouched,
    Drafted,
    Published,
}
/// #331's E2 (`window_never_turns_over`): with narrowed scopes no review turns the window
/// over, and before L1 publishes L2 still uses it: the pair is kept. Once L1 publishes, the
/// L2 drafts are independent: the split applies and both spaces build (#357).
fn foundation(l1: L1) -> (TempDir, Engine, Client, Client, RegroupingProposal) {
    let (dir, engine, alice, bob) = layered(LAYERED);
    register(&engine, &alice, "a", "a", &["core.go#Money"]);
    register(&engine, &bob, "b", "b", &["model.go#Item"]);
    if l1 != L1::Untouched {
        applied(&engine, &alice, "money", "core.go", &go("Money", "100"));
        applied(&engine, &bob, "item", "model.go", &go("Item", "3"));
    }
    if l1 == L1::Published {
        publish(&engine, &alice, &bob, "l1");
    }
    register(&engine, &alice, "a", "a", &["disc.go#Disc"]);
    register(&engine, &bob, "b", "b", &["fx.go#Fx"]);
    for n in 1..=3 {
        let disc = go("Disc", &format!("Money() * {n}"));
        applied(&engine, &alice, &format!("disc{n}"), "disc.go", &disc);
        let fx = go("Fx", &format!("Money() + {n}"));
        applied(&engine, &bob, &format!("fx{n}"), "fx.go", &fx);
    }
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let proposal = engine.reconsider().unwrap();
    (dir, engine, alice, bob, proposal)
}
#[test]
fn an_unpublished_foundation_keeps_the_pair_without_reviews() {
    let (_dir, _engine, _alice, _bob, keep) = foundation(L1::Drafted);
    assert_eq!(keep.action, Action::Keep, "{}", keep.reasoning);
    assert_eq!(keep.signals.compiler, vec!["0:drafts 1:drafts".to_string()]);
}
#[test]
fn a_published_foundation_lets_independent_drafts_split_and_both_spaces_build() {
    let (_dir, engine, alice, bob, split) = foundation(L1::Published);
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    agree(&engine, &alice, &split);
    let applied = agree(&engine, &bob, &split);
    assert!(
        matches!(applied.state, ProposalState::Applied { .. }),
        "{applied:?}"
    );
    assert!(vets(&engine.root(&alice).unwrap()));
    assert!(vets(&engine.root(&bob).unwrap()));
}

/// #331's control for E2: the same L2 drafts with no L1 drafts split.
#[test]
fn the_same_drafts_without_foundation_drafts_split() {
    let (_dir, _engine, _alice, _bob, split) = foundation(L1::Untouched);
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
}

/// Splits Alice's `disc.go` draft from Bob's `fx.go` draft by the engine's recommendation.
fn layered_split(dir: &TempDir, engine: &Engine, alice: &Client, bob: &Client) {
    applied(engine, alice, "disc", "disc.go", &go("Disc", "1"));
    applied(engine, bob, "fx", "fx.go", &go("Fx", "2"));
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = engine.reconsider().unwrap();
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    agree(engine, alice, &split);
    let applied = agree(engine, bob, &split);
    assert!(
        matches!(applied.state, ProposalState::Applied { .. }),
        "{applied:?}"
    );
}

/// #331's E3 (`split_is_followed_by`): scopes that both name the hot spot are not current
/// work once the agents have drafts, so the split is not reversed at the same boundary.
#[test]
fn a_split_is_not_followed_by_a_join_on_overlapping_scopes() {
    let (dir, engine, alice, bob) = layered(LAYERED);
    register(
        &engine,
        &alice,
        "a",
        "a",
        &["disc.go#Disc", "summary.go#Summary"],
    );
    register(&engine, &bob, "b", "b", &["fx.go#Fx", "summary.go#Summary"]);
    layered_split(&dir, &engine, &alice, &bob);
    let proposals = engine.proposals().unwrap();
    assert!(
        proposals.iter().all(|p| p.action != Action::Join),
        "{proposals:?}"
    );
    let last = proposals.last().unwrap();
    assert_eq!(last.action, Action::Keep);
    assert!(
        last.reasoning.contains("independent progress continues"),
        "{}",
        last.reasoning
    );
}

/// #357 decisions 3 and 4: after a split, both agents drafting the hot spot is new evidence.
/// The join is proposed, waits while both spaces hold drafts of the file, and applies once
/// one agent publishes and the other incorporates.
#[test]
fn a_post_split_overlap_between_drafts_joins_after_a_publication() {
    let (dir, engine, alice, bob) = layered(LAYERED);
    register(&engine, &alice, "a", "a", &["disc.go#Disc"]);
    register(&engine, &bob, "b", "b", &["fx.go#Fx"]);
    layered_split(&dir, &engine, &alice, &bob);
    let quiet = engine.reconsider().unwrap();
    assert_eq!(quiet.action, Action::Keep, "{}", quiet.reasoning);
    applied(&engine, &alice, "s1", "summary.go", &go("Summary", "1"));
    applied(&engine, &bob, "s2", "summary.go", &go("Summary", "2"));
    let join = engine.reconsider().unwrap();
    assert_eq!(join.action, Action::Join, "{}", join.reasoning);
    assert_eq!(join.signals.overlap, vec!["0:drafts 1:drafts".to_string()]);
    agree(&engine, &alice, &join);
    let blocked = agree(&engine, &bob, &join);
    assert_eq!(
        blocked.state,
        ProposalState::Blocked {
            blocker: "both workspaces hold unpublished drafts of [\"summary.go\"]".into()
        }
    );

    offer(&engine, &alice, "l3");
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    let mine = engine.capture_for(&bob).unwrap();
    let merged = go("Summary", "1 + 2");
    let outcome = engine
        .incorporate(&bob, edit("merge", &mine, "summary.go", &merged))
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let joined = engine.proposal(&join.id).unwrap().unwrap();
    assert!(
        matches!(joined.state, ProposalState::Applied { .. }),
        "{joined:?}"
    );
    assert_eq!(engine.placement().unwrap(), [0, 0]);
    assert_eq!(read(&engine.root(&alice).unwrap(), "summary.go"), merged);
}

/// #357's `agent_split_scope_edge_then_join`: the published Fx calls Disc and Bob has not
/// edited, so his edge comes from his scope. With no moving drafts his split applies; after it
/// he has no current work, and a later overlap between drafts proposes a join.
#[test]
fn an_agent_split_over_a_scope_edge_applies_and_a_later_draft_overlap_joins() {
    let (dir, engine, alice, bob) = layered(LAYERED);
    register(&engine, &alice, "a", "a", &["disc.go#Disc"]);
    register(&engine, &bob, "b", "b", &["fx.go#Fx"]);
    applied(&engine, &alice, "fxbase", "fx.go", &go("Fx", "Disc() + 2"));
    offer(&engine, &alice, "base");
    let run = engine.validate().unwrap().unwrap();
    assert!(
        matches!(run.outcome, Some(RunOutcome::Published { .. })),
        "{run:?}"
    );
    applied(&engine, &alice, "disc", "disc.go", &go("Disc", "1"));
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = split_by(&engine, &bob);
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    assert_eq!(split.signals.compiler, vec!["0:drafts 1:b".to_string()]);
    agree(&engine, &alice, &split);
    let applied_split = agree(&engine, &bob, &split);
    assert!(
        matches!(applied_split.state, ProposalState::Applied { .. }),
        "{applied_split:?}"
    );
    let quiet = engine.reconsider().unwrap();
    assert!(
        quiet.reasoning.contains("independent progress continues"),
        "{}",
        quiet.reasoning
    );
    applied(&engine, &alice, "s1", "summary.go", &go("Summary", "1"));
    applied(&engine, &bob, "s2", "summary.go", &go("Summary", "2"));
    let join = engine.reconsider().unwrap();
    assert_eq!(join.action, Action::Join, "{}", join.reasoning);
}

/// #357: a draft whose relevance is unbounded (a manifest) relates to every other draft, so
/// it voids an agreed split, and an agent's split is kept.
#[test]
fn a_draft_with_unbounded_relevance_refuses_a_split() {
    let (dir, engine, alice, bob) = layered(LAYERED);
    register(&engine, &alice, "a", "a", &["disc.go#Disc"]);
    register(&engine, &bob, "b", "b", &["fx.go#Fx"]);
    applied(&engine, &bob, "fx", "fx.go", &go("Fx", "2"));
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = split_by(&engine, &bob);
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    agree(&engine, &alice, &split);
    let manifest = "module example.com/probe\n\n// pinned\ngo 1.22\n";
    applied(&engine, &alice, "pin", "go.mod", manifest);
    let unbounded = "unsafe until publication: unbounded relevance of unpublished drafts";
    let stale = agree(&engine, &bob, &split);
    assert_eq!(
        stale.state,
        ProposalState::Stale {
            reason: unbounded.into()
        }
    );
    let own = split_by(&engine, &bob);
    assert_eq!(own.action, Action::Keep, "{}", own.reasoning);
    assert!(own.reasoning.contains(unbounded), "{}", own.reasoning);
    assert_eq!(engine.placement().unwrap(), [0, 0]);
}

/// `Rate` before a long `Money`: once `Rate` is deleted, its old bytes fall inside `Money`'s
/// new range, so only the publication's own evidence names `Rate`.
const RATED: &str = "package probe\n\nfunc Rate() int {\n\treturn 2\n}\n\nfunc Money() int {\n\treturn 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1\n}\n";

/// #364: a draft that deletes a symbol the peer's draft uses is named through the
/// publication-side evidence, so the relationship is seen.
#[test]
fn a_draft_deleting_a_symbol_the_peers_draft_uses_is_related() {
    let mut files = LAYERED.to_vec();
    files[1] = ("core.go", RATED);
    let (dir, engine, alice, bob) = layered(&files);
    register(&engine, &alice, "a", "a", &["core.go#Money"]);
    register(&engine, &bob, "b", "b", &["fx.go#Fx"]);
    engine.add_workspace(&dir.path().join("second")).unwrap();
    let split = split_by(&engine, &bob);
    agree(&engine, &alice, &split);
    agree(&engine, &bob, &split);
    assert_eq!(engine.placement().unwrap(), [0, 1]);
    let deleted = RATED.replace("func Rate() int {\n\treturn 2\n}\n\n", "");
    applied(&engine, &alice, "delete", "core.go", &deleted);
    applied(&engine, &bob, "fx", "fx.go", &go("Fx", "Rate() * 2"));
    let keep = engine.reconsider().unwrap();
    assert_eq!(keep.signals.compiler, vec!["0:drafts 1:drafts".to_string()]);
    assert!(
        keep.reasoning.contains("compiler relationship alone"),
        "{}",
        keep.reasoning
    );
}

/// #357 decision 1: reverted drafts drop out, for `plan` too. A file two authors wrote back
/// to its published bytes does not interleave, so the split applies.
#[test]
fn a_file_both_authors_reverted_does_not_void_a_split() {
    let (dir, engine, alice, bob) = fixture();
    engine.add_workspace(&dir.path().join("second")).unwrap();
    applied(&engine, &alice, "left", "siblings.go", LEFT);
    applied(&engine, &bob, "revert", "siblings.go", GO[1].1);
    let split = split_by(&engine, &alice);
    assert_eq!(split.action, Action::Split, "{}", split.reasoning);
    agree(&engine, &alice, &split);
    let applied = agree(&engine, &bob, &split);
    assert!(
        matches!(applied.state, ProposalState::Applied { .. }),
        "{applied:?}"
    );
}
