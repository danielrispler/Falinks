//! The per-stage timing report for edits (#199).
mod support;

use falinks::*;
use std::time::{Duration, Instant};
use support::{PAIR, edit, fixture};

/// The stages `apply_timed` reported, after checking their durations fit inside the call.
#[track_caller]
fn stages(
    engine: &Engine,
    client: &Client,
    request: Request,
    mode: Mode,
) -> (Result<Outcome>, Vec<EditStage>) {
    let started = Instant::now();
    let (outcome, report) = engine.apply_timed(client, request, mode);
    let total = report.iter().map(|(_, took)| *took).sum::<Duration>();
    assert!(total <= started.elapsed(), "{report:?}");
    (
        outcome,
        report.into_iter().map(|(stage, _)| stage).collect(),
    )
}

#[test]
fn an_accepted_edit_reports_every_stage() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let (outcome, reached) = stages(
        &engine,
        &alice,
        edit("e", &base, "a.rs", "fn a() { 1; }\n"),
        Mode::Apply,
    );
    assert!(
        matches!(outcome, Ok(Outcome::Applied { .. })),
        "{outcome:?}"
    );
    assert_eq!(
        reached,
        [
            EditStage::LockWait,
            EditStage::Reviews,
            EditStage::Audit,
            EditStage::Freshness,
            EditStage::Hunks,
            EditStage::Proposal,
            EditStage::Gate,
            EditStage::Retention,
            EditStage::Install,
            EditStage::Commit,
            EditStage::Boundary,
        ]
    );
}

#[test]
fn a_stale_refusal_reports_stages_up_to_freshness() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    engine
        .apply(&alice, edit("first", &base, "a.rs", "fn a() { 1; }\n"))
        .unwrap();
    let (outcome, reached) = stages(
        &engine,
        &alice,
        edit("second", &base, "a.rs", "fn a() { 2; }\n"),
        Mode::Apply,
    );
    assert!(matches!(outcome, Ok(Outcome::Stale { .. })), "{outcome:?}");
    assert_eq!(
        reached,
        [
            EditStage::LockWait,
            EditStage::Reviews,
            EditStage::Audit,
            EditStage::Freshness,
            EditStage::Boundary,
        ]
    );
}

#[test]
fn a_replayed_id_reports_no_gate_stage() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let request = edit("e", &base, "a.rs", "fn a() { 1; }\n");
    let applied = engine.apply(&alice, request.clone()).unwrap();
    let (outcome, reached) = stages(&engine, &alice, request, Mode::Apply);
    assert_eq!(outcome.unwrap(), applied);
    assert_eq!(reached, [EditStage::LockWait, EditStage::Boundary]);
}
