//! The card store (#328, module 6): cards and their reveal state survive a restart.

use studio_core::{CardRequest, Step};
use studio_understanding::{CardStore, CardWriter, FakeWriter};

fn edit(tool_use_id: &str, path: &str) -> CardRequest {
    CardRequest {
        session_id: "5b9e3d71-0c2a-4f8e-b6d4-8a1c3e5f7092".into(),
        tool_use_id: tool_use_id.into(),
        step: Step {
            tool: "Edit".into(),
            detail: Some(path.into()),
        },
        change: "@@ -1,1 +1,1 @@\n-old\n+new\n".into(),
    }
}

#[test]
fn a_reopened_store_keeps_the_cards_and_which_ones_daniel_revealed() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("cards.json");
    let mut store = CardStore::open(path.clone()).unwrap();
    for request in [
        edit("toolu_01A", "src/lib.rs"),
        edit("toolu_01B", "README.md"),
    ] {
        let card = FakeWriter.write(&request).unwrap();
        store.add(request, card).unwrap();
    }
    store.reveal("toolu_01A", 1_761_744_000_000).unwrap();
    // Opening it again later keeps the first reveal.
    store.reveal("toolu_01A", 1_761_744_009_000).unwrap();

    let reopened = CardStore::open(path).unwrap();

    let revealed: Vec<(&str, Option<u64>)> = reopened
        .cards()
        .iter()
        .map(|kept| (kept.request.tool_use_id.as_str(), kept.revealed_at))
        .collect();
    assert_eq!(
        revealed,
        [("toolu_01A", Some(1_761_744_000_000)), ("toolu_01B", None)]
    );
    assert_eq!(reopened.cards(), store.cards());
}

#[test]
fn a_missing_store_starts_empty() {
    let data = tempfile::tempdir().unwrap();

    let store = CardStore::open(data.path().join("cards.json")).unwrap();

    assert_eq!(store.cards(), []);
}
