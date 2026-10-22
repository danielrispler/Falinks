//! Deterministic code-quality metrics (#228 stage 2, #230), by the frozen definitions in
//! `code-quality.json`. `quality --runs DIR` measures every run of a finished batch directory
//! and writes `DIR/quality.json`, never into a run directory; `report` reads it into each
//! fixture's `code_quality`. Only runs whose final oracle passed are measured. Each measured
//! run's final snapshot is exported from its arm's own run storage; the pinned formatters run
//! on copies of it, of the initial tree and of the reference, and every metric uses those
//! copies. Formatters and lints run through the oracle's contained check path.
use crate::{
    Output, Result, SCRATCH_DIRS, Sandbox, bail, blobs, check_denied, check_env, command, fixture,
    frozen, git, protected, read, resolve, restore, root, strings, utc_now, write,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub type Files = BTreeMap<String, Vec<u8>>;

const TIMEOUT: Duration = Duration::from_secs(300);
const VERSIONS: [(&str, &[&str]); 4] = [
    ("go", &["go", "version"]),
    ("rustc", &["rustc", "-V"]),
    ("clippy", &["clippy-driver", "-V"]),
    ("rustfmt", &["rustfmt", "-V"]),
];

/// A private copy beneath a fresh temporary directory, run like the oracle's checks: the
/// same environment, no network, and the checkout, its Git storage and `denied` unreadable.
struct Contained {
    _tmp: tempfile::TempDir,
    private: PathBuf,
    runs: std::cell::Cell<usize>,
    denied: Vec<PathBuf>,
}

impl Contained {
    fn new(denied: Vec<PathBuf>) -> Result<Self> {
        let tmp = tempfile::Builder::new()
            .prefix("falinks-quality-")
            .tempdir()?;
        let private = resolve(tmp.path()).join("private");
        fs::create_dir(&private)?;
        Ok(Contained {
            runs: Default::default(),
            _tmp: tmp,
            private,
            denied,
        })
    }

    /// The latest run's copy.
    fn work(&self) -> PathBuf {
        self.private.join(format!("check-{}", self.runs.get()))
    }

    /// Runs argv on a fresh copy of `files`. Only a formatter (`rewrite`) may change the copy;
    /// it is returned as the run left it.
    fn run(&self, files: &Files, argv: &[&str], rewrite: bool) -> Result<(Output, Files)> {
        // A fresh directory per run: a tool's lingering helper may still hold the last one.
        self.runs.set(self.runs.get() + 1);
        let work = self.work();
        restore(&work, files)?;
        let writable = if rewrite {
            vec![work.clone()]
        } else {
            SCRATCH_DIRS.map(|n| work.join(n)).to_vec()
        };
        let sandbox = Sandbox {
            writable,
            denied: check_denied(&self.private, &work, &self.denied)?,
            reopened: Vec::new(),
            network: false,
        };
        let output = command(
            &sandbox.wrap(strings(argv))?,
            &work,
            &check_env(&work),
            TIMEOUT,
        )?;
        let mut after = Files::new();
        for name in files.keys() {
            after.insert(name.clone(), fs::read(work.join(name))?);
        }
        Ok((output, after))
    }

    /// The pinned formatter over every source file (#228 decision 6); its error otherwise.
    fn format(&self, files: &Files, rust: bool) -> Result<Files> {
        let mut argv = if rust {
            vec!["rustfmt", "--edition", "2021"]
        } else {
            vec!["gofmt", "-w"]
        };
        let extension = if rust { ".rs" } else { ".go" };
        argv.extend(
            files
                .keys()
                .filter(|n| n.ends_with(extension))
                .map(String::as_str),
        );
        let (output, after) = self.run(files, &argv, true)?;
        if !output.success {
            bail!("{} failed: {}", argv[0], output.text().trim());
        }
        Ok(after)
    }

    /// Findings of `go vet` or `clippy` as (lint, file, line), de-duplicated (#228 decision 7).
    fn lints(&self, files: &Files, rust: bool) -> Result<BTreeSet<(String, String, usize)>> {
        let argv: &[&str] = if rust {
            &[
                "cargo",
                "clippy",
                "--offline",
                "--locked",
                "--all-targets",
                "--message-format=json",
            ]
        } else {
            &["go", "vet", "-json", "./..."]
        };
        let (output, _) = self.run(files, argv, false)?;
        if !output.success {
            bail!("{} failed: {}", argv.join(" "), output.text().trim());
        }
        let mut found = BTreeSet::new();
        if rust {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let Ok(row) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                let message = &row["message"];
                // Summary lines ("N warnings emitted") carry no lint code.
                let (Some(lint), true) = (
                    message["code"]["code"].as_str(),
                    row["reason"] == "compiler-message",
                ) else {
                    continue;
                };
                let spans = message["spans"].as_array().into_iter().flatten();
                for span in spans.filter(|s| s["is_primary"] == true) {
                    if let (Some(file), Some(line)) =
                        (span["file_name"].as_str(), span["line_start"].as_u64())
                    {
                        found.insert((lint.to_string(), file.to_string(), line as usize));
                    }
                }
            }
            return Ok(found);
        }
        // `go vet -json` writes one JSON object per package, after a `# package` line on older Go
        // versions, to stderr or (newer) stdout.
        let json: String = output
            .text()
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let prefix = format!("{}/", self.work().display());
        for object in serde_json::Deserializer::from_str(&json).into_iter::<Value>() {
            for analyzers in object?.as_object().into_iter().flat_map(Map::values) {
                for (lint, diagnostics) in analyzers.as_object().into_iter().flatten() {
                    let Some(diagnostics) = diagnostics.as_array() else {
                        bail!("go vet {lint} failed: {diagnostics}");
                    };
                    for posn in diagnostics.iter().filter_map(|d| d["posn"].as_str()) {
                        let posn = posn.strip_prefix(&prefix).unwrap_or(posn);
                        let mut parts = posn.rsplitn(3, ':');
                        let (_, line, file) = (parts.next(), parts.next(), parts.next());
                        if let (Some(file), Some(Ok(line))) = (file, line.map(str::parse)) {
                            found.insert((lint.clone(), file.to_string(), line));
                        }
                    }
                }
            }
        }
        Ok(found)
    }
}

