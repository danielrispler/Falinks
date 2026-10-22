//! Scored-batch interpretation: every outcome, paired successes, medians, API-equivalent cost
//! per successful task and the agreed continuation signal. Exploratory evidence only; never a
//! statistical claim, and the cost is list price, never billed cost.
use crate::{
    FIXTURES,
    batch::replaceable,
    tokens::{CLASSES, usd},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Continuation signal: at least this much lower median time on two of three fixtures.
const REDUCTION: f64 = 0.2;

/// Tokens by class and API-equivalent cost of every scored run of one arm on one fixture,
/// failed, interrupted and replaced runs included, and that cost per successful run. A run
/// without session JSONL is counted as missing, and the cost per success is then unknown.
fn cost(runs: &[&Value], successes: u64) -> Value {
    let missing = runs
        .iter()
        .filter(|r| r["tokens"]["cost_usd"].is_null())
        .count();
    let tokens: serde_json::Map<String, Value> = CLASSES
        .iter()
        .map(|c| {
            let sum: u64 = runs
                .iter()
                .filter_map(|r| r["tokens"]["tokens"][c].as_u64())
                .sum();
            (c.to_string(), json!(sum))
        })
        .collect();
    let dollars: f64 = runs
        .iter()
        .filter_map(|r| r["tokens"]["cost_usd"].as_f64())
        .sum();
    let per_success = (missing == 0 && successes > 0).then(|| usd(dollars / successes as f64));
    json!({"runs": runs.len(), "missing": missing, "tokens": tokens,
        "cost_usd": usd(dollars), "cost_per_success_usd": per_success})
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(values[n / 2]),
        _ => Some((values[n / 2 - 1] + values[n / 2]) / 2.0),
    }
}

