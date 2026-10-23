//! The judge's known-answer set and its calibration (#228 decisions 20-23, #233).
//! `protected/known-answer.json` holds, per scored fixture, the tests that join the reference
//! to make the known-answer base, and one degraded copy per judge dimension as a declaration
//! edit of the formatted base. `known-answer` checks the base and every copy against the
//! subtlety rule: the formatter, the oracle, every property and the lints. With `--judge
//! CONFIG` it then rates each tree k times with the pinned runtime and writes the calibration
//! record, which a second calibration after an amendment never overwrites.
use crate::judge::{self, DIMENSIONS};
use crate::quality::{Baseline, Contained, Files, baseline, measure};
use crate::{Result, bail, check, fixture, frozen, git, initial, read, root, utc_now};
use serde_json::{Map, Value, json};
use std::path::Path;
use std::time::Duration;

/// The formatted known-answer base, then each dimension's copy.
fn trees(space: &Contained, name: &str, rust: bool, set: &Value) -> Result<Vec<(String, Files)>> {
    let mut base = Files::new();
    for files in [
        &fixture(name)?["files"],
        &crate::protected(name)?["reference"],
        &set[name]["tests"],
    ] {
        for (file, text) in files.as_object().into_iter().flatten() {
            base.insert(file.clone(), text.as_str().unwrap_or_default().into());
        }
    }
    let base = space.format(&base, rust)?;
    let mut trees = vec![("reference".to_string(), base.clone())];
    for dimension in DIMENSIONS {
        let edit = &set[name]["copies"][dimension];
        let (Some(file), Some(old), Some(new)) = (
            edit["file"].as_str(),
            edit["old"].as_str(),
            edit["new"].as_str(),
        ) else {
            bail!("{name}: no {dimension} copy");
        };
        let text =
            String::from_utf8_lossy(base.get(file).map_or(&[][..], Vec::as_slice)).into_owned();
        if text.matches(old).count() != 1 || old == new {
            bail!("{name}: the {dimension} edit does not occur exactly once in {file}");
        }
        let mut copy = base.clone();
        copy.insert(file.into(), text.replacen(old, new, 1).into_bytes());
        trees.push((dimension.to_string(), copy));
    }
    Ok(trees)
}

/// Why a tree breaks the subtlety rule's checks (#228 decision 21); empty when it passes.
fn failures(
    space: &Contained,
    base: &Baseline,
    name: &str,
    tree: &Files,
    repo: &Path,
) -> Vec<String> {
    let mut failed = vec![];
    match space.format(tree, base.rust) {
        Ok(formatted) if formatted == *tree => {}
        Ok(_) => failed.push("the formatter changes it".into()),
        Err(error) => failed.push(error.to_string()),
    }
    let (metrics, _) = measure(space, base, tree);
    let lint = if base.rust {
        "clippy_findings"
    } else {
        "vet_findings"
    };
    for metric in ["failing_properties", lint] {
        if metrics[metric]["value"] != 0 {
            failed.push(format!("{metric}: {}", metrics[metric]));
        }
    }
    let oracle = (|| -> Result<Value> {
        let fixture = fixture(name)?;
        initial(repo, &fixture)?;
        for (file, data) in tree {
            let path = repo.join(file);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, data)?;
        }
        git(repo, &["add", "."], None)?;
        git(repo, &["commit", "-m", "Known-answer tree"], None)?;
        let private = repo.with_extension("private");
        std::fs::create_dir(&private)?;
        check(repo, &fixture, &private, Duration::from_secs(120), &[])
    })();
    match oracle {
        Ok(result) if result["oracle"]["passed"] == true && result["visible"]["passed"] == true => {
        }
        Ok(result) => failed.push(format!("the oracle or its tests fail: {result}")),
        Err(error) => failed.push(format!("the oracle could not run: {error}")),
    }
    failed
}

/// One fixture's requirement text, formatted initial tree and known-answer trees.
type Rated = (String, String, Files, Vec<(String, Files)>);