/// The 1-based lines of `old` and of `new` outside one longest common subsequence: removed
/// and added lines.
pub fn changed(old: &[&str], new: &[&str]) -> (Vec<usize>, Vec<usize>) {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut removed, mut added, mut i, mut j) = (vec![], vec![], 0, 0);
    while i < n || j < m {
        if i < n && j < m && old[i] == new[j] {
            (i, j) = (i + 1, j + 1);
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            j += 1;
            added.push(j);
        } else {
            i += 1;
            removed.push(i);
        }
    }
    (removed, added)
}

/// Whether each line of a file is test code: Go `_test.go` files, Rust files under `tests/`,
/// and Rust `#[cfg(test)]` items (#228 decision 2).
pub fn test_lines(name: &str, lines: &[&str]) -> Vec<bool> {
    if name.ends_with("_test.go") || (name.starts_with("tests/") && name.ends_with(".rs")) {
        return vec![true; lines.len()];
    }
    let mut test = vec![false; lines.len()];
    if !name.ends_with(".rs") {
        return test;
    }
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]" {
            // shortcut: counts braces without lexing strings, chars or comments, which holds
            // for formatted fixture code; lex them if a test item ever holds an unbalanced one.
            let (start, mut depth, mut opened) = (i, 0i64, false);
            loop {
                for c in lines[i].chars() {
                    match c {
                        '{' => (depth, opened) = (depth + 1, true),
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                let ended = if opened {
                    depth <= 0
                } else {
                    lines[i].trim_end().ends_with(';')
                };
                if ended || i + 1 == lines.len() {
                    break;
                }
                i += 1;
            }
            test[start..=i].fill(true);
        }
        i += 1;
    }
    test
}

/// The formatted diff's figures (#228 decisions 8 and 9), and the lines it added as (file, line).
struct Change {
    production_lines_changed: usize,
    test_lines_added: usize,
    test_functions: usize,
    added: BTreeSet<(String, usize)>,
}

fn change(initial: &Files, last: &Files) -> Change {
    let mut out = Change {
        production_lines_changed: 0,
        test_lines_added: 0,
        test_functions: 0,
        added: BTreeSet::new(),
    };
    let names: BTreeSet<&String> = initial.keys().chain(last.keys()).collect();
    for name in names {
        let text = |files: &Files| {
            String::from_utf8_lossy(files.get(name).map_or(&[][..], Vec::as_slice)).into_owned()
        };
        let (old, new) = (text(initial), text(last));
        let (old, new): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
        let (old_test, new_test) = (test_lines(name, &old), test_lines(name, &new));
        let (removed, added) = changed(&old, &new);
        out.production_lines_changed += removed.iter().filter(|l| !old_test[*l - 1]).count();
        for line in added {
            out.added.insert((name.clone(), line));
            if !new_test[line - 1] {
                out.production_lines_changed += 1;
                continue;
            }
            out.test_lines_added += 1;
            let text = new[line - 1].trim_start();
            if text.starts_with("#[test]") || text.starts_with("func Test") {
                out.test_functions += 1;
            }
        }
    }
    out
}