/// `runs` are every recorded result (pilot included) in batch order; all are listed. Scoring
/// uses scored runs only. A record naming an earlier one in `replaces` supersedes it. A
/// replaceable outcome is an interruption, not a task outcome: its pair stays incomplete and,
/// until replaced, it is named as a reason the verdict is inconclusive.
pub fn report(runs: &[Value]) -> Value {
    let replaced: Vec<&Value> = runs.iter().filter_map(|r| r.get("replaces")).collect();
    let (interrupted, scored): (Vec<&Value>, Vec<&Value>) = runs
        .iter()
        .filter(|r| r["scored"] == true && !replaced.contains(&&r["run_id"]))
        .partition(|r| replaceable(r));
    let elapsed = |r: &Value| r["timing"]["elapsed_seconds"].as_f64();
    // A run overlapped another if it saw one active at its start or stop, or one saw it: a
    // run entirely inside another sees it at start without being seen.
    let seen = |r: &Value| {
        ["start", "stop"]
            .map(|k| r["concurrency"][k].as_array().cloned().unwrap_or_default())
            .concat()
    };
    let named: Vec<Value> = runs.iter().flat_map(seen).collect();
    let concurrent = |r: &Value| !seen(r).is_empty() || named.contains(&r["run_id"]);
    let mut fixtures = serde_json::Map::new();
    let (mut counted, mut successes) = (0, BTreeMap::from([("git", 0), ("falinks", 0)]));
    let mut inconclusive = Vec::new();
    for fixture in FIXTURES {
        let mut pairs: BTreeMap<&str, BTreeMap<&str, &Value>> = BTreeMap::new();
        for run in scored.iter().filter(|r| r["fixture"] == fixture) {
            let (Some(pair), Some(arm)) = (run["pair"].as_str(), run["arm"].as_str()) else {
                continue;
            };
            pairs.entry(pair).or_default().insert(arm, run);
        }
        let mut arm_successes = BTreeMap::from([("git", 0), ("falinks", 0)]);
        let mut times: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        let mut qualifying = 0;
        for arms in pairs.values() {
            for (arm, run) in arms {
                if run["outcome"] == "success" {
                    *arm_successes.get_mut(arm).unwrap() += 1;
                }
            }
            let both = ["git", "falinks"].map(|a| {
                arms.get(a)
                    .filter(|r| r["outcome"] == "success")
                    .and_then(|r| elapsed(r))
            });
            if let [Some(git), Some(falinks)] = both {
                qualifying += 1;
                times.entry("git").or_default().push(git);
                times.entry("falinks").or_default().push(falinks);
            }
        }
        let git = median(times.remove("git").unwrap_or_default());
        let falinks = median(times.remove("falinks").unwrap_or_default());
        // No jointly successful pairs means no time comparison.
        let reduction = git.zip(falinks).map(|(g, f)| 1.0 - f / g);
        // Tolerance: exactly 20% (800 s against 1000 s) must count despite float rounding.
        let meets = reduction.is_some_and(|r| r >= REDUCTION - 1e-9)
            && arm_successes["falinks"] >= arm_successes["git"];
        counted += usize::from(meets);
        for (arm, n) in &arm_successes {
            *successes.get_mut(arm).unwrap() += n;
        }
        let costs: serde_json::Map<String, Value> = ["git", "falinks"]
            .iter()
            .map(|arm| {
                let all: Vec<&Value> = runs
                    .iter()
                    .filter(|r| r["scored"] == true && r["fixture"] == fixture && r["arm"] == *arm)
                    .collect();
                (arm.to_string(), cost(&all, arm_successes[arm]))
            })
            .collect();
        let complete = pairs.values().filter(|arms| arms.len() == 2).count();
        if complete < 3 {
            inconclusive.push(format!("{fixture}: {complete} of 3 complete pairs"));
        }
        if qualifying == 0 {
            inconclusive.push(format!("{fixture}: no jointly successful pairs"));
        }
        fixtures.insert(
            fixture.into(),
            json!({
                "pairs": pairs.iter().map(|(pair, arms)| json!({"pair": pair,
                    "arms": arms.iter().map(|(arm, r)| (arm.to_string(), json!({"run_id": r["run_id"],
                        "outcome": r["outcome"], "elapsed_seconds": elapsed(r), "concurrent": concurrent(r)})))
                        .collect::<serde_json::Map<_, _>>()}))
                    .collect::<Vec<_>>(),
                "successes": arm_successes, "qualifying_pairs": qualifying,
                "median_seconds": {"git": git, "falinks": falinks},
                "median_reduction": reduction, "meets_threshold": meets,
                "cost": costs,
            }),
        );
    }
    for run in interrupted {
        // As `replaceable` reads it: no outcome is an infrastructure failure.
        let outcome = run["outcome"].as_str().unwrap_or("infrastructure_failure");
        let id = run["run_id"].as_str().unwrap_or_default();
        inconclusive.push(format!("unreplaced interruption {id}: {outcome}"));
    }
    for run in runs.iter().filter(|r| r["outcome"] == "run_invalid") {
        let id = run["run_id"].as_str().unwrap_or_default();
        inconclusive.push(format!("invalid run {id}: nested agent"));
    }
    let unsafe_runs: Vec<&Value> = runs
        .iter()
        .filter(|r| r["outcome"] == "safety_failure")
        .map(|r| &r["run_id"])
        .collect();
    // Runs a nested agent made invalid (#178 C7), per arm.
    let nested = |arm: &str| {
        runs.iter()
            .filter(|r| r["arm"] == arm && !r["nested_agent"].is_null())
            .count()
    };
    let signal = counted >= 2 && successes["falinks"] >= successes["git"];
    let transitions: u64 = scored
        .iter()
        .filter(|r| r["arm"] == "falinks")
        .filter_map(|r| r["transitions"].as_u64())
        .sum();
    let verdict = if !unsafe_runs.is_empty() {
        "paused_unsafe"
    } else if !inconclusive.is_empty() {
        "inconclusive"
    } else if signal {
        "continuation_signal"
    } else {
        "no_signal"
    };
    json!({
        "verdict": verdict,
        "qualification": "Exploratory: three repetitions per fixture; not statistical proof, general superiority or billed cost.",
        "runs": runs.iter().map(|r| json!({"run_id": r["run_id"], "pair": r["pair"], "arm": r["arm"],
            "scored": r["scored"], "outcome": r["outcome"], "replaces": r.get("replaces"),
            "failure_reason": r.get("failure_reason"), "concurrency": r.get("concurrency"),
            "nested_agent": r.get("nested_agent"), "tokens": r.get("tokens")})).collect::<Vec<_>>(),
        "concurrent_runs": runs.iter().filter(|r| concurrent(r)).map(|r| &r["run_id"]).collect::<Vec<_>>(),
        "concurrency_note": "Times of concurrent_runs were measured while another shard's run was active.",
        "fixtures": fixtures,
        "successes": successes,
        "fixtures_meeting_threshold": counted,
        "continuation_signal": signal,
        "inconclusive_reasons": inconclusive,
        "safety_failures": unsafe_runs,
        "nested_agent_hits": {"git": nested("git"), "falinks": nested("falinks")},
        "falinks_transitions": transitions,
        "regrouping_benefit": if transitions == 0 { "unestablished: no transitions" } else { "observed transitions; benefit needs review" },
        "replaced_runs": replaced,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, fixture: &str, rep: u32, arm: &str, outcome: &str, secs: f64) -> Value {
        json!({"run_id": id, "fixture": fixture, "pair": format!("{fixture}-{rep}"), "arm": arm,
            "scored": true, "outcome": outcome, "timing": {"elapsed_seconds": secs}, "transitions": 0})
    }

    /// Three complete pairs per fixture; `falinks_secs` are the Falinks-arm times.
    fn batch(falinks_secs: [[f64; 3]; 3]) -> Vec<Value> {
        let mut runs = vec![];
        for (f, fixture) in FIXTURES.iter().enumerate() {
            for rep in 0..3 {
                let id = format!("{fixture}-{rep}");
                runs.push(run(
                    &format!("{id}-g"),
                    fixture,
                    rep + 1,
                    "git",
                    "success",
                    1000.0,
                ));
                let secs = falinks_secs[f][rep as usize];
                runs.push(run(
                    &format!("{id}-f"),
                    fixture,
                    rep + 1,
                    "falinks",
                    "success",
                    secs,
                ));
            }
        }
        runs
    }

    #[test]
    fn medians_handle_odd_even_and_empty() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![4.0, 1.0]), Some(2.5));
        assert_eq!(median(vec![]), None);
    }

    #[test]
    fn two_fixtures_twenty_percent_faster_is_a_signal() {
        let out = report(&batch([[700.0; 3], [800.0; 3], [1100.0; 3]]));
        assert_eq!(out["fixtures_meeting_threshold"], 2);
        assert_eq!(out["verdict"], "continuation_signal");
        assert_eq!(out["fixtures"]["go-page"]["qualifying_pairs"], 3);
        assert_eq!(out["regrouping_benefit"], "unestablished: no transitions");
    }

    #[test]
    fn one_fast_fixture_is_no_signal() {
        let out = report(&batch([[700.0; 3], [900.0; 3], [1100.0; 3]]));
        assert_eq!(out["fixtures_meeting_threshold"], 1);
        assert_eq!(out["verdict"], "no_signal");
    }

    #[test]
    fn lower_success_rate_blocks_the_signal() {
        let mut runs = batch([[700.0; 3], [700.0; 3], [700.0; 3]]);
        // A Falinks failure on every fixture: faster medians cannot compensate.
        for fixture in FIXTURES {
            let run = runs
                .iter_mut()
                .find(|r| r["fixture"] == fixture && r["arm"] == "falinks")
                .unwrap();
            run["outcome"] = "task_failure".into();
        }
        let out = report(&runs);
        assert_eq!(out["fixtures"]["go-page"]["qualifying_pairs"], 2);
        assert_eq!(out["continuation_signal"], false);
    }

    #[test]
    fn no_joint_successes_means_no_time_comparison() {
        let mut runs = batch([[700.0; 3], [700.0; 3], [700.0; 3]]);
        for run in runs
            .iter_mut()
            .filter(|r| r["fixture"] == "go-page" && r["arm"] == "git")
        {
            run["outcome"] = "timeout".into();
        }
        let out = report(&runs);
        let page = &out["fixtures"]["go-page"];
        assert_eq!(page["qualifying_pairs"], 0);
        assert_eq!(page["median_reduction"], Value::Null);
        assert_eq!(out["verdict"], "inconclusive");
    }

    #[test]
    fn replacements_supersede_infrastructure_failures_and_stay_listed() {
        let mut runs = batch([[700.0; 3], [700.0; 3], [700.0; 3]]);
        runs[0]["outcome"] = "infrastructure_failure".into();
        let mut replacement = runs[0].clone();
        replacement["run_id"] = "rust-errors-0-g-r1".into();
        replacement["replaces"] = runs[0]["run_id"].clone();
        replacement["outcome"] = "success".into();
        runs.push(replacement);
        let out = report(&runs);
        assert_eq!(out["fixtures"]["rust-errors"]["qualifying_pairs"], 3);
        assert_eq!(out["replaced_runs"], json!(["rust-errors-0-g"]));
    }

    #[test]
    fn runs_overlapping_a_sibling_shard_are_flagged_both_ways() {
        let mut runs = batch([[700.0; 3], [700.0; 3], [700.0; 3]]);
        // go-page-0-g started while rust-errors-0-g ran; rust-errors-0-g saw nothing itself.
        let page = runs
            .iter()
            .position(|r| r["run_id"] == "go-page-0-g")
            .unwrap();
        runs[page]["concurrency"] = json!({"start": ["rust-errors-0-g"], "stop": []});
        let out = report(&runs);
        assert_eq!(
            out["concurrent_runs"],
            json!(["rust-errors-0-g", "go-page-0-g"])
        );
        let arms = |f: &str| out["fixtures"][f]["pairs"][0]["arms"].clone();
        assert_eq!(arms("go-page")["git"]["concurrent"], true);
        assert_eq!(arms("rust-errors")["git"]["concurrent"], true);
        assert_eq!(arms("rust-errors")["falinks"]["concurrent"], false);
        assert_eq!(
            out["runs"][page]["concurrency"]["start"][0],
            "rust-errors-0-g"
        );
    }

    #[test]
    fn missing_pairs_usage_exhaustion_and_unsafe_runs_are_reported() {
        let mut runs = batch([[700.0; 3], [700.0; 3], [700.0; 3]]);
        runs.truncate(16);
        runs[15]["outcome"] = "usage_limit".into();
        let out = report(&runs);
        assert_eq!(out["verdict"], "inconclusive");
        let reasons = out["inconclusive_reasons"].as_array().unwrap();
        assert_eq!(reasons.len(), 2);
        assert!(
            reasons[1]
                .to_string()
                .contains(runs[15]["run_id"].as_str().unwrap())
        );
        runs[3]["outcome"] = "safety_failure".into();
        assert_eq!(report(&runs)["verdict"], "paused_unsafe");
    }

    /// Two fixtures meet the threshold: a continuation signal when nothing is interrupted.
    fn signalling() -> Vec<Value> {
        batch([[700.0; 3], [800.0; 3], [1100.0; 3]])
    }

    #[test]
    fn unreplaced_interruptions_in_either_arm_are_inconclusive_and_named() {
        let f0 = FIXTURES[0];
        // A result with no outcome reads as an infrastructure failure.
        for (id, outcome, named) in [
            (
                format!("{f0}-0-f"),
                json!("infrastructure_failure"),
                "infrastructure_failure",
            ),
            (
                format!("{f0}-1-g"),
                json!("infrastructure_failure"),
                "infrastructure_failure",
            ),
            (format!("{f0}-2-f"), json!("usage_limit"), "usage_limit"),
            (format!("{f0}-2-g"), Value::Null, "infrastructure_failure"),
        ] {
            let mut runs = signalling();
            runs.iter_mut().find(|r| r["run_id"] == *id).unwrap()["outcome"] = outcome;
            let out = report(&runs);
            assert_eq!(out["verdict"], "inconclusive", "{id}: {out}");
            assert_eq!(out["fixtures"][f0]["pairs"].as_array().unwrap().len(), 3);
            let reason = json!(format!("unreplaced interruption {id}: {named}"));
            assert!(
                out["inconclusive_reasons"]
                    .as_array()
                    .unwrap()
                    .contains(&reason),
                "{id}: {out}"
            );
        }
    }

    #[test]
    fn replaced_interruptions_of_either_kind_leave_the_verdict_alone() {
        let mut runs = signalling();
        for (i, outcome) in [(0, "infrastructure_failure"), (3, "usage_limit")] {
            let mut replacement = runs[i].clone();
            replacement["run_id"] = format!("{}-r1", runs[i]["run_id"].as_str().unwrap()).into();
            replacement["replaces"] = runs[i]["run_id"].clone();
            runs[i]["outcome"] = outcome.into();
            runs.push(replacement);
        }
        let out = report(&runs);
        assert_eq!(out["verdict"], "continuation_signal", "{out}");
        assert_eq!(out["replaced_runs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_usage_limited_pilot_run_does_not_affect_the_verdict() {
        let mut runs = signalling();
        let mut pilot = runs[0].clone();
        pilot["run_id"] = "pilot-g".into();
        pilot["pair"] = "pilot".into();
        pilot["scored"] = false.into();
        pilot["outcome"] = "usage_limit".into();
        runs.insert(0, pilot);
        assert_eq!(report(&runs)["verdict"], "continuation_signal");
    }

    #[test]
    fn cost_per_success_counts_every_run_and_refuses_missing_usage() {
        let mut runs = signalling();
        let priced = |usd: f64| json!({"cost_usd": usd, "tokens": {"output": 10}});
        for run in runs.iter_mut() {
            run["tokens"] = priced(1.0);
        }
        let failed = runs
            .iter()
            .position(|r| r["run_id"] == "go-page-0-f")
            .unwrap();
        runs[failed]["outcome"] = "task_failure".into();
        let out = report(&runs);
        let falinks = &out["fixtures"]["go-page"]["cost"]["falinks"];
        // Three runs cost $3 and two succeeded.
        assert_eq!(falinks["cost_per_success_usd"], 1.5);
        assert_eq!(falinks["tokens"]["output"], 30);
        runs[failed]["tokens"] = json!({"status": "missing", "cost_usd": null});
        let falinks = &report(&runs)["fixtures"]["go-page"]["cost"]["falinks"];
        assert_eq!(falinks["missing"], 1);
        assert_eq!(falinks["cost_per_success_usd"], Value::Null);
    }
}
