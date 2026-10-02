//! THROWAWAY — issue #15. In-memory source, simulated clients, no publication.
use std::collections::BTreeMap;
use std::sync::{Barrier, Mutex};
use std::thread;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    revision: u64,
    text: String,
}

#[derive(Debug)]
struct Edit {
    path: &'static str,
    expected_revision: u64,
    replacement: String,
}

#[derive(Debug)]
struct Conflict {
    proposed: Edit,
    current: Source,
}

// ponytail: one global lock serializes reads/writes; per-file locks if throughput matters.
struct Engine(Mutex<BTreeMap<&'static str, Source>>);

impl Engine {
    fn new(fixtures: &[(&'static str, &str)]) -> Self {
        Self(Mutex::new(
            fixtures
                .iter()
                .map(|&(path, text)| (path, Source { revision: 0, text: text.into() }))
                .collect(),
        ))
    }

    fn read(&self, path: &str) -> Source {
        let files = self.0.lock().unwrap();
        let source = files.get(path).unwrap().clone();
        println!("READ {path}: {source:?}");
        source
    }

    fn apply(&self, edit: Edit) -> Result<Source, Conflict> {
        // Check and replacement share this lock. No unlocked check/write gap.
        let mut files = self.0.lock().unwrap();
        let current = files.get_mut(edit.path).unwrap();
        if current.revision != edit.expected_revision {
            let conflict = Conflict { proposed: edit, current: current.clone() };
            println!("CONFLICT: {conflict:?}");
            return Err(conflict);
        }
        *current = Source { revision: current.revision + 1, text: edit.replacement };
        println!("ACCEPT {}: {current:?}", edit.path);
        Ok(current.clone())
    }
}

// Experimental edit representation: whole-file replacement plus expected revision.
fn replacement(path: &'static str, base: &Source, old: &str, new: &str) -> Edit {
    assert_eq!(base.text.matches(old).count(), 1, "fixture needs one edit target");
    Edit {
        path,
        expected_revision: base.revision,
        replacement: base.text.replace(old, new),
    }
}

fn race(
    engine: &Engine,
    plans: [(&'static str, &str, &str); 2],
) -> [Result<Source, Conflict>; 2] {
    let ready = Barrier::new(2);
    thread::scope(|scope| {
        let clients = plans.map(|(path, old, new)| {
            let ready = &ready;
            scope.spawn(move || {
                let base = engine.read(path);
                let edit = replacement(path, &base, old, new);
                // Both clients finish reading before either submits a mutation.
                // Which client acquires the mutation lock first is immaterial.
                ready.wait();
                engine.apply(edit)
            })
        });
        clients.map(|client| client.join().unwrap())
    })
}

fn one_winner(results: [Result<Source, Conflict>; 2]) -> (Source, Conflict, usize) {
    match results {
        [Ok(accepted), Err(conflict)] => (accepted, conflict, 1),
        [Err(conflict), Ok(accepted)] => (accepted, conflict, 0),
        other => panic!("expected one accepted edit and one preserved conflict: {other:?}"),
    }
}

fn main() {
    println!("THROWAWAY #15: can atomic revision check/write prevent lost updates?\n");

    println!("1. Different files: both edits survive.");
    let engine = Engine::new(&[
        ("alpha.rs", "fn alpha() -> u8 { 0 }\n"),
        ("beta.rs", "fn beta() -> u8 { 0 }\n"),
    ]);
    let results = race(&engine, [("alpha.rs", "0", "1"), ("beta.rs", "0", "2")]);
    for (result, path, text) in [
        (&results[0], "alpha.rs", "fn alpha() -> u8 { 1 }\n"),
        (&results[1], "beta.rs", "fn beta() -> u8 { 2 }\n"),
    ] {
        let accepted = result.as_ref().expect("different files should both succeed");
        assert_eq!(accepted.revision, 1);
        assert_eq!(accepted.text, text);
        assert_eq!(engine.read(path), *accepted);
    }

    println!("\n2. Same target: stale edit conflicts and retains both versions.");
    let base = "fn target() -> u8 { 0 }\n";
    let engine = Engine::new(&[("target.rs", base)]);
    let (accepted, conflict, loser) = one_winner(race(
        &engine,
        [("target.rs", "0", "1"), ("target.rs", "0", "2")],
    ));
    let proposed_values = ["1", "2"];
    assert_eq!(accepted.revision, 1);
    assert_eq!(accepted.text, base.replace("0", proposed_values[1 - loser]));
    assert_eq!(conflict.proposed.path, "target.rs");
    assert_eq!(conflict.proposed.expected_revision, 0);
    assert_eq!(conflict.proposed.replacement, base.replace("0", proposed_values[loser]));
    assert_eq!(conflict.current, accepted);
    assert_eq!(engine.read("target.rs"), accepted);

    println!("\n3. Disjoint same-file edits: one conflict, then one explicit reread/retry.");
    let base = "fn alpha() -> u8 { 0 }\nfn beta() -> u8 { 0 }\n";
    let engine = Engine::new(&[("shared.rs", base)]);
    let plans = [
        ("shared.rs", "alpha() -> u8 { 0 }", "alpha() -> u8 { 1 }"),
        ("shared.rs", "beta() -> u8 { 0 }", "beta() -> u8 { 2 }"),
    ];
    let (accepted, conflict, loser) = one_winner(race(&engine, plans));
    let (path, old, new) = plans[loser];
    assert_eq!(accepted.revision, 1);
    assert_eq!(conflict.proposed.path, path);
    assert_eq!(conflict.proposed.expected_revision, 0);
    assert_eq!(conflict.proposed.replacement, base.replace(old, new));
    assert_eq!(conflict.current, accepted);
    let fresh = engine.read(path);
    assert_eq!(fresh, accepted, "reread must observe the peer's completed edit");
    // Rebuild from fresh bytes; merely updating the stale request's revision loses the peer edit.
    let final_source = engine.apply(replacement(path, &fresh, old, new)).unwrap();
    assert_eq!(final_source.revision, 2);
    assert_eq!(final_source.text, "fn alpha() -> u8 { 1 }\nfn beta() -> u8 { 2 }\n");
    assert_eq!(engine.read(path), final_source);
    println!("RETRIES: 1; both disjoint edits retained.");

    println!("\n4. Incomplete source: edits admitted without compiling the fixture.");
    let engine = Engine::new(&[("unfinished.rs", "fn draft() { let answer = ; }\n")]);
    let fresh = engine.read("unfinished.rs");
    let accepted = engine
        .apply(replacement("unfinished.rs", &fresh, "draft", "unfinished"))
        .unwrap();
    assert_eq!(accepted.revision, 1);
    assert_eq!(accepted.text, "fn unfinished() { let answer = ; }\n");
    assert_eq!(engine.read("unfinished.rs"), accepted);

    println!("\nPASS: all five ticket observations checked (fresh reads included above).");
    println!("Limit: one global lock; whole-file revisions; same-file disjoint edit needs 1 retry.");
}