/// One metric's value, or `missing` with the reason.
fn missing(reason: impl ToString) -> Value {
    json!({"missing": reason.to_string()})
}

/// The final snapshot's exact bytes, from the arm's own run storage (#228, "Run storage"):
/// Git's agent repository, or Falinks's oracle candidate repository that holds it.
fn final_snapshot(run_dir: &Path, result: &Value) -> Result<Files> {
    let sha = result["final_snapshot"]
        .as_str()
        .ok_or("the run records no final_snapshot")?;
    let repo = if result["arm"] == "falinks" {
        let oracle = run_dir.join("controller/oracle");
        let candidates = fs::read_dir(&oracle).map_err(|e| format!("{}: {e}", oracle.display()))?;
        candidates
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("candidate-"))
            })
            .find(|p| git(p, &["rev-parse", "HEAD"], None).ok().as_deref() == Some(sha))
            .ok_or("no oracle candidate repository holds the final snapshot")?
    } else {
        run_dir.join("agents/repo")
    };
    blobs(&repo, sha)
}

/// One fixture's formatted initial tree and the formatted reference's own figure.
fn baseline(space: &Contained, name: &str) -> Result<(Files, bool, Value)> {
    let fixture = fixture(name)?;
    let rust = fixture["language"] == "rust";
    let files = |value: &Value| -> Files {
        value
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.as_str().unwrap_or_default().as_bytes().to_vec(),
                )
            })
            .collect()
    };
    let initial = space.format(&files(&fixture["files"]), rust)?;
    let mut reference = files(&fixture["files"]);
    reference.extend(files(&protected(name)?["reference"]));
    let figure = match space.format(&reference, rust) {
        Ok(reference) => json!({"value": change(&initial, &reference).production_lines_changed}),
        Err(error) => missing(error),
    };
    Ok((initial, rust, figure))
}

/// One oracle-passing run's metrics, and its formatted final snapshot; a tool failure makes
/// only its own metrics missing.
fn measure(space: &Contained, initial: &Files, rust: bool, last: &Files) -> (Value, Option<Files>) {
    let lint_metric = if rust {
        "clippy_findings"
    } else {
        "vet_findings"
    };
    let names = [
        "production_lines_changed",
        "test_lines_added",
        "test_functions",
        lint_metric,
    ];
    let last = match space.format(last, rust) {
        Ok(last) => last,
        Err(error) => {
            let metrics = names.map(|n| (n.to_string(), missing(&error)));
            return (metrics.into_iter().collect(), None);
        }
    };
    let diff = change(initial, &last);
    let lints = match space.lints(&last, rust) {
        Ok(found) => {
            let mut per_lint = BTreeMap::<String, u64>::new();
            let counted = found
                .into_iter()
                .filter(|(_, file, line)| diff.added.contains(&(file.clone(), *line)));
            for (lint, _, _) in counted {
                *per_lint.entry(lint).or_default() += 1;
            }
            json!({"value": per_lint.values().sum::<u64>(), "lints": per_lint})
        }
        Err(error) => missing(error),
    };
    let metrics = json!({
        "production_lines_changed": {"value": diff.production_lines_changed},
        "test_lines_added": {"value": diff.test_lines_added},
        "test_functions": {"value": diff.test_functions},
        lint_metric: lints,
    });
    (metrics, Some(last))
}

