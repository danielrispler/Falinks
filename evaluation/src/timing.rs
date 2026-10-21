//! `timing <run-dir>` (#179): per-agent time, round-trips and edit output from one run's
//! stream-json transcripts, by #116's method.
//!
//! - A **round-trip** is a distinct assistant `message.id` with at least one tool use.
//! - **Model time** runs from the latest input (any user event: prompt, notice or tool result)
//!   to the message's last block. **Tool time** runs from a tool use's emission to its result;
//!   calls made in parallel overlap, so it is approximate.
//! - **Busy** time is spent in turns (first input to last event), clipped to the stopped clock.
//!   **Launch** runs from the run's start to the agent's first turn. **Waiting on peer** is the
//!   rest of the run after launch: the agent was idle until a host event woke it.
//! - Round-trip categories, first match wins: `edit` (an accepted source write), `refused_edit`,
//!   `native` (any other non-protocol tool), `coordination` (protocol calls only). Falinks
//!   source writes are `edit`, `retry`, `incorporate` and `apply_job`. A Git agent writes with
//!   `Write`, `Edit` or a `Bash` here-document redirected outside scratch; a `Bash` command
//!   naming `FALINKS_HOST` is coordination.
//! - **Output bytes** are the file content the model wrote, scratch included. **Changed bytes**
//!   are the lines of each source file's last write that a line diff against its initial
//!   content does not match.
use crate::{Result, bail, fixture, read};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const FALINKS: &str = "mcp__falinks__falinks_";
const SOURCE_WRITES: [&str; 4] = ["edit", "retry", "incorporate", "apply_job"];
const CATEGORIES: [&str; 4] = ["edit", "refused_edit", "native", "coordination"];

/// Seconds since the Unix epoch of a UTC timestamp such as `2026-10-15T17:37:34.552Z`.
pub fn unix(timestamp: &str) -> Option<f64> {
    let (date, time) = timestamp.strip_suffix('Z')?.split_once('T')?;
    let mut date = date.split('-').map(str::parse::<i64>);
    let (year, month, day) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let mut time = time.split(':').map(str::parse::<f64>);
    let (hour, minute, second) = (time.next()?.ok()?, time.next()?.ok()?, time.next()?.ok()?);
    // Days from civil (Howard Hinnant), the inverse of `utc`.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(days as f64 * 86_400.0 + hour * 3600.0 + minute * 60.0 + second)
}

fn millis(seconds: f64) -> Value {
    json!((seconds * 1000.0).round() / 1000.0)
}

struct Call {
    name: String,
    input: Value,
    emitted: f64,
    /// When its result arrived, and whether it was refused.
    result: Option<(f64, bool)>,
}

struct Message {
    last: f64,
    input: Option<f64>,
    calls: Vec<usize>,
}

/// A tool result is refused when it is an error or a Falinks result with `accepted: false`.
fn refused(block: &Value) -> bool {
    let content = &block["content"];
    let text: String = match content.as_str() {
        Some(text) => text.into(),
        None => content
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| b["text"].as_str())
            .collect(),
    };
    block["is_error"] == true
        || serde_json::from_str::<Value>(&text).is_ok_and(|v| v["accepted"] == false)
}

/// File writes in a `Bash` command made by here-documents redirected to a file:
/// `(path, append, body)`. Other shell writes are not recognized.
fn heredocs(command: &str) -> Vec<(String, bool, String)> {
    let lines: Vec<&str> = command.split('\n').collect();
    let (mut writes, mut i) = (vec![], 0);
    while i < lines.len() {
        let words: Vec<&str> = lines[i].split_whitespace().collect();
        i += 1;
        let Some(at) = words.iter().position(|w| w.starts_with("<<")) else {
            continue;
        };
        let mut tag = words[at].trim_start_matches("<<").trim_start_matches('-');
        if tag.is_empty() {
            tag = words.get(at + 1).copied().unwrap_or_default();
        }
        let tag = tag.trim_matches(|c| c == '\'' || c == '"');
        let mut body = String::new();
        while i < lines.len() && lines[i] != tag {
            body += lines[i];
            body.push('\n');
            i += 1;
        }
        i += 1;
        let redirect = words.iter().enumerate().find_map(|(n, w)| {
            let (append, path) = match w.strip_prefix(">>") {
                Some(path) => (true, path),
                None => (false, w.strip_prefix('>')?),
            };
            let path = if path.is_empty() {
                words.get(n + 1)?
            } else {
                path
            };
            Some((append, path.trim_matches(|c| c == '\'' || c == '"')))
        });
        if let Some((append, path)) = redirect {
            writes.push((path.to_string(), append, body));
        }
    }
    writes
}

