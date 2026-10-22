//! Crash matrix for transitions, synchronization and restart recovery (#162).
//!
//! Each case stops the engine at one `Fault` point, once with an error from the host fault
//! hook (in process) and once with a process exit through `crash_worker`, then restarts and
//! runs one operation that reaches the boundary. An oracle walks every root and reads the
//! ledger itself; it never calls the engine's audit.
//!
//! Limits: no power loss or operating-system crash; an error inside the commit transaction
//! (`Fault::Commit`) stands in for an I/O failure of the commit itself; concurrent writers
//! that bypass the engine are out of scope; so are crash points between the listed steps.
mod support;

use falinks::*;
use rusqlite::OptionalExtension;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use support::{PAIR, edit, fails_with, halt, incidents, ledger};
use tempfile::TempDir;

const A: &str = "fn a() {}\n";
const B: &str = "fn b() {}\n";
/// Bob's unpublished draft, which a split moves to the second root.
const BOB: &str = "fn b() { 2; }\n";
/// Alice's edit that the synchronization case publishes into Bob's split workspace.
const ALICE: &str = "fn a() { 1; }\n";
/// The boundary operation's own edit after restart.
const NUDGE_B: &str = "fn b() { 3; }\n";
const NUDGE_A: &str = "fn a() { 9; }\n";
/// The only regrouping proposal in every case.
const PROPOSAL: &str = "regroup:1";

/// What a root holds: path and bytes, never executable.
type Files = &'static [(&'static str, &'static str)];
/// Before the split: Bob's draft is in the shared root and the second root is empty.
const UNSPLIT: [Files; 2] = [&[("a.rs", A), ("b.rs", BOB)], &[]];
/// The split applied, then Bob's boundary edit landed in his own workspace.
const SPLIT: [Files; 2] = [
    &[("a.rs", A), ("b.rs", B)],
    &[("a.rs", A), ("b.rs", NUDGE_B)],
];
/// Split, with Alice's edit published but not yet in Bob's workspace.
const BEHIND: [Files; 2] = [
    &[("a.rs", ALICE), ("b.rs", B)],
    &[("a.rs", A), ("b.rs", BOB)],
];
/// The publication reached Bob's workspace, then Alice's boundary edit landed in hers.
const SYNCED: [Files; 2] = [
    &[("a.rs", NUDGE_A), ("b.rs", B)],
    &[("a.rs", ALICE), ("b.rs", BOB)],
];

#[derive(Clone, Copy, Debug)]
enum Model {
    /// The fault hook returns an error, as a failing step would.
    Error,
    /// The process exits at the fault point.
    Exit,
}
use Model::{Error, Exit};

fn installed(space: usize, path: &str) -> Fault {
    Fault::Installed {
        space,
        path: path.into(),
    }
}
fn restored(space: usize, path: &str) -> Fault {
    Fault::Restored {
        space,
        path: path.into(),
    }
}
/// Every durable step of the split, in order: the new root installs first.
fn transition_points() -> Vec<Fault> {
    vec![
        Fault::Journaling,
        Fault::Journaled,
        installed(1, "a.rs"),
        installed(1, "b.rs"),
        // Between roots.
        Fault::Audited { space: 1 },
        installed(0, "b.rs"),
        // After the last audit.
        Fault::Audited { space: 0 },
        Fault::Commit,
        // Adoption is in memory only, so a process exit here is also "before adoption".
        Fault::Committed,
    ]
}
fn synchronization_points() -> Vec<Fault> {
    vec![
        Fault::Journaling,
        Fault::Journaled,
        installed(1, "a.rs"),
        Fault::Audited { space: 1 },
        Fault::Commit,
        Fault::Committed,
    ]
}

