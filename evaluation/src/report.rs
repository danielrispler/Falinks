//! Scored-batch interpretation: every outcome, paired successes, medians and the agreed
//! continuation signal. Exploratory evidence only; never a statistical or cost claim.
use crate::FIXTURES;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Continuation signal: at least this much lower median time on two of three fixtures.
const REDUCTION: f64 = 0.2;

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
/// usage-limited run is an interruption, not a task outcome: its pair stays incomplete.
pub fn report(runs: &[Value]) -> Value {
    let replaced: Vec<&Value> = runs.iter().filter_map(|r| r.get("replaces")).collect();
    let scored: Vec<&Value> = runs
        .iter()
        .filter(|r| r["scored"] == true && !replaced.contains(&&r["run_id"]))
        .filter(|r| r["outcome"] != "usage_limit")
        .collect();
    let elapsed = |r: &Value| r["timing"]["elapsed_seconds"].as_f64();
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
                        "outcome": r["outcome"], "elapsed_seconds": elapsed(r)}))).collect::<serde_json::Map<_, _>>()}))
                    .collect::<Vec<_>>(),
                "successes": arm_successes, "qualifying_pairs": qualifying,
                "median_seconds": {"git": git, "falinks": falinks},
                "median_reduction": reduction, "meets_threshold": meets,
            }),
        );
    }
    let limited: Vec<&Value> = runs
        .iter()
        .filter(|r| r["outcome"] == "usage_limit")
        .map(|r| &r["run_id"])
        .collect();
    if !limited.is_empty() {
        inconclusive.push(format!("usage exhaustion in {limited:?}"));
    }
    let unsafe_runs: Vec<&Value> = runs
        .iter()
        .filter(|r| r["outcome"] == "safety_failure")
        .map(|r| &r["run_id"])
        .collect();
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
        "qualification": "Exploratory: three repetitions per fixture; not statistical proof, general superiority or a cost claim.",
        "runs": runs.iter().map(|r| json!({"run_id": r["run_id"], "pair": r["pair"], "arm": r["arm"],
            "scored": r["scored"], "outcome": r["outcome"], "replaces": r.get("replaces"),
            "failure_reason": r.get("failure_reason")})).collect::<Vec<_>>(),
        "fixtures": fixtures,
        "successes": successes,
        "fixtures_meeting_threshold": counted,
        "continuation_signal": signal,
        "inconclusive_reasons": inconclusive,
        "safety_failures": unsafe_runs,
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
    fn missing_pairs_usage_exhaustion_and_unsafe_runs_are_reported() {
        let mut runs = batch([[700.0; 3], [700.0; 3], [700.0; 3]]);
        runs.truncate(16);
        runs[15]["outcome"] = "usage_limit".into();
        let out = report(&runs);
        assert_eq!(out["verdict"], "inconclusive");
        assert_eq!(out["inconclusive_reasons"].as_array().unwrap().len(), 2);
        runs[3]["outcome"] = "safety_failure".into();
        assert_eq!(report(&runs)["verdict"], "paused_unsafe");
    }
}
