//! Collisions and collision waste (#334; #205's resolution, "Measures"), read from one run
//! directory the same way in both arms.
//!
//! - A **collision**: both agents change the same file region, or both implement the same
//!   requirement, before their work is combined. A **region** is a run of changed lines of the
//!   version both sides started from; two regions are the same when they overlap or touch, Git's
//!   own conflict rule. Each collision is classed by its outcome: `line_conflict`,
//!   `duplicate_work`, `rework` or `no_fault`.
//! - **Collision waste**: lines an agent wrote and later discarded or rewrote because of a
//!   collision, plus final-tree lines that duplicate the peer's lines for the same requirement.
//!   Lines a gate stopped count zero. Each written line counts once per run.
//! - **Falinks** (`controller/history.json`): a `Stale` refusal against a file version the peer
//!   wrote, whose intended change touches the peer's, is a line conflict detected by that
//!   refusal; its waste is the peer's lines the agent's next write of the file removed. An
//!   `incorporate` (split workspaces) is a line conflict resolved by that operation: the
//!   agent's own lines it removed, and the lines of the peer's latest version it dropped.
//! - **Git** (`draft_shared` events, `agents/repo`): a pair of shares whose merge conflicts is a
//!   line conflict, detected by a `CONFLICT (` line in an agent's transcript or a host check's
//!   conflict; its waste is each side's lines its resolving merge commit dropped. A share that
//!   removes the agent's own lines in a region the peer's latest share changed is rework,
//!   detected by that peer share.
//! - **Broken combined check**, both arms: a failed check holding both agents' work. When lines
//!   it held were removed before the next passing check, it is rework; otherwise it is a
//!   premature check (not a collision) when work was added before that pass, else no fault.
//! - **Duplicate work**: Daniel's classifications of the pairs `duplicates` proposes: one
//!   function of each agent with one name, in files one requirement names, or with similar
//!   bodies. A pair's waste is the later copy's lines when the final tree holds both, else the
//!   lines of each copy written and then discarded; a copy only in a refused request counts 0.
//!
//! Each collision lists its files and where each agent detected it, for #337's handling cost.
//! Lines are credited by `authorship`, the model `timing`'s changed bytes use, and each
//! written line counts once per run.
use crate::authorship::{self, Blame, Line, agent_name, applied, lines, matches, text};
use crate::{
    Result, bail, command, fixture, git, git_env, jsonl, read, sha256, strings, timing::unix,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

/// Frozen (#334): two functions whose token similarity reaches this are a duplicate-work
/// candidate. Similarity is 2 × the longest common token subsequence over both token counts.
/// It favours recall, as the classifier decides: on #249's twelve Go runs, 0.6 proposed 128
/// pairs, almost all tests of one table shape, and 0.9 dropped tests both agents wrote for
/// one development (0.74 to 0.86).
pub const SIMILARITY: f64 = 0.75;
pub const CLASSES: [&str; 4] = ["line_conflict", "duplicate_work", "rework", "no_fault"];
/// Written into `discarded_or_rewritten_work`.
const SOURCE: &str = "collisions (#334): falinks-eval collisions RUN_DIR";

/// The lines of `old` (0-based) that `new` does not keep, by `authorship`'s line diff.
fn dropped(old: &[&str], new: &[&str]) -> Vec<usize> {
    let kept: BTreeSet<usize> = matches(old, new).into_iter().flatten().collect();
    (0..old.len()).filter(|i| !kept.contains(i)).collect()
}

/// The fixture's initial files, `authorship`'s initial lines; none when it is unknown.
fn initial_files(result: &Value) -> Map<String, Value> {
    let fixture = fixture(result["fixture"].as_str().unwrap_or_default());
    fixture
        .ok()
        .and_then(|f| f["files"].as_object().cloned())
        .unwrap_or_default()
}

/// Lines of `lines[start..=end]` as one text, without the last line ending.
fn body(lines: &[&str], start: usize, end: usize) -> String {
    let text = lines[start..=end].concat();
    text.strip_suffix('\n').unwrap_or(&text).to_string()
}

type Region = (usize, usize);

/// The changed regions of `old` as [start, end) line ranges (an insertion is empty), and for
/// each line of `new` the region it was added in, or None when it is kept.
fn regions(old: &[&str], new: &[&str]) -> (Vec<Region>, Vec<Option<Region>>) {
    let (mut out, mut fills, mut start, mut pending) =
        (vec![], vec![None; new.len()], 0, Vec::new());
    for (j, kept) in matches(old, new)
        .into_iter()
        .chain([Some(old.len())])
        .enumerate()
    {
        let Some(i) = kept else {
            pending.push(j);
            continue;
        };
        if i > start || !pending.is_empty() {
            out.push((start, i));
            pending.drain(..).for_each(|p| fills[p] = Some((start, i)));
        }
        start = i + 1;
    }
    (out, fills)
}

fn touch(a: &[Region], b: &[Region]) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.0 <= y.1 && y.0 <= x.1))
}

/// One collision record; `lines` is its waste.
fn collision(
    class: &str,
    files: BTreeSet<String>,
    detected: Vec<Value>,
    waste: BTreeMap<String, u64>,
) -> Value {
    let lines: u64 = waste.values().sum();
    json!({"class": class, "files": files, "detected": detected, "waste": waste, "lines": lines})
}

/// A run's collisions, their waste and the duplicate-work candidates still unclassified, given
/// `classified`, the batch's `duplicates.json` (or null). A team of one has none (`n/a`); a run
/// whose records cannot be read is `unavailable`, never zero.
pub fn collisions(run: &Path, result: &Value, classified: &Value) -> Value {
    if result["team_size"].as_u64() == Some(1) {
        return json!({"status": "n/a", "reason": "a team of one has no peer"});
    }
    measure(run, result, classified)
        .unwrap_or_else(|e| json!({"status": "unavailable", "reason": e.to_string()}))
}