enum Write {
    Replace(String),
    Append(String),
    Patch { old: String, new: String, all: bool },
}

/// The file writes in one tool call, with the bytes the model emitted for each.
fn writes(call: &Call, bash_writes_source: bool) -> Vec<(String, Write, bool)> {
    let input = &call.input;
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    match call.name.as_str() {
        "Write" => vec![(
            text(&input["file_path"]),
            Write::Replace(text(&input["content"])),
            true,
        )],
        "Edit" => vec![(
            text(&input["file_path"]),
            Write::Patch {
                old: text(&input["old_string"]),
                new: text(&input["new_string"]),
                all: input["replace_all"] == true,
            },
            true,
        )],
        "Bash" => heredocs(input["command"].as_str().unwrap_or_default())
            .into_iter()
            .map(|(path, append, body)| {
                let write = if append {
                    Write::Append(body)
                } else {
                    Write::Replace(body)
                };
                (path, write, bash_writes_source)
            })
            .collect(),
        name if name
            .strip_prefix(FALINKS)
            .is_some_and(|op| SOURCE_WRITES.contains(&op)) =>
        {
            let request = match &input["request"] {
                Value::String(text) => serde_json::from_str(text).unwrap_or_default(),
                request => request.clone(),
            };
            let output = request["output"].as_object().cloned().unwrap_or_default();
            output
                .into_iter()
                .map(|(path, file)| (path, Write::Replace(text(&file["content"])), true))
                .collect()
        }
        _ => vec![],
    }
}

fn category(call: &Call, bash_writes_source: bool) -> usize {
    let writes_source = writes(call, bash_writes_source)
        .iter()
        .any(|(path, _, source)| *source && !scratch(path));
    let refused = call.result.is_some_and(|(_, refused)| refused);
    match call.name.strip_prefix(FALINKS) {
        Some(op) if SOURCE_WRITES.contains(&op) => usize::from(refused),
        Some(_) => 3,
        None if writes_source => usize::from(refused),
        None if call.name == "Bash" && call.input.to_string().contains("FALINKS_HOST") => 3,
        None => 2,
    }
}

fn scratch(path: &str) -> bool {
    path.split('/').any(|part| part == "scratch")
}

/// Bytes of the lines in `new` that a longest-common-subsequence line diff leaves unmatched
/// in `old`.
pub fn changed_bytes(old: &str, new: &str) -> usize {
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut changed) = (0, 0, 0);
    while j < b.len() {
        if i < a.len() && a[i] == b[j] {
            (i, j) = (i + 1, j + 1);
        } else if i < a.len() && lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            changed += b[j].len();
            j += 1;
        }
    }
    changed
}

