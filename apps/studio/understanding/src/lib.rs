//! The understanding layer's I/O, which the core leaves to the shell: the card writer (#328,
//! module 4) and the card store (module 6).

use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process::Command};
use studio_core::CardRequest;

mod store;
pub use store::{CardStore, StoredCard};

/// A card on one step, in Hebrew; code stays in English, and code inside the Hebrew text is in
/// backticks. It opens with the question; the rest shows when Daniel taps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub question: String,
    pub answer: String,
    // The fields below default to empty, so cards stored before they existed still load.
    /// The files the step changed.
    #[serde(default)]
    pub paths: Vec<String>,
    /// The step's real code: the lines of its change that matter most.
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub why_it_matters: String,
    #[serde(default)]
    pub not_checked: String,
}

impl Card {
    /// The card in a writer's reply, which must hold one JSON object, perhaps in a code fence. When
    /// it does not parse, the error is one line for the log.
    pub fn from_reply(reply: &str) -> Result<Card, String> {
        let object = reply.find('{').zip(reply.rfind('}'));
        let object = object.map_or("", |(start, end)| &reply[start..=end]);
        serde_json::from_str(object).map_err(|error| {
            let head: String = reply.chars().take(300).collect();
            format!("the card writer's reply does not parse ({error}): {head:?}")
        })
    }
}

/// Writes the card on a finished card step.
pub trait CardWriter {
    /// The card, or `None` when the writer's output does not parse.
    fn write(&self, request: &CardRequest) -> Option<Card>;
}

/// Writes a fixed Hebrew card that shows the step's change, so the app can run without `claude -p`.
pub struct FakeWriter;

impl CardWriter for FakeWriter {
    fn write(&self, request: &CardRequest) -> Option<Card> {
        let what = request.step.detail.as_deref().unwrap_or(&request.step.tool);
        Some(Card {
            question: format!("למה הסוכן שינה את `{what}`?"),
            answer: "כרטיס לדוגמה של הכותב המזויף, שלא קורא את הקוד. השינוי עצמו למטה.".into(),
            paths: request.step.detail.iter().cloned().collect(),
            code: request.change.clone(),
            why_it_matters: "כאן יופיע למה הצעד חשוב, עם קוד בתוך הטקסט כמו `if a <= b {`.".into(),
            not_checked: "לא נבדק: כלום, זה כרטיס לדוגמה.".into(),
        })
    }
}

/// Writes the card with `claude -p` on Daniel's subscription, in a neutral folder that holds none of
/// his projects, so it loads no repository's instructions and has nothing to wander into.
pub struct ClaudeWriter {
    folder: PathBuf,
}

impl ClaudeWriter {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    /// The `claude -p` run for one step. Without the two variables and without `--bare`, `claude`
    /// uses the subscription login (#320, "Fact"); the studio never reads that login itself.
    pub fn command(&self, request: &CardRequest) -> Command {
        let step = &request.step;
        let detail = step.detail.as_deref().unwrap_or_default();
        let prompt = format!(
            "{PROMPT}\nStep: {} {detail}\nChange:\n{}",
            step.tool, request.change
        );
        let mut command = Command::new("claude");
        command
            .args(["-p", &prompt])
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .current_dir(&self.folder);
        command
    }
}

impl CardWriter for ClaudeWriter {
    /// Runs `claude -p`, which takes seconds, so the shell calls it off its log thread. A step with
    /// no card logs one line on stderr.
    // shortcut: no timeout, as in v0; add one if a stuck `claude -p` ever holds a card back.
    fn write(&self, request: &CardRequest) -> Option<Card> {
        let card = match self.command(request).output() {
            Ok(output) => Card::from_reply(&String::from_utf8_lossy(&output.stdout)),
            Err(error) => Err(format!("`claude -p` did not start: {error}")),
        };
        card.inspect_err(|line| eprintln!("no card on {}: {line}", request.tool_use_id))
            .ok()
    }
}

/// The card writer's instructions. Its input is only the step and its change, not the whole payload
/// (v0 note, proposal 8).
const PROMPT: &str = r#"You write one card that teaches a developer about one step a coding agent took while he waited on it. The card opens with a question he tries to answer before he taps to see the answer.

Ask why the agent took this step, and answer from the change below. Use only this input: do not use tools or read files.

Write question, answer, why_it_matters and not_checked in Hebrew, in plain words. Code, code terms, file paths and commands stay in English; inside Hebrew text, wrap each of them in backticks.

Reply with one JSON object and nothing else:
{"question": string, "answer": string, "paths": [string], "code": string, "why_it_matters": string, "not_checked": string}
- question: why the agent took this step, in one sentence.
- answer: the reason, in at most three sentences.
- paths: the files the step changed.
- code: the lines of the change that matter most, copied from it, at most 12.
- why_it_matters: what the step changes for the code, in one or two sentences.
- not_checked: one thing the step's result does not show.
"#;
