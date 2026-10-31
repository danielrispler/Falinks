//! The judge and the blinding check (#228 stage 2, #232), by the frozen definitions in
//! `code-quality.json`. `quality --runs DIR --judge CONFIG` rates each measured final snapshot
//! with the pinned runtime and model of the run configuration CONFIG: no tools, a fresh session
//! per call and the workers' isolation flags. Each call's usage is recorded.
use crate::quality::{Files, changed, test_lines};
use crate::{Result, bail, command, read, resolve, root, sha256};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The rated dimensions, in the order the frozen prompt's reply form names them.
pub const DIMENSIONS: [&str; 4] = [
    "maintainability",
    "duplication",
    "unnecessary_changes",
    "test_quality",
];
const TIMEOUT: Duration = Duration::from_secs(300);

/// One call: the reply text, or why there is none; and the call's record, usage included.
pub type Call<'a> = dyn FnMut(&str) -> (std::result::Result<String, String>, Value) + 'a;

/// The pinned runtime, checked against the frozen judge pin and the configuration's hash.
pub struct Runtime {
    binary: PathBuf,
    home: Option<String>,
    model: String,
}

impl Runtime {
    pub fn load(config: &Path, definitions: &Value) -> Result<Self> {
        let config = read(config)?;
        let pin = &definitions["judge"];
        for key in ["runtime_version", "model"] {
            if config[key] != pin[key] {
                bail!(
                    "judge: the run configuration's {key} {} is not the frozen {}",
                    config[key],
                    pin[key]
                );
            }
        }
        let binary = resolve(Path::new(
            config["runtime_binary"]
                .as_str()
                .ok_or("judge: runtime_binary must be a path")?,
        ));
        if config["runtime_sha256"] != sha256(&fs::read(&binary)?).as_str() {
            bail!("judge: runtime binary hash differs from the run configuration");
        }
        Ok(Runtime {
            binary,
            home: config["runtime_home"].as_str().map(String::from),
            model: pin["model"].as_str().unwrap_or_default().into(),
        })
    }

    /// One fresh `claude -p` session with no tools, from an empty directory.
    pub fn call(&self, prompt: &str) -> (std::result::Result<String, String>, Value) {
        let outcome = (|| -> Result<(String, Value)> {
            let cwd = tempfile::Builder::new()
                .prefix("falinks-judge-")
                .tempdir()?;
            let mut env = vec![("DISABLE_AUTOUPDATER".to_string(), "1".to_string())];
            for key in ["PATH", "USER", "HOME", "FALINKS_FAKE_JUDGE"] {
                if let Ok(value) = std::env::var(key) {
                    env.push((key.into(), value));
                }
            }
            if let Some(home) = &self.home {
                // Login state lives in the runtime home.
                env.push(("HOME".into(), home.clone()));
            }
            let argv = [
                self.binary.to_str().unwrap_or_default(),
                "-p",
                // Before the flags: a variadic flag such as `--tools` would take a trailing prompt.
                prompt,
                "--output-format",
                "json",
                "--model",
                &self.model,
                "--setting-sources",
                "",
                "--strict-mcp-config",
                "--disable-slash-commands",
                "--tools",
                "",
                "--permission-mode",
                "dontAsk",
            ];
            let output = command(&crate::strings(&argv), cwd.path(), &env, TIMEOUT)?;
            let reply: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
            let record = json!({"usage": reply["usage"], "model_usage": reply["modelUsage"]});
            match reply["result"].as_str() {
                Some(text) if output.success && reply["is_error"] != true => {
                    Ok((text.to_string(), record))
                }
                _ => Err(format!("runtime call failed: {}", output.text().trim()).into()),
            }
        })();
        match outcome {
            Ok((text, record)) => (Ok(text), record),
            Err(error) => (
                Err(error.to_string()),
                json!({"usage": null, "error": error.to_string()}),
            ),
        }
    }
}

