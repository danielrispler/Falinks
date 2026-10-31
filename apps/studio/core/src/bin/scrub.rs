//! Turns event log lines into a test fixture (#370): reads the log on stdin and writes the events of
//! the sessions named as arguments, or of every session when none is named, to stdout. Each event
//! keeps its shape and loses its content (#369, "Further Notes", "Fixtures and Daniel's data").
//!
//!     cargo run -p studio-core --bin scrub -- <session-id>... < events.jsonl > fixture.jsonl

use serde_json::{Map, Value};
use std::io::{self, BufRead, Write};

fn main() -> io::Result<()> {
    let sessions: Vec<String> = std::env::args().skip(1).collect();
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let Ok(Value::Object(event)) = serde_json::from_str(&line?) else {
            continue;
        };
        let session = event["session_id"].as_str().unwrap_or_default();
        if sessions.is_empty() || sessions.iter().any(|s| s == session) {
            writeln!(out, "{}", Value::Object(scrub(event)))?;
        }
    }
    Ok(())
}

/// Top-level text fields that stay as they are: ids, names, folders and states. Numbers and flags,
/// timings among them, stay anywhere.
const KEPT: &[&str] = &[
    "agent_id",
    "agent_type",
    "cwd",
    "hook_event_name",
    "notification_type",
    "permission_mode",
    "prompt_id",
    "reason",
    "session_id",
    "source",
    "studio_terminal_id",
    "tool_name",
    "tool_use_id",
];

/// Text fields of a tool's input or result, or of a background task, that stay: ids, types,
/// statuses, paths, and the short label an agent gives a spawn or a command.
const KEPT_TEXT: &[&str] = &[
    "agentId",
    "agent_type",
    "backgroundTaskId",
    "commandName",
    "description",
    "filePath",
    "file_path",
    "id",
    "isolation",
    "name",
    "skill",
    "status",
    "subagent_type",
    "task_id",
    "task_type",
    "type",
];

/// An event with its content gone: prompt text, messages, tool inputs and results, file contents.
fn scrub(event: Map<String, Value>) -> Map<String, Value> {
    let field = |(key, value): (String, Value)| {
        let text = value.as_str().unwrap_or_default();
        let value = match key.as_str() {
            "prompt" => markup(text).into(),
            // A turn whose last message asks Daniel something needs him, so the scrubbed one still asks.
            "last_assistant_message" if studio_core::asks(text) => "…?".into(),
            // The core tells a command that ran by the exit code on the error's first line.
            "error" if text.starts_with("Exit code") => {
                text.lines().next().unwrap_or_default().into()
            }
            "tool_input" | "tool_response" => fields(value),
            "background_tasks" => match value {
                Value::Array(tasks) => tasks.into_iter().map(fields).collect(),
                other => blank(other),
            },
            key if KEPT.contains(&key) => value,
            _ => blank(value),
        };
        (key, value)
    };
    event.into_iter().map(field).collect()
}

/// A tool's input or result, or a background task, with only its labels' text. A label of several
/// lines or over 200 bytes is content.
fn fields(value: Value) -> Value {
    let Value::Object(fields) = value else {
        return blank(value);
    };
    let label = |key: &str, value: &Value| {
        KEPT_TEXT.contains(&key)
            && value
                .as_str()
                .is_some_and(|s| s.len() <= 200 && !s.contains('\n'))
    };
    let field = |(key, value): (String, Value)| {
        let value = if label(&key, &value) {
            value
        } else {
            blank(value)
        };
        (key, value)
    };
    Value::Object(fields.into_iter().map(field).collect())
}

/// A value with its content gone and its type kept: text becomes `…`, a list or an object empty.
fn blank(value: Value) -> Value {
    match value {
        Value::String(_) => "…".into(),
        Value::Array(_) => Value::Array(Vec::new()),
        Value::Object(_) => Value::Object(Map::new()),
        scalar => scalar,
    }
}

