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
//! - **Integration** (#337, Git only), a figure beside the categories: the round-trips with a
//!   write under `agents/integration`, or a `git merge`, `rebase`, `cherry-pick` or
//!   `checkout --theirs/--ours` in a command that does not name the host. A heuristic.
//! - **Runtime refusals** (#205 D1), a figure beside the categories: the round-trips holding a
//!   `Write` or `Edit` that Claude Code refused by its own read-before-write rule ("File has not
//!   been read yet", "File has been modified since read"). They stay in `refused_edit`.
//! - **Collision handling** (#205, "Measures"), per class of the run's `collisions`: the
//!   round-trips, and their model seconds, from each signal that reached the agent to its
//!   resolution, each charged to the first window that holds it (see `window`). A rework the
//!   peer's share detected (`peer_diff`) has no window: its time is the share's, not the
//!   agent's notice. A signal whose call the transcript lacks is `unlocated`; one the
//!   transcript ends in is `unresolved`.
//!   **Rereads** (Q7c): after a Falinks `Stale` whose conflicts name a file, a collision or not,
//!   or a Git `CONFLICT (` collision signal naming it, the agent's first read of it before its
//!   next accepted write of it. A collision's rereads count in its handling cost.
//! - **Coordination overhead** (Q7b): the round-trips and model seconds of `refused_edit`,
//!   `coordination`, integration and rereads, each round-trip once, less runtime refusals, plus
//!   a Falinks agent's **review estimate**: the inline `reviews` of its accepted edits, as
//!   compact JSON, at #193's 3.56 ms per character. Engine and host refusals stay in it.
//!   Unreadable collision records leave handling, rereads and overhead missing.
//! - **Output bytes** are the file content the model wrote, scratch included. **Changed bytes**
//!   are the lines of the agent's last version of each source file that a line diff against
//!   its initial content does not match, read from the arm's records, not the transcript
//!   (#325). A changed line counts only for the agent whose operation or commit introduced
//!   it, traced through each version's base by the same diff (`authorship`, which collision
//!   waste shares). A peer's lines carried in a whole-file write, a merge or a byte-identical
//!   copy of the peer's earlier version stay the peer's, and a merge or conflict resolution
//!   introduces no one's. A line one agent wrote and the other rewrote counts twice: the
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
use crate::authorship::{self, Origin, agent_name, lines, matches};
use crate::collisions::HANDLED;
use crate::{Result, bail, fixture, jsonl, read};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const FALINKS: &str = "mcp__falinks__falinks_";
pub(crate) const SOURCE_WRITES: [&str; 4] = ["edit", "retry", "incorporate", "apply_job"];

/// A round-trip's category: the first of its calls' categories, in this order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Category {
    Edit,
    RefusedEdit,
    Native,
    Coordination,
}
const CATEGORIES: [(Category, &str); 4] = [
    (Category::Edit, "edit"),
    (Category::RefusedEdit, "refused_edit"),
    (Category::Native, "native"),
    (Category::Coordination, "coordination"),
];
/// Falinks mutations that take output forms, and the forms (#193 decision 1), as
/// `adapters/claude/arm.rs` counts them; its `tests/arm.rs` checks they agree.
const EDITS: [&str; 3] = ["edit", "retry", "incorporate"];
const FORMS: [&str; 3] = ["content", "replace", "from"];
/// #193's fit: a round-trip's model time grows 3.56 ms per emitted character.
const SECONDS_PER_CHARACTER: f64 = 0.00356;

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
    /// Its result's text.
    output: String,
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

/// A Git command that integrates (#337): `git merge`, `rebase` or `cherry-pick`, or a
/// `checkout` naming `--theirs` or `--ours`. A heuristic: the verb follows `git` and its
/// global options; `merge-base` and plain checkouts do not integrate.
fn integrates(command: &str) -> bool {
    let words = words(command);
    (0..words.len()).filter(|&i| words[i] == "git").any(|i| {
        let mut j = i + 1;
        while words.get(j).is_some_and(|w| w.starts_with('-')) {
            j += if matches!(words[j], "-C" | "-c") {
                2
            } else {
                1
            };
        }
        match words.get(j) {
            Some(&("merge" | "rebase" | "cherry-pick")) => true,
            Some(&"checkout") => command.contains("--theirs") || command.contains("--ours"),
            _ => false,
        }
    })
}

/// The source files a call writes, scratch excluded.
fn source_writes(call: &Call, bash_writes_source: bool) -> Vec<String> {
    let writes = writes(call, bash_writes_source).into_iter();
    writes
        .filter(|(path, _, source)| *source && !scratch(path))
        .map(|(path, _, _)| path)
        .collect()
}

