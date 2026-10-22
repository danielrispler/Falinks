//! Tokens and API-equivalent cost of one run (#202), from the Claude Code session JSONL that
//! `archive` moved into `controller/claude-projects`.
//!
//! - The JSONL repeats a message's usage on every content block, so each `message.id` counts
//!   once (its last line wins). It is written as the model streams, so an agent's last turn is
//!   counted even when the run stopped before that turn's `result` arrived.
//! - Cost is tokens × the frozen `prices` table in `config.json`: an API-equivalent list price,
//!   not billed cost, since the runs use a subscription login.
//! - Cross-checks, reported and never reconciled: the drained turn `result` usage in
//!   `result.json` (#196) and the last `cost-state` `totalCostUSD` of each session.
//! - Two more cost lines (#206 decisions 5 and 6), both list price:
//!   - **Cold-start**: the measured cost with each session's first API call's cache read
//!     repriced as a 1-hour cache write. A process resumed later in the same session writes
//!     to the same JSONL, so its first call is not repriced: its read is the session's context.
//!   - **Without `sandbox_instructions`** (estimate ±15%): the attachment's characters over
//!     the first request's characters per token give its tokens. Their 1-hour cache write and
//!     their read on every later call of the session are removed. A session without the
//!     attachment leaves the run without this value.
use crate::{Result, root};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::Path;

pub const CLASSES: [&str; 5] = [
    "input",
    "output",
    "cache_write_5m",
    "cache_write_1h",
    "cache_read",
];

/// The five token classes of one API `usage` object. Cache writes without a TTL breakdown
/// count as 5-minute writes.
fn classes(usage: &Value) -> [u64; 5] {
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let hour = n(&usage["cache_creation"]["ephemeral_1h_input_tokens"]);
    [
        n(&usage["input_tokens"]),
        n(&usage["output_tokens"]),
        n(&usage["cache_creation_input_tokens"]).saturating_sub(hour),
        hour,
        n(&usage["cache_read_input_tokens"]),
    ]
}

fn add(total: &mut [u64; 5], more: [u64; 5]) {
    total.iter_mut().zip(more).for_each(|(t, m)| *t += m);
}

fn named(tokens: [u64; 5]) -> Value {
    CLASSES
        .iter()
        .zip(tokens)
        .map(|(c, t)| (c.to_string(), json!(t)))
        .collect::<Map<_, _>>()
        .into()
}

/// Characters of a rendered attachment or a message's content.
fn chars(content: &Value) -> usize {
    match content {
        Value::String(text) => text.chars().count(),
        Value::Array(blocks) => blocks
            .iter()
            .map(|b| chars(b.get("content").or(b.get("text")).unwrap_or(&Value::Null)))
            .sum(),
        _ => 0,
    }
}

/// Tokens of the session's `sandbox_instructions` attachment, by the characters per token of
/// its first request (the call `first`, with `context` tokens in): tools, CLI prefix and
/// system text from the first prompt snapshot listing tools (logged after that call), plus the
/// first message and the attachments rendered before it. None without the attachment.
fn sandbox_tokens(events: &[Value], first: &str, context: u64) -> Option<f64> {
    let before: Vec<&Value> = events
        .iter()
        .take_while(|e| e["message"]["id"] != first)
        .collect();
    let sandbox = before
        .iter()
        .find(|e| e["attachment"]["type"] == "sandbox_instructions")?;
    let snapshot = events
        .iter()
        .map(|e| &e["attachment"])
        .find(|a| a["type"] == "prompt_snapshot" && a["tools"].is_array())?;
    let tools: usize = snapshot["tools"]
        .as_array()?
        .iter()
        .map(|t| t["schema"].to_string().chars().count())
        .sum();
    let system: usize = snapshot["systemPrompt"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| *s != "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__")
        .map(chars)
        .sum();
    let message = before
        .iter()
        .find(|e| e["type"] == "user")
        .map_or(0, |e| chars(&e["message"]["content"]));
    let attachments: usize = before.iter().map(|e| chars(&e["rendered"])).sum();
    let request = tools + chars(&snapshot["cliPrefix"]) + system + message + attachments;
    let tokens = chars(&sandbox["rendered"]) as f64 * context as f64 / request as f64;
    Some(tokens.round()).filter(|t| t.is_finite())
}

/// The `prices` table of the frozen `config.json`.
pub fn prices() -> Result<Value> {
    Ok(crate::read(root().join("config.json"))?["prices"].clone())
}