/// A prompt's markup without its text, so a wake-up stays one (#369, decision 7) and Daniel's prompt
/// keeps none of its words. Each tag stays, with the attribute values that look like ids, as in
/// `<agent-message from="a2526f0d7353298ee">`. So does an id or a status right after a tag named
/// `status` or ending in `id`, as in `<task-id>a1191df87fb730aa0</task-id>`. Other text becomes `…`.
fn markup(prompt: &str) -> String {
    let mut out = String::new();
    let mut text = String::new();
    // Whether the tag right before `text` may hold an id or a status.
    let mut holds_id = false;
    let mut rest = prompt;
    while let Some(at) = rest.find('<') {
        text.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some((tag, len, name)) = tag(rest) else {
            text.push('<');
            rest = &rest[1..];
            continue;
        };
        let kept = text.trim();
        if holds_id && id_like(kept) {
            out.push_str(kept);
        } else if !kept.is_empty() {
            out.push('…');
        }
        out.push_str(&tag);
        holds_id = !tag.starts_with("</") && (name == "status" || name.ends_with("id"));
        text.clear();
        rest = &rest[len..];
    }
    if !(text + rest).trim().is_empty() {
        out.push('…');
    }
    out
}

/// The tag at the start of `s`: rebuilt with its name and its attributes, each value that does not
/// look like an id replaced by `…`; its length in `s`; and its name. `None` when the `<` opens no
/// tag, as in `3 < 4`.
fn tag(s: &str) -> Option<(String, usize, &str)> {
    let end = s.find('>')?;
    let inner = s[1..end].trim_end_matches('/');
    let (close, inner) = match inner.strip_prefix('/') {
        Some(inner) => ("/", inner),
        None => ("", inner),
    };
    let name_end = inner
        .find(|c: char| !(c.is_ascii_alphanumeric() || "-_:".contains(c)))
        .unwrap_or(inner.len());
    let name = &inner[..name_end];
    if inner.contains('<') || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut tag = format!("<{close}{name}");
    // Quoted values sit between every other pair of quotes, each after its `key=`.
    let parts: Vec<&str> = inner[name_end..].split('"').collect();
    for pair in parts.chunks(2) {
        if let [before, value] = pair
            && let Some(key) = before.trim_end().strip_suffix('=')
            && let Some(key) = key.split_whitespace().last()
        {
            let value = if id_like(value) { value } else { "…" };
            tag.push_str(&format!(" {key}=\"{value}\""));
        }
    }
    if s[1..end].ends_with('/') {
        tag.push('/');
    }
    tag.push('>');
    Some((tag, end + 1, name))
}