fn host(call: &Call) -> bool {
    call.name == "Bash" && call.input.to_string().contains("FALINKS_HOST")
}

fn category(call: &Call, bash_writes_source: bool) -> Category {
    let refused = call.result.is_some_and(|(_, refused)| refused);
    let write = if refused {
        Category::RefusedEdit
    } else {
        Category::Edit
    };
    match call.name.strip_prefix(FALINKS) {
        Some(op) if SOURCE_WRITES.contains(&op) => write,
        Some(_) => Category::Coordination,
        None if !source_writes(call, bash_writes_source).is_empty() => write,
        None if host(call) => Category::Coordination,
        None => Category::Native,
    }
}

/// A Git integration act (#337): a write under `agents/integration`, or an integrating command
/// that does not name the host. Never in Falinks, whose shell writes no source.
fn integration(call: &Call, bash_writes_source: bool) -> bool {
    let source = source_writes(call, bash_writes_source);
    let command = call.input["command"].as_str().unwrap_or_default();
    // A here-document after `cd` into the integration worktree names a relative path.
    let write = !source.is_empty()
        && (command.contains("agents/integration")
            || source.iter().all(|p| p.contains("agents/integration/")));
    let merge = call.name == "Bash" && !host(call) && integrates(command);
    bash_writes_source && !call.name.starts_with(FALINKS) && (write || merge)
}

/// A write Claude Code itself refused by its read-before-write rule (#205 D1), not the engine
/// or the host: its cost is the runtime's, not coordination's.
fn runtime_refused(call: &Call) -> bool {
    let rule = [
        "File has not been read yet",
        "File has been modified since read",
    ];
    matches!(call.name.as_str(), "Write" | "Edit")
        && call.result.is_some_and(|(_, refused)| refused)
        && rule.iter().any(|text| call.output.contains(text))
}

