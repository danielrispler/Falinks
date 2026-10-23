//! The judge's known-answer set and its calibration (#228 decisions 20-23, #233).
//! `protected/known-answer.json` holds, per scored fixture, the tests that join the reference
//! to make the known-answer base, and one degraded copy per judge dimension as a declaration
//! edit of the formatted base. `known-answer` checks the base and every copy against the
//! subtlety rule: the formatter, the oracle, every property and the lints. With `--judge
//! CONFIG` it then rates each tree k times with the pinned runtime and writes the calibration
//! record, which a second calibration after an amendment never overwrites.
use crate::judge::{self, DIMENSIONS};
use crate::quality::{Baseline, Contained, Files, baseline, measure};
use crate::{Result, bail, check, fixture, frozen, git, initial, read, root, utc_now, write};
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

pub fn known_answer(judge_config: Option<&Path>) -> Result<i32> {
    // The copies are frozen before any calibration call (#233).
    let manifest = frozen()?;
    let set = read(root().join("protected/known-answer.json"))?;
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
    for name in crate::SCORED_FIXTURES {
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
        fixtures.push((name, base, trees));
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
    if path.exists() {
        bail!(
            "{} exists: a calibration record is never overwritten",
            path.display()
        );
    }
    let mut record = Map::new();
    let mut calls = vec![];
    let mut dimensions: Map<String, Value> = DIMENSIONS
        .iter()
        .map(|d| (d.to_string(), true.into()))
        .collect();
    for (name, base, trees) in &fixtures {
        let requirements = judge::requirements(&fixture(name)?);
        let mut ratings = Map::new();
        for (label, tree) in trees {
            let material = judge::material(&base.initial, tree);
            let mut call = |prompt: &str| runtime.call(prompt);
            let (rated, made) = judge::rate(&mut call, &definitions, &requirements, &material);
            calls.extend(made.into_iter().map(|mut c| {
                c["fixture"] = (*name).into();
                c["tree"] = label.as_str().into();
                c
            }));
            ratings.insert(label.clone(), rated);
        }
        let mut entry = Map::new();
        for dimension in DIMENSIONS {
            let (reference, copy) = (
                &ratings["reference"][dimension],
                &ratings[dimension][dimension],
            );
            // #228 decision 22: the targeted copy's median is lower than the reference's.
            let pass = matches!((reference["value"].as_u64(), copy["value"].as_u64()),
                (Some(r), Some(c)) if c < r);
            if !pass {
                dimensions[dimension] = false.into();
            }
            entry.insert(
                dimension.into(),
                json!({"reference": reference, "copy": copy, "passed": pass}),
            );
        }
        record.insert((*name).into(), entry.into());
    }
    std::fs::create_dir_all(root().join("calibration"))?;
    write(
        &path,
        &json!({"utc": utc_now(), "manifest_version": manifest["version"],
            "definitions_version": version, "fixtures": record, "dimensions": dimensions,
            "calls": calls}),
    )?;
    println!("{}", path.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The subtlety rule's checks name a copy that is unformatted and changes behaviour.
    #[test]
    fn failures_name_an_unformatted_behaviour_change() {
        let space = Contained::new(vec![]).unwrap();
        let set = read(root().join("protected/known-answer.json")).unwrap();
        let base = baseline(&space, "go-relationships").unwrap();
        let mut bad = trees(&space, "go-relationships", false, &set).unwrap()[0]
            .1
            .clone();
        let text = String::from_utf8(bad["producer.go"].clone()).unwrap();
        bad.insert(
            "producer.go".into(),
            text.replace("ID: i + 1", "ID:i").into_bytes(),
        );
        let tmp = tempfile::tempdir().unwrap();
        let failed = failures(
            &space,
            &base,
            "go-relationships",
            &bad,
            &tmp.path().join("repo"),
        );
        for expected in [
            "the formatter changes it",
            "failing_properties",
            "the oracle or its tests fail",
        ] {
            assert!(failed.iter().any(|f| f.starts_with(expected)), "{failed:?}");
        }
    }
}