fn roots(dir: &Path) -> [std::path::PathBuf; 3] {
    ["live", "second", "third"].map(|r| dir.join(r))
}
/// Opens with `hook`, or as production does without one, and configures a trusted check.
fn opened(dir: &Path, hook: Option<FaultHook>) -> Result<Engine> {
    let [live, ..] = roots(dir);
    let (state, enrolled) = (dir.join("state"), ["a.rs", "b.rs"]);
    let engine = match hook {
        Some(hook) => Engine::open_with_faults(&live, &state, &enrolled, hook)?,
        None => Engine::open(&live, &state, &enrolled)?,
    };
    engine
        .configure_checks(vec![Check {
            name: "true".into(),
            program: "/usr/bin/true".into(),
            args: Vec::new(),
        }])
        .unwrap();
    Ok(engine)
}
fn open(dir: &Path) -> Result<Engine> {
    opened(dir, None)
}
fn client(engine: &Engine, agent: usize) -> Client {
    engine
        .authenticate(agent, &engine.credentials()[agent])
        .unwrap()
}
fn review(engine: &Engine, client: &Client, scope: &str) {
    engine
        .review(
            client,
            Review {
                scope: scope.into(),
                revision: engine.capture_for(client).unwrap().revision,
                decision: Decision::Keep,
                note: "keep".into(),
            },
        )
        .unwrap();
}
fn register(engine: &Engine, client: &Client, id: &str, node: &str) {
    engine
        .register(
            client,
            Scope {
                id: id.into(),
                task: id.into(),
                nodes: [node.to_string()].into(),
                depends: Default::default(),
            },
        )
        .unwrap();
}
/// `client` replaces `path` in its own workspace; the edit reaches the boundary.
fn nudge(engine: &Engine, client: &Client, path: &str, bytes: &str) {
    let id = format!("{path}:{bytes}");
    let current = engine.capture_for(client).unwrap();
    let outcome = engine
        .apply(client, edit(&id, &current, path, bytes))
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
}

/// Bob drafted `b.rs` and proposed a split into the empty second root; Alice agreed. Bob's
/// agreement is the step that runs the transition. `third` adds an unaffected empty root.
fn proposed(third: bool) -> TempDir {
    let (dir, engine, alice, bob) = support::fixture(PAIR);
    let [_, second, third_root] = roots(dir.path());
    fs::create_dir(&second).unwrap();
    engine.add_workspace(&second).unwrap();
    if third {
        fs::create_dir(&third_root).unwrap();
        engine.add_workspace(&third_root).unwrap();
    }
    register(&engine, &alice, "alice", "b.rs");
    register(&engine, &bob, "bob", "a.rs");
    let base = engine.capture().unwrap();
    engine.apply(&bob, edit("b", &base, "b.rs", BOB)).unwrap();
    review(&engine, &alice, "alice");
    let split = Propose {
        action: Action::Split,
        text: "split".into(),
        failing: false,
    };
    let proposal = engine.propose(&bob, split).unwrap();
    assert_eq!(proposal.id, PROPOSAL);
    engine
        .respond(&alice, &proposal.id, &proposal.context, Response::Agree)
        .unwrap();
    dir
}
/// The split applied; Alice edited `a.rs` and offered it. Validation publishes it and then
/// synchronizes Bob's workspace.
fn offered() -> TempDir {
    let dir = proposed(false);
    let engine = open(dir.path()).unwrap();
    let (alice, bob) = (client(&engine, 0), client(&engine, 1));
    let proposal = engine.proposal(PROPOSAL).unwrap().unwrap();
    engine
        .respond(&bob, PROPOSAL, &proposal.context, Response::Agree)
        .unwrap();
    review(&engine, &alice, "alice");
    nudge(&engine, &alice, "a.rs", ALICE);
    let revision = engine.capture_for(&alice).unwrap().revision;
    engine
        .offer(
            &alice,
            Offer {
                id: "alice".into(),
                task: "alice".into(),
                revision,
                scope: Vec::new(),
                text: "alice".into(),
                supersedes: None,
                reviews: Vec::new(),
            },
        )
        .unwrap();
    drop(engine);
    dir
}