fn scratch(path: &str) -> bool {
    path.split('/').any(|part| part == "scratch")
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

/// Whether a transcript path, or a word of a command, names `file`, a path from the project
/// root (`git show A:file` included).
fn names(path: &str, file: &str) -> bool {
    path == file || path.ends_with(&format!("/{file}")) || path.ends_with(&format!(":{file}"))
}

/// A command's words, split at blanks, shell punctuation and quotes.
fn words(command: &str) -> Vec<&str> {
    let split = |c: char| c.is_whitespace() || ";&|()`'\"<>".contains(c);
    command.split(split).filter(|w| !w.is_empty()).collect()
}

/// A read of `file` (#205, Q7c): `Read`, or a `Bash` command naming it with `cat`, `head`,
/// `tail`, `sed -n`, `git diff` or `git show`.
fn reads(call: &Call, file: &str) -> bool {
    let command = call.input["command"].as_str().unwrap_or_default();
    match call.name.as_str() {
        "Read" => names(call.input["file_path"].as_str().unwrap_or_default(), file),
        "Bash" => {
            let words = words(command);
            let form = words.iter().any(|w| matches!(*w, "cat" | "head" | "tail"))
                || ["sed -n", "git diff", "git show"]
                    .iter()
                    .any(|f| command.contains(f));
            form && words.iter().any(|w| names(w, file))
        }
        _ => false,
    }
}

/// A host result's JSON lines: a Git `Bash` command may call the host more than once.
fn results(call: &Call) -> Vec<Value> {
    let lines = call.output.lines();
    lines.filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// A Falinks check's result: passed or not, and its revision.
fn falinks_check(call: &Call) -> Option<(bool, Option<u64>)> {
    if call.name != format!("{FALINKS}check") {
        return None;
    }
    let result = results(call).into_iter().next()?;
    Some((result["passed"].as_bool()?, result["revision"].as_u64()))
}

/// Whether a call resolves a collision's handling.
type Resolves<'a> = Box<dyn Fn(&Call) -> bool + 'a>;

/// One collision class's handling cost in one agent (#337).
#[derive(Default)]
struct Cost {
    round_trips: u64,
    seconds: f64,
    unresolved: u64,
    unlocated: u64,
}

/// Where a collision signal reached one agent and the round-trips that handled it (#337).
struct Window {
    /// The first round-trip after the signal.
    start: usize,
    /// The round-trips up to and including the resolving one.
    held: Vec<usize>,
    resolved: bool,
    /// The round-trips that reread a file the collision names.
    rereads: Vec<usize>,
}

/// Per file in `files`, the round-trip of the agent's first read of it from round-trip `start`,
/// unless an accepted write of it comes first (#205, Q7c).
fn rereads<'a>(
    trips: &[&Message],
    calls: &[Call],
    git: bool,
    start: usize,
    files: impl IntoIterator<Item = &'a str>,
) -> Vec<usize> {
    let mut out = vec![];
    for file in files {
        let touch = (start..trips.len()).find_map(|k| {
            trips[k].calls.iter().find_map(|&c| {
                // A refused write changes nothing: the window stays open.
                let write = calls[c].result.is_some_and(|(_, refused)| !refused)
                    && writes(&calls[c], git)
                        .iter()
                        .any(|(path, _, _)| !scratch(path) && names(path, file));
                let read = reads(&calls[c], file);
                (read || write).then_some((k, read))
            })
        });
        if let Some((k, true)) = touch {
            out.push(k);
        }
    }
    out
}

/// The files a refused Falinks write's `Stale` names: its result's `conflicts`.
fn stale_files(call: &Call) -> Option<Vec<String>> {
    let edit = call
        .name
        .strip_prefix(FALINKS)
        .is_some_and(|op| EDITS.contains(&op));
    let result = results(call).into_iter().next()?;
    let conflicts = result["outcome"]["Stale"]["conflicts"].as_object()?;
    edit.then(|| conflicts.keys().cloned().collect())
}

/// A collision signal's handling window in this agent's round-trips; None when the transcript
/// lacks the signalling call. A rework detected by the peer's share (`peer_diff`) gets none
/// (see `agent`).
///
/// - Falinks `stale` and `incorporate` (`request_id`): `refusal_recovery`'s window, from the
///   refused call to the next accepted edit, retry or incorporate. An accepted `incorporate`
///   resolves its own collision, so its round-trip is the handling.
/// - Falinks `broken_check` (`revision`): from the failed check to the next accepted source
///   write or passing check.
/// - Git `merge_conflict`, `check_conflict` and `broken_check` (`at`, seconds since the epoch):
///   from the first round-trip whose input arrived at or after the signal to the next share or
///   passing check.
///
/// Its rereads follow a `stale` or a `merge_conflict` only, of the collision's files.
fn window(trips: &[&Message], calls: &[Call], git: bool, signal: &Value) -> Option<Window> {
    let position = |c: usize| trips.iter().position(|m| m.calls.contains(&c));
    let accepted = |c: &Call, ops: &[&str]| {
        c.result.is_some_and(|(_, refused)| !refused)
            && c.name
                .strip_prefix(FALINKS)
                .is_some_and(|op| ops.contains(&op))
    };
    let kind = signal["signal"].as_str().unwrap_or_default();
    let (start, end): (usize, Resolves) = match kind {
        "stale" | "incorporate" => {
            let id = signal["request_id"].as_str()?;
            let c = (0..calls.len()).find(|&c| {
                accepted(&calls[c], &EDITS) == (kind == "incorporate")
                    && calls[c]
                        .name
                        .strip_prefix(FALINKS)
                        .is_some_and(|op| EDITS.contains(&op))
                    && request(&calls[c])["id"] == id
            })?;
            let after = usize::from(kind == "stale");
            (position(c)? + after, Box::new(|c| accepted(c, &EDITS)))
        }
        _ if !git => {
            let revision = signal["revision"].as_u64();
            let c =
                (0..calls.len()).find(|&c| falinks_check(&calls[c]) == Some((false, revision)))?;
            let end = |c: &Call| {
                accepted(c, &SOURCE_WRITES) || falinks_check(c).is_some_and(|(passed, _)| passed)
            };
            (position(c)? + 1, Box::new(end))
        }
        _ => {
            // The transcript's clock is the millisecond; the signal's is a float sum.
            let at = signal["at"].as_f64()? - 0.0005;
            let start = trips.iter().position(|m| m.input.is_some_and(|i| i >= at));
            let end = move |c: &Call| {
                host(c)
                    && results(c).iter().any(|r| {
                        r["type"] == "shared"
                            || (r["type"] == "check_result" && r["passed"] == true)
                    })
            };
            (start.unwrap_or(trips.len()), Box::new(end))
        }
    };
    let resolving = (start..trips.len()).find(|&k| trips[k].calls.iter().any(|&c| end(&calls[c])));
    let held: Vec<usize> = (start..resolving.map_or(trips.len(), |k| k + 1)).collect();
    let files = signal["files"].as_array().into_iter().flatten();
    let files = files.filter_map(Value::as_str);
    let rereads = match kind {
        "stale" | "merge_conflict" => rereads(trips, calls, git, start, files),
        _ => vec![],
    };
    Some(Window {
        start,
        held,
        resolved: resolving.is_some(),
        rereads,
    })
}

/// One agent's figures from its stream-json events, sessions concatenated, with the collision
/// signals that reached it (`collisions`' detections, each with its collision's `class` and
/// `files`; Git's with `at`, seconds since the epoch). None when the collision records are
/// unreadable: every collision figure is then missing.
fn agent(
    events: &[Value],
    bash_writes_source: bool,
    clock: (Option<f64>, Option<f64>),
    signals: Option<&[Value]>,
) -> Value {
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
                        call.output = text(block);
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
                            output: String::new(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    let model = |m: &Message| m.input.map_or(0.0, |input| m.last - input);
    let trips: Vec<&Message> = messages.iter().filter(|m| !m.calls.is_empty()).collect();
    let kinds: Vec<Category> = trips
        .iter()
        .map(|m| {
            let kinds = m
                .calls
                .iter()
                .map(|&c| category(&calls[c], bash_writes_source));
            kinds.min().unwrap()
        })
        .collect();
    let mut categories = [(0, 0.0); CATEGORIES.len()];
    for (message, &kind) in trips.iter().zip(&kinds) {
        categories[kind as usize].0 += 1;
        categories[kind as usize].1 += model(message);
    }
    let mut round_trips: Map<String, Value> = CATEGORIES
        .iter()
        .zip(categories)
        .map(|((_, name), (count, seconds))| {
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

    let trips_calls = |k: usize| trips[k].calls.iter().copied();
    // Integration (#337, a heuristic) and runtime refusals (#205 D1): figures of their own,
    // beside the categories, which stay as they were.
    let holding = |f: &dyn Fn(&Call) -> bool| -> BTreeSet<usize> {
        let all = 0..trips.len();
        all.filter(|&k| trips_calls(k).any(|c| f(&calls[c])))
            .collect()
    };
    let integrating = holding(&|c| integration(c, bash_writes_source));
    let runtime = holding(&runtime_refused);

    // Collision handling (#337): each round-trip charged to the first window that holds it.
    let handling = signals.map(|signals| {
        let mut located = vec![];
        let mut classes: BTreeMap<&str, Cost> = HANDLED.map(|c| (c, Cost::default())).into();
        // A rework detected by the peer's share has no window: its time is the share's, not
        // the agent's notice (#205, "Measures"); its waste stays.
        let signals = signals.iter().filter(|s| s["signal"] != "peer_diff");
        for signal in signals {
            let class = signal["class"].as_str().unwrap_or_default();
            let Some(cost) = classes.get_mut(class) else {
                continue;
            };
            match window(&trips, &calls, bash_writes_source, signal) {
                Some(found) => located.push((class, found)),
                None => cost.unlocated += 1,
            }
        }
        located.sort_by_key(|(_, found)| found.start);
        let mut charged = BTreeSet::new();
        for (class, found) in located {
            let cost = classes.get_mut(class).unwrap();
            cost.unresolved += u64::from(!found.resolved);
            for k in found.held.into_iter().chain(found.rereads) {
                if charged.insert(k) {
                    cost.round_trips += 1;
                    cost.seconds += model(trips[k]);
                }
            }
        }
        classes
    });
    // Rereads (Q7c): after any Falinks `Stale` naming the file, a collision or not, or a Git
    // `CONFLICT (` collision signal.
    let rereads: Option<BTreeSet<usize>> = signals.map(|signals| {
        let mut out = BTreeSet::new();
        if bash_writes_source {
            let conflicts = signals.iter().filter(|s| s["signal"] == "merge_conflict");
            for signal in conflicts {
                let found = window(&trips, &calls, true, signal);
                out.extend(found.into_iter().flat_map(|w| w.rereads));
            }
        } else {
            for k in 0..trips.len() {
                for files in trips_calls(k).filter_map(|c| stale_files(&calls[c])) {
                    let files = files.iter().map(String::as_str);
                    out.extend(rereads(&trips, &calls, false, k + 1, files));
                }
            }
        }
        out
    });
    // Coordination overhead (#205, Q7b): refused edits, coordination, integration and rereads,
    // each round-trip once, less the runtime's own refusals (#205 D1).
    let overhead: Option<BTreeSet<usize>> = rereads.as_ref().map(|rereads| {
        let protocol =
            |k: &usize| matches!(kinds[*k], Category::RefusedEdit | Category::Coordination);
        let counted = (0..trips.len())
            .filter(protocol)
            .chain(integrating.iter().copied());
        counted
            .chain(rereads.iter().copied())
            .filter(|k| !runtime.contains(k))
            .collect()
    });
    // Inline reviews ride inside accepted edits; an overhead round-trip already holds its own.
    let characters: usize = (0..trips.len())
        .filter(|&k| {
            kinds[k] == Category::Edit && !overhead.as_ref().is_some_and(|o| o.contains(&k))
        })
        .flat_map(trips_calls)
        .map(|c| &calls[c])
        .filter(|c| c.name.starts_with(FALINKS))
        .map(|c| request(c)["reviews"].clone())
        .filter(|reviews| reviews.as_array().is_some_and(|r| !r.is_empty()))
        .map(|reviews| reviews.to_string().chars().count())
        .sum();
    let review_seconds = characters as f64 * SECONDS_PER_CHARACTER;
    // From 0.0: an empty f64 sum is -0.0.
    let trips_seconds = |set: &BTreeSet<usize>| set.iter().fold(0.0, |s, &k| s + model(trips[k]));
    let figure = |set: &BTreeSet<usize>| json!({"round_trips": set.len(), "model_seconds": rounded(trips_seconds(set))});
    let handling = handling.map(|classes| {
        let classes = classes.iter().map(|(class, cost)| {
            let cost = json!({"round_trips": cost.round_trips, "model_seconds": rounded(cost.seconds),
                "unresolved": cost.unresolved, "unlocated": cost.unlocated});
            (class.to_string(), cost)
        });
        Value::Object(classes.collect())
    });
    let overhead = overhead.map(|set| {
        let seconds = trips_seconds(&set)
            + if bash_writes_source {
                0.0
            } else {
                review_seconds
            };
        json!({"round_trips": set.len(), "model_seconds": rounded(seconds)})
    });
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
        "integration": figure(&integrating),
        "runtime_refusals": figure(&runtime),
        "collision_handling": handling,
        "rereads": rereads.as_ref().map(figure),
        "coordination_overhead": overhead,
    });
    // A Git agent makes no Falinks call, so its figures stay as they were before #316.
    if !bash_writes_source {
        figures["review_estimate"] =
            json!({"characters": characters, "model_seconds": rounded(review_seconds)});
        figures["operations"] = operations.into();
        figures["edit_gate"] = json!({"accepted": accepted, "seconds": rounded(gate_seconds),
            "per_accepted_seconds": (accepted > 0).then(|| rounded(gate_seconds / accepted as f64))});
    }
    figures
}

/// Per agent, the changed bytes of its last version of every file it changed that it
/// introduced itself, read from the arm's records (#325), not the transcript, with
/// `authorship`'s line model.
///
/// - A Falinks agent's last version of a file is its last applied operation on it in
///   `controller/history.json`.
/// - A Git agent's last version is its last shared commit (`controller/events.jsonl`) in
///   `agents/repo`; work it never shared counts nothing.
fn changed(run: &Path, arm: &str, files: &Map<String, Value>) -> Result<BTreeMap<String, usize>> {
    let (blame, last) = if arm == "git" {
        let shares = authorship::shares(run)?;
        let mut repo = authorship::Repo::new(run, &shares, files)?;
        let mut last = BTreeMap::new();
        let last_shares: BTreeMap<String, String> = shares
            .into_iter()
            .map(|(agent, share, _)| (agent, share))
            .collect();
        for (agent, share) in last_shares {
            for (path, text) in repo.tree(&share)? {
                last.insert((agent.clone(), path.clone()), text.clone());
            }
        }
        (repo.blame, last)
    } else {
        let history = read(run.join("controller/history.json"))?;
        let records = history
            .as_array()
            .ok_or("history.json must list operations")?;
        authorship::falinks(records, files)
    };
    let mut changed = BTreeMap::new();
    for ((agent, path), text) in last {
        let initial = files.get(&path).and_then(Value::as_str).unwrap_or("");
        let origins = blame.lines(&path, &text);
        // A changed line traced to the initial file is the holder's: no peer wrote it.
        let own: usize = changed_lines(initial, &text)
            .into_iter()
            .filter(|(j, _)| match &origins[*j].origin {
                Origin::Initial => true,
                Origin::Agent(author, _) => *author == agent,
                Origin::Nobody => false,
            })
            .map(|(_, line)| line.len())
            .sum();
        *changed.entry(agent).or_insert(0) += own;
    }
    Ok(changed)
}

/// Per-agent timing for one run directory of either arm, with the handling cost of the run's
/// `collisions` (`collisions::collisions`; unavailable leaves it missing).
pub fn timing(run: &Path, collisions: &Value) -> Result<Value> {
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
                (agent_name(n), events)
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
    // Each agent's collision signals; `collisions` keys Git's by seconds after `start_unix`.
    let origin = timing["start_unix"].as_f64().unwrap_or(0.0);
    let readable = matches!(
        collisions["status"].as_str(),
        Some("complete" | "partial" | "n/a")
    );
    let mut signals = BTreeMap::<String, Vec<Value>>::new();
    for event in collisions["events"].as_array().into_iter().flatten() {
        for detected in event["detected"].as_array().into_iter().flatten() {
            let mut signal = detected.clone();
            signal["class"] = event["class"].clone();
            signal["files"] = event["files"].clone();
            if let Some(elapsed) = detected["elapsed"].as_f64() {
                signal["at"] = json!(origin + elapsed);
            }
            let agent = detected["agent"].as_str().unwrap_or_default().to_string();
            signals.entry(agent).or_default().push(signal);
        }
    }
    let mut figures = Map::new();
    for (name, events) in &agents {
        let mine = readable.then(|| signals.get(name).map_or(&[][..], Vec::as_slice));
        let mut figure = agent(events, arm == "git", (start, stop), mine);
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
        let figures = agent(&events, false, (None, None), None);
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
            agent(&events, false, (None, None), None)
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
        let figures = agent(&events, false, (None, None), None);
        assert_eq!(
            figures["request_key_refusals"],
            json!({"ack": 1, "handle": 2})
        );
    }

    /// Events of one session: call `n` alone in message `mn`, emitted at second n + 0.5 and
    /// answered at n + 1 with its `output` (`ok` by default), so each round-trip's model time
    /// is 0.5 s.
    fn session(calls: &[Value]) -> Vec<Value> {
        let at = |s: f64| json!(utc(s));
        let mut events = vec![
            json!({"type": "system", "subtype": "init"}),
            json!({"type": "user", "timestamp": at(0.0), "message": {"content": "go"}}),
        ];
        for (n, call) in calls.iter().enumerate() {
            let id = format!("m{n}");
            let (name, input) = (call["name"].clone(), call["input"].clone());
            let output = call["output"].as_str().unwrap_or("ok");
            events.push(json!({"type": "assistant", "timestamp": at(n as f64 + 0.5),
                "message": {"id": id, "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}}));
            events.push(json!({"type": "user", "timestamp": at(n as f64 + 1.0), "message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "content": output, "is_error": call["is_error"] == true}]}}));
        }
        events
    }

    /// `2026-10-21T10:00:00Z` plus `seconds`, under a minute.
    fn utc(seconds: f64) -> String {
        format!("2026-10-21T10:00:{seconds:06.3}Z")
    }

    fn bash(command: &str) -> Value {
        json!({"name": "Bash", "input": {"command": command}})
    }

    /// Handling cost (#205, "Measures"): from a collision's detection to its resolution, each
    /// round-trip charged to the first collision whose window holds it. Falinks: a `Stale`
    /// refusal's recovery to the next accepted edit, an `incorporate` its own round-trip, a
    /// failed check to the next accepted write or passing check. Git: from the signal's time to
    /// the next share or passing check; a rework the peer's share detected has none. A reread is
    /// the first read of a file a Falinks `Stale` (a collision or not) or a Git `CONFLICT (`
    /// named, before the next accepted write of it; a collision's counts in its handling cost.
    /// Coordination overhead: `refused_edit`, `coordination`, integration and rereads, less the
    /// runtime's own refusals, and for Falinks inline reviews at 3.56 ms per character.
    #[test]
    fn collision_handling_runs_from_detection_to_resolution() {
        let op = |op: &str, input: Value, output: Value| {
            json!({"name": format!("{FALINKS}{op}"), "input": input, "output": output.to_string(),
                "is_error": output["accepted"] == false})
        };
        let stale = |file: &str| json!({"accepted": false, "outcome": {"Stale": {"conflicts": {file: {"expected": 0}}}}});
        let reviews = json!([{"scope": "s", "revision": 1, "decision": "Keep", "note": "ok"}]);
        let calls = [
            op(
                "edit",
                json!({"id": "e1", "output": {"h.go": {"replace": [{"old": "a", "new": "b"}]}}}),
                stale("h.go"),
            ),
            json!({"name": "Read", "input": {"file_path": "/w/space0/h.go"}}),
            bash("cat /w/space0/h.go"),
            op(
                "edit",
                json!({"id": "e2", "output": {"h.go": {"content": "b\n"}}, "reviews": reviews}),
                json!({"accepted": true}),
            ),
            op(
                "check",
                json!({}),
                json!({"accepted": true, "passed": false, "revision": 3}),
            ),
            op("post", json!({"text": "fixing"}), json!({"accepted": true})),
            op(
                "check",
                json!({}),
                json!({"accepted": true, "passed": true, "revision": 4}),
            ),
            op(
                "incorporate",
                json!({"id": "i1", "output": {"h.go": {"content": "c\n"}}}),
                json!({"accepted": true}),
            ),
            // A `Stale` that is no collision still leaves its file's next read a reread.
            op(
                "edit",
                json!({"id": "e3", "output": {"g.go": {"content": "g\n"}}}),
                stale("g.go"),
            ),
            json!({"name": "Read", "input": {"file_path": "/w/space0/g.go"}}),
            op(
                "edit",
                json!({"id": "e4", "output": {"g.go": {"content": "g\n"}}}),
                json!({"accepted": true}),
            ),
        ];
        let signals = [
            json!({"class": "line_conflict", "files": ["h.go"], "signal": "stale", "request_id": "e1"}),
            json!({"class": "rework", "files": ["h.go"], "signal": "broken_check", "check": 1, "revision": 3}),
            json!({"class": "line_conflict", "files": ["h.go"], "signal": "incorporate", "request_id": "i1"}),
            json!({"class": "no_fault", "files": [], "signal": "stale", "request_id": "unseen"}),
        ];
        let falinks = agent(&session(&calls), false, (None, None), Some(&signals));
        let cost = |trips: u64, seconds: f64, unresolved: u64, unlocated: u64| json!({"round_trips": trips, "model_seconds": seconds, "unresolved": unresolved, "unlocated": unlocated});
        assert_eq!(
            falinks["collision_handling"],
            json!({
                // m1, m2 and m3 after e1's refusal; i1's own round-trip, m7.
                "line_conflict": cost(4, 2.0, 0, 0),
                // m5 and m6 after the failed check of revision 3.
                "rework": cost(2, 1.0, 0, 0),
                "no_fault": cost(0, 0.0, 0, 1),
            })
        );
        // m1's Read of h.go (m2's cat is not the first read) and m9's of g.go.
        assert_eq!(
            falinks["rereads"],
            json!({"round_trips": 2, "model_seconds": 1.0})
        );
        // m3's reviews: 58 characters as compact JSON, 0.206 s.
        assert_eq!(
            falinks["review_estimate"],
            json!({"characters": 58, "model_seconds": 0.206})
        );
        // Engine refusals stay: m0 and m8 refused, m4-m6 coordination, m1 and m9 rereads: 7
        // round-trips, 3.5 s plus the estimate.
        assert_eq!(
            falinks["coordination_overhead"],
            json!({"round_trips": 7, "model_seconds": 3.706})
        );
        assert_eq!(
            falinks["runtime_refusals"],
            json!({"round_trips": 0, "model_seconds": 0.0})
        );

        let shared = r#"{"commit":"c","type":"shared"}
{"action":"message","type":"ok"}"#;
        let calls = [
            json!({"name": "Bash", "input": {"command": "cd /r/agents/integration && git merge --no-edit b1"},
                "output": "CONFLICT (content): Merge conflict in h.go\nAutomatic merge failed"}),
            bash("cd /r/agents/integration && sed -n 1,40p h.go"),
            json!({"name": "Edit", "input": {"file_path": "/r/agents/integration/h.go",
                "old_string": "a", "new_string": "b"}}),
            json!({"name": "Bash", "input": {"command": "\"$FALINKS_HOST\" check abc"},
                "output": r#"{"passed":false,"type":"check_result"}"#}),
            json!({"name": "Bash", "input": {"command": "\"$FALINKS_HOST\" check def"},
                "output": r#"{"candidate":"def","conflict":null,"passed":true,"type":"check_result"}"#}),
            json!({"name": "Write", "input": {"file_path": "/r/agents/A/h.go", "content": "x\n"}}),
            json!({"name": "Bash", "input": {"command": "\"$FALINKS_HOST\" share; \"$FALINKS_HOST\" message hi"},
                "output": shared}),
            // Claude Code's own read-before-write refusal (#205 D1).
            json!({"name": "Write", "input": {"file_path": "/r/agents/A/g.go", "content": "g\n"},
                "is_error": true, "output": "<tool_use_error>File has not been read yet. Read it first before writing to it.</tool_use_error>"}),
        ];
        let at = |s: f64| unix(&utc(s)).unwrap();
        let signals = [
            json!({"class": "line_conflict", "files": ["h.go"], "signal": "merge_conflict", "at": at(1.0)}),
            json!({"class": "rework", "files": ["h.go"], "signal": "broken_check", "check": 1, "at": at(4.0)}),
            json!({"class": "rework", "files": ["h.go"], "signal": "peer_diff", "at": at(5.25)}),
            json!({"class": "no_fault", "files": ["h.go"], "signal": "broken_check", "check": 2, "at": at(20.0)}),
        ];
        let git = agent(&session(&calls), true, (None, None), Some(&signals));
        assert_eq!(
            git["collision_handling"],
            json!({
                // m1 to the passing check m4.
                "line_conflict": cost(4, 2.0, 0, 0),
                // The failed check's window, m4, is the line conflict's; the peer's share at
                // 5.25 opens none.
                "rework": cost(0, 0.0, 0, 0),
                // Nothing follows: unresolved.
                "no_fault": cost(0, 0.0, 1, 0),
            })
        );
        // m1's `sed -n` names h.go.
        assert_eq!(
            git["rereads"],
            json!({"round_trips": 1, "model_seconds": 0.5})
        );
        // m0 and m2 integrate, beside their categories (native, edit).
        assert_eq!(
            git["integration"],
            json!({"round_trips": 2, "model_seconds": 1.0})
        );
        // m7's runtime refusal stays in refused_edit but leaves coordination overhead.
        assert_eq!(
            git["round_trips"]["refused_edit"],
            json!({"count": 1, "model_seconds": 0.5})
        );
        assert_eq!(
            git["runtime_refusals"],
            json!({"round_trips": 1, "model_seconds": 0.5})
        );
        // m0, m2 integration; m3, m4, m6 coordination; m1 a reread.
        assert_eq!(
            git["coordination_overhead"],
            json!({"round_trips": 6, "model_seconds": 3.0})
        );
        assert!(git.get("review_estimate").is_none());
        // Unreadable collision records leave every collision figure missing, never zero.
        let unknown = agent(&session(&calls), true, (None, None), None);
        for key in ["collision_handling", "rereads", "coordination_overhead"] {
            assert_eq!(unknown[key], Value::Null, "{key}");
        }
    }

    /// Only an accepted write ends a file's reread window: Claude Code refuses an `Edit` of a
    /// file not yet read (as in #249's `go-page-2-git`), and the read that follows is a reread.
    #[test]
    fn a_refused_write_leaves_the_next_read_a_reread() {
        let rereads = |write_refused: bool| {
            let calls = [
                json!({"name": "Bash", "input": {"command": "git merge b1"},
                    "output": "CONFLICT (content): Merge conflict in h.go"}),
                json!({"name": "Edit", "input": {"file_path": "/r/agents/integration/h.go",
                    "old_string": "a", "new_string": "b"}, "is_error": write_refused,
                    "output": "File has not been read yet."}),
                json!({"name": "Read", "input": {"file_path": "/r/agents/integration/h.go"}}),
            ];
            let at = unix(&utc(1.0)).unwrap();
            let signal = json!({"class": "line_conflict", "files": ["h.go"], "signal": "merge_conflict", "at": at});
            agent(&session(&calls), true, (None, None), Some(&[signal]))["rereads"].clone()
        };
        assert_eq!(
            rereads(true),
            json!({"round_trips": 1, "model_seconds": 0.5})
        );
        assert_eq!(
            rereads(false),
            json!({"round_trips": 0, "model_seconds": 0.0})
        );
    }

    /// #205 finding 6: Git merges and conflict fixes were `native` (or `edit`). Integration is
    /// a figure beside the categories, which stay as they were: merge, rebase, cherry-pick,
    /// `checkout --theirs/--ours`, and writes under `agents/integration`. A command naming the
    /// host does not integrate.
    #[test]
    fn git_integration_is_a_figure_beside_the_categories() {
        let calls = [
            bash("cd /r/agents/integration && git merge --no-edit abc 2>&1 | tail -5"),
            bash("git -C /r/agents/integration rebase main"),
            bash("git cherry-pick abc"),
            bash("cd /r/agents/integration && git checkout --theirs h.go"),
            json!({"name": "Edit", "input": {"file_path": "/r/agents/integration/h.go",
                "old_string": "a", "new_string": "b"}}),
            bash("cd /r/agents/integration && cat > h.go <<'EOF'\nx\nEOF"),
            // Not an integration verb, or a write in the agent's own worktree.
            bash("git merge-base a b; git checkout h.go"),
            json!({"name": "Write", "input": {"file_path": "/r/agents/A/h.go", "content": "x\n"}}),
            bash("\"$FALINKS_HOST\" ack peer-drafts; git checkout --ours h.go"),
        ];
        let figures = agent(&session(&calls), true, (None, None), None);
        assert_eq!(
            figures["integration"],
            json!({"round_trips": 6, "model_seconds": 3.0})
        );
        let trips = &figures["round_trips"];
        assert!(trips.get("integration").is_none());
        // m0-m3 and m6 native; m4, m5 and m7 edits; m8 coordination.
        assert_eq!(trips["native"], json!({"count": 5, "model_seconds": 2.5}));
        assert_eq!(trips["edit"], json!({"count": 3, "model_seconds": 1.5}));
        assert_eq!(
            trips["coordination"],
            json!({"count": 1, "model_seconds": 0.5})
        );
        assert_eq!(trips["total"]["count"], 9);
        // A Falinks agent's shell never integrates.
        let falinks = agent(&session(&calls[..1]), false, (None, None), None);
        assert_eq!(falinks["integration"]["round_trips"], 0);
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
