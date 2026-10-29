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
    pub state: State,
    /// The tool calls whose `PreToolUse` landed and whose `PostToolUse` has not.
    #[serde(skip)]
    running: Vec<Call>,
    /// The tool calls blocked on Daniel.
    #[serde(skip)]
    blocks: Vec<Call>,
    /// A block no tool call clears: a question in the turn's final message.
    #[serde(skip)]
    asked: bool,
    #[serde(skip)]
    turn_ended: bool,
}

impl Session {
    fn needs_you(&self) -> bool {
        self.asked || !self.blocks.is_empty()
    }

    /// The id of the latest running call that `matches`: the tool a block waits on, so that its
    /// `PostToolUse` clears the block. With none, the block lasts until the turn ends.
    fn running_id(&self, matches: impl Fn(&Call) -> bool) -> Option<String> {
        self.running.iter().rfind(|c| matches(c))?.id.clone()
    }

    /// Clears every block (#328, module 3, "Needs you": `UserPromptSubmit`, `Stop`, `SessionEnd`).
    fn unblock(&mut self) {
        self.blocks.clear();
        self.asked = false;
    }
}

/// Tools that wait on Daniel in any permission mode, before any `PermissionRequest`.
const ASKING_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];

/// Whether a session waits on Daniel (#328, module 3, "Needs you").
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Working,
    NeedsYou,
    YourTurn,
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
    /// A session newly needs Daniel.
    NeedsYou { session_id: String },
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

/// One tool call: its `tool_use_id`, and its tool and input, which are all `PermissionRequest`
/// names it by.
#[derive(Clone, Debug, PartialEq)]
struct Call {
    id: Option<String>,
    tool: String,
    input: Value,
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
    #[serde(default)]
    last_assistant_message: String,
    #[serde(default)]
    notification_type: String,
}

/// Whether the turn's final message asks Daniel something: a sentence ending in `?`, after removing
/// fenced code, inline code, blockquotes, headings and table rows (#321's evidence, "Transcripts").
fn asks(message: &str) -> bool {
    let mut in_fence = false;
    message.lines().any(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            return false;
        }
        // Inline code sits between pairs of backticks; an unpaired last one starts no code.
        let parts: Vec<&str> = line.split('`').collect();
        let prose: String = parts
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == 0 || i + 1 == parts.len())
            .map(|(_, part)| *part)
            .collect();
        let prose = prose
            .trim()
            .trim_end_matches([' ', '\t', '*', '_', ')', '"', '\'', '”', '’']);
        !in_fence
            && !prose.starts_with(['>', '#', '|'])
            && prose.split_whitespace().any(|word| word.ends_with('?'))
    })
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
                state: State::Working,
                running: Vec::new(),
                blocks: Vec::new(),
                asked: false,
                turn_ended: false,
            });
        if !event.cwd.is_empty() {
            session.cwd.clone_from(&event.cwd);
        }
        let mut outputs = Vec::new();
        let mut blocked_before = session.needs_you();
        let tool = event.tool_name.unwrap_or_default();
        let id = Some(event.tool_use_id.clone()).filter(|id| !id.is_empty());
        match event.hook_event_name.as_str() {
            // A prompt opens a wait, also after an interrupt, which fires no `Stop`.
            "UserPromptSubmit" => {
                self.awaiting_card.insert(event.session_id.clone());
                self.prompted_last = Some(event.session_id.clone());
                session.unblock();
                session.turn_ended = false;
            }
            "Stop" => {
                self.awaiting_card.remove(&event.session_id);
                // The order on `Stop`: clear every block first, then ask if the message asks.
                session.unblock();
                session.turn_ended = true;
                blocked_before = false;
                session.asked = asks(&event.last_assistant_message);
            }
            // shortcut: a closed session shows "your turn"; drop closed sessions once the board
            // lists sessions.
            "SessionEnd" => {
                session.unblock();
                session.turn_ended = true;
            }
            "PreToolUse" => {
                let detail = step_detail(&event.tool_input, &event.cwd);
                session.last_step = Some(Step {
                    tool: tool.clone(),
                    detail,
                });
                let call = Call {
                    id,
                    tool,
                    input: event.tool_input,
                };
                if ASKING_TOOLS.contains(&call.tool.as_str()) {
                    session.blocks.push(call.clone());
                }
                // The hooks are async, so this call's `PermissionRequest` may have landed first.
                // shortcut: a block whose call ended before the block landed stays until the turn
                // ends; remember ended calls if auto-denied calls (#322) show it.
                if let Some(block) = session
                    .blocks
                    .iter_mut()
                    .find(|b| b.id.is_none() && b.tool == call.tool && b.input == call.input)
                {
                    block.id.clone_from(&call.id);
                }
                session.running.push(call);
            }
            "PermissionRequest" if !ASKING_TOOLS.contains(&tool.as_str()) => {
                // It has no `tool_use_id`, so it names its call by tool and input.
                let id = session.running_id(|c| c.tool == tool && c.input == event.tool_input);
                session.blocks.push(Call {
                    id,
                    tool,
                    input: event.tool_input,
                });
            }
            // A sandboxed command's network request fires no `PermissionRequest`; any other
            // prompt already has its block.
            "Notification"
                if event.notification_type == "permission_prompt" && !session.needs_you() =>
            {
                let id = session.running_id(|c| c.tool == "Bash");
                session.blocks.push(Call {
                    id,
                    tool,
                    input: Value::Null,
                });
            }
            "Elicitation" => {
                let id = session.running_id(|c| c.tool.starts_with("mcp__"));
                session.blocks.push(Call {
                    id,
                    tool,
                    input: Value::Null,
                });
            }
            "PostToolUse" | "PostToolUseFailure" => {
                session.running.retain(|c| c.id != id);
                session.blocks.retain(|c| c.id != id);
                // Only a step that succeeded counts: a failed Edit or Write changed nothing.
                if event.hook_event_name == "PostToolUse"
                    && is_card_step(&tool, &event.tool_input)
                    && self.awaiting_card.remove(&event.session_id)
                {
                    let detail = step_detail(&event.tool_input, &event.cwd);
                    outputs.push(Output::Card(CardRequest {
                        session_id: event.session_id.clone(),
                        tool_use_id: event.tool_use_id,
                        step: Step { tool, detail },
                        change: change(&event.tool_response),
                    }));
                }
            }
            _ => {}
        }
        session.state = if session.needs_you() {
            State::NeedsYou
        } else if session.turn_ended {
            State::YourTurn
        } else {
            State::Working
        };
        if !blocked_before && session.needs_you() {
            outputs.push(Output::NeedsYou {
                session_id: event.session_id,
            });
        }
        outputs
    }

    /// Takes one event log line replayed at app start. It updates state exactly like `apply` and
    /// requests nothing, so steps that ran while the app was down get no cards and no notices.
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
