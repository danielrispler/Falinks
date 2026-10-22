//! Replacement hunks in an edit's output (#198).
mod support;

use falinks::*;
use std::collections::BTreeMap;
use support::{PAIR, edit, fails_with, fixture, ledger, reopen};

const LINES: &[(&str, &str)] = &[("a.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")];

/// One output of exact replacements for `path` at the version `capture` saw.
fn hunks(id: &str, capture: &Capture, path: &str, pairs: &[(&str, &str)]) -> Request {
    let mut request = edit(id, capture, path, "");
    request.output.insert(
        path.into(),
        Output::Replace {
            replace: pairs
                .iter()
                .map(|(old, new)| Hunk {
                    old: old.as_bytes().to_vec(),
                    new: new.as_bytes().to_vec(),
                })
                .collect(),
            executable: None,
        },
    );
    request
}
fn bytes(engine: &Engine, path: &str) -> Vec<u8> {
    engine.capture().unwrap().files[path]
        .file
        .clone()
        .unwrap()
        .bytes
}
#[track_caller]
fn rejected(outcome: Outcome, reason: &str) {
    match outcome {
        Outcome::Rejected { reason: got } => assert!(got.contains(reason), "{got}"),
        other => panic!("expected rejection {reason:?}, got {other:?}"),
    }
}

#[test]
fn hunks_install_the_resolved_bytes_and_keep_the_mode() {
    let (_dir, engine, alice, _) = fixture(LINES);
    let base = engine.capture().unwrap();
    let request = hunks(
        "h",
        &base,
        "a.rs",
        &[
            ("fn c() {}", "fn c() { 3; }"),
            ("fn a() {}", "fn a() { 1; }"),
        ],
    );
    let outcome = engine.apply(&alice, request.clone()).unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(
        bytes(&engine, "a.rs"),
        b"fn a() { 1; }\nfn b() {}\nfn c() { 3; }\n"
    );
    assert!(
        !engine.capture().unwrap().files["a.rs"]
            .file
            .as_ref()
            .unwrap()
            .executable
    );
    let record = engine.request(&alice, "h").unwrap().unwrap();
    assert_eq!(record.request, request, "stored as sent");
    // The proposal is the resolved file.
    assert_eq!(
        record.proposed.unwrap().files["a.rs"]
            .file
            .as_ref()
            .unwrap()
            .bytes,
        b"fn a() { 1; }\nfn b() {}\nfn c() { 3; }\n"
    );
    // Replaying the same ID returns the recorded outcome; changed hunks are refused.
    assert_eq!(engine.apply(&alice, request.clone()).unwrap(), outcome);
    let changed = hunks("h", &base, "a.rs", &[("fn b() {}", "fn b() { 2; }")]);
    fails_with(
        engine.apply(&alice, changed),
        "request ID reused with changed contents",
    );
}

#[test]
fn hunks_that_do_not_match_exactly_once_are_rejected() {
    let (_dir, engine, alice, _) = fixture(LINES);
    let base = engine.capture().unwrap();
    type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);
    let cases: &[Case] = &[
        (
            "none",
            &[("fn a() {}", "x"), ("fn z", "y")],
            "hunk 2: old text matches 0 places",
        ),
        (
            "two",
            &[("{}\nfn", "x")],
            "hunk 1: old text matches 2 places",
        ),
        ("empty", &[("", "x")], "hunk 1: old text is empty"),
        (
            "overlap",
            &[("fn a() {}\nfn b", "x"), ("fn b() {}", "y")],
            "hunk 1 overlaps hunk 2",
        ),
        // Hunks apply together to the expected bytes, not one after another.
        (
            "chained",
            &[("fn a() {}", "fn q() {}"), ("fn q", "fn r")],
            "hunk 2: old text matches 0 places",
        ),
    ];
    for (id, pairs, reason) in cases {
        rejected(
            engine
                .apply(&alice, hunks(id, &base, "a.rs", pairs))
                .unwrap(),
            reason,
        );
    }
    assert_eq!(engine.capture().unwrap(), base);
}

#[test]
fn hunks_on_an_absent_path_are_rejected() {
    let (_dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let mut gone = edit("gone", &base, "b.rs", "");
    gone.output.insert("b.rs".into(), Output::Delete);
    assert!(matches!(
        engine.apply(&alice, gone).unwrap(),
        Outcome::Applied { .. }
    ));
    let absent = engine.capture().unwrap();
    rejected(
        engine
            .apply(&alice, hunks("h", &absent, "b.rs", &[("fn b", "fn c")]))
            .unwrap(),
        "b.rs: hunks need a file at the expected version",
    );
}

#[test]
fn a_stale_expected_version_is_stale_before_any_hunk_is_checked() {
    let (_dir, engine, alice, bob) = fixture(PAIR);
    let base = engine.capture().unwrap();
    engine
        .apply(&bob, edit("b", &base, "a.rs", "fn a() { 1; }\n"))
        .unwrap();
    let outcome = engine
        .apply(&alice, hunks("h", &base, "a.rs", &[("missing", "x")]))
        .unwrap();
    assert!(matches!(outcome, Outcome::Stale { .. }), "{outcome:?}");
}

// A ledger written before hunks existed holds `null` and whole files, which read unchanged.
#[test]
fn a_request_recorded_before_hunks_opens_unchanged() {
    let (dir, engine, alice, _) = fixture(PAIR);
    let base = engine.capture().unwrap();
    let mut request = edit("old", &base, "a.rs", "fn a() { 1; }\n");
    request
        .expected
        .insert("b.rs".into(), base.files["b.rs"].version);
    request.output.insert("b.rs".into(), Output::Delete);
    engine.apply(&alice, request.clone()).unwrap();
    drop(engine);
    let body: String = ledger(dir.path())
        .query_row(
            "SELECT body FROM requests WHERE request_id='old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["request"]["output"]["b.rs"], serde_json::Value::Null);
    assert_eq!(
        json["request"]["output"]["a.rs"]["executable"],
        serde_json::Value::Bool(false)
    );
    let (engine, alice, _) = reopen(dir.path(), PAIR);
    assert_eq!(
        engine
            .request(&alice, "old")
            .unwrap()
            .unwrap()
            .request
            .output,
        BTreeMap::from([
            (
                "a.rs".into(),
                Output::Whole(File {
                    bytes: b"fn a() { 1; }\n".to_vec(),
                    executable: false,
                })
            ),
            ("b.rs".into(), Output::Delete),
        ])
    );
}

#[test]
fn a_retry_resolves_the_hunks_again_against_the_expected_bytes() {
    let (dir, engine, alice, _) = fixture(LINES);
    let base = engine.capture().unwrap();
    let mut request = hunks("r", &base, "a.rs", &[("fn b() {}", "fn b() { 2; }")]);
    if let Some(Output::Replace { executable, .. }) = request.output.get_mut("a.rs") {
        *executable = Some(true);
    }
    let outcome = engine
        .apply_observed(&alice, request.clone(), |phase| {
            if matches!(phase, Phase::Installed(_)) {
                return Err("controlled interruption".into());
            }
            Ok(())
        })
        .unwrap();
    assert!(
        matches!(outcome, Outcome::Interrupted { .. }),
        "{outcome:?}"
    );
    drop(engine);
    let (engine, alice, _) = reopen(dir.path(), LINES);
    let outcome = engine.retry(&alice, request).unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let file = engine.capture().unwrap().files["a.rs"]
        .file
        .clone()
        .unwrap();
    assert_eq!(file.bytes, b"fn a() {}\nfn b() { 2; }\nfn c() {}\n");
    assert!(file.executable);
}
