//! Line authorship (#325), one model for `timing`'s changed bytes and `collisions`' waste, so
//! both credit every line the same way.
//!
//! - Each version of a file is traced through its bases by one longest-common-subsequence line
//!   diff: a line the diff matches in a base keeps that line's origin, the first base first;
//!   every other line is the version's author's.
//! - A text seen before keeps its first origins, so a byte-identical copy of a peer's earlier
//!   version stays the peer's.
//! - **Falinks** (`controller/history.json`): one version per output path of each applied
//!   operation, in operation order, over the path's file in the operation's exact `before`
//!   revision. An `incorporate` also takes the peer's latest version as a base, so the peer's
//!   lines it installs stay the peer's.
//! - **Git** (`agents/repo`): every commit, share by share in the order they were made, over
//!   its parents. A commit on the first-parent line of an agent's shares is that agent's, the
//!   earliest sharer first; the initial commit is initial; a merge, or any other commit,
//!   introduces no one's lines.
use crate::{Result, blobs, git, jsonl};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A Falinks agent's name in every figure.
pub fn agent_name(agent: impl std::fmt::Display) -> String {
    format!("agent-{agent}")
}

/// A text's lines, each with its line ending.
pub fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The text of a history file value, `{"bytes": [...]}`; a deleted or absent file is None.
pub fn text(file: &Value) -> Option<String> {
    let bytes: Vec<u8> = file["bytes"]
        .as_array()?
        .iter()
        .map(|b| b.as_u64().map(|b| b as u8))
        .collect::<Option<_>>()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// For each line of `b`, the line of `a` that a longest-common-subsequence line diff matches
/// it to.
pub fn matches(a: &[&str], b: &[&str]) -> Vec<Option<usize>> {
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

/// Who introduced a line.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Initial,
    /// An agent, and the operation (Falinks) or traced commit (Git) that wrote the line.
    Agent(String, u64),
    /// A merge or conflict resolution, or a base no record shows.
    Nobody,
}

/// One line of a version: its origin and its index in the version that introduced it. With
/// the path, it names a written line once across every version that carries it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Line {
    pub origin: Origin,
    pub index: usize,
}

impl Line {
    /// The agent that wrote the line, and its operation or commit.
    pub fn agent(&self) -> Option<(&str, u64)> {
        match &self.origin {
            Origin::Agent(agent, at) => Some((agent, *at)),
            _ => None,
        }
    }
}

/// The lines of every file version seen, keyed by path and text.
#[derive(Default)]
pub struct Blame(BTreeMap<(String, String), Vec<Line>>);

impl Blame {
    /// The initial files (a fixture's `files`), every line initial.
    pub fn new(files: &Map<String, Value>) -> Self {
        let mut blame = Blame::default();
        for (path, text) in files {
            let text = text.as_str().unwrap_or_default();
            blame.version(path, Origin::Initial, &[], text);
        }
        blame
    }

    /// `text` at `path`, written by `author` over `bases`; a text seen before keeps its lines.
    pub fn version(&mut self, path: &str, author: Origin, bases: &[&str], text: &str) {
        let key = |text: &str| (path.to_string(), text.to_string());
        if self.0.contains_key(&key(text)) {
            return;
        }
        let new = lines(text);
        let mut out: Vec<Line> = (0..new.len())
            .map(|index| Line {
                origin: author.clone(),
                index,
            })
            .collect();
        for base in bases.iter().rev() {
            let known = self.0.get(&key(base));
            for (j, i) in matches(&lines(base), &new).into_iter().enumerate() {
                if let Some(i) = i {
                    out[j] = known.map_or(
                        Line {
                            origin: Origin::Nobody,
                            index: j,
                        },
                        |k| k[i].clone(),
                    );
                }
            }
        }
        self.0.insert(key(text), out);
    }

