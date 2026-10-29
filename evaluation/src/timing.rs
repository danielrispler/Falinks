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
//!   are the lines of the agent's last version of each source file that a line diff against
//!   its initial content does not match, read from the arm's records, not the transcript
//!   (#325). A changed line counts only for the agent whose operation or commit introduced
//!   it, traced through each version's base by the same diff. A peer's lines carried in a
//!   whole-file write, a merge or a byte-identical copy of the peer's earlier version stay
//!   the peer's, and a merge or conflict resolution introduces no one's. A line one agent wrote and the other rewrote counts twice: the
//!   original for its writer, in the writer's last version, and the rewrite for the rewriter.
//!   A changed line traced to the initial file (a moved line) is the holder's. Unreadable
//!   records leave changed bytes missing.
//! - **Refusal recovery** (#193 decision 9), per output form of a refused Falinks `edit`,
//!   `retry` or `incorporate`: the round-trips after it, and their model seconds, up to and
//!   including the agent's next accepted one. Refusals inside an open recovery belong to it, so
//!   a resend in another form is charged to the first; a refused deletion opens none. A
//!   recovery the transcript ends in is `unresolved`; its round-trips still count.
//! - **Request-key refusals** (#206 decision 2), per Falinks op: results whose reason starts
//!   `request keys:`, the adapter's refusal of unknown or missing request keys.
//! - **Operations** (#316), per Falinks op of a Falinks agent: its calls (acts), refused results
//!   and tool time.
//!   The **edit gate** is the tool time of accepted source writes, with its mean per accepted
//!   write. Both are transcript-side, so parallel calls make them approximate too; host and
//!   edit-gate instrumentation (#178's I-2) is not read.
use crate::{Result, bail, blobs, fixture, git, read};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const FALINKS: &str = "mcp__falinks__falinks_";
const SOURCE_WRITES: [&str; 4] = ["edit", "retry", "incorporate", "apply_job"];
const CATEGORIES: [&str; 4] = ["edit", "refused_edit", "native", "coordination"];
/// Falinks mutations that take output forms, and the forms (#193 decision 1), as
/// `adapters/claude/arm.rs` counts them; its `tests/arm.rs` checks they agree.
const EDITS: [&str; 3] = ["edit", "retry", "incorporate"];
const FORMS: [&str; 3] = ["content", "replace", "from"];

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

/// Seconds rounded to the millisecond.
fn rounded(seconds: f64) -> Value {
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

/// A tool result's text; a Falinks result's is its JSON.
fn text(block: &Value) -> String {
    let content = &block["content"];
    match content.as_str() {
        Some(text) => text.into(),
        None => content
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| b["text"].as_str())
            .collect(),
    }
}

/// A tool result is refused when it is an error or a Falinks result with `accepted: false`.
fn refused(block: &Value) -> bool {
    block["is_error"] == true
        || serde_json::from_str::<Value>(&text(block)).is_ok_and(|v| v["accepted"] == false)
}

/// A Falinks request refused by the adapter's key check (#206 decision 2).
fn key_refused(block: &Value) -> bool {
    serde_json::from_str::<Value>(&text(block)).is_ok_and(|v| {
        v["reason"]
            .as_str()
            .is_some_and(|r| r.starts_with("request keys:"))
    })
}

/// File writes in a `Bash` command made by here-documents redirected to a file. Other shell
/// writes (`sed -i`, `cp`, `printf >`, `tee`) are not recognized, so they count as native.
fn heredocs(command: &str) -> Vec<(String, Write)> {
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
            let write = if append {
                Write::Append(body)
            } else {
                Write::Replace(body)
            };
            writes.push((path.to_string(), write));
        }
    }
    writes
}

#[derive(Debug, PartialEq)]
enum Write {
    Replace(String),
    Append(String),
    Patch {
        old: String,
        new: String,
        all: bool,
    },
    /// A Falinks `from` output: the whole file, read from a scratch path.
    From,
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
            .map(|(path, write)| (path, write, bash_writes_source))
            .collect(),
        name if name
            .strip_prefix(FALINKS)
            .is_some_and(|op| SOURCE_WRITES.contains(&op)) =>
        {
            let output = request(call)["output"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            output
                .into_iter()
                .flat_map(|(path, file)| {
                    let writes: Vec<Write> = if let Some(hunks) = file["replace"].as_array() {
                        hunks
                            .iter()
                            .map(|h| Write::Patch {
                                old: text(&h["old"]),
                                new: text(&h["new"]),
                                all: false,
                            })
                            .collect()
                    } else if file["from"].is_string() {
                        vec![Write::From]
                    } else {
                        vec![Write::Replace(text(&file["content"]))]
                    };
                    writes.into_iter().map(move |w| (path.clone(), w, true))
                })
                .collect()
        }
        _ => vec![],
    }
}

