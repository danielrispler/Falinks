//! The transcript reader (#461, decision 12): a tolerant reader of a session's transcript at the
//! hook's `transcript_path`, for what the session is doing and why. It reads the lines of its own
//! agent and skips line types it does not know. A line that does not parse, or a user or assistant
//! line without `message.content`, makes the transcript unavailable: the session view falls back to
//! step text only. The Claude Code version notice (#373) is the early warning of a format change.
//!
//! It is the one place the core reads a file, and it opens only a `.jsonl` path, so a payload that
//! names a credential file is never read (`apps/studio/AGENTS.md`, "Hard rules").

use serde::Serialize;
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
};

/// What the transcript says of the session's latest turn, which the session view shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Story {
    /// The intention: the last line of the agent's latest text in the turn.
    pub intention: Option<String>,
    /// The steps (tool calls) the agent made since that text.
    pub steps_ago: usize,
    /// The agent's full texts in the turn, oldest first.
    pub responses: Vec<String>,
    /// Why the transcript cannot be read, `None` while it can.
    pub unavailable: Option<String>,
}

impl Story {
    /// Takes in transcript line `line`, or says why it cannot.
    pub fn read(&mut self, line: &str) -> Result<(), String> {
        let line: Value =
            serde_json::from_str(line).map_err(|e| format!("a line does not parse: {e}"))?;
        let kind = line["type"].as_str().unwrap_or_default();
        // A helper's lines are its own; meta lines (a skill's text, a command's caveat) are no prompt.
        if !matches!(kind, "user" | "assistant")
            || line["isSidechain"] == true
            || line["isMeta"] == true
        {
            return Ok(());
        }
        let content = &line["message"]["content"];
        let blocks = match content {
            Value::String(_) if kind == "user" => {
                self.new_turn();
                return Ok(());
            }
            Value::Array(blocks) => blocks,
            _ => return Err(format!("a {kind} line has no message.content")),
        };
        for block in blocks {
            match (kind, block["type"].as_str()) {
                ("user", Some("text")) if line["isCompactSummary"] != true => self.new_turn(),
                ("assistant", Some("tool_use")) => self.steps_ago += 1,
                ("assistant", Some("text")) => {
                    let text = block["text"].as_str().unwrap_or_default().trim();
                    let Some(last) = text.lines().map(str::trim).rfind(|l| !l.is_empty()) else {
                        continue;
                    };
                    self.intention = Some(last.into());
                    self.steps_ago = 0;
                    self.responses.push(text.into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn new_turn(&mut self) {
        *self = Story::default();
    }
}

/// Where a session's transcript is, and how far it was read.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Transcript {
    pub(crate) path: String,
    read_to: u64,
}

impl Transcript {
    /// Reads the complete lines added to the transcript since the last read into `story`. Returns
    /// `story.unavailable`'s new reason when it changed to one, for one stderr line.
    pub(crate) fn follow(&mut self, story: &mut Story) -> Option<String> {
        let before = story.unavailable.clone();
        // A corrupted transcript stays so; a gone file may come back.
        if before.as_deref().is_some_and(|why| why != GONE) {
            return None;
        }
        match self.read(story) {
            Ok(()) => story.unavailable = None,
            Err(why) => {
                *story = Story::default();
                story.unavailable = Some(why);
                // From the start again, once a gone file comes back.
                self.read_to = 0;
            }
        }
        (story.unavailable != before)
            .then(|| story.unavailable.clone())
            .flatten()
    }

    fn read(&mut self, story: &mut Story) -> Result<(), String> {
        let path = Path::new(&self.path);
        if path.extension().is_none_or(|e| e != "jsonl") {
            return Err("the transcript path is not a .jsonl file".into());
        }
        let mut file = File::open(path).map_err(|_| GONE.to_string())?;
        file.seek(SeekFrom::Start(self.read_to))
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        // A line counts once it ends in a newline, so one Claude Code is still writing waits.
        while reader
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?
            > 0
            && line.ends_with(b"\n")
        {
            self.read_to += line.len() as u64;
            story.read(&String::from_utf8_lossy(&line))?;
            line.clear();
        }
        Ok(())
    }
}

const GONE: &str = "the transcript file is gone";