    /// The lines of `text` at `path`; empty for a version never traced.
    pub fn lines(&self, path: &str, text: &str) -> &[Line] {
        self.0
            .get(&(path.to_string(), text.to_string()))
            .map_or(&[], Vec::as_slice)
    }
}

/// An applied Falinks operation: its outcome is `Applied` and it records its result.
pub fn applied(record: &Value) -> bool {
    record["outcome"].get("Applied").is_some() && record["proposed"].is_object()
}

/// A Falinks run's authorship from its history records, and each agent's last version of every
/// path it wrote, keyed by agent and path.
pub fn falinks(
    records: &[Value],
    files: &Map<String, Value>,
) -> (Blame, BTreeMap<(String, String), String>) {
    let mut blame = Blame::new(files);
    let mut records: Vec<&Value> = records.iter().filter(|r| applied(r)).collect();
    records.sort_by_key(|r| r["operation"].as_u64());
    // Every version written so far, in order: agent, path and text.
    let mut written: Vec<(String, String, String)> = vec![];
    let mut last = BTreeMap::new();
    for record in records {
        let agent = agent_name(&record["agent"]);
        let op = record["operation"].as_u64().unwrap_or(0);
        for path in record["request"]["output"]
            .as_object()
            .into_iter()
            .flat_map(|o| o.keys())
        {
            // A path absent from a revision, or deleted, is empty.
            let file = |capture: &Value| text(&capture["files"][path]["file"]).unwrap_or_default();
            let (base, version) = (file(&record["before"]), file(&record["proposed"]));
            let mut bases = vec![base.as_str()];
            let peer = written
                .iter()
                .rev()
                .find(|(a, p, _)| p == path && *a != agent);
            if let (true, Some((_, _, theirs))) = (record["incorporates"].is_u64(), peer) {
                bases.push(theirs);
            }
            blame.version(path, Origin::Agent(agent.clone(), op), &bases, &version);
            written.push((agent.clone(), path.clone(), version.clone()));
            last.insert((agent.clone(), path.clone()), version);
        }
    }
    (blame, last)
}

/// A Git run's shares from `controller/events.jsonl`, in the order they were made: agent,
/// commit and seconds after the run's start.
pub fn shares(run: &Path) -> Result<Vec<(String, String, f64)>> {
    let rows = jsonl(&run.join("controller/events.jsonl"))?;
    let share = |row: &Value| {
        let field = |key: &str| row[key].as_str().map(String::from);
        Some((field("agent")?, field("commit")?, row["elapsed"].as_f64()?))
    };
    Ok(rows
        .iter()
        .filter(|row| row["kind"] == "draft_shared")
        .filter_map(share)
        .collect())
}

/// A Git run's authorship over `agents/repo`, with the tree of every commit it traced.
pub struct Repo {
    dir: PathBuf,
    /// The agent whose first-parent share line holds each commit, the earliest sharer first.
    owners: BTreeMap<String, String>,
    trees: BTreeMap<String, BTreeMap<String, String>>,
    pub blame: Blame,
}

impl Repo {
    /// Traces every one of the run's `shares`.
    pub fn new(
        run: &Path,
        shares: &[(String, String, f64)],
        files: &Map<String, Value>,
    ) -> Result<Self> {
        let dir = &run.join("agents/repo");
        let mut owners = BTreeMap::new();
        for (agent, share, _) in shares {
            for commit in git(dir, &["rev-list", "--first-parent", share], None)?.lines() {
                owners.entry(commit.to_string()).or_insert(agent.clone());
            }
        }
        let mut repo = Repo {
            dir: dir.to_path_buf(),
            owners,
            trees: BTreeMap::new(),
            blame: Blame::new(files),
        };
        // Share by share, in order, so a text shared earlier keeps its authorship.
        for (_, share, _) in shares {
            repo.trace(share)?;
        }
        Ok(repo)
    }

