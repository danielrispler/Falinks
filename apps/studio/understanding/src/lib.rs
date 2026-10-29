//! The understanding layer's I/O, which the core leaves to the shell: the card writer (#328,
//! module 4), the worktree fingerprint (module 5) and the card store (module 6).

use serde::{Deserialize, Serialize};
use studio_core::CardRequest;

mod fingerprint;
mod store;
pub use fingerprint::Fingerprints;
pub use store::{CardStore, StoredCard};

/// A card on one step. It opens with the question; the answer shows when Daniel taps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub question: String,
    pub answer: String,
}

/// Writes the card on a finished card step.
pub trait CardWriter {
    /// The card, or `None` when the writer's output does not parse.
    fn write(&self, request: &CardRequest) -> Option<Card>;
}

/// Writes a fixed card that shows the step's change, as v0's fake did, until the real writer (#342).
pub struct FakeWriter;

impl CardWriter for FakeWriter {
    fn write(&self, request: &CardRequest) -> Option<Card> {
        let what = request.step.detail.as_deref().unwrap_or(&request.step.tool);
        Some(Card {
            question: format!("What did the agent change in {what}, and why?"),
            answer: format!("A fake card. The step's change:\n{}", request.change),
        })
    }
}