/// Text shaped like an agent, task or tool call id, or a status: one short word of letters, digits,
/// `-` and `_`.
fn id_like(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    fn scrub(event: Value) -> Value {
        let Value::Object(event) = event else {
            panic!("an event is an object")
        };
        Value::Object(super::scrub(event))
    }

    #[test]
    fn a_wake_up_keeps_its_markup_and_ids_and_loses_its_text() {
        let prompt = "<task-notification>\n<task-id>a1191df87fb730aa0</task-id>\n\
            <tool-use-id>toolu_01Bg1Ag2Ct3Dv4Ew5Fx6Gy7H</tool-use-id>\n\
            <output-file>/private/tmp/x/tasks/a1191df87fb730aa0.output</output-file>\n\
            <status>killed</status>\n<summary>Agent \"Fix the parser\" was stopped</summary>\n\
            <result>The parser now rejects empty input.</result>\n</task-notification>";
        let message = "<agent-message from=\"a2526f0d7353298ee\">Done, see Vec<String> in \
            <a href=\"https://example.com/a b\">the diff</a></agent-message>";

        let scrubbed = |prompt: &str| {
            let event = json!({"hook_event_name": "UserPromptSubmit", "prompt": prompt});
            scrub(event)["prompt"].as_str().unwrap().to_string()
        };

        assert_eq!(
            scrubbed(prompt),
            "<task-notification><task-id>a1191df87fb730aa0</task-id>\
            <tool-use-id>toolu_01Bg1Ag2Ct3Dv4Ew5Fx6Gy7H</tool-use-id><output-file>…</output-file>\
            <status>killed</status><summary>…</summary><result>…</result></task-notification>"
        );
        assert_eq!(
            scrubbed(message),
            "<agent-message from=\"a2526f0d7353298ee\">…<String>…<a href=\"…\">…</a></agent-message>"
        );
        assert_eq!(scrubbed("continue"), "…");
        assert_eq!(
            scrubbed(
                "Is 3 < 4? Read <pasted_content id=\"fa32\">hunter2</pasted_content id=\"fa32\">"
            ),
            "…<pasted_content id=\"fa32\">…</pasted_content id=\"fa32\">"
        );
    }

    #[test]
    fn a_spawn_keeps_its_ids_description_and_isolation_and_loses_its_prompt() {
        let spawn = json!({
            "session_id": "5d7f69ab", "hook_event_name": "PostToolUse", "cwd": "/w/Falinks",
            "transcript_path": "/h/.claude/projects/x.jsonl", "effort": {"level": "high"},
            "tool_name": "Agent", "tool_use_id": "toolu_01", "duration_ms": 12,
            "tool_input": {"description": "Implement #355", "isolation": "worktree", "prompt": "Read #355."},
            "tool_response": {"agentId": "a1191df87fb730aa0", "status": "async_launched", "isAsync": true,
                "prompt": "Read #355.", "outputFile": "/private/tmp/x.output"},
            "studio_received_ms": 1793264003290u64,
        });

        assert_eq!(
            scrub(spawn),
            json!({
                "session_id": "5d7f69ab", "hook_event_name": "PostToolUse", "cwd": "/w/Falinks",
                "transcript_path": "…", "effort": {},
                "tool_name": "Agent", "tool_use_id": "toolu_01", "duration_ms": 12,
                "tool_input": {"description": "Implement #355", "isolation": "worktree", "prompt": "…"},
                "tool_response": {"agentId": "a1191df87fb730aa0", "status": "async_launched", "isAsync": true,
                    "prompt": "…", "outputFile": "…"},
                "studio_received_ms": 1793264003290u64,
            })
        );
    }

    #[test]
    fn steps_lose_their_inputs_results_and_file_contents() {
        let edit = json!({
            "hook_event_name": "PostToolUse", "agent_id": "a1191df87fb730aa0", "agent_type": "general-purpose",
            "tool_name": "Edit",
            "tool_input": {"file_path": "/w/src/lib.rs", "old_string": "a", "new_string": "b", "replace_all": false},
            "tool_response": {"filePath": "/w/src/lib.rs", "originalFile": "a\n",
                "structuredPatch": [{"lines": ["-a", "+b"]}]},
        });
        let failed = json!({"hook_event_name": "PostToolUseFailure", "tool_name": "Bash",
            "tool_input": {"command": "cargo test", "description": "Run the tests\nsecond line"},
            "error": "Exit code 101\ntest result: FAILED"});
        let refused = json!({"hook_event_name": "PostToolUseFailure", "error": "This agent is isolated in /w"});

        assert_eq!(
            scrub(edit),
            json!({
                "hook_event_name": "PostToolUse", "agent_id": "a1191df87fb730aa0", "agent_type": "general-purpose",
                "tool_name": "Edit",
                "tool_input": {"file_path": "/w/src/lib.rs", "old_string": "…", "new_string": "…", "replace_all": false},
                "tool_response": {"filePath": "/w/src/lib.rs", "originalFile": "…", "structuredPatch": []},
            })
        );
        assert_eq!(
            scrub(failed),
            json!({"hook_event_name": "PostToolUseFailure", "tool_name": "Bash",
                "tool_input": {"command": "…", "description": "…"}, "error": "Exit code 101"})
        );
        assert_eq!(scrub(refused)["error"], "…");
    }

    #[test]
    fn a_stop_keeps_whether_it_asks_and_its_background_tasks_without_their_commands() {
        let stop = |message: &str| {
            json!({"hook_event_name": "Stop", "last_assistant_message": message,
                "background_tasks": [
                    {"id": "bbr1524wy", "type": "shell", "status": "running", "description": "Run the gate",
                        "command": "TOKEN=x ./gate"},
                    {"id": "a32aaf17ca17f12b2", "type": "subagent", "status": "running",
                        "description": "Standards review", "agent_type": "general-purpose"}],
                "session_crons": [{"id": "9d12fe2c", "prompt": "Continue #364"}]})
        };

        assert_eq!(
            scrub(stop("Merged. Shall I close #370?")),
            json!({"hook_event_name": "Stop", "last_assistant_message": "…?",
                "background_tasks": [
                    {"id": "bbr1524wy", "type": "shell", "status": "running", "description": "Run the gate",
                        "command": "…"},
                    {"id": "a32aaf17ca17f12b2", "type": "subagent", "status": "running",
                        "description": "Standards review", "agent_type": "general-purpose"}],
                "session_crons": []})
        );
        assert_eq!(scrub(stop("Merged `x?`."))["last_assistant_message"], "…");
    }
}