    /// Traces `commit` and every ancestor not traced yet, parents first.
    fn trace(&mut self, commit: &str) -> Result<()> {
        if self.trees.contains_key(commit) {
            return Ok(());
        }
        let args = ["rev-list", "--topo-order", "--reverse", "--parents", commit];
        for line in git(&self.dir, &args, None)?.lines() {
            let (commit, parents) = line.split_once(' ').unwrap_or((line, ""));
            let parents: Vec<&str> = parents.split_whitespace().collect();
            if self.trees.contains_key(commit) {
                continue;
            }
            let order = self.trees.len() as u64;
            // The initial commit has no parent; a merge introduces no one's lines.
            let author = match (parents.len(), self.owners.get(commit)) {
                (0, _) => Origin::Initial,
                (1, Some(agent)) => Origin::Agent(agent.clone(), order),
                _ => Origin::Nobody,
            };
            let tree: BTreeMap<String, String> = blobs(&self.dir, commit)?
                .into_iter()
                .map(|(path, bytes)| (path, String::from_utf8_lossy(&bytes).into_owned()))
                .collect();
            for (path, text) in &tree {
                let bases: Vec<&str> = parents
                    .iter()
                    .map(|p| self.trees[*p].get(path).map_or("", String::as_str))
                    .collect();
                self.blame.version(path, author.clone(), &bases, text);
            }
            self.trees.insert(commit.to_string(), tree);
        }
        Ok(())
    }

    /// The tree of `commit`, traced.
    pub fn tree(&mut self, commit: &str) -> Result<&BTreeMap<String, String>> {
        self.trace(commit)?;
        Ok(&self.trees[commit])
    }

    /// `path` at `commit` (empty when absent) and its lines.
    pub fn file(&mut self, commit: &str, path: &str) -> Result<(String, Vec<Line>)> {
        let text = self.tree(commit)?.get(path).cloned().unwrap_or_default();
        let lines = self.blame.lines(path, &text).to_vec();
        Ok((text, lines))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copied_version_keeps_its_first_authorship() {
        let mut blame = Blame::default();
        let b = Origin::Agent("B".into(), 1);
        blame.version("a.go", Origin::Initial, &[], "a\n");
        blame.version("a.go", b.clone(), &["a\n"], "a\nb\n");
        // A copies B's file whole, with no merge: "b\n" stays B's.
        blame.version("a.go", Origin::Agent("A".into(), 2), &["a\n"], "a\nb\n");
        let origins: Vec<&Origin> = blame
            .lines("a.go", "a\nb\n")
            .iter()
            .map(|l| &l.origin)
            .collect();
        assert_eq!(origins, [&Origin::Initial, &b]);
    }

    #[test]
    fn an_incorporation_keeps_the_peers_installed_lines() {
        let files: Map<String, Value> = [("h.go".to_string(), Value::from("x\n"))]
            .into_iter()
            .collect();
        let record = |op: u64, agent: u64, before: &str, after: &str| {
            let capture = |text: &str| serde_json::json!({"files": {"h.go": {"file": {"bytes": text.as_bytes()}}}});
            serde_json::json!({"operation": op, "agent": agent, "outcome": {"Applied": {}},
                "request": {"output": {"h.go": {}}}, "before": capture(before),
                "proposed": capture(after), "incorporates": null})
        };
        let mut incorporation = record(3, 1, "x\nc\n", "x\na\nc\n");
        incorporation["incorporates"] = 1.into();
        let records = [
            record(1, 0, "x\n", "x\na\n"),
            record(2, 1, "x\n", "x\nc\n"),
            incorporation,
        ];
        let (blame, last) = falinks(&records, &files);
        let final_text = &last[&("agent-1".to_string(), "h.go".to_string())];
        let agents: Vec<Option<(&str, u64)>> = blame
            .lines("h.go", final_text)
            .iter()
            .map(Line::agent)
            .collect();
        assert_eq!(agents, [None, Some(("agent-0", 1)), Some(("agent-1", 2))]);
    }
}