/// API-equivalent USD of `tokens` (as `named` gives them) at `prices`.
pub fn cost(tokens: &Value, prices: &Value) -> Option<f64> {
    CLASSES.iter().try_fold(0.0, |sum, c| {
        Some(sum + tokens[c].as_f64()? * prices["usd_per_million_tokens"][c].as_f64()? / 1e6)
    })
}

/// Rounded to a hundredth of a cent.
pub fn usd(dollars: f64) -> Value {
    json!((dollars * 1e4).round() / 1e4)
}

/// Per agent session folder and in total: tokens by class, cost, and both cross-checks.
/// `status` is `missing` when the run archived no session JSONL.
pub fn tokens(run_dir: &Path, result: &Value, prices: &Value) -> Result<Value> {
    let projects = run_dir.join("controller/claude-projects");
    let prefix = crate::batch::project_folder(run_dir) + "-";
    let (mut agents, mut total, mut cost_state, mut models) = (Map::new(), [0; 5], 0.0, vec![]);
    let mut unreadable = 0;
    // Cache reads of first calls, and the attachment tokens written and later read (None once
    // a session lacks it).
    let (mut first_reads, mut removed) = (0, Some((0.0, 0.0)));
    let mut folders: Vec<_> = fs::read_dir(&projects)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    folders.sort_by_key(|e| e.file_name());
    for folder in folders {
        let name = folder.file_name().to_string_lossy().into_owned();
        let (mut messages, mut sessions) = (Map::new(), vec![]);
        let mut files: Vec<_> = fs::read_dir(folder.path())?
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .collect();
        files.sort();
        for path in &files {
            let (mut last_state, mut events, mut calls) = (None, vec![], Vec::<String>::new());
            for line in fs::read_to_string(path)?
                .lines()
                .filter(|l| !l.trim().is_empty())
            {
                // A worker stopped mid-write can leave a truncated last line.
                let Ok(event) = serde_json::from_str::<Value>(line) else {
                    unreadable += 1;
                    continue;
                };
                let message = &event["message"];
                match (event["type"].as_str(), message["id"].as_str()) {
                    (Some("assistant"), Some(id)) if message["usage"].is_object() => {
                        if message["model"] != "<synthetic>" && !calls.iter().any(|c| c == id) {
                            calls.push(id.into());
                        }
                        let model = message["model"].as_str().unwrap_or_default().to_string();
                        if !models.contains(&model) && classes(&message["usage"]) != [0; 5] {
                            models.push(model);
                        }
                        messages.insert(id.into(), message["usage"].clone());
                    }
                    (Some("cost-state"), _) => last_state = event["totalCostUSD"].as_f64(),
                    _ => {}
                }
                events.push(event);
            }
            cost_state += last_state.unwrap_or(0.0);
            let Some(first) = calls.first() else {
                continue;
            };
            let [input, _, write_5m, write_1h, read] = classes(&messages[first]);
            let sandbox = sandbox_tokens(&events, first, input + write_5m + write_1h + read);
            let later = (calls.len() - 1) as f64;
            first_reads += read;
            removed = removed
                .zip(sandbox)
                .map(|((written, reread), t)| (written + t, reread + t * later));
            sessions.push(
                json!({"first_call": {"cache_read": read, "cache_write": write_5m + write_1h},
                "sandbox_tokens": sandbox}),
            );
        }
        if files.is_empty() {
            continue;
        }
        let mut sum = [0; 5];
        messages.values().for_each(|u| add(&mut sum, classes(u)));
        add(&mut total, sum);
        let agent = name.strip_prefix(&prefix).unwrap_or(&name).to_string();
        agents.insert(
            agent,
            json!({"messages": messages.len(), "tokens": named(sum), "sessions": sessions}),
        );
    }
    let mut reported = [0; 5];
    for report in result["usage"]["agents"]
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, r)| r.as_array().cloned().unwrap_or_default())
    {
        add(&mut reported, classes(&report["usage"]));
    }
    if agents.is_empty() {
        return Ok(
            json!({"status": "missing", "tokens": null, "cost_usd": null,
            "result_usage": named(reported)}),
        );
    }
    let tokens = named(total);
    // A message from a model the table does not price leaves the cost unknown.
    let priced = models.iter().all(|m| prices["model"] == m.as_str());
    let cost = cost(&tokens, prices).filter(|_| priced);
    // Per token; Claude Code writes its prompt cache with a 1-hour TTL.
    let price = |c: &str| prices["usd_per_million_tokens"][c].as_f64().unwrap_or(0.0) / 1e6;
    let (hour_price, read_price) = (price("cache_write_1h"), price("cache_read"));
    let cold_start = cost.map(|c| c + first_reads as f64 * (hour_price - read_price));
    let sandbox_excluded = cost
        .zip(removed)
        .map(|(c, (written, reread))| c - written * hour_price - reread * read_price);
    let mismatched: Vec<&str> = CLASSES
        .iter()
        .zip(total.iter().zip(reported))
        .filter(|(_, (a, b))| **a != *b)
        .map(|(c, _)| *c)
        .collect();
    Ok(json!({
        "status": "available", "agents": agents, "tokens": tokens, "models": models,
        "cost_usd": cost.map(usd), "cold_start_cost_usd": cold_start.map(usd),
        "sandbox_excluded_cost_usd": sandbox_excluded.map(usd),
        "cost_label": prices["label"], "unreadable_lines": unreadable,
        "cross_checks": {
            "result_usage": named(reported), "result_mismatched_classes": mismatched,
            "cost_state_usd": usd(cost_state),
            "cost_state_mismatch": cost.is_none_or(|c| (c - cost_state).abs() >= 0.0001),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRICES: &str = r#"{"model": "claude-opus-5-5", "usd_per_million_tokens":
        {"input": 4, "output": 20, "cache_write_5m": 5, "cache_write_1h": 8, "cache_read": 0.2}}"#;

    /// Assistant lines as Claude Code writes them: one per content block, usage repeated.
    fn line(id: &str, output: u64) -> String {
        json!({"type": "assistant", "message": {"id": id, "model": "claude-opus-5-5", "usage": {
            "input_tokens": 2, "output_tokens": output, "cache_read_input_tokens": 1000,
            "cache_creation_input_tokens": 300,
            "cache_creation": {"ephemeral_1h_input_tokens": 200, "ephemeral_5m_input_tokens": 100}}}})
        .to_string()
    }

    fn session(run: &Path, agent: &str, lines: &[String]) {
        let folder = run
            .join("controller/claude-projects")
            .join(format!("{}-{agent}", crate::batch::project_folder(run)));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("s.jsonl"), lines.join("\n") + "\n").unwrap();
    }

    #[test]
    fn repeated_message_lines_count_once_and_unfinished_turns_count() {
        let tmp = tempfile::tempdir().unwrap();
        let run = tmp.path();
        let state = r#"{"type":"cost-state","totalCostUSD":0.0129}"#.to_string();
        // m2 is the open last turn: streamed into the JSONL, with no `result` report.
        session(
            run,
            "A",
            &[
                line("m1", 100),
                line("m1", 100),
                line("m2", 50),
                line("m2", 50),
                state,
                r#"{"type":"assi"#.into(),
            ],
        );
        let result = json!({"usage": {"agents": {"A": [{"usage": {"input_tokens": 2,
            "output_tokens": 100, "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 300,
            "cache_creation": {"ephemeral_1h_input_tokens": 200}}}]}}});
        let out = tokens(run, &result, &serde_json::from_str(PRICES).unwrap()).unwrap();
        assert_eq!(out["status"], "available");
        assert_eq!(out["agents"]["A"]["messages"], 2);
        assert_eq!(out["unreadable_lines"], 1);
        assert_eq!(
            out["tokens"],
            json!({"input": 4, "output": 150, "cache_write_5m": 200, "cache_write_1h": 400, "cache_read": 2000})
        );
        // 16 + 3000 + 1000 + 3200 + 400 micro-dollars.
        assert_eq!(out["cost_usd"], 0.0076);
        let checks = &out["cross_checks"];
        assert_eq!(checks["result_mismatched_classes"], json!(CLASSES));
        assert_eq!(checks["cost_state_mismatch"], true);
    }

    fn call(id: &str, read: u64, write: u64) -> String {
        json!({"type": "assistant", "message": {"id": id, "model": "claude-opus-5-5", "usage": {
            "input_tokens": 0, "output_tokens": 10, "cache_read_input_tokens": read,
            "cache_creation_input_tokens": write, "cache_creation": {"ephemeral_1h_input_tokens": write}}}})
        .to_string()
    }

    fn user(chars: usize) -> String {
        json!({"type": "user", "message": {"role": "user", "content": "u".repeat(chars)}})
            .to_string()
    }

    fn attachment(kind: &str, chars: usize) -> String {
        json!({"type": "attachment", "attachment": {"type": kind},
            "rendered": [{"content": "a".repeat(chars)}]})
        .to_string()
    }

    /// A session whose first request is 1,000 characters over 2,000 tokens: a 100-character
    /// message, a 300-character `sandbox_instructions`, a 100-character attachment, and a
    /// snapshot (logged after the first call) of 100 characters of tools, 100 of CLI prefix
    /// and 300 of system text. Three calls: the first, a later one, and a resumed process's.
    fn resumed_session() -> Vec<String> {
        let snapshot = json!({"type": "attachment", "attachment": {"type": "prompt_snapshot",
            "cliPrefix": "c".repeat(100),
            "systemPrompt": ["p".repeat(300), "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__"],
            "tools": [{"name": "T", "description": "d", "schema": {"d": "s".repeat(92)}}]}});
        vec![
            user(100),
            attachment("sandbox_instructions", 300),
            attachment("environment", 100),
            call("m1", 1000, 1000),
            snapshot.to_string(),
            call("m2", 2000, 100),
            // The same session resumed after a regrouping transition: its read is its own context.
            user(7),
            call("m3", 5000, 50),
        ]
    }

    #[test]
    fn only_each_sessions_first_call_is_repriced_and_a_resumed_process_is_not() {
        let tmp = tempfile::tempdir().unwrap();
        let run = tmp.path();
        session(run, "A", &resumed_session());
        session(run, "B", &[user(1), call("m4", 500, 500)]);
        let out = tokens(run, &json!({}), &serde_json::from_str(PRICES).unwrap()).unwrap();
        assert_eq!(
            out["agents"]["A"]["sessions"],
            json!([{"first_call": {"cache_read": 1000, "cache_write": 1000}, "sandbox_tokens": 600.0}])
        );
        assert_eq!(
            out["agents"]["B"]["sessions"],
            json!([{"first_call": {"cache_read": 500, "cache_write": 500}, "sandbox_tokens": null}])
        );
        // 11,400 + 4,300 micro-dollars measured, plus m1's and m4's reads at 8 instead of 0.2.
        assert_eq!(out["cost_usd"], 0.0157);
        assert_eq!(out["cold_start_cost_usd"], 0.0274);
        // B has no attachment, so the run has no value: missing, never zero.
        assert_eq!(out["sandbox_excluded_cost_usd"], Value::Null);
    }

    #[test]
    fn the_attachments_cache_write_and_later_reads_are_removed() {
        let tmp = tempfile::tempdir().unwrap();
        session(tmp.path(), "A", &resumed_session());
        let out = tokens(
            tmp.path(),
            &json!({}),
            &serde_json::from_str(PRICES).unwrap(),
        )
        .unwrap();
        // 600 tokens at 2 tokens per character: 11,400 - 600 × 8 - 2 × 600 × 0.2 micro-dollars.
        assert_eq!(out["cost_usd"], 0.0114);
        assert_eq!(out["sandbox_excluded_cost_usd"], 0.0064);
    }

    #[test]
    fn a_run_without_session_jsonl_has_missing_usage() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tokens(
            tmp.path(),
            &json!({}),
            &serde_json::from_str(PRICES).unwrap(),
        )
        .unwrap();
        assert_eq!(out["status"], "missing");
        assert_eq!(out["tokens"], Value::Null);
        assert_eq!(out["cost_usd"], Value::Null);
    }

    #[test]
    fn an_unpriced_model_leaves_the_cost_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        session(
            tmp.path(),
            "A",
            &[line("m1", 10).replace("claude-opus-5-5", "claude-x")],
        );
        let out = tokens(
            tmp.path(),
            &json!({}),
            &serde_json::from_str(PRICES).unwrap(),
        )
        .unwrap();
        assert_eq!(out["cost_usd"], Value::Null);
        assert_eq!(out["models"], json!(["claude-x"]));
    }

    #[test]
    fn the_frozen_table_prices_every_class() {
        let prices = prices().unwrap();
        assert!(cost(&named([1; 5]), &prices).is_some(), "{prices}");
        for key in ["model", "source", "date", "label"] {
            assert!(prices[key].is_string(), "{key}");
        }
    }
}