/// Rates every tree k times and applies #228 decision 22 per fixture and dimension. Any rating
/// still missing after its retry is a tool failure (decision 25), not a calibration result: it
/// is an error, so no record is written.
fn calibrate(call: &mut judge::Call, definitions: &Value, fixtures: &[Rated]) -> Result<Value> {
    let mut record = Map::new();
    let mut calls = vec![];
    let mut dimensions: Map<String, Value> = DIMENSIONS
        .iter()
        .map(|d| (d.to_string(), true.into()))
        .collect();
    for (name, requirements, initial, trees) in fixtures {
        let mut ratings = Map::new();
        for (label, tree) in trees {
            let material = judge::material(initial, tree);
            let (rated, made) = judge::rate(call, definitions, requirements, &material);
            calls.extend(made.into_iter().map(|mut c| {
                c["fixture"] = name.as_str().into();
                c["tree"] = label.as_str().into();
                c
            }));
            if let Some(reason) = rated["missing"].as_str() {
                bail!("known-answer: {name} {label}: {reason}; no record written");
            }
            ratings.insert(label.clone(), rated);
        }
        let mut entry = Map::new();
        for dimension in DIMENSIONS {
            let (reference, copy) = (
                &ratings["reference"][dimension],
                &ratings[dimension][dimension],
            );
            // #228 decision 22: the targeted copy's median is lower than the reference's.
            let pass = copy["value"].as_u64() < reference["value"].as_u64();
            if !pass {
                dimensions[dimension] = false.into();
            }
            entry.insert(
                dimension.into(),
                json!({"reference": reference, "copy": copy, "passed": pass}),
            );
        }
        record.insert(name.clone(), entry.into());
    }
    Ok(json!({"fixtures": record, "dimensions": dimensions, "calls": calls}))
}