/// The operation a stopped engine was running: Bob's agreement, a validation, or a restart.
fn run(dir: &Path, step: &str, hook: FaultHook) {
    let engine = match opened(dir, Some(hook)) {
        Ok(engine) => engine,
        Err(error) if step == "open" => return assert_eq!(error.to_string(), "fault"),
        Err(error) => panic!("{error}"),
    };
    match step {
        "split" => {
            let context = engine.proposal(PROPOSAL).unwrap().unwrap().context;
            engine
                .respond(&client(&engine, 1), PROPOSAL, &context, Response::Agree)
                .unwrap();
        }
        "sync" => {
            engine.validate().unwrap().unwrap();
        }
        "open" => {}
        _ => panic!("unknown step {step}"),
    }
}
/// Runs `step` and stops the engine the first time it reaches `fault`.
fn stop(dir: &Path, step: &str, fault: &Fault, model: Model) {
    match model {
        Error => {
            let (target, fired) = (fault.clone(), Arc::new(AtomicBool::new(false)));
            let hook_fired = fired.clone();
            run(
                dir,
                step,
                Box::new(move |_, reached| {
                    if reached == target && !hook_fired.swap(true, Ordering::SeqCst) {
                        return Err("fault".into());
                    }
                    Ok(())
                }),
            );
            assert!(
                fired.load(Ordering::SeqCst),
                "{step} never reached {fault:?}"
            );
        }
        Exit => {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "crash_worker"])
                .env("FALINKS_CRASH_ROOT", dir)
                .env("FALINKS_CRASH_STEP", step)
                .env("FALINKS_CRASH_FAULT", format!("{fault:?}"))
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(73), "{step} at {fault:?}");
        }
    }
}
#[test]
#[ignore = "subprocess crash worker; invoked by the crash matrix"]
fn crash_worker() {
    let dir = std::path::PathBuf::from(std::env::var("FALINKS_CRASH_ROOT").unwrap());
    let step = std::env::var("FALINKS_CRASH_STEP").unwrap();
    let fault = std::env::var("FALINKS_CRASH_FAULT").unwrap();
    run(
        &dir,
        &step,
        Box::new(move |_, reached| {
            if format!("{reached:?}") == fault {
                std::process::exit(73);
            }
            Ok(())
        }),
    );
    panic!("crash barrier missed");
}

// The oracle: roots and ledger read directly, judged against literals.

/// A root's files: path to bytes and permission bits.
type Tree = BTreeMap<String, (Vec<u8>, u32)>;
/// Every file under `root` with its bytes and permission bits, except the writer lease.
fn walk(root: &Path) -> Tree {
    fn visit(root: &Path, dir: &Path, found: &mut Tree) {
        for entry in fs::read_dir(dir).into_iter().flatten() {
            let path = entry.unwrap().path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            if relative == ".falinks-writer.lock" {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                visit(root, &path, found);
            } else {
                let mode = metadata.permissions().mode() & 0o7777;
                found.insert(relative, (fs::read(&path).unwrap(), mode));
            }
        }
    }
    let mut found = BTreeMap::new();
    visit(root, root, &mut found);
    found
}
#[track_caller]
fn holds(root: &Path, files: Files, case: &str) {
    let found = walk(root);
    if let Some(staging) = found.keys().find(|p| p.contains(".falinks-install-")) {
        panic!("{case}: staging remnant {staging} in {}", root.display());
    }
    let expected: BTreeMap<_, _> = files
        .iter()
        .map(|(p, b)| (p.to_string(), (b.as_bytes().to_vec(), 0o644)))
        .collect();
    assert_eq!(found, expected, "{case}: {}", root.display());
}
fn count(dir: &Path, sql: &str) -> i64 {
    ledger(dir).query_row(sql, [], |r| r.get(0)).unwrap()
}
/// Regrouping events that announce the proposal applied.
const APPLIED: &str =
    "SELECT seq FROM events WHERE json_extract(body, '$.Regrouping.id')='regroup:1'
     AND json_extract(body, '$.Regrouping.state.Applied') IS NOT NULL";
const INCORPORATED: &str =
    "SELECT seq FROM events WHERE json_extract(body, '$.Incorporated') IS NOT NULL";