/// A Falinks call's request: the input itself since #220, before it under a `request` key,
/// which models sometimes sent as a JSON string.
fn request(call: &Call) -> Value {
    match call.input.get("request") {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_default(),
        Some(request) => request.clone(),
        None => call.input.clone(),
    }
}

/// The output forms a Falinks edit request uses.
fn forms(request: &Value) -> Vec<&'static str> {
    let outputs: Vec<&Value> = request["output"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .collect();
    FORMS
        .into_iter()
        .filter(|form| outputs.iter().any(|o| o.get(form).is_some()))
        .collect()
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

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// For each line of `b`, the line of `a` that a longest-common-subsequence line diff matches
/// it to.
fn matches(a: &[&str], b: &[&str]) -> Vec<Option<usize>> {
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
    let (mut i, mut j, mut matched) = (0, 0, vec![None; b.len()]);
    while j < b.len() {
        if i < a.len() && a[i] == b[j] {
            matched[j] = Some(i);
            (i, j) = (i + 1, j + 1);
        } else if i < a.len() && lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    matched
}

/// The changed lines of `new`, by index: those a longest-common-subsequence line diff leaves
/// unmatched in `old`. Changed bytes are their bytes.
fn changed_lines<'a>(old: &str, new: &'a str) -> Vec<(usize, &'a str)> {
    let new = lines(new);
    let matched = matches(&lines(old), &new);
    new.into_iter()
        .enumerate()
        .zip(matched)
        .filter_map(|(line, m)| m.is_none().then_some(line))
        .collect()
}

/// Who introduced a line.
#[derive(Clone, Debug, PartialEq)]
enum Origin {
    Initial,
    Agent(String),
    /// A merge or conflict resolution, or a base no record shows.
    Nobody,
}

/// The origin of each line of every file version seen, keyed by path and text.
#[derive(Default)]
struct Blame(BTreeMap<(String, String), Vec<Origin>>);

impl Blame {
    /// `text` at `path`, written by `author` over `bases`: a line the diff matches in a base
    /// keeps that line's origin, the first base first; every other line is the author's. A
    /// text seen before keeps its first origins, so a whole copy of a peer's file is the peer's.
    fn version(&mut self, path: &str, author: Origin, bases: &[String], text: String) {
        let key = |text: &String| (path.to_string(), text.clone());
        if self.0.contains_key(&key(&text)) {
            return;
        }
        let new = lines(&text);
        let mut origins = vec![author; new.len()];
        for base in bases.iter().rev() {
            let known = self.0.get(&key(base));
            for (j, i) in matches(&lines(base), &new).into_iter().enumerate() {
                if let Some(i) = i {
                    origins[j] = known.map_or(Origin::Nobody, |k| k[i].clone());
                }
            }
        }
        self.0.insert(key(&text), origins);
    }
}

/// One agent's figures from its stream-json events, sessions concatenated.
fn agent(events: &[Value], bash_writes_source: bool, clock: (Option<f64>, Option<f64>)) -> Value {
    let (mut turns, mut messages, mut calls) = (Vec::<(Option<f64>, f64)>::new(), vec![], vec![]);
    let (mut ids, mut tool_ids) = (BTreeMap::new(), BTreeMap::new());
    let mut key_refusals = BTreeMap::<String, u64>::new();
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
                        if let Some(op) = call.name.strip_prefix(FALINKS)
                            && key_refused(block)
                        {
                            *key_refusals.entry(op.to_string()).or_insert(0) += 1;
                        }
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
                json!({"count": count, "model_seconds": rounded(seconds)}),
            )
        })
        .collect();
    let total: (usize, f64) = categories
        .iter()
        .fold((0, 0.0), |sum, c| (sum.0 + c.0, sum.1 + c.1));
    round_trips.insert(
        "total".into(),
        json!({"count": total.0, "model_seconds": rounded(total.1)}),
    );

    // Per form: refusals, unresolved, round-trips and model seconds.
    let mut recovery: BTreeMap<&str, (u64, u64, u64, f64)> =
        FORMS.map(|f| (f, (0, 0, 0, 0.0))).into();
    let mut open: Option<Vec<&str>> = None;
    for message in messages.iter().filter(|m| !m.calls.is_empty()) {
        for form in open.iter().flatten() {
            let entry = recovery.get_mut(form).unwrap();
            (entry.2, entry.3) = (entry.2 + 1, entry.3 + model(message));
        }
        for call in message.calls.iter().map(|&c| &calls[c]) {
            let edit = call
                .name
                .strip_prefix(FALINKS)
                .is_some_and(|op| EDITS.contains(&op));
            match call.result {
                Some((_, false)) if edit => open = None,
                Some((_, true)) if edit && open.is_none() => {
                    let used = forms(&request(call));
                    used.iter()
                        .for_each(|f| recovery.get_mut(f).unwrap().0 += 1);
                    open = Some(used).filter(|used| !used.is_empty());
                }
                _ => {}
            }
        }
    }
    for form in open.iter().flatten() {
        recovery.get_mut(form).unwrap().1 += 1;
    }
    let recovery: Map<String, Value> = recovery
        .into_iter()
        .map(|(form, (refusals, unresolved, count, seconds))| {
            (
                form.to_string(),
                json!({"refusals": refusals, "unresolved": unresolved,
                "round_trips": count, "model_seconds": rounded(seconds)}),
            )
        })
        .collect();

    let output: usize = calls
        .iter()
        .flat_map(|call| writes(call, bash_writes_source))
        .map(|(_, write, _)| match write {
            Write::Replace(text) | Write::Append(text) => text.len(),
            Write::Patch { new, .. } => new.len(),
            // Its bytes were counted when the model wrote the scratch file.
            Write::From => 0,
        })
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
    // Acts, refusals and tool seconds per Falinks op; accepted source writes and their seconds.
    let mut operations = BTreeMap::<&str, (u64, u64, f64)>::new();
    let (mut accepted, mut gate_seconds) = (0u64, 0.0);
    for call in &calls {
        let Some(op) = call.name.strip_prefix(FALINKS) else {
            continue;
        };
        let entry = operations.entry(op).or_default();
        entry.0 += 1;
        if let Some((at, refused)) = call.result {
            entry.1 += u64::from(refused);
            entry.2 += at - call.emitted;
            if !refused && SOURCE_WRITES.contains(&op) {
                accepted += 1;
                gate_seconds += at - call.emitted;
            }
        }
    }
    let operations: Map<String, Value> = operations
        .into_iter()
        .map(|(op, (acts, refused, seconds))| {
            let entry = json!({"acts": acts, "refused": refused, "tool_seconds": rounded(seconds)});
            (op.to_string(), entry)
        })
        .collect();
    let mut figures = json!({
        "busy_seconds": rounded(busy),
        "model_seconds": rounded(messages.iter().map(model).sum()),
        "tool_seconds": rounded(tool),
        "launch_seconds": start.zip(first).map(|(start, first)| rounded(first - start)),
        "waiting_on_peer_seconds": stop.zip(first).map(|(stop, first)| rounded((stop - first - busy).max(0.0))),
        "round_trips": round_trips,
        "refusal_recovery": recovery,
        "request_key_refusals": key_refusals,
        "output_bytes": output,
        "results": events.iter().filter(|e| e["type"] == "result").count(),
    });
    // A Git agent makes no Falinks call, so its figures stay as they were before #316.
    if !bash_writes_source {
        figures["operations"] = operations.into();
        figures["edit_gate"] = json!({"accepted": accepted, "seconds": rounded(gate_seconds),
            "per_accepted_seconds": (accepted > 0).then(|| rounded(gate_seconds / accepted as f64))});
    }
    figures
}