/// Checks the known-answer trees of `only` (every scored fixture by default) and, with a run
/// configuration, calibrates the judge on the full set.
pub fn known_answer(judge_config: Option<&Path>, only: Option<&str>) -> Result<i32> {
    // The copies are frozen before any calibration call (#233).
    let manifest = frozen()?;
    let names: Vec<&str> = only.map_or(crate::SCORED_FIXTURES.to_vec(), |list| {
        list.split(',').collect()
    });
    if names.iter().any(|n| !crate::SCORED_FIXTURES.contains(n)) {
        bail!("--fixtures takes {:?}", crate::SCORED_FIXTURES);
    }
    if judge_config.is_some() && names.len() != crate::SCORED_FIXTURES.len() {
        bail!("known-answer: calibration needs every fixture (#228 decision 22)");
    }
    // Tests point `FALINKS_KNOWN_ANSWER` at their own set.
    let set = read(std::env::var_os("FALINKS_KNOWN_ANSWER").map_or_else(
        || root().join("protected/known-answer.json"),
        std::path::PathBuf::from,
    ))?;
    let definitions = judge::definitions()?;
    let runtime = judge_config
        .map(|config| judge::Runtime::load(config, &definitions))
        .transpose()?;
    let space = Contained::new(vec![])?;
    let tmp = tempfile::Builder::new()
        .prefix("falinks-known-answer-")
        .tempdir()?;
    let mut checks = Map::new();
    let mut fixtures = vec![];
    for name in names {
        let base = baseline(&space, name)?;
        let trees = trees(&space, name, base.rust, &set)?;
        let mut entry = Map::new();
        for (label, tree) in &trees {
            let repo = tmp.path().join(format!("{name}-{label}"));
            entry.insert(
                label.clone(),
                json!(failures(&space, &base, name, tree, &repo)),
            );
        }
        checks.insert(name.into(), entry.into());
        let requirements = judge::requirements(&fixture(name)?);
        fixtures.push((name.to_string(), requirements, base.initial, trees));
    }
    let passed = checks
        .values()
        .flat_map(|f| f.as_object().into_iter().flatten())
        .all(|(_, failed)| failed == &json!([]));
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"checks": checks, "passed": passed}))?
    );
    let Some(runtime) = runtime else {
        return Ok(if passed { 0 } else { 1 });
    };
    if !passed {
        bail!("known-answer: not calibrating; a tree fails its checks");
    }
    let version = &definitions["version"];
    let path = root().join(format!("calibration/v{version}.json"));
    let mut record = calibrate(
        &mut |prompt: &str| runtime.call(prompt),
        &definitions,
        &fixtures,
    )?;
    record["utc"] = utc_now().into();
    record["manifest_version"] = manifest["version"].clone();
    record["definitions_version"] = version.clone();
    std::fs::create_dir_all(root().join("calibration"))?;
    // A calibration record is never overwritten (#228 decision 23).
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            format!(
                "{}: {e}; a calibration record is never overwritten",
                path.display()
            )
        })?;
    std::io::Write::write_all(
        &mut file,
        format!("{}\n", serde_json::to_string_pretty(&record)?).as_bytes(),
    )?;
    println!("{}", path.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(ratings: [u64; 4]) -> String {
        let field = |r: u64| json!({"rating": r, "reason": "r."});
        json!({"maintainability": field(ratings[0]), "duplication": field(ratings[1]),
            "unnecessary_changes": field(ratings[2]), "test_quality": field(ratings[3])})
        .to_string()
    }

    /// Two fixtures, each the reference and one copy per dimension. A tree's single file holds
    /// its label, so a canned judge knows which tree it rates.
    fn fixtures() -> Vec<Rated> {
        let tree =
            |label: &str| Files::from([("a.go".to_string(), format!("{label}\n").into_bytes())]);
        ["one", "two"]
            .iter()
            .map(|name| {
                let trees = std::iter::once("reference")
                    .chain(DIMENSIONS)
                    .map(|label| (label.to_string(), tree(&format!("{name}-{label}"))))
                    .collect();
                (
                    name.to_string(),
                    "Do it.".to_string(),
                    tree("initial"),
                    trees,
                )
            })
            .collect()
    }

    /// #228 decision 22: a dimension passes only when its copy rates lower on every fixture;
    /// a tie fails it. A rating still missing after its retry writes no record (decision 25).
    #[test]
    fn calibration_needs_a_lower_copy_on_every_fixture_and_every_rating() {
        let definitions = judge::definitions().unwrap();
        let mut canned = |prompt: &str| {
            let ratings =
                if prompt.contains("+one-duplication") || prompt.contains("+two-duplication") {
                    [4, 2, 4, 4]
                } else if prompt.contains("+one-test_quality") {
                    [4, 4, 4, 3]
                } else {
                    [4; 4]
                };
            (Ok(reply(ratings)), json!({"usage": null}))
        };
        let record = calibrate(&mut canned, &definitions, &fixtures()).unwrap();
        assert_eq!(
            record["dimensions"],
            json!({"maintainability": false, "duplication": true,
                "unnecessary_changes": false, "test_quality": false})
        );
        let one = &record["fixtures"]["one"];
        assert_eq!(one["test_quality"]["passed"], true);
        assert_eq!(one["test_quality"]["copy"]["ratings"], json!([3, 3, 3]));
        assert_eq!(one["test_quality"]["reference"]["value"], 4);
        assert_eq!(record["fixtures"]["two"]["test_quality"]["passed"], false);
        // k = 3 calls per tree: 2 fixtures x 5 trees x 3.
        assert_eq!(record["calls"].as_array().unwrap().len(), 30);
        assert_eq!(record["calls"][0]["tree"], "reference");

        let mut failing = |prompt: &str| {
            if prompt.contains("+two-maintainability") {
                (
                    Err("runtime call failed: boom".to_string()),
                    json!({"usage": null}),
                )
            } else {
                (Ok(reply([4; 4])), json!({"usage": null}))
            }
        };
        let error = calibrate(&mut failing, &definitions, &fixtures())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("two maintainability") && error.contains("boom"),
            "{error}"
        );
        assert!(error.ends_with("no record written"), "{error}");
    }
}
