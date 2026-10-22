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
fn usd(dollars: f64) -> Value {
    json!((dollars * 1e4).round() / 1e4)
}

/// Per agent session folder and in total: tokens by class, cost, and both cross-checks.
/// `status` is `missing` when the run archived no session JSONL.
pub fn tokens(run_dir: &Path, result: &Value, prices: &Value) -> Result<Value> {
    let projects = run_dir.join("controller/claude-projects");
    let prefix = crate::batch::project_folder(run_dir) + "-";
    let (mut agents, mut total, mut cost_state, mut models) = (Map::new(), [0; 5], 0.0, vec![]);
    let mut folders: Vec<_> = fs::read_dir(&projects)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    folders.sort_by_key(|e| e.file_name());
    for folder in folders {
        let name = folder.file_name().to_string_lossy().into_owned();
        let (mut messages, mut sessions) = (Map::new(), 0);
        for file in fs::read_dir(folder.path())?.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            sessions += 1;
            let mut last_state = None;
            for line in fs::read_to_string(&path)?
                .lines()
                .filter(|l| !l.trim().is_empty())
            {
                let event: Value = serde_json::from_str(line)?;
                let message = &event["message"];
                match (event["type"].as_str(), message["id"].as_str()) {
                    (Some("assistant"), Some(id)) if message["usage"].is_object() => {
                        let model = message["model"].as_str().unwrap_or_default().to_string();
                        if !models.contains(&model) && classes(&message["usage"]) != [0; 5] {
                            models.push(model);
                        }
                        messages.insert(id.into(), message["usage"].clone());
                    }
                    (Some("cost-state"), _) => last_state = event["totalCostUSD"].as_f64(),
                    _ => {}
                }
            }
            cost_state += last_state.unwrap_or(0.0);
        }
        if sessions == 0 {
            continue;
        }
        let mut sum = [0; 5];
        messages.values().for_each(|u| add(&mut sum, classes(u)));
        add(&mut total, sum);
        let agent = name.strip_prefix(&prefix).unwrap_or(&name).to_string();
        agents.insert(
            agent,
            json!({"messages": messages.len(), "tokens": named(sum)}),
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
    let mismatched: Vec<&str> = CLASSES
        .iter()
        .zip(total.iter().zip(reported))
        .filter(|(_, (a, b))| **a != *b)
        .map(|(c, _)| *c)
        .collect();
    Ok(json!({
        "status": "available", "agents": agents, "tokens": tokens, "models": models,
        "cost_usd": cost.map(usd),
        "cost_label": "API-equivalent list price, not billed cost",
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
            ],
        );
        let result = json!({"usage": {"agents": {"A": [{"usage": {"input_tokens": 2,
            "output_tokens": 100, "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 300,
            "cache_creation": {"ephemeral_1h_input_tokens": 200}}}]}}});
        let out = tokens(run, &result, &serde_json::from_str(PRICES).unwrap()).unwrap();
        assert_eq!(out["status"], "available");
        assert_eq!(out["agents"]["A"]["messages"], 2);
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