/// Obligation events that the events `cause` selects raised.
fn renewals(dir: &Path, cause: &str) -> i64 {
    count(
        dir,
        &format!(
            "SELECT count(*) FROM events WHERE json_extract(body, '$.Obligation.cause') IN ({cause})"
        ),
    )
}
/// Pending obligations per agent: a required revision not yet reviewed.
fn pending(dir: &Path) -> [i64; 2] {
    [0, 1].map(|agent| {
        count(
            dir,
            &format!(
                "SELECT count(*) FROM scopes WHERE agent={agent}
                 AND json_extract(body, '$.required') > json_extract(body, '$.reviewed')"
            ),
        )
    })
}
/// No journal is pending; placement and each workspace pointer agree with the roots.
#[track_caller]
fn settled(dir: &Path, placement: [usize; 2], files: [Files; 2], case: &str) {
    assert_eq!(
        count(
            dir,
            "SELECT count(*) FROM requests WHERE json_extract(body, '$.installation_pending')"
        ),
        0,
        "{case}: pending edit record"
    );
    assert_eq!(
        count(
            dir,
            "SELECT count(*) FROM shifts WHERE json_extract(body, '$.installing')"
        ),
        0,
        "{case}: pending shift"
    );
    let db = ledger(dir);
    let stored: String = db
        .query_row("SELECT value FROM meta WHERE key='placement'", [], |r| {
            r.get(0)
        })
        .optional()
        .unwrap()
        // Never regrouped: everyone in the primary root.
        .unwrap_or_else(|| "[0,0]".into());
    assert_eq!(
        serde_json::from_str::<[usize; 2]>(&stored).unwrap(),
        placement,
        "{case}: placement"
    );
    for (space, files) in files.iter().enumerate() {
        // An unprovisioned root has no row; a provisioned, never used one has no pointer.
        let completed: Option<i64> = db
            .query_row(
                "SELECT completed FROM spaces WHERE id=?",
                [space as i64],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
            .flatten();
        let group: Vec<usize> = (0..2).filter(|a| placement[*a] == space).collect();
        let Some(completed) = completed else {
            assert!(
                files.is_empty() && group.is_empty(),
                "{case}: no pointer for {space}"
            );
            continue;
        };
        let body: String = db
            .query_row("SELECT body FROM revisions WHERE id=?", [completed], |r| {
                r.get(0)
            })
            .unwrap();
        let capture: Capture = serde_json::from_str(&body).unwrap();
        assert_eq!(capture.space, space, "{case}: pointer {space}");
        if !group.is_empty() {
            assert_eq!(
                capture.group.into_iter().collect::<Vec<_>>(),
                group,
                "{case}: group {space}"
            );
        }
        let held: BTreeMap<String, String> = capture
            .files
            .into_iter()
            .filter_map(|(p, f)| Some((p, String::from_utf8(f.file?.bytes).unwrap())))
            .collect();
        let expected: BTreeMap<String, String> = files
            .iter()
            .map(|(p, b)| (p.to_string(), b.to_string()))
            .collect();
        assert_eq!(held, expected, "{case}: pointer {space}");
    }
}
/// The published files, read from the ledger.
#[track_caller]
fn published(dir: &Path, files: Files, case: &str) {
    let db = ledger(dir);
    let body: String = db
        .query_row(
            "SELECT body FROM revisions WHERE id=(SELECT value FROM meta WHERE key='published')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let capture: Capture = serde_json::from_str(&body).unwrap();
    let held: Vec<(String, String)> = capture
        .files
        .into_iter()
        .map(|(p, f)| (p, String::from_utf8(f.file.unwrap().bytes).unwrap()))
        .collect();
    let expected: Vec<(String, String)> = files
        .iter()
        .map(|(p, b)| (p.to_string(), b.to_string()))
        .collect();
    assert_eq!(held, expected, "{case}: published");
}
/// Every root's files and every ledger row.
fn everything(dir: &Path) -> (Vec<Tree>, Vec<String>) {
    let db = ledger(dir);
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let mut rows = Vec::new();
    for table in tables {
        let mut stmt = db.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let columns = stmt.column_count();
        let mut query = stmt.query([]).unwrap();
        while let Some(row) = query.next().unwrap() {
            let values: Vec<rusqlite::types::Value> =
                (0..columns).map(|i| row.get(i).unwrap()).collect();
            rows.push(format!("{table}: {values:?}"));
        }
    }
    (roots(dir).iter().map(|r| walk(r)).collect(), rows)
}
/// A restart over a recovered engine changes nothing.
#[track_caller]
fn stable(dir: &Path, case: &str) {
    let before = everything(dir);
    drop(open(dir).unwrap());
    assert_eq!(everything(dir), before, "{case}: second recovery");
}
#[track_caller]
fn clear(dir: &Path, case: &str) {
    assert_eq!(halt(dir), None, "{case}");
    assert_eq!(incidents(dir), 0, "{case}");
}

/// A split stopped at `fault`, then a restart and Bob's edit at the boundary: applied once.
fn transition(fault: &Fault, model: Model) {
    let case = format!("transition {fault:?} {model:?}");
    let dir = proposed(false);
    let d = dir.path();
    let [live, second, _] = roots(d);
    stop(d, "split", fault, model);
    let engine = open(d).unwrap();
    clear(d, &case);
    if *fault == Fault::Committed {
        holds(&live, SPLIT[0], &case);
        holds(&second, &[("a.rs", A), ("b.rs", BOB)], &case);
        settled(d, [0, 1], [SPLIT[0], &[("a.rs", A), ("b.rs", BOB)]], &case);
    } else {
        holds(&live, UNSPLIT[0], &case);
        holds(&second, UNSPLIT[1], &case);
        settled(d, [0, 0], UNSPLIT, &case);
    }
    nudge(&engine, &client(&engine, 1), "b.rs", NUDGE_B);
    assert!(
        matches!(
            engine.proposal(PROPOSAL).unwrap().unwrap().state,
            ProposalState::Applied { .. }
        ),
        "{case}"
    );
    drop(engine);
    holds(&live, SPLIT[0], &case);
    holds(&second, SPLIT[1], &case);
    settled(d, [0, 1], SPLIT, &case);
    assert_eq!(
        count(d, &format!("SELECT count(*) FROM ({APPLIED})")),
        1,
        "{case}"
    );
    // Alice's view of `b.rs` changed once; Bob's did not.
    assert_eq!(renewals(d, APPLIED), 1, "{case}");
    assert_eq!(pending(d), [1, 0], "{case}");
    published(d, &[("a.rs", A), ("b.rs", B)], &case);
    clear(d, &case);
    stable(d, &case);
}
#[test]
fn transition_stopped_in_process_is_retried_once_at_the_next_boundary() {
    for fault in transition_points() {
        transition(&fault, Error);
    }
}
#[test]
fn transition_stopped_by_process_exit_is_retried_once_at_the_next_boundary() {
    for fault in transition_points() {
        transition(&fault, Exit);
    }
}

/// A split stopped before its commit, then a context change before the next boundary: stale.
fn stale(model: Model) {
    let case = format!("stale {model:?}");
    let dir = proposed(false);
    let d = dir.path();
    let [live, second, _] = roots(d);
    stop(d, "split", &installed(1, "a.rs"), model);
    let engine = open(d).unwrap();
    register(&engine, &client(&engine, 0), "later", "a.rs");
    assert!(
        matches!(
            engine.proposal(PROPOSAL).unwrap().unwrap().state,
            ProposalState::Stale { .. }
        ),
        "{case}"
    );
    drop(engine);
    holds(&live, UNSPLIT[0], &case);
    holds(&second, UNSPLIT[1], &case);
    settled(d, [0, 0], UNSPLIT, &case);
    assert_eq!(
        count(d, &format!("SELECT count(*) FROM ({APPLIED})")),
        0,
        "{case}"
    );
    assert_eq!(pending(d), [0, 0], "{case}");
    published(d, &[("a.rs", A), ("b.rs", B)], &case);
    clear(d, &case);
    stable(d, &case);
}
#[test]
fn transition_whose_context_changed_before_the_retry_becomes_stale() {
    stale(Error);
    stale(Exit);
}

/// A synchronization stopped at `fault`, then a restart and Alice's edit at the boundary:
/// the publication reaches Bob's workspace once.
fn synchronization(fault: &Fault, model: Model) {
    let case = format!("synchronization {fault:?} {model:?}");
    let dir = offered();
    let d = dir.path();
    let [live, second, _] = roots(d);
    stop(d, "sync", fault, model);
    let engine = open(d).unwrap();
    clear(d, &case);
    holds(&live, BEHIND[0], &case);
    if *fault == Fault::Committed {
        holds(&second, SYNCED[1], &case);
    } else {
        holds(&second, BEHIND[1], &case);
        settled(d, [0, 1], BEHIND, &case);
    }
    nudge(&engine, &client(&engine, 0), "a.rs", NUDGE_A);
    drop(engine);
    holds(&live, SYNCED[0], &case);
    holds(&second, SYNCED[1], &case);
    settled(d, [0, 1], SYNCED, &case);
    assert_eq!(
        count(d, &format!("SELECT count(*) FROM ({APPLIED})")),
        1,
        "{case}"
    );
    assert_eq!(
        count(d, &format!("SELECT count(*) FROM ({INCORPORATED})")),
        1,
        "{case}"
    );
    // Bob's view of `a.rs` changed once.
    assert_eq!(renewals(d, INCORPORATED), 1, "{case}");
    assert_eq!(pending(d), [0, 1], "{case}");
    published(d, &[("a.rs", ALICE), ("b.rs", B)], &case);
    clear(d, &case);
    stable(d, &case);
}
#[test]
fn synchronization_stopped_in_process_is_retried_once_at_the_next_boundary() {
    for fault in synchronization_points() {
        synchronization(&fault, Error);
    }
}
#[test]
fn synchronization_stopped_by_process_exit_is_retried_once_at_the_next_boundary() {
    for fault in synchronization_points() {
        synchronization(&fault, Exit);
    }
}

/// Replaces `a.rs` and `b.rs`, installing `a.rs` first.
fn two_files(base: &Capture) -> Request {
    let mut request = edit("two", base, "a.rs", "new a");
    request
        .expected
        .insert("b.rs".into(), base.files["b.rs"].version);
    request.output.insert(
        "b.rs".into(),
        Output::Whole(File {
            bytes: b"new b".to_vec(),
            executable: false,
        }),
    );
    request
}
/// An edit installed in full and stopped before its commit; the first restart stops after
/// restoring `a.rs`, the second finishes. The edit needs an explicit retry.
fn edit_recovery(model: Model) {
    let case = format!("edit recovery {model:?}");
    let (dir, engine, alice, _) = support::fixture(PAIR);
    let d = dir.path();
    let [live, ..] = roots(d);
    let base = engine.capture().unwrap();
    let request = two_files(&base);
    let outcome = engine
        .apply_observed(&alice, request.clone(), |phase| {
            if phase == Phase::BeforeCommit {
                return Err("interrupt".into());
            }
            Ok(())
        })
        .unwrap();
    assert!(
        matches!(outcome, Outcome::Interrupted { .. }),
        "{outcome:?}"
    );
    drop(engine);
    stop(d, "open", &restored(0, "a.rs"), model);
    holds(&live, &[("a.rs", A), ("b.rs", "new b")], &case);

    let engine = open(d).unwrap();
    clear(d, &case);
    holds(&live, &[("a.rs", A), ("b.rs", B)], &case);
    settled(d, [0, 0], [&[("a.rs", A), ("b.rs", B)], &[]], &case);
    let alice = client(&engine, 0);
    let record = engine.request(&alice, "two").unwrap().unwrap();
    assert!(
        matches!(record.outcome, Some(Outcome::Interrupted { .. })),
        "{case}"
    );
    assert!(
        matches!(
            engine.apply(&alice, request.clone()).unwrap(),
            Outcome::Interrupted { .. }
        ),
        "{case}: no implicit replay"
    );
    assert!(
        matches!(
            engine.retry(&alice, request).unwrap(),
            Outcome::Applied { .. }
        ),
        "{case}"
    );
    drop(engine);
    holds(&live, &[("a.rs", "new a"), ("b.rs", "new b")], &case);
    settled(
        d,
        [0, 0],
        [&[("a.rs", "new a"), ("b.rs", "new b")], &[]],
        &case,
    );
    // No scope is registered, so the retried edit raises no obligation.
    assert_eq!(count(d, "SELECT count(*) FROM scopes"), 0, "{case}");
    published(d, &[("a.rs", A), ("b.rs", B)], &case);
    clear(d, &case);
    stable(d, &case);
}
#[test]
fn edit_recovery_stopped_partway_finishes_at_the_next_restart() {
    edit_recovery(Error);
    edit_recovery(Exit);
}

/// A split installed in full and stopped in its commit; the first restart stops at `fault`,
/// the second finishes, and the next boundary applies the split once.
fn shift_recovery(fault: &Fault, model: Model) {
    let case = format!("shift recovery {fault:?} {model:?}");
    let dir = proposed(false);
    let d = dir.path();
    let [live, second, _] = roots(d);
    stop(d, "split", &Fault::Commit, Error);
    stop(d, "open", fault, model);
    if *fault == Fault::Recovered {
        holds(&live, UNSPLIT[0], &case);
        holds(&second, UNSPLIT[1], &case);
        assert!(matches!(halt(d), Some(Halt::Interruption { .. })), "{case}");
    } else {
        // `b.rs` in the new root is not restored yet.
        holds(&second, &[("b.rs", BOB)], &case);
    }

    let engine = open(d).unwrap();
    clear(d, &case);
    holds(&live, UNSPLIT[0], &case);
    holds(&second, UNSPLIT[1], &case);
    settled(d, [0, 0], UNSPLIT, &case);
    nudge(&engine, &client(&engine, 1), "b.rs", NUDGE_B);
    drop(engine);
    holds(&live, SPLIT[0], &case);
    holds(&second, SPLIT[1], &case);
    settled(d, [0, 1], SPLIT, &case);
    assert_eq!(
        count(d, &format!("SELECT count(*) FROM ({APPLIED})")),
        1,
        "{case}"
    );
    assert_eq!(renewals(d, APPLIED), 1, "{case}");
    assert_eq!(pending(d), [1, 0], "{case}");
    published(d, &[("a.rs", A), ("b.rs", B)], &case);
    clear(d, &case);
    stable(d, &case);
}
#[test]
fn shift_recovery_stopped_partway_finishes_at_the_next_restart() {
    for model in [Error, Exit] {
        shift_recovery(&restored(1, "a.rs"), model);
    }
}
#[test]
fn recovery_stopped_before_clearing_the_halt_finishes_at_the_next_restart() {
    for model in [Error, Exit] {
        shift_recovery(&Fault::Recovered, model);
    }
}

/// A split installed in full and stopped in its commit, then `tamper` changes a root:
/// restart refuses, restores nothing and leaves the shift pending.
fn tampering(name: &str, tamper: fn(&Path), refusal: &str, model: Model) {
    let case = format!("tampering {name} {model:?}");
    let dir = proposed(true);
    let d = dir.path();
    stop(d, "split", &Fault::Commit, model);
    tamper(d);
    let before = everything(d);

    fails_with(open(d), refusal);
    let after = everything(d);
    assert_eq!(after.0, before.0, "{case}: nothing restored");
    assert_eq!(
        count(
            d,
            "SELECT count(*) FROM shifts WHERE json_extract(body, '$.installing')"
        ),
        1,
        "{case}: shift still pending"
    );
    assert_eq!(incidents(d), 1, "{case}");
    assert!(matches!(halt(d), Some(Halt::Incident { .. })), "{case}");
    // The evidence is still there: a second restart refuses again and records nothing new.
    fails_with(open(d), refusal);
    assert_eq!(everything(d), after, "{case}: second refusal");
}
/// A name, the change, and the refusal it must cause.
type Tampering = (&'static str, fn(&Path), &'static str);
#[test]
fn tampered_roots_refuse_restart_with_nothing_restored() {
    let cases: [Tampering; 6] = [
        (
            "changed bytes",
            |d| fs::write(d.join("second/b.rs"), "tampered").unwrap(),
            "ambiguous interrupted source: b.rs",
        ),
        (
            "extra file",
            |d| fs::write(d.join("second/x.rs"), "fn x() {}\n").unwrap(),
            "unenrolled source during recovery: x.rs",
        ),
        (
            "changed mode",
            |d| {
                fs::set_permissions(d.join("live/a.rs"), fs::Permissions::from_mode(0o755)).unwrap()
            },
            "ambiguous interrupted source: a.rs",
        ),
        (
            "deleted path",
            |d| fs::remove_file(d.join("live/a.rs")).unwrap(),
            "ambiguous interrupted source: a.rs",
        ),
        (
            "staging remnant",
            |d| fs::write(d.join("second/.falinks-install-0"), "new b").unwrap(),
            "reserved or aliased path",
        ),
        (
            "unaffected root",
            |d| fs::write(d.join("third/x.rs"), "fn x() {}\n").unwrap(),
            "unexplained source write: x.rs",
        ),
    ];
    for model in [Error, Exit] {
        for (name, tamper, refusal) in cases {
            tampering(name, tamper, refusal, model);
        }
    }
}
