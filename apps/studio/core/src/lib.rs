//! The studio core (#328, module 3): plain Rust with no I/O. It takes hook events one at a time, in
//! event log order, and keeps the state the app shows. App start replays the whole log through
//! `replay`; live events go through `apply`, which updates state the same way and also returns what
//! the shell must do.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Studio {
    sessions: HashMap<String, Session>,
    prompted_last: Option<String>,
    /// Sessions in a wait that has not had its card step yet.
    awaiting_card: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Session {
    pub id: String,
    /// The folder the session works in, from its latest event.
    pub cwd: String,
    pub last_step: Option<Step>,
}

/// A tool call the agent made, as the "now" line names it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub tool: String,
    pub detail: Option<String>,
}

/// What the shell must act on after a live event.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Output {
    /// A card step finished; ask the card writer.
    Card(CardRequest),
}

/// A finished card step, with what the card writer needs: the step's change, not the whole payload.
/// The card store keeps it with its card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardRequest {
    pub session_id: String,
    /// The step's tool call, which names the card.
    pub tool_use_id: String,
    pub step: Step,
    /// The diff lines of an edit, or a new file's content as added lines.
    pub change: String,
}

/// The fields of a hook payload the core reads; the rest of the line is ignored.
#[derive(Deserialize)]
struct Event {
    session_id: String,
    hook_event_name: String,
    #[serde(default)]
    cwd: String,
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: Value,
    #[serde(default)]
    tool_use_id: String,
    #[serde(default)]
    tool_response: Value,
}

/// The tool input that says what a step works on: a command's first line, a path relative to the
/// session's folder, the first question asked, or a search pattern, URL, query or task description.
fn step_detail(input: &Value, cwd: &str) -> Option<String> {
    if let Some(command) = input["command"].as_str() {
        return command.lines().next().map(Into::into);
    }
    if let Some(question) = input["questions"][0]["question"].as_str() {
        return Some(question.into());
    }
    if let Some(path) = input["file_path"].as_str() {
        let relative = path.strip_prefix(cwd).and_then(|p| p.strip_prefix('/'));
        return Some(relative.unwrap_or(path).into());
    }
    ["pattern", "url", "query", "description"]
        .iter()
        .find_map(|key| input[key].as_str())
        .map(Into::into)
}

/// An Edit or Write outside the plan files (`~/.claude/plans/`): the card step this version knows.
fn is_card_step(tool: &str, input: &Value) -> bool {
    let plan_file = input["file_path"]
        .as_str()
        .is_some_and(|path| path.contains("/.claude/plans/"));
    matches!(tool, "Edit" | "Write") && !plan_file
}

/// An edit's change from its result: the patch's hunks, or a new file's content, which has none.
fn change(response: &Value) -> String {
    let hunks = response["structuredPatch"].as_array().into_iter().flatten();
    let patch: String = hunks
        .map(|hunk| {
            let lines = hunk["lines"].as_array().into_iter().flatten();
            let lines: String = lines
                .map(|line| format!("{}\n", line.as_str().unwrap_or_default()))
                .collect();
            let (old, old_lines) = (&hunk["oldStart"], &hunk["oldLines"]);
            let (new, new_lines) = (&hunk["newStart"], &hunk["newLines"]);
            format!("@@ -{old},{old_lines} +{new},{new_lines} @@\n{lines}")
        })
        .collect();
    if !patch.is_empty() {
        return patch;
    }
    let content = response["content"].as_str().unwrap_or_default().lines();
    content.map(|line| format!("+{line}\n")).collect()
}

impl Studio {
    /// Takes one live event log line and returns what the shell must do. A line that is not a hook
    /// event is skipped.
    pub fn apply(&mut self, line: &str) -> Vec<Output> {
        let Ok(event) = serde_json::from_str::<Event>(line) else {
            return Vec::new();
        };
        let session = self
            .sessions
            .entry(event.session_id.clone())
            .or_insert_with(|| Session {
                id: event.session_id.clone(),
                cwd: String::new(),
                last_step: None,
            });
        if !event.cwd.is_empty() {
            session.cwd.clone_from(&event.cwd);
        }
        let tool = event.tool_name.unwrap_or_default();
        match event.hook_event_name.as_str() {
            // A prompt opens a wait, also after an interrupt, which fires no `Stop`.
            "UserPromptSubmit" => {
                self.awaiting_card.insert(event.session_id.clone());
                self.prompted_last = Some(event.session_id);
            }
            "Stop" => {
                self.awaiting_card.remove(&event.session_id);
            }
            "PreToolUse" => {
                let detail = step_detail(&event.tool_input, &event.cwd);
                session.last_step = Some(Step { tool, detail });
            }
            // Only a step that succeeded counts: a failed Edit or Write changed nothing.
            "PostToolUse"
                if is_card_step(&tool, &event.tool_input)
                    && self.awaiting_card.remove(&event.session_id) =>
            {
                let detail = step_detail(&event.tool_input, &event.cwd);
                return vec![Output::Card(CardRequest {
                    session_id: event.session_id,
                    tool_use_id: event.tool_use_id,
                    step: Step { tool, detail },
                    change: change(&event.tool_response),
                })];
            }
            _ => {}
        }
        Vec::new()
    }

    /// Takes one event log line replayed at app start. It updates state exactly like `apply` and
    /// requests nothing, so steps that ran while the app was down get no cards.
    pub fn replay(&mut self, line: &str) {
        self.apply(line);
    }

    pub fn session(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    /// The session Daniel prompted last, which the pane follows before the workbench exists.
    pub fn prompted_last(&self) -> Option<&Session> {
        self.session(self.prompted_last.as_deref()?)
    }
}