/// Per agent, the changed bytes of its last version of every file it changed that it
/// introduced itself, read from the arm's records (#325), not the transcript.
///
/// - A Falinks agent's versions are its applied operations in `controller/history.json`: each
///   output path's resolved file in the operation's exact `before` and `proposed` revisions.
/// - A Git agent's last version is its last shared commit (`controller/events.jsonl`) in
///   `agents/repo`; work it never shared counts nothing. Its own commits are those on the
///   first-parent line of its shares, first sharer first. A merge introduces no one's lines,
///   so a conflict resolution counts for no one; an integration commit reached only through
///   a merge has no author.
fn changed(run: &Path, arm: &str, files: &Map<String, Value>) -> Result<BTreeMap<String, usize>> {
    let mut blame = Blame::default();
    for (path, text) in files {
        blame.version(
            path,
            Origin::Initial,
            &[],
            text.as_str().unwrap_or("").into(),
        );
    }
    let mut last = BTreeMap::<(String, String), String>::new();
    if arm == "git" {
        let repo = run.join("agents/repo");
        let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let shares: Vec<(String, String)> = jsonl(&run.join("controller/events.jsonl"))?
            .iter()
            .filter(|event| event["kind"] == "draft_shared")
            .map(|event| (text(&event["agent"]), text(&event["commit"])))
            .collect();
        let mut owners = BTreeMap::new();
        for (agent, share) in &shares {
            for commit in git(&repo, &["rev-list", "--first-parent", share], None)?.lines() {
                owners.entry(commit.to_string()).or_insert(agent.clone());
            }
        }
        let mut trees = BTreeMap::<String, BTreeMap<String, String>>::new();
        // Share by share, in order, so a text shared earlier keeps its authorship.
        let mut commits = vec![];
        for (_, share) in &shares {
            let args = ["rev-list", "--topo-order", "--reverse", "--parents", share];
            commits.extend(git(&repo, &args, None)?.lines().map(String::from));
        }
        for line in &commits {
            let (commit, parents) = line.split_once(' ').unwrap_or((line, ""));
            let parents: Vec<&str> = parents.split_whitespace().collect();
            if trees.contains_key(commit) {
                continue;
            }
            // The initial commit has no parent; a merge introduces no one's lines.
            let author = match (parents.len(), owners.get(commit)) {
                (0, _) => Origin::Initial,
                (1, Some(agent)) => Origin::Agent(agent.clone()),
                _ => Origin::Nobody,
            };
            let tree: BTreeMap<String, String> = blobs(&repo, commit)?
                .into_iter()
                .map(|(path, bytes)| (path, String::from_utf8_lossy(&bytes).into_owned()))
                .collect();
            for (path, text) in &tree {
                let bases: Vec<String> = parents
                    .iter()
                    .map(|p| trees[*p].get(path).cloned().unwrap_or_default())
                    .collect();
                blame.version(path, author.clone(), &bases, text.clone());
            }
            trees.insert(commit.to_string(), tree);
        }
        let last_shares: BTreeMap<String, String> = shares.into_iter().collect();
        for (agent, share) in last_shares {
            for (path, text) in &trees[&share] {
                last.insert((agent.clone(), path.clone()), text.clone());
            }
        }
    } else {
        // A path absent from a revision, or deleted, is empty.
        let file = |v: &Value| {
            let bytes = v["file"]["bytes"].as_array().into_iter().flatten();
            let bytes: Vec<u8> = bytes.filter_map(Value::as_u64).map(|b| b as u8).collect();
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let history = read(run.join("controller/history.json"))?;
        for record in history.as_array().into_iter().flatten() {
            if record["outcome"].get("Applied").is_none() {
                continue;
            }
            let agent = format!("agent-{}", record["agent"]);
            for (path, _) in record["request"]["output"]
                .as_object()
                .into_iter()
                .flatten()
            {
                let base = file(&record["before"]["files"][path]);
                let text = file(&record["proposed"]["files"][path]);
                blame.version(path, Origin::Agent(agent.clone()), &[base], text.clone());
                last.insert((agent.clone(), path.clone()), text);
            }
        }
    }
    let mut changed = BTreeMap::new();
    for ((agent, path), text) in last {
        let initial = files.get(&path).and_then(Value::as_str).unwrap_or("");
        let origins = &blame.0[&(path, text.clone())];
        // A changed line traced to the initial file is the holder's: no peer wrote it.
        let own: usize = changed_lines(initial, &text)
            .into_iter()
            .filter(|(j, _)| match &origins[*j] {
                Origin::Initial => true,
                Origin::Agent(author) => *author == agent,
                Origin::Nobody => false,
            })
            .map(|(_, line)| line.len())
            .sum();
        *changed.entry(agent).or_insert(0) += own;
    }
    Ok(changed)
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
        // A team of one has no agent B (#178 C1); runs before it record no team size.
        "git" => ["A", "B"]
            .iter()
            .take(result["team_size"].as_u64().unwrap_or(2) as usize)
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
    // Unreadable records leave changed bytes missing, never zero.
    let changed = changed(run, arm, &files).ok();
    let mut figures = Map::new();
    for (name, events) in &agents {
        let mut figure = agent(events, arm == "git", (start, stop));
        figure["changed_bytes"] = json!(changed.as_ref().map(|c| c.get(name).map_or(0, |n| *n)));
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
    let recovery: Map<String, Value> = FORMS
        .iter()
        .map(|form| {
            let sum = |key: &str| -> f64 {
                figures
                    .values()
                    .map(|f| f["refusal_recovery"][form][key].as_f64().unwrap())
                    .sum()
            };
            let entry = json!({"refusals": sum("refusals") as u64, "unresolved": sum("unresolved") as u64,
                "round_trips": sum("round_trips") as u64, "model_seconds": rounded(sum("model_seconds"))});
            (form.to_string(), entry)
        })
        .collect();
    let mut key_refusals = BTreeMap::<String, u64>::new();
    for (op, n) in figures.values().flat_map(|f| {
        f["request_key_refusals"]
            .as_object()
            .cloned()
            .unwrap_or_default()
    }) {
        *key_refusals.entry(op).or_insert(0) += n.as_u64().unwrap_or(0);
    }
    Ok(json!({
        "run": result["run_id"], "arm": arm, "fixture": result["fixture"],
        "elapsed_seconds": timing["elapsed_seconds"], "start_precision": precision,
        "slowest_agent": slowest,
        "round_trips": {"count": count as u64, "model_seconds": rounded(seconds)},
        "refusal_recovery": recovery,
        "request_key_refusals": key_refusals,
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
        let changed_bytes = |old: &str, new: &str| -> usize {
            changed_lines(old, new).iter().map(|(_, l)| l.len()).sum()
        };
        assert_eq!(changed_bytes("a\nb\nc\n", "a\nb\nc\n"), 0);
        assert_eq!(changed_bytes("a\nb\nc\n", "a\nx\nc\nd\n"), 4);
        assert_eq!(changed_bytes("", "abc"), 3);
        assert_eq!(changed_bytes("a\nb\n", ""), 0);
    }

    #[test]
    fn a_copied_version_keeps_its_first_authorship() {
        let mut blame = Blame::default();
        let b = Origin::Agent("B".into());
        blame.version("a.go", Origin::Initial, &[], "a\n".into());
        blame.version("a.go", b.clone(), &["a\n".into()], "a\nb\n".into());
        // A copies B's file whole, with no merge: "b\n" stays B's.
        blame.version(
            "a.go",
            Origin::Agent("A".into()),
            &["a\n".into()],
            "a\nb\n".into(),
        );
        assert_eq!(
            blame.0[&("a.go".into(), "a\nb\n".into())],
            [Origin::Initial, b]
        );
    }

    #[test]
    fn refusal_recovery_runs_to_the_next_accepted_edit() {
        let at = |s: u32| json!(format!("2026-10-21T10:00:{s:02}.000Z"));
        let call = |id: &str, s: u32, name: &str, form: &str| {
            let input = json!({"request": {"output": {"a.go": {form: "x"}}}});
            json!({"type": "assistant", "timestamp": at(s), "message": {"id": id,
                "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}})
        };
        let result = |id: &str, s: u32, accepted: bool| {
            json!({"type": "user", "timestamp": at(s), "message": {"content": [{"type": "tool_result",
                "tool_use_id": id, "content": json!({"accepted": accepted}).to_string()}]}})
        };
        let edit = format!("{FALINKS}edit");
        let events = [
            json!({"type": "system", "subtype": "init"}),
            json!({"type": "user", "timestamp": at(0), "message": {"content": "go"}}),
            call("m1", 2, &edit, "content"),
            result("m1", 3, false),
            call("m2", 5, "Read", ""),
            result("m2", 6, true),
            call("m3", 9, &edit, "content"),
            result("m3", 10, true),
            call("m4", 11, &edit, "replace"),
            result("m4", 12, false),
        ];
        let figures = agent(&events, false, (None, None));
        let recovery = &figures["refusal_recovery"];
        // m2 (3 to 5) and m3 (6 to 9) follow m1's refusal; m4's refusal is never recovered.
        assert_eq!(
            recovery["content"],
            json!({"refusals": 1, "unresolved": 0, "round_trips": 2, "model_seconds": 5.0})
        );
        assert_eq!(
            recovery["replace"],
            json!({"refusals": 1, "unresolved": 1, "round_trips": 0, "model_seconds": 0.0})
        );
        assert_eq!(recovery["from"]["refusals"], 0);
    }

    #[test]
    fn falinks_edits_count_every_form_flat_or_wrapped() {
        let at = |s: u32| json!(format!("2026-10-21T10:00:{s:02}.000Z"));
        let call = |id: &str, s: u32, name: &str, input: Value| {
            json!({"type": "assistant", "timestamp": at(s), "message": {"id": id,
                "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}})
        };
        let result = |id: &str, s: u32, accepted: bool| {
            json!({"type": "user", "timestamp": at(s), "message": {"content": [{"type": "tool_result",
                "tool_use_id": id, "content": json!({"accepted": accepted}).to_string()}]}})
        };
        let edit = format!("{FALINKS}edit");
        let figures = |wrap: fn(Value) -> Value| {
            let events = [
                json!({"type": "system", "subtype": "init"}),
                json!({"type": "user", "timestamp": at(0), "message": {"content": "go"}}),
                // Agents run in their scratch directory: a relative write lands there.
                call(
                    "m1",
                    1,
                    "Bash",
                    json!({"command": "cat > c.go <<'EOF'\nc\nEOF"}),
                ),
                result("m1", 2, true),
                call(
                    "m2",
                    3,
                    &edit,
                    wrap(json!({"output": {"a.go": {"replace": [{"old": "b", "new": "bb"}]}}})),
                ),
                result("m2", 4, false),
                call(
                    "m3",
                    5,
                    &edit,
                    wrap(
                        json!({"output": {"a.go": {"replace": [{"old": "b", "new": "bc"}]},
                    "b.go": {"content": "y\n"}, "c.go": {"from": "c.go"}}}),
                    ),
                ),
                result("m3", 6, true),
            ];
            agent(&events, false, (None, None))
        };
        let flat = figures(|request| request);
        assert_eq!(flat, figures(|request| json!({"request": request})));
        assert_eq!(
            flat,
            figures(|request| json!({"request": request.to_string()}))
        );
        // Heredoc "c\n" 2, refused "bb" 2, "bc" 2, "y\n" 2; `from` emits no file bytes.
        assert_eq!(flat["output_bytes"], 8);
        assert_eq!(
            flat["refusal_recovery"]["replace"],
            json!({"refusals": 1, "unresolved": 0, "round_trips": 1, "model_seconds": 1.0})
        );
    }

    #[test]
    fn request_key_refusals_are_counted_per_op() {
        let at = json!("2026-10-21T10:00:00.000Z");
        let call = |id: &str, op: &str| {
            json!({"type": "assistant", "timestamp": at, "message": {"id": id, "content": [
                {"type": "tool_use", "id": id, "name": format!("{FALINKS}{op}"), "input": {}}]}})
        };
        // As the MCP tool returns a refusal: an error whose text is the JSON result.
        let result = |id: &str, reason: &str| {
            let text = json!({"accepted": false, "reason": reason}).to_string();
            json!({"type": "user", "timestamp": at, "message": {"content": [{"type": "tool_result",
                "tool_use_id": id, "is_error": true, "content": [{"type": "text", "text": text}]}]}})
        };
        let events = [
            call("m1", "handle"),
            result("m1", "request keys: handle takes through"),
            call("m2", "handle"),
            result("m2", "request keys: handle takes through"),
            call("m3", "ack"),
            result("m3", "request keys: ack needs milestone"),
            call("m4", "edit"),
            result("m4", "a.go: hunk 1: old text matches 0 places"),
        ];
        let figures = agent(&events, false, (None, None));
        assert_eq!(
            figures["request_key_refusals"],
            json!({"ack": 1, "handle": 2})
        );
    }

    #[test]
    fn heredocs_find_redirected_bodies_only() {
        let command = "cd /w/A && cat > producer.go <<'EOF'\nline 1\nline 2\nEOF\n\
                       cat <<EOF >> scratch/log\nx\nEOF\n\"$FALINKS_HOST\" message \"$(cat <<'EOF'\nhi\nEOF\n)\"";
        assert_eq!(
            heredocs(command),
            [
                (
                    "producer.go".into(),
                    Write::Replace("line 1\nline 2\n".into())
                ),
                ("scratch/log".into(), Write::Append("x\n".into())),
            ]
        );
    }
}