/// Measures every run of each batch directory and writes its `quality.json`. With `judge`, a
/// run configuration, the judge also rates each measured final snapshot (#232).
pub fn quality(dirs: &[&Path], judge: Option<&Path>) -> Result<i32> {
    // The initial trees and references come from the frozen materials only.
    frozen()?;
    let definitions = crate::judge::definitions()?;
    let runtime = judge
        .map(|config| crate::judge::Runtime::load(config, &definitions))
        .transpose()?;
    let dirs: Vec<PathBuf> = dirs.iter().map(|d| resolve(d)).collect();
    let commit = git(&root(), &["rev-parse", "HEAD"], None)?;
    for dir in &dirs {
        let ledger = read(dir.join("batch.json"))?;
        // Agent code never reads another run: every batch directory is denied, as each run's own.
        let space = Contained::new(dirs.clone())?;
        let mut versions = Map::new();
        for (name, argv) in VERSIONS {
            let version = match space.run(&Files::new(), argv, false) {
                Ok((output, _)) if output.success => output.text().trim().to_string(),
                Ok((output, _)) => format!("unavailable: {}", output.text().trim()),
                Err(error) => format!("unavailable: {error}"),
            };
            versions.insert(name.into(), version.into());
        }
        let invocations = ledger["invocations"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let recorded = |key: &str| -> BTreeSet<String> {
            invocations
                .iter()
                .filter_map(|i| i[key].as_str().map(String::from))
                .collect()
        };
        let batch_go = recorded("go");
        let go = versions["go"].as_str().unwrap_or_default();
        // #228 decision 26: the tools' Go must be the Go the batch's checks resolved.
        let go_note = if batch_go.is_empty() {
            Some(
                "The batch recorded no Go version, so the Go its checks resolved is unknown."
                    .to_string(),
            )
        } else if batch_go.len() > 1 || !batch_go.contains(go) {
            Some(format!(
                "The batch's checks resolved {batch_go:?}; these tools used {go}."
            ))
        } else {
            None
        };
        let mut measurement = json!({"utc": utc_now(), "commit": commit,
            "batch_commits": recorded("commit")});
        if recorded("commit").is_empty() {
            measurement["batch_commit_note"] =
                "batch.json records no commit: it predates #230".into();
        }
        let mut fixtures: BTreeMap<String, std::result::Result<(Files, bool, Value), String>> =
            BTreeMap::new();
        let mut runs = Map::new();
        // Without --judge, a run keeps the paid ratings an earlier measurement recorded.
        let previous = read(dir.join("quality.json")).unwrap_or(Value::Null);
        for entry in ledger["runs"].as_array().into_iter().flatten() {
            let id = entry["run_id"].as_str().unwrap_or_default();
            let recorded_dir = PathBuf::from(entry["run_dir"].as_str().unwrap_or_default());
            // A batch directory moved after the run still holds the run under its ID.
            let run_dir = if recorded_dir.is_dir() {
                recorded_dir
            } else {
                dir.join(id)
            };
            let result = read(run_dir.join("controller/result.json"))
                .unwrap_or_else(|_| json!({"outcome": "infrastructure_failure"}));
            let outcome = result["outcome"]
                .as_str()
                .unwrap_or("infrastructure_failure");
            let name = entry["fixture"].as_str().unwrap_or_default().to_string();
            let mut record = json!({"fixture": name, "arm": entry["arm"], "outcome": outcome,
                "measurement": measurement});
            if outcome != "success" {
                record["missing"] = format!("not measured: outcome {outcome}").into();
                runs.insert(id.into(), record);
                continue;
            }
            let base = fixtures
                .entry(name.clone())
                .or_insert_with(|| baseline(&space, &name).map_err(|e| e.to_string()));
            let (initial, rust, reference) = match base {
                Ok(base) => base,
                Err(error) => {
                    record["missing"] = format!("initial tree: {error}").into();
                    runs.insert(id.into(), record);
                    continue;
                }
            };
            record["versions"] = versions.clone().into();
            record["go_note"] = json!(go_note);
            record["reference_production_lines_changed"] = reference.clone();
            let formatted = match final_snapshot(&run_dir, &result) {
                Ok(last) => {
                    let (metrics, formatted) = measure(&space, initial, *rust, &last);
                    record["metrics"] = metrics;
                    formatted
                }
                Err(error) => {
                    record["missing"] = format!("final snapshot: {error}").into();
                    record["metrics"] = Value::Null;
                    None
                }
            };
            if let Some(runtime) = &runtime {
                record["judge"] = match formatted {
                    Some(last) => {
                        let material = crate::judge::material(initial, &last);
                        let requirements = crate::judge::requirements(&fixture(&name)?);
                        let arm = entry["arm"].as_str().unwrap_or_default();
                        let mut call = |prompt: &str| runtime.call(prompt);
                        crate::judge::judge(&mut call, &definitions, &requirements, &material, arm)
                    }
                    None => json!({"missing": "not judged: the final snapshot was not formatted"}),
                };
            } else if let Some(judged) = previous["runs"][id].get("judge") {
                record["judge"] = judged.clone();
            }
            runs.insert(id.into(), record);
        }
        write(
            dir.join("quality.json"),
            &json!({"measurement": measurement, "runs": runs}),
        )?;
        println!("{}", dir.join("quality.json").display());
    }
    Ok(0)
}