fn measure(run: &Path, result: &Value, classified: &Value) -> Result<Value> {
    let verdicts: BTreeMap<&str, bool> = classified["pairs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["id"].as_str()?, p["duplicate"].as_bool()?)))
        .collect();
    let requirements = requirements(result);
    let (mut events, premature, candidates, unclassified) = match result["arm"].as_str() {
        Some("falinks") => {
            let ledger = Ledger::read(run, result)?;
            let mut used = BTreeSet::new();
            let (mut events, premature) = ledger.collisions(result, &mut used);
            let functions = ledger.functions(result);
            let pairs = pairs(&functions, &requirements, result);
            let discard = |f: &Function| Ok(ledger.discard(f, &mut used));
            let unclassified =
                duplicate_events(&pairs, &functions, &verdicts, discard, &mut events)?;
            (events, premature, pairs.len(), unclassified)
        }
        Some("git") => {
            let mut repo = Repo::read(run, result)?;
            let (mut events, premature) = repo.collisions(run, result)?;
            let functions = repo.functions(result)?;
            let pairs = pairs(&functions, &requirements, result);
            let discard = |f: &Function| repo.discard(f);
            let unclassified =
                duplicate_events(&pairs, &functions, &verdicts, discard, &mut events)?;
            (events, premature, pairs.len(), unclassified)
        }
        other => bail!("unknown arm {other:?}"),
    };
    let status = if unclassified == 0 {
        "complete"
    } else {
        "partial"
    };
    let lines: u64 = events.iter().filter_map(|e| e["lines"].as_u64()).sum();
    let by_class: Map<String, Value> = CLASSES
        .iter()
        .map(|class| {
            let of: Vec<&Value> = events.iter().filter(|e| e["class"] == *class).collect();
            let lines: u64 = of.iter().filter_map(|e| e["lines"].as_u64()).sum();
            (
                class.to_string(),
                json!({"count": of.len(), "lines": lines}),
            )
        })
        .collect();
    events.sort_by_key(|e| CLASSES.iter().position(|c| e["class"] == *c));
    Ok(
        json!({"status": status, "events": events, "by_class": by_class,
        "premature_checks": premature, "lines": lines,
        "duplicates": {"candidates": candidates, "unclassified": unclassified}}),
    )
}

/// The run's `discarded_or_rewritten_work` from its collisions: the waste in lines, unknown
/// while a duplicate-work candidate is unclassified.
pub fn waste(collisions: &Value) -> Value {
    let status = collisions["status"].as_str().unwrap_or("unavailable");
    let mut out = json!({"status": status, "source": SOURCE});
    match status {
        "complete" => {
            out["lines"] = collisions["lines"].clone();
            out["by_class"] = collisions["by_class"].clone();
        }
        "partial" => {
            out["lines"] = Value::Null;
            out["counted_lines"] = collisions["lines"].clone();
            out["reason"] = format!(
                "{} duplicate-work candidates unclassified: run `falinks-eval duplicates`",
                collisions["duplicates"]["unclassified"]
            )
            .into();
        }
        _ => out["reason"] = collisions["reason"].clone(),
    }
    out
}

/// The duplicate-work candidate pairs of one run, unclassified; none for a team of one.
pub fn candidates(run: &Path, result: &Value) -> Result<Vec<Value>> {
    if result["team_size"].as_u64() == Some(1) {
        return Ok(vec![]);
    }
    let functions = match result["arm"].as_str() {
        Some("falinks") => Ledger::read(run, result)?.functions(result),
        Some("git") => Repo::read(run, result)?.functions(result)?,
        other => bail!("unknown arm {other:?}"),
    };
    let pairs = pairs(&functions, &requirements(result), result);
    Ok(pairs.into_iter().map(|(pair, ..)| pair).collect())
}

/// Kept in `duplicates.json` beside the pairs.
const DUPLICATES_NOTE: &str = "Duplicate-work candidates (#334): functions both agents added, for one requirement, with one name or with bodies at or above the similarity threshold. `duplicate` is the classifier's verdict, null until classified; `report` reads it. Pairs were shown in hash order across runs, without run, arm, agent or state.";

/// `duplicates --runs DIR`: proposes the candidate pairs of every scored run in the batch,
/// keeps the verdicts `DIR/duplicates.json` already holds, and asks for the rest one at a time,
/// in an order shuffled across runs, showing each side's file and body only. Each answer is
/// saved at once, so quitting keeps it.
pub fn classify(runs: &Path, mut input: impl BufRead, mut out: impl Write) -> Result<i32> {
    let ledger = read(runs.join("batch.json"))?;
    let path = runs.join("duplicates.json");
    let old = read(&path).unwrap_or(Value::Null);
    let verdicts: BTreeMap<&str, &Value> = old["pairs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["id"].as_str()?, &p["duplicate"])))
        .collect();
    let mut pairs = vec![];
    for entry in ledger["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["scored"] == true)
    {
        let run = Path::new(entry["run_dir"].as_str().unwrap_or_default());
        let Ok(mut result) = read(run.join("controller/result.json")) else {
            continue;
        };
        for key in ["run_id", "arm", "fixture", "team_size"] {
            if !entry[key].is_null() {
                result[key] = entry[key].clone();
            }
        }
        match candidates(run, &result) {
            Ok(found) => pairs.extend(found),
            Err(error) => writeln!(out, "skipped {}: {error}", entry["run_id"])?,
        }
    }
    for pair in &mut pairs {
        if let Some(verdict) = verdicts.get(pair["id"].as_str().unwrap_or_default()) {
            pair["duplicate"] = (*verdict).clone();
        }
    }
    let hash = |pair: &Value| sha256(pair["id"].as_str().unwrap_or_default().as_bytes());
    pairs.sort_by_key(hash);
    let save = |pairs: &[Value]| {
        crate::write(
            &path,
            &json!({"note": DUPLICATES_NOTE, "similarity_threshold": SIMILARITY, "pairs": pairs}),
        )
    };
    save(&pairs)?;
    let total = pairs.len();
    for k in 0..total {
        if pairs[k]["duplicate"].is_boolean() {
            continue;
        }
        let done = pairs.iter().filter(|p| p["duplicate"].is_boolean()).count();
        let mut sides = pairs[k]["sides"].as_array().cloned().unwrap_or_default();
        // The hash orders the sides too, so agent A is not always first.
        if hash(&pairs[k]).ends_with(['1', '3', '5', '7', '9', 'b', 'd', 'f']) {
            sides.reverse();
        }
        writeln!(out, "\nPair {} of {total} ({done} classified)", k + 1)?;
        for (label, side) in ["first", "second"].iter().zip(&sides) {
            let body = side["body"]
                .as_str()
                .unwrap_or_default()
                .replace('\t', "    ");
            writeln!(
                out,
                "--- {label}: {}\n{body}",
                side["file"].as_str().unwrap_or_default()
            )?;
        }
        write!(
            out,
            "Do both implement one requirement, so that one duplicates the other? [y]es, [n]o, [s]kip, [q]uit: "
        )?;
        out.flush()?;
        let mut answer = String::new();
        if input.read_line(&mut answer)? == 0 {
            break;
        }
        pairs[k]["duplicate"] = match answer.trim() {
            "y" | "yes" => true.into(),
            "n" | "no" => false.into(),
            "q" | "quit" => break,
            _ => continue,
        };
        save(&pairs)?;
    }
    let left = pairs
        .iter()
        .filter(|p| !p["duplicate"].is_boolean())
        .count();
    writeln!(
        out,
        "\n{} of {total} classified, {left} left; saved to {}",
        total - left,
        path.display()
    )?;
    Ok(0)
}

/// A function an agent added: its last lines (`at` is a Git commit or a Falinks file version)
/// and whether the final tree holds it (`final`), only an earlier write (`discarded`), or only
/// a refused request (`stopped`). `first` orders the agents' first writes of it.
struct Function {
    agent: String,
    file: String,
    name: String,
    body: String,
    state: &'static str,
    at: String,
    start: usize,
    end: usize,
    first: f64,
}

/// The top-level Go functions of `text`: name (`Type.Method` for a method), first and last line.
// shortcut: Go only, as #205's fixtures are; extract Rust `fn` items for a Rust fixture.
fn go_functions(path: &str, text: &str) -> Vec<(String, usize, usize)> {
    if !path.ends_with(".go") {
        return vec![];
    }
    let lines = lines(text);
    let (mut out, mut i) = (vec![], 0);
    while i < lines.len() {
        let Some(rest) = lines[i].strip_prefix("func ") else {
            i += 1;
            continue;
        };
        let (receiver, rest) = match rest.strip_prefix('(').and_then(|r| r.split_once(')')) {
            Some((receiver, rest)) => {
                let kind = receiver.split_whitespace().last().unwrap_or("");
                (
                    format!("{}.", kind.trim_start_matches('*')),
                    rest.trim_start(),
                )
            }
            None => (String::new(), rest),
        };
        let name = rest.split(['(', '[', ' ']).next().unwrap_or("");
        let end = if lines[i].trim_end().ends_with('}') {
            i
        } else {
            (i + 1..lines.len())
                .find(|&j| lines[j].starts_with('}'))
                .unwrap_or(lines.len() - 1)
        };
        out.push((format!("{receiver}{name}"), i, end));
        i = end + 1;
    }
    out
}

/// 2 × the longest common token subsequence over both token counts.
fn similarity(a: &str, b: &str) -> f64 {
    let tokens = |s: &str| -> Vec<String> {
        let mut out: Vec<String> = vec![];
        let mut word = false;
        for c in s.chars() {
            let part = c.is_alphanumeric() || c == '_';
            match (part, word) {
                (true, true) => out.last_mut().unwrap().push(c),
                _ if c.is_whitespace() => {}
                _ => out.push(c.to_string()),
            }
            word = part;
        }
        out
    };
    let (a, b) = (tokens(a), tokens(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut row = vec![0u32; b.len() + 1];
    for x in &a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    2.0 * f64::from(row[b.len()]) / (a.len() + b.len()) as f64
}

/// The fixture's requirement entries, developments' included.
fn requirements(result: &Value) -> Vec<String> {
    let Ok(fixture) = fixture(result["fixture"].as_str().unwrap_or_default()) else {
        return vec![];
    };
    let developments = fixture["milestones"].as_array().into_iter().flatten();
    let entries = fixture["requirements"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(developments.flat_map(|m| m["requirements"].as_array().into_iter().flatten()));
    entries
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

/// The requirement entries naming a production file, or its package directory alone (`tax/`,
/// not `tax/rates.go`).
fn named_by(requirements: &[String], path: &str) -> BTreeSet<usize> {
    if path.ends_with("_test.go") {
        return BTreeSet::new();
    }
    let directory = path.rsplit_once('/').map(|(d, _)| format!("{d}/"));
    let package = |r: &String, d: &String| {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        r.match_indices(d.as_str())
            .any(|(i, _)| !r[i + d.len()..].starts_with(word))
    };
    let names = |r: &String| r.contains(path) || directory.as_ref().is_some_and(|d| package(r, d));
    (0..requirements.len())
        .filter(|&i| names(&requirements[i]))
        .collect()
}

/// Candidate pairs, one function of each agent: the same name, one requirement naming both
/// files, or similar bodies. Each with the indices of its two functions.
fn pairs(
    functions: &[Function],
    requirements: &[String],
    result: &Value,
) -> Vec<(Value, usize, usize)> {
    let mut out = vec![];
    for (i, a) in functions.iter().enumerate() {
        for (j, b) in functions
            .iter()
            .enumerate()
            .filter(|(_, b)| b.agent > a.agent)
        {
            let score = similarity(&a.body, &b.body);
            let shared =
                !named_by(requirements, &a.file).is_disjoint(&named_by(requirements, &b.file));
            let reason = match () {
                _ if a.name == b.name => "name",
                _ if shared => "requirement",
                _ if score >= SIMILARITY => "similarity",
                _ => continue,
            };
            let side = |f: &Function| {
                json!({"agent": f.agent, "file": f.file, "name": f.name,
                "state": f.state, "lines": f.end - f.start + 1, "body": f.body})
            };
            let key: String = [&a.file, &a.name, &a.body, &b.file, &b.name, &b.body]
                .iter()
                .fold(
                    result["run_id"].as_str().unwrap_or_default().to_string(),
                    |k, s| k + "\0" + s,
                );
            let id = sha256(key.as_bytes())[..16].to_string();
            let pair = json!({"id": id, "run_id": result["run_id"], "reason": reason,
                "similarity": (score * 100.0).round() / 100.0, "sides": [side(a), side(b)],
                "duplicate": null});
            out.push((pair, i, j));
        }
    }
    out
}

/// Adds a duplicate-work collision per pair classified as duplicates and returns how many pairs
/// are unclassified. Waste: the later copy's lines when the final tree holds both, else the
/// lines of each copy written and then discarded; a gate-stopped copy wastes nothing.
fn duplicate_events(
    pairs: &[(Value, usize, usize)],
    functions: &[Function],
    verdicts: &BTreeMap<&str, bool>,
    mut discard: impl FnMut(&Function) -> Result<BTreeMap<String, u64>>,
    events: &mut Vec<Value>,
) -> Result<usize> {
    let mut unclassified = 0;
    for (pair, i, j) in pairs {
        match verdicts.get(pair["id"].as_str().unwrap_or_default()) {
            None => unclassified += 1,
            Some(false) => {}
            Some(true) => {
                let (a, b) = (&functions[*i], &functions[*j]);
                let mut waste = BTreeMap::new();
                if a.state == "final" && b.state == "final" {
                    let later = if b.first >= a.first { b } else { a };
                    waste.insert(later.agent.clone(), (later.end - later.start + 1) as u64);
                } else {
                    for side in [a, b].into_iter().filter(|f| f.state == "discarded") {
                        for (agent, n) in discard(side)? {
                            *waste.entry(agent).or_default() += n;
                        }
                    }
                }
                let files = [a.file.clone(), b.file.clone()].into();
                events.push(collision("duplicate_work", files, vec![], waste));
            }
        }
    }
    Ok(unclassified)
}

fn writes(record: &Value, path: &str) -> bool {
    record["request"]["output"]
        .as_object()
        .is_some_and(|o| o.contains_key(path))
}

/// What an output would make of `expected`; None when its hunks do not apply there.
fn intended(output: &Value, expected: &str) -> Option<String> {
    let Some(hunks) = output["replace"].as_array() else {
        return Some(text(output).unwrap_or_default());
    };
    let mut result = expected.to_string();
    for hunk in hunks {
        let (old, new) = (
            text(&json!({"bytes": hunk["old"]}))?,
            text(&json!({"bytes": hunk["new"]}))?,
        );
        if old.is_empty() || expected.matches(old.as_str()).count() != 1 {
            return None;
        }
        result = result.replacen(&old, &new, 1);
    }
    Some(result)
}

/// A written line an applied operation removed: its path and the line.
type Removal = (String, Line);

/// Written lines already counted as waste, each once per run.
type Used = BTreeSet<(String, Line)>;

/// Counts `line` of `path` for its agent, once per run; false for an initial or unowned line
/// and for one already counted.
fn count(used: &mut Used, path: &str, line: &Line, waste: &mut BTreeMap<String, u64>) -> bool {
    let Some((agent, _)) = line.agent() else {
        return false;
    };
    let counted = used.insert((path.to_string(), line.clone()));
    if counted {
        *waste.entry(agent.to_string()).or_default() += 1;
    }
    counted
}

/// A Falinks run's history with each file version's text, and its line authorship.
#[derive(Default)]
struct Ledger {
    records: Vec<Value>,
    texts: BTreeMap<(String, u64), String>,
    blame: Blame,
    /// The agent and operation that wrote each file version.
    writer: BTreeMap<(String, u64), (usize, u64)>,
    removed: BTreeMap<u64, Vec<Removal>>,
    revisions: BTreeMap<u64, Value>,
}

impl Ledger {
    fn read(run: &Path, result: &Value) -> Result<Self> {
        let history = read(run.join("controller/history.json"))?;
        let records = history
            .as_array()
            .ok_or("history.json must list operations")?;
        let (blame, _) = authorship::falinks(records, &initial_files(result));
        let mut ledger = Ledger {
            records: records.clone(),
            blame,
            ..Default::default()
        };
        ledger.records.sort_by_key(|r| r["operation"].as_u64());
        for record in &ledger.records {
            for capture in [&record["before"], &record["proposed"]] {
                for (path, file) in capture["files"].as_object().into_iter().flatten() {
                    if let Some(version) = file["version"].as_u64() {
                        let text = text(&file["file"]).unwrap_or_default();
                        ledger.texts.entry((path.clone(), version)).or_insert(text);
                    }
                }
                if let Some(revision) = capture["revision"].as_u64() {
                    ledger.revisions.entry(revision).or_insert(capture.clone());
                }
            }
            let conflicts = record["outcome"]["Stale"]["conflicts"].as_object();
            for (path, conflict) in conflicts.into_iter().flatten() {
                if let Some(version) = conflict["current"]["version"].as_u64() {
                    let text = text(&conflict["current"]["file"]).unwrap_or_default();
                    ledger.texts.entry((path.clone(), version)).or_insert(text);
                }
            }
        }
        for record in ledger.records.iter().filter(|r| applied(r)) {
            let agent = record["agent"].as_u64().unwrap_or(0) as usize;
            let op = record["operation"].as_u64().unwrap_or(0);
            for path in record["request"]["output"]
                .as_object()
                .into_iter()
                .flat_map(|o| o.keys())
            {
                let old_version = record["before"]["files"][path]["version"].as_u64();
                let Some(version) = record["proposed"]["files"][path]["version"].as_u64() else {
                    continue;
                };
                let key = (path.clone(), version);
                if old_version == Some(version) || ledger.writer.contains_key(&key) {
                    continue;
                }
                let old = old_version.map_or(&[][..], |v| ledger.lines_of(path, v));
                let old_text = old_version.and_then(|v| ledger.texts.get(&(path.clone(), v)));
                let new_text = ledger.texts.get(&key).map_or("", String::as_str);
                let removals = dropped(
                    &lines(old_text.map_or("", String::as_str)),
                    &lines(new_text),
                )
                .into_iter()
                .filter_map(|i| Some((path.clone(), old.get(i)?.clone())))
                .collect::<Vec<_>>();
                ledger.removed.entry(op).or_default().extend(removals);
                ledger.writer.insert(key, (agent, op));
            }
        }
        Ok(ledger)
    }

    /// The lines of `path` at `version`; empty for a version the history does not hold.
    fn lines_of(&self, path: &str, version: u64) -> &[Line] {
        let text = self.texts.get(&(path.to_string(), version));
        text.map_or(&[], |text| self.blame.lines(path, text))
    }

    /// Counts the lines operation `op` removed that `keep` selects, once per run, by author.
    fn take(
        &self,
        used: &mut Used,
        op: u64,
        keep: impl Fn(&str, &Line) -> bool,
        waste: &mut BTreeMap<String, u64>,
    ) -> BTreeSet<String> {
        let mut files = BTreeSet::new();
        let removed = self.removed.get(&op).into_iter().flatten();
        for (path, line) in removed.filter(|(path, line)| keep(path, line)) {
            if count(used, path, line, waste) {
                files.insert(path.clone());
            }
        }
        files
    }

    /// The run's collisions, and how many failed combined checks were premature.
    fn collisions(&self, result: &Value, used: &mut Used) -> (Vec<Value>, u64) {
        let mut out = vec![];
        for (n, record) in self.records.iter().enumerate() {
            let Some(conflicts) = record["outcome"]["Stale"]["conflicts"].as_object() else {
                continue;
            };
            let agent = record["agent"].as_u64().unwrap_or(0) as usize;
            let (mut files, mut peers) = (BTreeSet::new(), BTreeSet::new());
            for (path, conflict) in conflicts {
                let (Some(expected), Some(current)) = (
                    conflict["expected"].as_u64(),
                    conflict["current"]["version"].as_u64(),
                ) else {
                    continue;
                };
                let Some(&(peer, _)) = self.writer.get(&(path.clone(), current)) else {
                    continue;
                };
                let text = |v: u64| {
                    self.texts
                        .get(&(path.clone(), v))
                        .cloned()
                        .unwrap_or_default()
                };
                let (base, theirs) = (text(expected), text(current));
                let old = lines(&base);
                let ours = match intended(&record["request"]["output"][path], &base) {
                    Some(new) => regions(&old, &lines(&new)).0,
                    None => vec![(0, old.len())],
                };
                if peer != agent && touch(&ours, &regions(&old, &lines(&theirs)).0) {
                    files.insert(path.clone());
                    peers.insert(agent_name(peer));
                }
            }
            if files.is_empty() {
                continue;
            }
            let mut waste = BTreeMap::new();
            let next = self.records[n + 1..].iter().find(|w| {
                w["agent"] == record["agent"] && applied(w) && files.iter().any(|f| writes(w, f))
            });
            if let Some(next) = next {
                let op = next["operation"].as_u64().unwrap_or(0);
                let peer_line = |path: &str, line: &Line| {
                    files.contains(path) && line.agent().is_some_and(|(a, _)| peers.contains(a))
                };
                self.take(used, op, peer_line, &mut waste);
            }
            let detected = json!({"agent": agent_name(agent), "signal": "stale",
                "operation": record["operation"], "request_id": record["request"]["id"]});
            out.push(collision("line_conflict", files, vec![detected], waste));
        }
        for record in self
            .records
            .iter()
            .filter(|r| applied(r) && r["incorporates"].is_u64())
        {
            out.push(self.incorporation(record, used));
        }
        let mut premature = 0;
        let attempts = result["validation_attempts"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for (k, check) in attempts.iter().enumerate() {
            let Some(held) = check["revision"].as_u64().map(|r| self.included(r)) else {
                continue;
            };
            let authors: BTreeSet<u64> = self
                .records
                .iter()
                .filter(|r| {
                    applied(r) && r["operation"].as_u64().is_some_and(|o| held.contains(&o))
                })
                .filter_map(|r| r["agent"].as_u64())
                .collect();
            if check["passed"] != false || authors.len() < 2 {
                continue;
            }
            // Up to the next passing check, or the last revision when none passed.
            let next = attempts[k + 1..]
                .iter()
                .find(|c| c["passed"] == true)
                .and_then(|c| c["revision"].as_u64())
                .or_else(|| self.revisions.keys().max().copied());
            let later: BTreeSet<u64> = next
                .map(|r| self.included(r))
                .unwrap_or_default()
                .difference(&held)
                .copied()
                .collect();
            let mut waste = BTreeMap::new();
            let mut files = BTreeSet::new();
            for op in &later {
                let held_line =
                    |_: &str, line: &Line| line.agent().is_some_and(|(_, o)| held.contains(&o));
                files.extend(self.take(used, *op, held_line, &mut waste));
            }
            let class = match (waste.is_empty(), later.is_empty()) {
                (false, _) => "rework",
                (true, false) => {
                    premature += 1;
                    continue;
                }
                (true, true) => "no_fault",
            };
            let detected = json!({"agent": check["requested_by"], "signal": "broken_check",
                "check": check["number"], "revision": check["revision"]});
            out.push(collision(class, files, vec![detected], waste));
        }
        (out, premature)
    }

    /// The functions each agent added: written by its applied operations, held by the final
    /// check's revision, or only in its refused `Stale` or `Unreviewed` requests.
    fn functions(&self, result: &Value) -> Vec<Function> {
        let initial = |path: &str| -> BTreeSet<String> {
            let text = self
                .texts
                .get(&(path.to_string(), 0))
                .map_or("", String::as_str);
            go_functions(path, text).into_iter().map(|f| f.0).collect()
        };
        let mut found: BTreeMap<(String, String, String), Function> = BTreeMap::new();
        let mut add = |path: &str, version: u64, state: &'static str, op: u64| {
            let text = self
                .texts
                .get(&(path.to_string(), version))
                .cloned()
                .unwrap_or_default();
            let authors = self.lines_of(path, version);
            for (name, start, end) in go_functions(path, &text) {
                let Some((agent, _)) = authors.get(start).and_then(Line::agent) else {
                    continue;
                };
                if initial(path).contains(&name) {
                    continue;
                }
                let key = (agent.to_string(), path.to_string(), name.clone());
                let first = found.get(&key).map_or(op as f64, |f| f.first);
                found.insert(
                    key.clone(),
                    Function {
                        agent: key.0,
                        file: key.1,
                        name,
                        body: body(&lines(&text), start, end),
                        state,
                        at: version.to_string(),
                        start,
                        end,
                        first,
                    },
                );
            }
        };
        for record in self.records.iter().filter(|r| applied(r)) {
            let op = record["operation"].as_u64().unwrap_or(0);
            let written = self.writer.iter().filter(|(_, (_, o))| *o == op);
            for ((path, version), _) in written {
                add(path, *version, "discarded", op);
            }
        }
        // The final check's revision; one the engine made (after a split) is not in the history,
        // so each file's last written version stands for it.
        let last = result["validation_attempts"]
            .as_array()
            .and_then(|a| a.last());
        let revision = last.and_then(|a| a["revision"].as_u64());
        let files = revision
            .and_then(|r| self.revisions.get(&r))
            .map(|c| &c["files"]);
        let mut finals: BTreeMap<&String, u64> = files
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(path, file)| Some((path, file["version"].as_u64()?)))
            .collect();
        if files.is_none() {
            let mut latest: BTreeMap<&String, (u64, u64)> = BTreeMap::new();
            for ((path, version), (_, op)) in &self.writer {
                let entry = latest.entry(path).or_insert((*op, *version));
                if *op > entry.0 {
                    *entry = (*op, *version);
                }
            }
            finals = latest.into_iter().map(|(p, (_, v))| (p, v)).collect();
        }
        for (path, version) in finals {
            add(path, version, "final", u64::MAX);
        }
        let refused = self.records.iter().filter(|r| {
            r["outcome"].get("Stale").is_some() || r["outcome"].get("Unreviewed").is_some()
        });
        for record in refused {
            let agent = agent_name(record["agent"].as_u64().unwrap_or(0));
            let op = record["operation"].as_u64().unwrap_or(0);
            for (path, output) in record["request"]["output"]
                .as_object()
                .into_iter()
                .flatten()
            {
                let expected = record["request"]["expected"][path].as_u64();
                let base = expected.and_then(|v| self.texts.get(&(path.clone(), v)));
                let base = base.map_or("", String::as_str);
                let Some(text) = intended(output, base) else {
                    continue;
                };
                let had: BTreeSet<String> =
                    go_functions(path, base).into_iter().map(|f| f.0).collect();
                for (name, start, end) in go_functions(path, &text) {
                    if had.contains(&name) || initial(path).contains(&name) {
                        continue;
                    }
                    let key = (agent.clone(), path.clone(), name.clone());
                    found.entry(key).or_insert(Function {
                        agent: agent.clone(),
                        file: path.clone(),
                        name,
                        body: body(&lines(&text), start, end),
                        state: "stopped",
                        at: String::new(),
                        start,
                        end,
                        first: op as f64,
                    });
                }
            }
        }
        found.into_values().collect()
    }

    /// The lines of a discarded function's last version, by author, once per run.
    fn discard(&self, f: &Function, used: &mut Used) -> BTreeMap<String, u64> {
        let version: u64 = f.at.parse().unwrap_or(0);
        let authors = self.lines_of(&f.file, version);
        let mut waste = BTreeMap::new();
        for line in authors.get(f.start..=f.end).into_iter().flatten() {
            if line.agent().is_some_and(|(a, _)| a == f.agent) {
                count(used, &f.file, line, &mut waste);
            }
        }
        waste
    }

    /// The operations revision `revision` holds.
    fn included(&self, revision: u64) -> BTreeSet<u64> {
        let included = self.revisions.get(&revision).map(|c| &c["included"]);
        let ops = included.and_then(Value::as_array).into_iter().flatten();
        ops.filter_map(Value::as_u64).collect()
    }

    /// An `incorporate` resolves the files both split workspaces drafted: a line conflict whose
    /// waste is the lines the operation removed and the lines of the peer's latest version of
    /// each file that the resolution dropped.
    fn incorporation(&self, record: &Value, used: &mut Used) -> Value {
        let agent = record["agent"].as_u64().unwrap_or(0) as usize;
        let op = record["operation"].as_u64().unwrap_or(0);
        let files: BTreeSet<String> = record["request"]["output"]
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        let mut waste = BTreeMap::new();
        self.take(used, op, |path, _| files.contains(path), &mut waste);
        for path in &files {
            let peer = self
                .writer
                .iter()
                .filter(|((p, _), (a, o))| p == path && *a != agent && *o < op)
                .max_by_key(|(_, (_, o))| *o);
            let resolved = record["proposed"]["files"][path]["version"].as_u64();
            let (Some(((_, version), _)), Some(resolved)) = (peer, resolved) else {
                continue;
            };
            let text = |v: u64| {
                self.texts
                    .get(&(path.clone(), v))
                    .cloned()
                    .unwrap_or_default()
            };
            let (theirs, ours) = (text(*version), text(resolved));
            let authors = self.lines_of(path, *version);
            for i in dropped(&lines(&theirs), &lines(&ours)) {
                let Some(line) = authors.get(i) else { continue };
                if line.agent().is_some_and(|(a, _)| a != agent_name(agent)) {
                    count(used, path, line, &mut waste);
                }
            }
        }
        let detected = json!({"agent": agent_name(agent), "signal": "incorporate",
            "operation": op, "request_id": record["request"]["id"]});
        collision("line_conflict", files, vec![detected], waste)
    }
}

/// The files a Git `CONFLICT (` line names, from merge output or a host check's conflict.
fn conflict_files(text: &str) -> BTreeSet<String> {
    let names = text.lines().filter_map(|line| {
        let (_, rest) = line.trim().strip_prefix("CONFLICT (")?.split_once("): ")?;
        let name = match rest.rsplit_once("Merge conflict in ") {
            Some((_, name)) => name.trim(),
            None => rest.split_whitespace().next()?,
        };
        Some(name.to_string())
    });
    names.filter(|n| !n.is_empty()).collect()
}

/// A Git line conflict: its files, detections and the merge commits that resolved it.
type Conflict = (BTreeSet<String>, Vec<Value>, Vec<String>);

/// A Git run's agent repository, its shares and each commit's line authors.
struct Repo {
    dir: std::path::PathBuf,
    /// Per agent, its shares in order: commit and elapsed seconds.
    shares: BTreeMap<String, Vec<(String, f64)>>,
    initial: String,
    lines: authorship::Repo,
    ancestors: BTreeMap<(String, String), bool>,
    used: Used,
}

impl Repo {
    fn read(run: &Path, result: &Value) -> Result<Self> {
        let initial = result["initial_snapshot"]
            .as_str()
            .ok_or("the run records no initial_snapshot")?
            .into();
        let list = authorship::shares(run)?;
        let mut shares: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
        for (agent, commit, at) in &list {
            shares
                .entry(agent.clone())
                .or_default()
                .push((commit.clone(), *at));
        }
        Ok(Repo {
            dir: run.join("agents/repo"),
            shares,
            initial,
            lines: authorship::Repo::new(run, &list, &initial_files(result))?,
            ancestors: BTreeMap::new(),
            used: BTreeSet::new(),
        })
    }

    fn text(&mut self, commit: &str, path: &str) -> Result<String> {
        Ok(self.lines.file(commit, path)?.0)
    }

    fn paths(&mut self, commits: &[&str]) -> Result<BTreeSet<String>> {
        let mut paths = BTreeSet::new();
        for commit in commits {
            paths.extend(self.lines.tree(commit)?.keys().cloned());
        }
        Ok(paths)
    }

    fn ancestor(&mut self, a: &str, b: &str) -> bool {
        let key = (a.to_string(), b.to_string());
        let dir = &self.dir;
        *self
            .ancestors
            .entry(key)
            .or_insert_with(|| git(dir, &["merge-base", "--is-ancestor", a, b], None).is_ok())
    }

    /// Per agent, the number of its latest share `commit` holds (0 for none).
    fn side(&mut self, commit: &str) -> BTreeMap<String, usize> {
        let shares = self.shares.clone();
        shares
            .iter()
            .map(|(agent, list)| {
                let held = list.iter().rposition(|(s, _)| self.ancestor(s, commit));
                (agent.clone(), held.map_or(0, |i| i + 1))
            })
            .collect()
    }

    /// Per agent, the number of its latest share by `elapsed` seconds.
    fn at(&self, elapsed: f64) -> BTreeMap<String, usize> {
        let latest =
            |list: &Vec<(String, f64)>| list.iter().filter(|(_, at)| *at <= elapsed).count();
        self.shares
            .iter()
            .map(|(a, list)| (a.clone(), latest(list)))
            .collect()
    }

    /// The lines of `path` at `commit`, by `authorship`.
    fn line_ids(&mut self, commit: &str, path: &str) -> Result<Vec<Line>> {
        Ok(self.lines.file(commit, path)?.1)
    }

    /// Counts line `j` of `ids` at `path` once per run, by its agent.
    fn count(
        &mut self,
        path: &str,
        ids: &[Line],
        j: usize,
        waste: &mut BTreeMap<String, u64>,
    ) -> bool {
        ids.get(j)
            .is_some_and(|line| count(&mut self.used, path, line, waste))
    }

    /// The files whose merge of `a` and `b` conflicts. The merge's objects go to a temporary
    /// store, so the run's evidence is left as it was.
    fn conflicts(&self, a: &str, b: &str) -> Result<Vec<String>> {
        let argv = strings(&[
            "git",
            "-c",
            "core.hooksPath=/dev/null",
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            a,
            b,
        ]);
        let store = tempfile::tempdir()?;
        let mut env = git_env();
        env.push((
            "GIT_OBJECT_DIRECTORY".into(),
            store.path().to_string_lossy().into_owned(),
        ));
        let objects = git(
            &self.dir,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "objects",
            ],
            None,
        )?;
        env.push(("GIT_ALTERNATE_OBJECT_DIRECTORIES".into(), objects));
        let merged = command(&argv, &self.dir, &env, Duration::from_secs(120))?;
        let text = String::from_utf8_lossy(&merged.stdout).into_owned();
        Ok(match merged.success {
            true => vec![],
            false => text
                .lines()
                .skip(1)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect(),
        })
    }

    fn collisions(&mut self, run: &Path, result: &Value) -> Result<(Vec<Value>, u64)> {
        let start = result["timing"]["start_unix"].as_f64().unwrap_or(0.0);
        // Line conflicts, keyed by the share numbers of each side.
        let mut keyed: BTreeMap<Vec<usize>, Conflict> = BTreeMap::new();
        let key = |side: BTreeMap<String, usize>| -> Option<Vec<usize>> {
            Some(side.into_values().collect::<Vec<_>>())
                .filter(|k| k.len() == 2 && k.iter().all(|n| *n > 0))
        };
        for agent in self.shares.keys().cloned().collect::<Vec<_>>() {
            let transcript = run.join(format!("agents/{agent}/scratch/claude-events.jsonl"));
            let events = jsonl(&transcript).unwrap_or_default();
            for event in events.iter().filter(|e| e["type"] == "user") {
                let Some(stamp) = event["timestamp"].as_str() else {
                    continue;
                };
                let blocks = event["message"]["content"].as_array().into_iter().flatten();
                for block in blocks.filter(|b| b["type"] == "tool_result") {
                    let text = match block["content"].as_str() {
                        Some(text) => text.to_string(),
                        None => block["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|b| b["text"].as_str())
                            .collect(),
                    };
                    let files = conflict_files(&text);
                    let elapsed = unix(stamp).map(|t| t - start).unwrap_or(0.0);
                    if let (false, Some(k)) = (files.is_empty(), key(self.at(elapsed))) {
                        let entry = keyed.entry(k).or_default();
                        entry.0.extend(files);
                        entry
                            .1
                            .push(json!({"agent": agent, "signal": "merge_conflict",
                            "timestamp": stamp, "elapsed": elapsed}));
                    }
                }
            }
        }
        let checks = self.checks(run, result);
        for (attempt, agent, elapsed) in &checks {
            let files = conflict_files(attempt["conflict"].as_str().unwrap_or_default());
            if let (false, Some(k)) = (files.is_empty(), key(self.at(*elapsed))) {
                let entry = keyed.entry(k).or_default();
                entry.0.extend(files);
                entry
                    .1
                    .push(json!({"agent": agent, "signal": "check_conflict",
                    "check": attempt["number"], "elapsed": elapsed}));
            }
        }
        let merges = git(
            &self.dir,
            &["rev-list", "--all", "--reflog", "--merges", "--parents"],
            None,
        )?;
        for line in merges.lines() {
            let commits: Vec<&str> = line.split_whitespace().collect();
            let [merge, a, b] = commits[..] else { continue };
            let files = self.conflicts(a, b)?;
            let side = self.side(merge);
            if let (false, Some(k)) = (files.is_empty(), key(side)) {
                let entry = keyed.entry(k).or_default();
                entry.0.extend(files);
                entry.2.push(merge.to_string());
            }
        }
        let mut out = vec![];
        let last = result["final_snapshot"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        for (_, (files, mut detected, merges)) in keyed {
            // The resolution the final snapshot holds, else the latest.
            let resolved = merges
                .iter()
                .find(|m| self.ancestor(m, &last))
                .or(merges.first())
                .cloned();
            let mut waste = BTreeMap::new();
            if let Some(merge) = &resolved {
                let parents = git(
                    &self.dir,
                    &["rev-list", "--parents", "-n", "1", merge],
                    None,
                )?;
                let parents: Vec<&str> = parents.split_whitespace().skip(1).collect();
                let base = git(&self.dir, &["merge-base", parents[0], parents[1]], None)?;
                for path in self.conflicts(parents[0], parents[1])? {
                    let (base_text, merged) = (self.text(&base, &path)?, self.text(merge, &path)?);
                    for parent in &parents {
                        let side = self.text(parent, &path)?;
                        let (old, ours) = (lines(&base_text), lines(&side));
                        let added: BTreeSet<usize> = matches(&old, &ours)
                            .iter()
                            .enumerate()
                            .filter(|(_, kept)| kept.is_none())
                            .map(|(j, _)| j)
                            .collect();
                        let ids = self.line_ids(parent, &path)?;
                        for j in dropped(&ours, &lines(&merged))
                            .into_iter()
                            .filter(|j| added.contains(j))
                        {
                            self.count(&path, &ids, j, &mut waste);
                        }
                    }
                }
            }
            detected.sort_by(|a, b| {
                a["elapsed"]
                    .as_f64()
                    .partial_cmp(&b["elapsed"].as_f64())
                    .unwrap()
            });
            let mut event = collision("line_conflict", files, detected, waste);
            event["resolved_by"] = json!(resolved);
            out.push(event);
        }
        let premature = self.broken_checks(&checks, &mut out)?;
        self.rework(&mut out)?;
        Ok((out, premature))
    }

    /// The functions each agent's shares added, and whether the final snapshot holds them;
    /// without one, each agent's last share stands for it.
    fn functions(&mut self, result: &Value) -> Result<Vec<Function>> {
        let initial = self.initial.clone();
        let mut found: BTreeMap<(String, String, String), Function> = BTreeMap::new();
        let last = result["final_snapshot"].as_str().map(String::from);
        let mut commits: Vec<(String, f64, Option<String>)> = vec![];
        for (agent, list) in self.shares.clone() {
            for (n, (commit, at)) in list.iter().enumerate() {
                let stands = last.is_none() && n + 1 == list.len();
                commits.push((commit.clone(), *at, stands.then(|| agent.clone())));
            }
        }
        commits.sort_by(|a, b| a.1.total_cmp(&b.1));
        commits.extend(last.map(|l| (l, f64::MAX, None)));
        let count = commits.len();
        for (k, (commit, at, stands)) in commits.into_iter().enumerate() {
            let is_final =
                k + 1 == count && stands.is_none() && result["final_snapshot"].is_string();
            for path in self.paths(&[&commit])? {
                let had: BTreeSet<String> = go_functions(&path, &self.text(&initial, &path)?)
                    .into_iter()
                    .map(|f| f.0)
                    .collect();
                let text = self.text(&commit, &path)?;
                let ids = self.line_ids(&commit, &path)?;
                for (name, start, end) in go_functions(&path, &text) {
                    let Some((agent, _)) = ids.get(start).and_then(Line::agent) else {
                        continue;
                    };
                    if had.contains(&name) {
                        continue;
                    }
                    let key = (agent.to_string(), path.clone(), name.clone());
                    let first = found.get(&key).map_or(at, |f| f.first);
                    let state = if is_final || stands.as_deref() == Some(agent) {
                        "final"
                    } else {
                        "discarded"
                    };
                    found.insert(
                        key.clone(),
                        Function {
                            agent: key.0,
                            file: path.clone(),
                            name,
                            body: body(&lines(&text), start, end),
                            state,
                            at: commit.clone(),
                            start,
                            end,
                            first,
                        },
                    );
                }
            }
        }
        Ok(found.into_values().collect())
    }

    /// The lines of a discarded function at its last commit, by agent, once per run.
    fn discard(&mut self, f: &Function) -> Result<BTreeMap<String, u64>> {
        let ids = self.line_ids(&f.at, &f.file)?;
        let mut waste = BTreeMap::new();
        for j in f.start..=f.end {
            if ids
                .get(j)
                .and_then(Line::agent)
                .is_some_and(|(a, _)| a == f.agent)
            {
                self.count(&f.file, &ids, j, &mut waste);
            }
        }
        Ok(waste)
    }

    /// Each check attempt with the agent that asked for it and when.
    fn checks(&self, run: &Path, result: &Value) -> Vec<(Value, String, f64)> {
        let rows = jsonl(&run.join("controller/events.jsonl")).unwrap_or_default();
        let asked: Vec<&Value> = rows
            .iter()
            .filter(|r| r["kind"] == "validation" || r["kind"] == "final_validation")
            .collect();
        let attempts = result["validation_attempts"]
            .as_array()
            .into_iter()
            .flatten();
        attempts
            .enumerate()
            .map(|(k, attempt)| {
                let row = asked.get(k);
                let agent = row.and_then(|r| r["agent"].as_str()).unwrap_or("final");
                let elapsed = row.and_then(|r| r["elapsed"].as_f64()).unwrap_or(0.0);
                (attempt.clone(), agent.to_string(), elapsed)
            })
            .collect()
    }

    /// Failed checks of a merge holding both agents' shares; returns the premature ones.
    fn broken_checks(
        &mut self,
        checks: &[(Value, String, f64)],
        out: &mut Vec<Value>,
    ) -> Result<u64> {
        let mut premature = 0;
        for (k, (attempt, agent, elapsed)) in checks.iter().enumerate() {
            let Some(candidate) = attempt["candidate"]
                .as_str()
                .filter(|_| attempt["passed"] == false)
            else {
                continue;
            };
            let held = self.side(candidate);
            if held.len() < 2 || held.values().any(|n| *n == 0) {
                continue;
            }
            let next = checks[k + 1..]
                .iter()
                .find(|(a, ..)| a["passed"] == true)
                .and_then(|(a, ..)| a["candidate"].as_str().map(String::from));
            let (mut waste, mut files, mut later) = (BTreeMap::new(), BTreeSet::new(), false);
            if let Some(next) = &next {
                later = self.side(next).iter().any(|(a, n)| *n > held[a]);
                for path in self.paths(&[candidate, next])? {
                    let (ours, theirs) = (self.text(candidate, &path)?, self.text(next, &path)?);
                    let ids = self.line_ids(candidate, &path)?;
                    for i in dropped(&lines(&ours), &lines(&theirs)) {
                        if self.count(&path, &ids, i, &mut waste) {
                            files.insert(path.clone());
                        }
                    }
                }
            }
            let class = match (waste.is_empty(), later) {
                (false, _) => "rework",
                (true, true) => {
                    premature += 1;
                    continue;
                }
                (true, false) => "no_fault",
            };
            let detected = json!({"agent": agent, "signal": "broken_check",
                "check": attempt["number"], "elapsed": elapsed});
            out.push(collision(class, files, vec![detected], waste));
        }
        Ok(premature)
    }

    /// Shares that remove the agent's own lines in a region the peer's latest share changed.
    fn rework(&mut self, out: &mut Vec<Value>) -> Result<()> {
        let initial = self.initial.clone();
        for (agent, list) in self.shares.clone() {
            let Some((peer, theirs)) = self.shares.iter().find(|(a, _)| **a != agent) else {
                continue;
            };
            let (peer, theirs) = (peer.clone(), theirs.clone());
            for n in 1..list.len() {
                let ((before, _), (after, at)) = (&list[n - 1], &list[n]);
                let Some(seen) = theirs.iter().rposition(|(_, t)| t < at) else {
                    continue;
                };
                let (peer_commit, peer_at) = &theirs[seen];
                let (mut waste, mut files) = (BTreeMap::new(), BTreeSet::new());
                for path in self.paths(&[before, after])? {
                    let base = self.text(&initial, &path)?;
                    let (ours, next) = (self.text(before, &path)?, self.text(after, &path)?);
                    let changed = regions(&lines(&base), &lines(&self.text(peer_commit, &path)?)).0;
                    let filled = regions(&lines(&base), &lines(&ours)).1;
                    let ids = self.line_ids(before, &path)?;
                    for i in dropped(&lines(&ours), &lines(&next)) {
                        let own = ids
                            .get(i)
                            .and_then(Line::agent)
                            .is_some_and(|(a, _)| a == agent);
                        let beside = filled[i].is_some_and(|r| touch(&[r], &changed));
                        if own && beside && self.count(&path, &ids, i, &mut waste) {
                            files.insert(path.clone());
                        }
                    }
                }
                if !waste.is_empty() {
                    let detected = json!({"agent": agent, "signal": "peer_diff", "peer": peer,
                        "peer_share": seen + 1, "share": n + 1, "elapsed": peer_at});
                    out.push(collision("rework", files, vec![detected], waste));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(text: &str) -> Value {
        json!({"bytes": text.as_bytes(), "executable": false})
    }

    /// A capture of revision `revision` holding `included` and each (path, version, text).
    fn capture(revision: u64, included: &[u64], files: &[(&str, u64, &str)]) -> Value {
        let files: serde_json::Map<String, Value> = files
            .iter()
            .map(|(p, v, t)| (p.to_string(), json!({"version": v, "file": file(t)})))
            .collect();
        json!({"revision": revision, "space": 0, "group": [0, 1], "included": included, "files": files})
    }

    fn record(
        op: u64,
        agent: u64,
        before: Value,
        proposed: Value,
        outcome: Value,
        output: Value,
    ) -> Value {
        json!({"operation": op, "agent": agent, "attempts": [], "installation_pending": false,
            "session": "s", "request": {"id": format!("r{op}"), "note": "n", "expected": {}, "reviews": [],
            "output": output}, "before": before, "proposed": proposed, "outcome": outcome, "incorporates": null})
    }

    fn applied(op: u64, agent: u64, before: Value, proposed: Value, output: Value) -> Value {
        let outcome = json!({"Applied": {"revision": proposed["revision"], "tree": "t"}});
        record(op, agent, before, proposed, outcome, output)
    }

    fn falinks_run(history: Value, attempts: Value) -> (tempfile::TempDir, Value) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("controller")).unwrap();
        crate::write(dir.path().join("controller/history.json"), &history).unwrap();
        let result = json!({"run_id": "f", "arm": "falinks", "team_size": 2, "validation_attempts": attempts});
        (dir, result)
    }

    const V0: &str = "package p\nfunc H() {\n    return\n}\n";
    const V1: &str = "package p\nfunc H() {\n    a := 1\n    b := 2\n    return\n}\n";
    const V2: &str = "package p\nfunc H() {\n    c := 3\n    return\n}\n";
    /// V1 with agent-0's `a := 1` dropped.
    const V0_B: &str = "package p\nfunc H() {\n    b := 2\n    return\n}\n";

    #[test]
    fn a_stale_refusal_against_the_peers_region_is_a_line_conflict_whose_resolution_wastes_the_peers_lines()
     {
        let rev = |r, inc: &[u64], v, t| capture(r, inc, &[("h.go", v, t)]);
        let replace = |old: &str, new: &str| json!({"h.go": {"replace": [{"old": old.as_bytes(), "new": new.as_bytes()}]}});
        let stale = |expected: u64, current: u64, text: &str| {
            json!({"Stale": {"conflicts": {"h.go": {"expected": expected,
                "current": {"version": current, "file": file(text), "blob": "b"}}}}})
        };
        let history = json!([
            // agent-0 adds two lines; agent-1, still on version 0, inserts at the same place.
            applied(
                1,
                0,
                rev(0, &[], 0, V0),
                rev(1, &[1], 1, V1),
                json!({"h.go": file(V1)})
            ),
            record(
                2,
                1,
                rev(1, &[1], 1, V1),
                Value::Null,
                stale(0, 1, V1),
                replace("    return\n", "    c := 3\n    return\n")
            ),
            // Its retry rewrites agent-0's two lines.
            applied(
                3,
                1,
                rev(1, &[1], 1, V1),
                rev(2, &[1, 3], 2, V2),
                json!({"h.go": file(V2)})
            ),
            // agent-0's own stale edit touches only the package line, apart from agent-1's change.
            record(
                4,
                0,
                rev(2, &[1, 3], 2, V2),
                Value::Null,
                stale(1, 2, V2),
                replace("package p\n", "package q\n")
            ),
        ]);
        let (dir, result) = falinks_run(history, json!([]));
        let found = collisions(dir.path(), &result, &Value::Null);
        assert_eq!(found["status"], "complete", "{found}");
        assert_eq!(
            found["events"],
            json!([{"class": "line_conflict", "files": ["h.go"], "lines": 2, "waste": {"agent-0": 2},
                "detected": [{"agent": "agent-1", "signal": "stale", "operation": 2, "request_id": "r2"}]}])
        );
        assert_eq!(found["lines"], 2);
    }

    /// agent-0 writes a.go, agent-1 writes b.go, then agent-1 changes b.go to `later`. Checks
    /// are (number, passed, revision).
    fn broken_check_run(later: &str, checks: &[(u64, bool, u64)]) -> Value {
        let (a, b) = (
            "package p\nfunc A() int {\n    return 1\n}\n",
            "package p\nfunc B() int {\n    x := 2\n    return x\n}\n",
        );
        let rev = |r, inc: &[u64], a_v, a_t, b_v, b_t| {
            capture(r, inc, &[("a.go", a_v, a_t), ("b.go", b_v, b_t)])
        };
        let history = json!([
            applied(
                1,
                0,
                rev(0, &[], 0, "", 0, ""),
                rev(1, &[1], 1, a, 0, ""),
                json!({"a.go": file(a)})
            ),
            applied(
                2,
                1,
                rev(1, &[1], 1, a, 0, ""),
                rev(2, &[1, 2], 1, a, 2, b),
                json!({"b.go": file(b)})
            ),
            applied(
                3,
                1,
                rev(2, &[1, 2], 1, a, 2, b),
                rev(3, &[1, 2, 3], 1, a, 3, later),
                json!({"b.go": file(later)})
            ),
        ]);
        let attempts: Vec<Value> = checks
            .iter()
            .map(|(n, passed, r)| json!({"number": n, "passed": passed, "revision": r, "requested_by": "agent-0", "conflict": null}))
            .collect();
        let (dir, result) = falinks_run(history, json!(attempts));
        collisions(dir.path(), &result, &Value::Null)
    }

    #[test]
    fn a_broken_combined_check_is_rework_premature_or_no_fault_by_what_followed_it() {
        // Its two lines of b.go are rewritten before the next pass: rework.
        let rewritten = "package p\nfunc B() int {\n    return 2\n}\n";
        let found = broken_check_run(rewritten, &[(1, false, 1), (2, false, 2), (3, true, 3)]);
        assert_eq!(
            found["events"],
            json!([{"class": "rework", "files": ["b.go"], "lines": 2, "waste": {"agent-1": 2},
                "detected": [{"agent": "agent-0", "signal": "broken_check", "check": 2, "revision": 2}]}]),
            "a check holding one agent's work is not combined"
        );
        assert_eq!(found["premature_checks"], 0);
        // Only additions before the pass: the peer's half did not exist yet.
        let added = "package p\nfunc B() int {\n    x := 2\n    return x\n}\nfunc C() {}\n";
        let found = broken_check_run(added, &[(1, false, 2), (2, true, 3)]);
        assert_eq!(found["events"], json!([]));
        assert_eq!(found["premature_checks"], 1);
        // Nothing changed before the pass.
        let found = broken_check_run(added, &[(1, false, 2), (2, true, 2)]);
        assert_eq!(
            found["events"],
            json!([{"class": "no_fault", "files": [], "lines": 0, "waste": {},
                "detected": [{"agent": "agent-0", "signal": "broken_check", "check": 1, "revision": 2}]}])
        );
    }

    /// A Git run directory: `agents/repo` with the initial commit, and events and transcripts
    /// added by the test. The run starts at Unix time 1000.
    struct GitRun {
        dir: tempfile::TempDir,
        events: Vec<Value>,
        initial: String,
    }

    impl GitRun {
        fn new(files: &[(&str, &str)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().join("agents/repo");
            std::fs::create_dir_all(dir.path().join("controller")).unwrap();
            std::fs::create_dir_all(&repo).unwrap();
            git(&repo, &["init", "-q", "-b", "main"], None).unwrap();
            let mut run = GitRun {
                dir,
                events: vec![],
                initial: String::new(),
            };
            run.initial = run.commit(None, files, "initial");
            run
        }

        fn repo(&self) -> std::path::PathBuf {
            self.dir.path().join("agents/repo")
        }

        /// Commits `files` on top of `parent` (or the current branch) and returns the commit.
        fn commit(&self, parent: Option<&str>, files: &[(&str, &str)], message: &str) -> String {
            let repo = self.repo();
            if let Some(parent) = parent {
                git(&repo, &["checkout", "-q", "--detach", parent], None).unwrap();
            }
            for (path, text) in files {
                std::fs::write(repo.join(path), text).unwrap();
            }
            git(&repo, &["add", "-A"], None).unwrap();
            git(&repo, &["commit", "-qm", message], None).unwrap();
            git(&repo, &["rev-parse", "HEAD"], None).unwrap()
        }

        /// A share by `agent` at `elapsed` seconds.
        fn share(
            &mut self,
            agent: &str,
            parent: &str,
            files: &[(&str, &str)],
            elapsed: f64,
        ) -> String {
            let commit = self.commit(Some(parent), files, &format!("{agent} draft"));
            git(&self.repo(), &["branch", "-f", agent, &commit], None).unwrap();
            self.events.push(json!({"kind": "draft_shared", "agent": agent, "commit": commit, "elapsed": elapsed}));
            commit
        }

        /// Merges `theirs` into `ours`; on a conflict, commits `resolution`.
        fn merge(&self, ours: &str, theirs: &str, resolution: &[(&str, &str)]) -> String {
            let repo = self.repo();
            git(&repo, &["checkout", "-q", "--detach", ours], None).unwrap();
            if git(&repo, &["merge", "-q", "--no-edit", theirs], None).is_err() {
                return self.commit(None, resolution, "resolved");
            }
            git(&repo, &["rev-parse", "HEAD"], None).unwrap()
        }

        fn transcript(&self, agent: &str, at: u64, text: &str) {
            let scratch = self.dir.path().join(format!("agents/{agent}/scratch"));
            std::fs::create_dir_all(&scratch).unwrap();
            let event = json!({"type": "user", "timestamp": crate::utc(at),
                "message": {"content": [{"type": "tool_result", "tool_use_id": "t", "content": text}]}});
            std::fs::write(scratch.join("claude-events.jsonl"), format!("{event}\n")).unwrap();
        }

        /// Checks as (agent, candidate, passed, elapsed).
        fn measure(&mut self, checks: &[(&str, &str, bool, f64)]) -> Value {
            let mut attempts = vec![];
            for (n, (agent, candidate, passed, elapsed)) in checks.iter().enumerate() {
                self.events.push(
                    json!({"kind": "validation", "agent": agent, "candidate": candidate,
                    "passed": passed, "elapsed": elapsed}),
                );
                attempts.push(json!({"number": n + 1, "candidate": candidate, "passed": passed, "conflict": null}));
            }
            let events: String = self.events.iter().map(|e| format!("{e}\n")).collect();
            std::fs::write(self.dir.path().join("controller/events.jsonl"), events).unwrap();
            let last = checks.last().map(|c| c.1);
            let result = json!({"run_id": "g", "arm": "git", "team_size": 2,
                "initial_snapshot": self.initial, "final_snapshot": last,
                "timing": {"start_unix": 1000.0}, "validation_attempts": attempts});
            collisions(self.dir.path(), &result, &Value::Null)
        }
    }

    #[test]
    fn a_conflicting_merge_is_a_line_conflict_whose_resolution_wastes_the_dropped_side() {
        let mut run = GitRun::new(&[("h.go", V0)]);
        let initial = run.initial.clone();
        let a = run.share(
            "A",
            &initial,
            &[("h.go", "package p\nfunc H() {\n    a := 1\n    return\n}\n")],
            10.0,
        );
        let b = run.share(
            "B",
            &initial,
            &[("h.go", "package p\nfunc H() {\n    b := 2\n    return\n}\n")],
            20.0,
        );
        let merged = run.merge(
            &a,
            &b,
            &[("h.go", "package p\nfunc H() {\n    b := 2\n    return\n}\n")],
        );
        run.transcript("A", 1050, "CONFLICT (content): Merge conflict in h.go\nAutomatic merge failed; fix conflicts and then commit the result.");
        let found = run.measure(&[("A", &merged, true, 60.0)]);
        assert_eq!(found["status"], "complete", "{found}");
        assert_eq!(
            found["events"],
            json!([{"class": "line_conflict", "files": ["h.go"], "lines": 1, "waste": {"A": 1},
                "resolved_by": merged, "detected": [{"agent": "A", "signal": "merge_conflict",
                    "timestamp": "1970-01-01T00:17:30Z", "elapsed": 50.0}]}])
        );
    }

    #[test]
    fn a_share_that_rewrites_own_lines_where_the_peers_share_changed_is_rework() {
        let mut run = GitRun::new(&[("h.go", V0), ("g.go", "package p\n")]);
        let initial = run.initial.clone();
        let a1 = run.share(
            "A",
            &initial,
            &[
                ("h.go", "package p\nfunc H() {\n    a := 1\n    return\n}\n"),
                ("g.go", "package p\nfunc G() {}\n"),
            ],
            10.0,
        );
        let b = run.share(
            "B",
            &initial,
            &[("h.go", "package p\nfunc H() {\n    return // b\n}\n")],
            20.0,
        );
        // A drops its line beside B's change, and its g.go line, which B never touched.
        let a2 = run.share("A", &a1, &[("h.go", V0), ("g.go", "package p\n")], 30.0);
        let merged = run.merge(&a2, &b, &[]);
        let found = run.measure(&[("A", &merged, true, 40.0)]);
        assert_eq!(
            found["events"],
            json!([{"class": "rework", "files": ["h.go"], "lines": 1, "waste": {"A": 1},
                "detected": [{"agent": "A", "signal": "peer_diff", "peer": "B", "peer_share": 1,
                    "share": 2, "elapsed": 20.0}]}])
        );
    }

    /// Waste credits lines as changed bytes do (`authorship`): a peer's line merged into an
    /// agent's share stays the peer's, so removing it is not that agent's rework.
    #[test]
    fn a_peer_line_merged_into_a_share_stays_the_peers() {
        let mut run = GitRun::new(&[("h.go", V0), ("g.go", "package p\n")]);
        let initial = run.initial.clone();
        let with_b = "package p\nfunc H() {\n    b := 2\n    return\n}\n";
        let b = run.share("B", &initial, &[("h.go", with_b)], 10.0);
        let own = run.commit(Some(&initial), &[("g.go", "package p\nfunc G() {}\n")], "A");
        let merged = run.merge(&own, &b, &[]);
        run.events
            .push(json!({"kind": "draft_shared", "agent": "A", "commit": merged, "elapsed": 20.0}));
        let dropped = run.share("A", &merged, &[("h.go", V0)], 30.0);
        let found = run.measure(&[("A", &dropped, true, 40.0)]);
        assert_eq!(found["status"], "complete", "{found}");
        assert_eq!(found["events"], json!([]), "B's line is not A's own");
    }

    /// A shares a.go and B b.go; their merge fails a check; B then shares `later`, whose merge
    /// passes.
    fn git_broken_check(later: &str) -> Value {
        let mut run = GitRun::new(&[("a.go", "package p\n"), ("b.go", "package p\n")]);
        let initial = run.initial.clone();
        let a = run.share(
            "A",
            &initial,
            &[("a.go", "package p\nfunc A() int {\n    return 1\n}\n")],
            10.0,
        );
        let b1 = run.share(
            "B",
            &initial,
            &[(
                "b.go",
                "package p\nfunc B() int {\n    x := 2\n    return x\n}\n",
            )],
            20.0,
        );
        let failed = run.merge(&a, &b1, &[]);
        let b2 = run.share("B", &b1, &[("b.go", later)], 40.0);
        let passed = run.merge(&a, &b2, &[]);
        run.measure(&[("A", &failed, false, 30.0), ("B", &passed, true, 50.0)])
    }

    #[test]
    fn a_failed_check_of_both_shares_is_rework_when_its_lines_are_rewritten_else_premature() {
        let found = git_broken_check("package p\nfunc B() int {\n    return 2\n}\n");
        assert_eq!(
            found["events"],
            json!([{"class": "rework", "files": ["b.go"], "lines": 2, "waste": {"B": 2},
                "detected": [{"agent": "A", "signal": "broken_check", "check": 1, "elapsed": 30.0}]}])
        );
        let found = git_broken_check(
            "package p\nfunc B() int {\n    x := 2\n    return x\n}\nfunc C() {}\n",
        );
        assert_eq!(found["events"], json!([]));
        assert_eq!(found["premature_checks"], 1);
    }

    const NET_A: &str = "package p\nfunc NetA(q, p int) int {\n    n := q * p\n    return n\n}\n";
    const NET_B: &str = "package p\nfunc NetB(q, p int) int {\n    n := q * p\n    return n\n}\n";

    /// Marks every candidate pair of `result`'s run as `verdict`.
    fn classified(run: &Path, result: &Value, verdict: bool) -> Value {
        let pairs = candidates(run, result).unwrap();
        let pairs: Vec<Value> = pairs
            .into_iter()
            .map(|mut p| {
                p["duplicate"] = verdict.into();
                p
            })
            .collect();
        json!({"pairs": pairs})
    }

    #[test]
    fn duplicate_work_counts_the_later_final_copy_once_classified() {
        let mut run = GitRun::new(&[("a.go", "package p\n"), ("b.go", "package p\n")]);
        let initial = run.initial.clone();
        let a = run.share("A", &initial, &[("a.go", NET_A)], 10.0);
        let b = run.share("B", &initial, &[("b.go", NET_B)], 20.0);
        let merged = run.merge(&a, &b, &[]);
        let found = run.measure(&[("A", &merged, true, 30.0)]);
        assert_eq!(
            found["status"], "partial",
            "an unclassified candidate leaves waste unknown"
        );
        assert_eq!(
            found["duplicates"],
            json!({"candidates": 1, "unclassified": 1})
        );
        assert_eq!(waste(&found)["lines"], Value::Null);

        let result = json!({"run_id": "g", "arm": "git", "team_size": 2, "initial_snapshot": initial,
            "final_snapshot": merged, "timing": {"start_unix": 1000.0}, "validation_attempts": []});
        let pairs = candidates(run.dir.path(), &result).unwrap();
        assert_eq!(pairs[0]["reason"], "similarity");
        assert_eq!(
            pairs[0]["sides"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| (&s["agent"], &s["name"], &s["state"], &s["lines"]))
                .collect::<Vec<_>>(),
            [
                (&json!("A"), &json!("NetA"), &json!("final"), &json!(4)),
                (&json!("B"), &json!("NetB"), &json!("final"), &json!(4))
            ]
        );
        let yes = classified(run.dir.path(), &result, true);
        let found = collisions(run.dir.path(), &result, &yes);
        assert_eq!(found["status"], "complete");
        assert_eq!(
            found["events"],
            json!([{"class": "duplicate_work", "files": ["a.go", "b.go"], "lines": 4, "waste": {"B": 4},
                "detected": []}])
        );
        assert_eq!(waste(&found)["lines"], 4);
        let no = classified(run.dir.path(), &result, false);
        let found = collisions(run.dir.path(), &result, &no);
        assert_eq!(
            (found["status"].clone(), found["lines"].clone()),
            (json!("complete"), json!(0))
        );
    }

    #[test]
    fn classify_shows_pairs_without_run_or_arm_and_keeps_each_answer_in_the_batch() {
        let mut run = GitRun::new(&[("a.go", "package p\n"), ("b.go", "package p\n")]);
        let initial = run.initial.clone();
        let a = run.share("A", &initial, &[("a.go", NET_A)], 10.0);
        let b = run.share("B", &initial, &[("b.go", NET_B)], 20.0);
        let merged = run.merge(&a, &b, &[]);
        run.measure(&[("A", &merged, true, 30.0)]);
        crate::write(run.dir.path().join("controller/result.json"), &json!({"arm": "git", "team_size": 2,
            "initial_snapshot": initial, "final_snapshot": merged, "timing": {"start_unix": 1000.0},
            "validation_attempts": [{"number": 1, "candidate": merged, "passed": true, "conflict": null}]})).unwrap();
        let batch = tempfile::tempdir().unwrap();
        crate::write(
            batch.path().join("batch.json"),
            &json!({"runs": [{"run_dir": run.dir.path(),
            "run_id": "go-page-1-git", "arm": "git", "fixture": "go-page", "scored": true}]}),
        )
        .unwrap();
        let mut shown = vec![];
        classify(batch.path(), "y\n".as_bytes(), &mut shown).unwrap();
        let shown = String::from_utf8(shown).unwrap();
        assert!(
            shown.contains("Pair 1 of 1") && shown.contains("func NetB"),
            "{shown}"
        );
        assert!(
            !shown.contains("git") && !shown.contains("final"),
            "{shown}"
        );
        let saved = read(batch.path().join("duplicates.json")).unwrap();
        assert_eq!(saved["pairs"][0]["duplicate"], true);
        assert_eq!(saved["pairs"][0]["run_id"], "go-page-1-git");
        // Proposing again keeps the answer and asks nothing.
        let mut again = vec![];
        classify(batch.path(), "".as_bytes(), &mut again).unwrap();
        assert!(!String::from_utf8(again).unwrap().contains("Pair 1"));
        let result = &crate::batch::results(batch.path()).unwrap()[0];
        assert_eq!(result["discarded_or_rewritten_work"]["lines"], 4);
        assert_eq!(
            result["collisions"]["by_class"]["duplicate_work"],
            json!({"count": 1, "lines": 4})
        );
    }

    #[test]
    fn a_requirement_names_its_files_and_bare_packages_but_not_their_tests() {
        let entries = [
            "T1 core/money.go: Money in cents".to_string(),
            "T3 discount/: percent".into(),
        ];
        assert_eq!(named_by(&entries, "core/money.go"), [0].into());
        assert_eq!(
            named_by(&entries, "core/model.go"),
            BTreeSet::new(),
            "core/ is a file path there"
        );
        assert_eq!(named_by(&entries, "discount/percent.go"), [1].into());
        assert_eq!(
            named_by(&entries, "discount/percent_test.go"),
            BTreeSet::new()
        );
        let methods = go_functions(
            "m.go",
            "package p\nfunc (m *Money) String() string {\n    return \"\"\n}\nfunc F[T any](t T) {}\n",
        );
        assert_eq!(
            methods,
            [("Money.String".to_string(), 1, 3), ("F".into(), 4, 4)]
        );
    }

    #[test]
    fn an_incorporation_across_split_workspaces_is_a_line_conflict_wasting_what_it_dropped() {
        // agent-0 adds a and b in space 0; agent-1, split off into space 1, adds c; after
        // agent-0's publication, agent-1 incorporates keeping a and dropping its c.
        let space = |mut c: Value, s: u64| {
            c["space"] = s.into();
            c
        };
        let c_only = "package p\nfunc H() {\n    c := 3\n    return\n}\n";
        let mut incorporation = applied(
            3,
            1,
            space(capture(2, &[2], &[("h.go", 2, c_only)]), 1),
            space(capture(3, &[1, 2, 3], &[("h.go", 3, V1)]), 1),
            json!({"h.go": file(V1)}),
        );
        incorporation["incorporates"] = 1.into();
        let history = json!([
            applied(
                1,
                0,
                capture(0, &[], &[("h.go", 0, V0)]),
                capture(1, &[1], &[("h.go", 1, V1)]),
                json!({"h.go": file(V1)})
            ),
            applied(
                2,
                1,
                space(capture(0, &[], &[("h.go", 0, V0)]), 1),
                space(capture(2, &[2], &[("h.go", 2, c_only)]), 1),
                json!({"h.go": file(c_only)})
            ),
            incorporation,
            // A failed check of the result, then agent-1 rewrites agent-0's incorporated line.
            applied(
                4,
                1,
                space(capture(3, &[1, 2, 3], &[("h.go", 3, V1)]), 1),
                space(capture(4, &[1, 2, 3, 4], &[("h.go", 4, V0_B)]), 1),
                json!({"h.go": file(V0_B)})
            ),
        ]);
        let checks = json!([{"number": 1, "passed": false, "revision": 3, "requested_by": "agent-1"},
            {"number": 2, "passed": true, "revision": 4, "requested_by": "agent-1"}]);
        let (dir, result) = falinks_run(history, checks);
        assert_eq!(
            collisions(dir.path(), &result, &Value::Null)["events"],
            json!([{"class": "line_conflict", "files": ["h.go"], "lines": 1, "waste": {"agent-1": 1},
                "detected": [{"agent": "agent-1", "signal": "incorporate", "operation": 3, "request_id": "r3"}]},
                {"class": "rework", "files": ["h.go"], "lines": 1, "waste": {"agent-0": 1},
                "detected": [{"agent": "agent-1", "signal": "broken_check", "check": 1, "revision": 3}]}]),
            "incorporated lines keep their author"
        );
    }

    #[test]
    fn a_team_of_one_has_no_collisions_and_no_waste() {
        let result = json!({"arm": "git", "team_size": 1});
        let found = collisions(Path::new("/nonexistent"), &result, &Value::Null);
        assert_eq!(found["status"], "n/a");
        assert_eq!(waste(&found)["status"], "n/a");
        assert_eq!(
            candidates(Path::new("/nonexistent"), &result).unwrap(),
            Vec::<Value>::new()
        );
        let missing = collisions(
            Path::new("/nonexistent"),
            &json!({"arm": "git", "team_size": 2}),
            &Value::Null,
        );
        assert_eq!(
            missing["status"], "unavailable",
            "unreadable records are missing, never zero"
        );
    }

    #[test]
    fn a_duplicate_discarded_before_the_final_tree_wastes_its_written_lines() {
        let rev = |r, inc: &[u64], a: (u64, &str), b: (u64, &str)| {
            capture(r, inc, &[("a.go", a.0, a.1), ("b.go", b.0, b.1)])
        };
        let empty = "package p\n";
        let history = json!([
            applied(
                1,
                0,
                rev(0, &[], (0, empty), (0, empty)),
                rev(1, &[1], (1, NET_A), (0, empty)),
                json!({"a.go": file(NET_A)})
            ),
            applied(
                2,
                1,
                rev(1, &[1], (1, NET_A), (0, empty)),
                rev(2, &[1, 2], (1, NET_A), (2, NET_B)),
                json!({"b.go": file(NET_B)})
            ),
            applied(
                3,
                0,
                rev(2, &[1, 2], (1, NET_A), (2, NET_B)),
                rev(3, &[1, 2, 3], (3, empty), (2, NET_B)),
                json!({"a.go": file(empty)})
            ),
        ]);
        let (dir, result) = falinks_run(
            history,
            json!([{"number": 1, "passed": true, "revision": 3, "requested_by": "final"}]),
        );
        let pairs = candidates(dir.path(), &result).unwrap();
        let states: Vec<&Value> = pairs[0]["sides"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| &s["state"])
            .collect();
        assert_eq!(states, ["discarded", "final"]);
        let found = collisions(dir.path(), &result, &classified(dir.path(), &result, true));
        assert_eq!(
            found["events"],
            json!([{"class": "duplicate_work", "files": ["a.go", "b.go"], "lines": 4, "waste": {"agent-0": 4},
                "detected": []}])
        );
    }
}