/// The requirement text: the initial requirements, then each milestone's.
pub fn requirements(fixture: &Value) -> String {
    let milestones = fixture["milestones"].as_array().into_iter().flatten();
    std::iter::once(&fixture["requirements"])
        .chain(milestones.map(|m| &m["requirements"]))
        .flat_map(|r| r.as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A source file's production and test lines, with Rust inline test modules moved out.
fn split(name: &str, text: &str) -> (Vec<String>, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let test = test_lines(name, &lines);
    let (mut production, mut tests) = (vec![], vec![]);
    for (line, is_test) in lines.into_iter().zip(test) {
        if is_test { &mut tests } else { &mut production }.push(line.to_string());
    }
    (production, tests)
}

fn sources(files: &Files) -> impl Iterator<Item = (&String, String)> {
    files
        .iter()
        .filter(|(n, _)| n.ends_with(".go") || n.ends_with(".rs"))
        .map(|(n, b)| (n, String::from_utf8_lossy(b).into_owned()))
}

/// The judge's material (#228 decision 16) from the formatted initial tree and final snapshot:
/// no file names, production code apart from all test code, and no reference.
pub struct Material {
    pub initial: String,
    pub change: String,
    pub tests: String,
}

pub fn material(initial: &Files, last: &Files) -> Material {
    let production = |files: &Files| -> Files {
        sources(files)
            .map(|(n, t)| (n.clone(), split(n, &t).0.join("\n").into_bytes()))
            .collect()
    };
    let (old, new) = (production(initial), production(last));
    let blocks = |files: &Files| -> String {
        files
            .values()
            .map(|b| String::from_utf8_lossy(b).trim_end().to_string())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let mut change = vec![];
    let names: std::collections::BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    for name in names {
        let text = |files: &Files| {
            String::from_utf8_lossy(files.get(name).map_or(&[][..], |b| b)).into_owned()
        };
        let (before, after) = (text(&old), text(&new));
        let (before, after): (Vec<&str>, Vec<&str>) =
            (before.lines().collect(), after.lines().collect());
        let (removed, added) = changed(&before, &after);
        if removed.is_empty() && added.is_empty() {
            continue;
        }
        // The whole file as a diff: every line marked unchanged, removed or added.
        let (mut i, mut j, mut lines) = (0, 0, vec![]);
        while i < before.len() || j < after.len() {
            if removed.contains(&(i + 1)) {
                lines.push(format!("-{}", before[i]));
                i += 1;
            } else if added.contains(&(j + 1)) {
                lines.push(format!("+{}", after[j]));
                j += 1;
            } else {
                lines.push(format!(" {}", after[j]));
                (i, j) = (i + 1, j + 1);
            }
        }
        change.push(lines.join("\n"));
    }
    let tests: Files = sources(last)
        .map(|(n, t)| (n.clone(), split(n, &t).1.join("\n").into_bytes()))
        .collect();
    Material {
        initial: blocks(&old),
        change: change.join("\n\n"),
        tests: blocks(&tests),
    }
}

fn sections(requirements: &str, material: &Material) -> String {
    format!(
        "Requirements:\n{requirements}\n\nInitial code:\n{}\n\nChange:\n{}\n\nTest code after the change:\n{}",
        material.initial, material.change, material.tests
    )
}

/// The frozen rating prompt, filled verbatim.
pub fn rating_prompt(definitions: &Value, requirements: &str, material: &Material) -> String {
    let judge = &definitions["judge"];
    let anchors = DIMENSIONS
        .map(|d| {
            let a = &judge["dimensions"][d];
            let anchor = |k: &str| a[k].as_str().unwrap_or_default().to_string();
            format!(
                "{d}:\n1: {}\n3: {}\n5: {}",
                anchor("1"),
                anchor("3"),
                anchor("5")
            )
        })
        .join("\n\n");
    let values = [
        ("{anchors}", anchors.as_str()),
        ("{requirements}", requirements),
        ("{initial}", &material.initial),
        ("{change}", &material.change),
        ("{tests}", &material.tests),
    ];
    // One pass, so a placeholder inside inserted code is never filled in turn.
    let (mut out, mut rest) = (String::new(), judge["prompt"].as_str().unwrap_or_default());
    while let Some(at) = rest.find('{') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match values.iter().find(|(key, _)| rest.starts_with(key)) {
            Some((key, value)) => {
                out.push_str(value);
                rest = &rest[key.len()..];
            }
            None => {
                out.push('{');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

/// The frozen forced-choice question, on the same material.
pub fn blinding_prompt(definitions: &Value, requirements: &str, material: &Material) -> String {
    let question = definitions["judge"]["blinding"]["question"]
        .as_str()
        .unwrap_or_default();
    format!("{question}\n\n{}", sections(requirements, material))
}

/// Each dimension's (rating, reason), or why the reply is unparseable.
fn ratings(text: &str) -> std::result::Result<Vec<(u64, String)>, String> {
    let (start, end) = (text.find('{'), text.rfind('}'));
    let object = match (start, end) {
        (Some(s), Some(e)) if s < e => &text[s..=e],
        _ => return Err(format!("unparseable: no JSON object in {text:?}")),
    };
    let reply: Value =
        serde_json::from_str(object).map_err(|e| format!("unparseable: {e} in {text:?}"))?;
    DIMENSIONS
        .iter()
        .map(|d| {
            let rating = reply[d]["rating"].as_u64().filter(|r| (1..=5).contains(r));
            let reason = reply[d]["reason"].as_str().filter(|r| !r.trim().is_empty());
            match (rating, reason) {
                (Some(r), Some(reason)) => Ok((r, reason.to_string())),
                _ => Err(format!(
                    "unparseable: {d} lacks a rating from 1 to 5 or a reason"
                )),
            }
        })
        .collect()
}

/// The guessed arm, or why the reply is unparseable.
fn guess(text: &str, definitions: &Value) -> std::result::Result<String, String> {
    let letter = text.trim().trim_end_matches('.');
    definitions["judge"]["blinding"]["answers"][letter]
        .as_str()
        .map(String::from)
        .ok_or_else(|| format!("unparseable guess: {text:?}"))
}

fn median(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

/// One snapshot's ratings (#228 decisions 17-18), and each call's record. Each of the k
/// rating calls is retried once when it fails or is unparseable; any rating still missing
/// makes the ratings missing.
pub fn rate(
    call: &mut Call,
    definitions: &Value,
    requirements: &str,
    material: &Material,
) -> (Value, Vec<Value>) {
    let k = definitions["judge"]["k"].as_u64().unwrap_or(3);
    let prompt = rating_prompt(definitions, requirements, material);
    let mut calls = vec![];
    let mut replies = vec![];
    let mut missing = None;
    for _ in 0..k {
        let mut last_error = String::new();
        let mut parsed = None;
        for _ in 0..2 {
            let (text, mut record) = call(&prompt);
            let outcome = text.and_then(|t| ratings(&t));
            record["purpose"] = "rating".into();
            if let Err(error) = &outcome {
                record["error"] = error.clone().into();
            }
            calls.push(record);
            match outcome {
                Ok(r) => {
                    parsed = Some(r);
                    break;
                }
                Err(error) => last_error = error,
            }
        }
        match parsed {
            Some(r) => replies.push(r),
            None => {
                missing = Some(format!(
                    "a rating call failed after one retry: {last_error}"
                ))
            }
        }
    }
    let ratings = match missing {
        Some(reason) => json!({"missing": reason}),
        None => DIMENSIONS
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let mut values: Vec<u64> = replies.iter().map(|r| r[i].0).collect();
                let reasons: Vec<&String> = replies.iter().map(|r| &r[i].1).collect();
                let spread = [*values.iter().min().unwrap(), *values.iter().max().unwrap()];
                let figure = json!({"ratings": values, "reasons": reasons});
                let mut figure = figure;
                figure["value"] = median(&mut values).into();
                figure["spread"] = json!(spread);
                (d.to_string(), figure)
            })
            .collect::<serde_json::Map<_, _>>()
            .into(),
    };
    (ratings, calls)
}

/// One measured snapshot's ratings and arm guess (#228 decision 19). A missing guess is not
/// retried. A team of one (`arm` None) gets no guess: the frozen question assumes two agents
/// (#336).
pub fn judge(
    call: &mut Call,
    definitions: &Value,
    requirements: &str,
    material: &Material,
    arm: Option<&str>,
) -> Value {
    let (ratings, mut calls) = rate(call, definitions, requirements, material);
    let Some(arm) = arm else {
        return json!({"ratings": ratings, "calls": calls});
    };
    let (text, mut record) = call(&blinding_prompt(definitions, requirements, material));
    record["purpose"] = "blinding".into();
    let guessed = match text.and_then(|t| guess(&t, definitions)) {
        Ok(guessed) => json!({"arm": guessed, "correct": guessed == arm}),
        Err(error) => {
            record["error"] = error.clone().into();
            json!({"missing": error})
        }
    };
    calls.push(record);
    json!({"ratings": ratings, "guess": guessed, "calls": calls})
}

/// The frozen definitions.
pub fn definitions() -> Result<Value> {
    read(root().join("code-quality.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(ratings: [u64; 4]) -> String {
        let mut reply = serde_json::Map::new();
        for (d, r) in DIMENSIONS.iter().zip(ratings) {
            reply.insert(
                d.to_string(),
                json!({"rating": r, "reason": format!("{d} reason.")}),
            );
        }
        format!("```json\n{}\n```", Value::Object(reply))
    }

    /// Replays canned replies in order, as the fake runtime does; `None` is a failed call.
    fn run(replies: Vec<Option<String>>, arm: &str) -> (Value, Vec<String>) {
        let definitions = definitions().unwrap();
        let material = Material {
            initial: "fn a() {}".into(),
            change: "+fn b() { format!(\"{tests}\"); }".into(),
            tests: "#[test]\nfn t() {}".into(),
        };
        let mut replies = replies.into_iter();
        let mut prompts = vec![];
        let mut call = |prompt: &str| {
            prompts.push(prompt.to_string());
            let usage = json!({"usage": {"input_tokens": 1, "output_tokens": 1}});
            match replies.next().expect("more calls than canned replies") {
                Some(text) => (Ok(text), usage),
                None => (Err("runtime call failed".to_string()), usage),
            }
        };
        let value = judge(&mut call, &definitions, "Add b.", &material, Some(arm));
        (value, prompts)
    }

    #[test]
    fn median_and_spread_per_dimension() {
        let (value, prompts) = run(
            vec![
                Some(reply([2, 5, 3, 1])),
                Some(reply([4, 5, 3, 2])),
                Some(reply([3, 5, 3, 5])),
                Some("B".into()),
            ],
            "falinks",
        );
        let m = &value["ratings"]["maintainability"];
        assert_eq!(
            (m["value"].clone(), m["spread"].clone()),
            (json!(3), json!([2, 4]))
        );
        assert_eq!(value["ratings"]["test_quality"]["value"], 2);
        assert_eq!(value["ratings"]["duplication"]["spread"], json!([5, 5]));
        assert_eq!(value["guess"], json!({"arm": "falinks", "correct": true}));
        assert_eq!(value["calls"].as_array().unwrap().len(), 4);
        assert!(value["calls"][0]["usage"]["input_tokens"].is_u64());
        // The frozen prompt and question are used verbatim, with the material filled in.
        let definitions = definitions().unwrap();
        let head = definitions["judge"]["prompt"]
            .as_str()
            .unwrap()
            .split("{anchors}")
            .next()
            .unwrap();
        assert!(
            prompts[0].starts_with(head)
                && prompts[0].contains("+fn b() { format!(\"{tests}\"); }")
        );
        let anchor = definitions["judge"]["dimensions"]["duplication"]["3"]
            .as_str()
            .unwrap();
        assert!(prompts[0].contains(anchor));
        let question = definitions["judge"]["blinding"]["question"]
            .as_str()
            .unwrap();
        assert!(prompts[3].starts_with(question) && prompts[3].contains("Add b."));
    }

    /// #336: the blinding question asks about two agents, so a team of one is not asked it.
    #[test]
    fn a_team_of_one_gets_no_blinding_call() {
        let definitions = definitions().unwrap();
        let material = Material {
            initial: "a".into(),
            change: "+b".into(),
            tests: String::new(),
        };
        let mut calls = 0;
        let mut call = |_: &str| {
            calls += 1;
            (Ok(reply([4; 4])), json!({"usage": null}))
        };
        let value = judge(&mut call, &definitions, "Add b.", &material, None);
        assert_eq!(calls, 3);
        assert_eq!(value["ratings"]["duplication"]["value"], 4);
        assert!(value.get("guess").is_none(), "{value}");
    }

    #[test]
    fn one_retry_then_missing() {
        // The first call fails then succeeds; the second fails twice.
        let (value, _) = run(
            vec![
                None,
                Some(reply([3; 4])),
                None,
                None,
                Some(reply([3; 4])),
                Some("A".into()),
            ],
            "falinks",
        );
        let reason = value["ratings"]["missing"].as_str().unwrap();
        assert!(reason.contains("after one retry"), "{reason}");
        assert_eq!(value["calls"].as_array().unwrap().len(), 6);
        assert_eq!(value["guess"], json!({"arm": "git", "correct": false}));
    }

    #[test]
    fn unparseable_output_is_retried_then_missing() {
        let out_of_range = reply([3, 6, 3, 3]);
        let (value, _) = run(
            vec![
                Some("I think it is fine.".into()),
                Some(out_of_range),
                Some(reply([3; 4])),
                Some(reply([3; 4])),
                Some("Probably A".into()),
            ],
            "git",
        );
        let reason = value["ratings"]["missing"].as_str().unwrap();
        assert!(reason.contains("duplication lacks a rating"), "{reason}");
        assert!(
            value["guess"]["missing"]
                .as_str()
                .unwrap()
                .starts_with("unparseable guess")
        );
    }

    #[test]
    fn material_has_no_file_names_and_moves_inline_tests_out() {
        let files = |pairs: &[(&str, &str)]| -> Files {
            pairs
                .iter()
                .map(|(n, t)| (n.to_string(), t.as_bytes().to_vec()))
                .collect()
        };
        let initial = files(&[
            ("src/catalog.rs", "pub fn lookup() -> u32 {\n    3\n}\n"),
            ("Cargo.toml", "[package]\nname = \"fixture\"\n"),
        ]);
        let last = files(&[
            (
                "src/catalog.rs",
                "pub fn lookup() -> u32 {\n    7\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn seven() {}\n}\n",
            ),
            ("tests/extra.rs", "#[test]\nfn extra() {}\n"),
            ("Cargo.toml", "[package]\nname = \"fixture\"\n"),
        ]);
        let material = material(&initial, &last);
        let all = format!(
            "{}\n{}\n{}",
            material.initial, material.change, material.tests
        );
        for word in [
            "catalog.rs",
            "extra.rs",
            "Cargo",
            "src/",
            "tests/",
            "git",
            "falinks",
            "worktree",
        ] {
            assert!(
                !all.to_lowercase().contains(&word.to_lowercase()),
                "{word} in {all}"
            );
        }
        assert_eq!(material.initial, "pub fn lookup() -> u32 {\n    3\n}");
        assert_eq!(
            material.change,
            " pub fn lookup() -> u32 {\n-    3\n+    7\n }"
        );
        assert!(material.tests.contains("fn seven()") && material.tests.contains("fn extra()"));
    }
}