/// One agent's figures from its stream-json events, sessions concatenated.
fn agent(
    events: &[Value],
    files: &Map<String, Value>,
    bash_writes_source: bool,
    clock: (Option<f64>, Option<f64>),
) -> Value {
    let (mut turns, mut messages, mut calls) = (Vec::<(Option<f64>, f64)>::new(), vec![], vec![]);
    let (mut ids, mut tool_ids) = (BTreeMap::new(), BTreeMap::new());
    let mut input: Option<f64> = None;
    for event in events {
        let at = event["timestamp"].as_str().and_then(unix);
        match (event["type"].as_str(), at) {
            (Some("system"), _) if event["subtype"] == "init" => turns.push((None, f64::MIN)),
            (Some("user"), Some(at)) => {
                if let Some(turn) = turns.last_mut() {
                    turn.0.get_or_insert(at);
                    turn.1 = turn.1.max(at);
                }
                input = Some(input.map_or(at, |i: f64| i.max(at)));
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    if let Some(&call) = tool_ids.get(block["tool_use_id"].as_str().unwrap_or("")) {
                        let call: &mut Call = &mut calls[call];
                        call.result = Some((at, refused(block)));
                    }
                }
            }
            (Some("assistant"), Some(at)) => {
                let Some(id) = event["message"]["id"].as_str() else {
                    continue;
                };
                let index = *ids.entry(id.to_string()).or_insert_with(|| {
                    messages.push(Message {
                        last: at,
                        input,
                        calls: vec![],
                    });
                    messages.len() - 1
                });
                messages[index].last = at;
                if let Some(turn) = turns.last_mut() {
                    turn.1 = turn.1.max(at);
                }
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_use" {
                        tool_ids
                            .insert(block["id"].as_str().unwrap_or("").to_string(), calls.len());
                        messages[index].calls.push(calls.len());
                        calls.push(Call {
                            name: block["name"].as_str().unwrap_or("").into(),
                            input: block["input"].clone(),
                            emitted: at,
                            result: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    let model = |m: &Message| m.input.map_or(0.0, |input| m.last - input);
    let mut categories = [(0, 0.0); 4];
    for message in messages.iter().filter(|m| !m.calls.is_empty()) {
        let category = message
            .calls
            .iter()
            .map(|&c| category(&calls[c], bash_writes_source))
            .min()
            .unwrap();
        categories[category].0 += 1;
        categories[category].1 += model(message);
    }
    let mut round_trips: Map<String, Value> = CATEGORIES
        .iter()
        .zip(categories)
        .map(|(name, (count, seconds))| {
            (
                name.to_string(),
                json!({"count": count, "model_seconds": millis(seconds)}),
            )
        })
        .collect();
    let total: (usize, f64) = categories
        .iter()
        .fold((0, 0.0), |sum, c| (sum.0 + c.0, sum.1 + c.1));
    round_trips.insert(
        "total".into(),
        json!({"count": total.0, "model_seconds": millis(total.1)}),
    );

    // Initial content of the source files, keyed by fixture path; other source writes start empty.
    let mut contents: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut output = 0;
    for call in &calls {
        let applied = !call.result.is_some_and(|(_, refused)| refused);
        for (path, write, source) in writes(call, bash_writes_source) {
            output += match &write {
                Write::Replace(text) | Write::Append(text) => text.len(),
                Write::Patch { new, .. } => new.len(),
            };
            if !applied || !source || scratch(&path) {
                continue;
            }
            let key = files
                .keys()
                .find(|f| path == **f || path.ends_with(&format!("/{f}")))
                .cloned()
                .unwrap_or(path);
            let initial = files
                .get(&key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let (_, current) = contents
                .entry(key)
                .or_insert_with(|| (initial.clone(), initial));
            match write {
                Write::Replace(text) => *current = text,
                Write::Append(text) => *current += &text,
                Write::Patch { old, new, all } if all => *current = current.replace(&old, &new),
                Write::Patch { old, new, .. } => *current = current.replacen(&old, &new, 1),
            }
        }
    }
    let changed: usize = contents
        .values()
        .map(|(initial, last)| changed_bytes(initial, last))
        .sum();

    let (start, stop) = clock;
    let busy: f64 = turns
        .iter()
        .filter_map(|&(begin, end)| Some((begin?, end)))
        .map(|(begin, end)| (stop.map_or(end, |stop| end.min(stop)) - begin).max(0.0))
        .sum();
    let first = turns.iter().find_map(|turn| turn.0);
    let tool: f64 = calls
        .iter()
        .filter_map(|c| Some(c.result?.0 - c.emitted))
        .sum();
    json!({
        "busy_seconds": millis(busy),
        "model_seconds": millis(messages.iter().map(model).sum()),
        "tool_seconds": millis(tool),
        "launch_seconds": start.zip(first).map(|(start, first)| millis(first - start)),
        "waiting_on_peer_seconds": stop.zip(first).map(|(stop, first)| millis((stop - first - busy).max(0.0))),
        "round_trips": round_trips,
        "output_bytes": output,
        "changed_bytes": changed,
        "results": events.iter().filter(|e| e["type"] == "result").count(),
    })
}

fn jsonl(path: &Path) -> Result<Vec<Value>> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?)
}

/// Per-agent timing for one run directory of either arm.
pub fn timing(run: &Path) -> Result<Value> {
    let result = read(run.join("controller/result.json"))?;
    let arm = result["arm"].as_str().unwrap_or_default();
    let agents: Vec<(String, Vec<Value>)> = match arm {
        "git" => ["A", "B"]
            .iter()
            .map(|a| {
                let path = run
                    .join("agents")
                    .join(a)
                    .join("scratch/claude-events.jsonl");
                Ok((a.to_string(), jsonl(&path)?))
            })
            .collect::<Result<_>>()?,
        "falinks" => read(run.join("controller/transcripts.json"))?
            .as_array()
            .ok_or("transcripts.json must list agents")?
            .iter()
            .enumerate()
            .map(|(n, sessions)| {
                let events = sessions
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|s| s["events"].as_array().cloned().unwrap_or_default())
                    .collect();
                (format!("agent-{n}"), events)
            })
            .collect(),
        other => bail!("unknown arm {other:?}"),
    };
    let timing = &result["timing"];
    // Runs before #179 record their start to the second only.
    let (start, precision) = match timing["start_unix"].as_f64() {
        Some(start) => (Some(start), "millisecond"),
        None => (timing["start_utc"].as_str().and_then(unix), "second"),
    };
    let stop = start
        .zip(timing["elapsed_seconds"].as_f64())
        .map(|(s, e)| s + e);
    let files = fixture(result["fixture"].as_str().unwrap_or_default())?["files"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut figures = Map::new();
    for (name, events) in &agents {
        let mut figure = agent(events, &files, arm == "git", (start, stop));
        let reports = &result["usage"]["agents"][name.as_str()];
        figure["usage_reports"] = reports.as_array().map_or(0, Vec::len).into();
        figures.insert(name.clone(), figure);
    }
    // The agent that worked longest: its launch and busy time end latest.
    let slowest = figures
        .iter()
        .max_by(|a, b| {
            let span = |f: &Value| {
                f["launch_seconds"].as_f64().unwrap_or(0.0) + f["busy_seconds"].as_f64().unwrap()
            };
            span(a.1).total_cmp(&span(b.1))
        })
        .map(|(name, _)| name.clone());
    let total = |f: &Value, key: &str| f["round_trips"]["total"][key].as_f64().unwrap();
    let count: f64 = figures.values().map(|f| total(f, "count")).sum();
    let seconds: f64 = figures.values().map(|f| total(f, "model_seconds")).sum();
    Ok(json!({
        "run": result["run_id"], "arm": arm, "fixture": result["fixture"],
        "elapsed_seconds": timing["elapsed_seconds"], "start_precision": precision,
        "slowest_agent": slowest,
        "round_trips": {"count": count as u64, "model_seconds": millis(seconds)},
        "agents": figures,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_inverts_utc() {
        for seconds in [0, 951_782_400, 1_792_085_854, 4_107_542_399] {
            assert_eq!(unix(&crate::utc(seconds)), Some(seconds as f64));
        }
        assert_eq!(
            unix("2026-10-15T17:37:34.552Z").map(|s| (s * 1000.0).round()),
            Some(1_792_085_854_552.0)
        );
    }

    #[test]
    fn changed_bytes_count_unmatched_new_lines() {
        assert_eq!(changed_bytes("a\nb\nc\n", "a\nb\nc\n"), 0);
        assert_eq!(changed_bytes("a\nb\nc\n", "a\nx\nc\nd\n"), 4);
        assert_eq!(changed_bytes("", "abc"), 3);
        assert_eq!(changed_bytes("a\nb\n", ""), 0);
    }

    #[test]
    fn heredocs_find_redirected_bodies_only() {
        let command = "cd /w/A && cat > producer.go <<'EOF'\nline 1\nline 2\nEOF\n\
                       cat <<EOF >> scratch/log\nx\nEOF\n\"$FALINKS_HOST\" message \"$(cat <<'EOF'\nhi\nEOF\n)\"";
        let writes: Vec<(String, bool, String)> = heredocs(command);
        assert_eq!(
            writes,
            [
                ("producer.go".into(), false, "line 1\nline 2\n".into()),
                ("scratch/log".into(), true, "x\n".into()),
            ]
        );
    }
}
