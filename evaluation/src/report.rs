//! Scored-batch report (#204): every dimension for both arms side by side, per fixture. Each
//! metric gives each repetition's value, the median, range and n, counts missing data and never
//! imputes it, and carries an evidence label. No winner is computed (#5 amendment): the decision
//! is written on the ticket. Exploratory evidence only; the cost is list price, never billed.
use crate::{
    FIXTURES, SCORED_FIXTURES,
    batch::replaceable,
    tokens::{CLASSES, usd},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const ARMS: [&str; 2] = ["git", "falinks"];
/// Compared for both arms, followed by `tokens_<class>`. `retries` is not compared: the Git arm
/// fixes it at 0.
const METRICS: [&str; 9] = [
    "completion_seconds",
    "model_seconds",
    "round_trips",
    "cost_usd",
    "cold_start_cost_usd",
    "success",
    "refused_operations",
    "failed_checks_before_pass",
    "safety_incidents",
];
/// Falinks output forms with the counts `adapters/claude/arm.rs` records per form (its
/// `tests/arm.rs` checks these copies agree), and the refusal recovery `timing` reads from the
/// transcripts.
const FORMS: [&str; 3] = ["content", "replace", "from"];
const EDIT_COUNTS: [&str; 7] = [
    "accepted",
    "stale",
    "unreviewed",
    "rejected",
    "interrupted",
    "host_refused",
    "resends",
];
const RECOVERY: [&str; 4] = ["refusals", "unresolved", "round_trips", "model_seconds"];
/// Printed beside every fixture's labels (#5 amendment, decision 5).
const LABEL_NOTE: &str = "consistent: every repetition of one arm is below every repetition of the other, with no data missing; mixed: the ranges overlap; incomplete: a repetition lacks the metric. Chance alone gives consistent 2/20 of the time per cell at n = 3, and it is not proof of a meaningful or generalizable advantage.";
/// Track T's team sizes (#178): one agent against two Git agents.
const SIZES: [&str; 2] = ["1", "2"];
/// Track T's measurements beyond `METRICS` (#178, "Measurements"), summed over agents unless
/// named otherwise. Work is busy time: load balance is the busiest agent's over the mean.
const TRACK_T_METRICS: [&str; 7] = [
    "coordination_model_seconds",
    "tool_seconds",
    "output_bytes",
    "changed_bytes",
    "load_balance",
    "first_development_seconds",
    "drafts_after_first_development",
];
/// Track T runs five repetitions per cell (#182), so its chance figures are for n = 5.
const TRACK_T_NOTE: &str = "consistent: every repetition of one team size is below every repetition of the other, with no data missing; mixed: the ranges overlap; incomplete: a repetition lacks the metric. Chance alone gives consistent 2/252 of the time per cell at n = 5, and five paired differences of one sign 2/32; neither is proof of a meaningful or generalizable advantage.";
/// Printed with the cost lines (#5 amendments, decision 3).
const COST_LABEL: &str = "API-equivalent list price, not billed cost";
const SANDBOX_LABEL: &str = "API-equivalent list price, not billed cost; estimate ±15%";

/// The sum of one per-run cost line (a `tokens` key) over `runs`, how many runs lack it, and
/// the sum per successful run, which is unknown when a run lacks it.
fn cost_line(runs: &[&Value], successes: u64, key: &str) -> (usize, f64, Option<Value>) {
    let values: Vec<Option<f64>> = runs.iter().map(|r| r["tokens"][key].as_f64()).collect();
    let missing = values.iter().filter(|v| v.is_none()).count();
    let dollars: f64 = values.iter().flatten().sum();
    let per_success = (missing == 0 && successes > 0).then(|| usd(dollars / successes as f64));
    (missing, dollars, per_success)
}

/// Tokens by class and API-equivalent cost, measured and cold-start, of every scored run of
/// one arm on one fixture, failed, interrupted and replaced runs included, and that cost per
/// successful run. A run without session JSONL is counted as missing, and the cost per success
/// is then unknown. `first_calls` gives each agent session's first-call cache figures.
fn cost(runs: &[&Value], successes: u64) -> Value {
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
    let (missing, dollars, per_success) = cost_line(runs, successes, "cost_usd");
    let (_, cold_start, cold_start_per_success) = cost_line(runs, successes, "cold_start_cost_usd");
    let first_calls: Vec<Value> = runs
        .iter()
        .flat_map(|r| {
            let agents = r["tokens"]["agents"].as_object().into_iter().flatten();
            agents.flat_map(move |(agent, figures)| {
                let sessions = figures["sessions"].as_array().into_iter().flatten();
                sessions.map(move |s| {
                    json!({"run_id": r["run_id"], "agent": agent,
                        "cache_read": s["first_call"]["cache_read"], "cache_write": s["first_call"]["cache_write"]})
                })
            })
        })
        .collect();
    json!({"runs": runs.len(), "missing": missing, "tokens": tokens, "label": COST_LABEL,
        "cost_usd": usd(dollars), "cost_per_success_usd": per_success,
        "cold_start_cost_usd": usd(cold_start), "cold_start_cost_per_success_usd": cold_start_per_success,
        "first_calls": first_calls})
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

/// One run's value of a compared metric, or None when the run lacks it.
fn value(run: &Value, metric: &str) -> Option<f64> {
    let outcome = run["outcome"].as_str();
    let is = |name: &str| outcome.map(|o| f64::from(u8::from(o == name)));
    match metric {
        // A failed run is reported as a failure and never timed as a success.
        "completion_seconds" => run["timing"]["elapsed_seconds"]
            .as_f64()
            .filter(|_| outcome == Some("success")),
        "model_seconds" => run["transcript"]["round_trips"]["model_seconds"].as_f64(),
        "round_trips" => run["transcript"]["round_trips"]["count"].as_f64(),
        "cost_usd" | "cold_start_cost_usd" => run["tokens"][metric].as_f64(),
        "success" => is("success"),
        "refused_operations" => run["rejected_operations"].as_f64(),
        // Every check when none passed.
        "failed_checks_before_pass" => run["validation_attempts"]
            .as_array()
            .map(|a| a.iter().take_while(|v| v["passed"] != true).count() as f64),
        "safety_incidents" => is("safety_failure"),
        "coordination_model_seconds" => sum(run, "/round_trips/coordination/model_seconds"),
        "tool_seconds" | "output_bytes" | "changed_bytes" => sum(run, &format!("/{metric}")),
        "load_balance" => {
            let busy = per_agent(run, "/busy_seconds")?;
            let mean = busy.iter().sum::<f64>() / busy.len() as f64;
            (mean > 0.0).then(|| busy.iter().copied().fold(0.0, f64::max) / mean)
        }
        "first_development_seconds" => run["developments"]["arrival_seconds"][0].as_f64(),
        "drafts_after_first_development" => {
            run["developments"]["drafts_shared_after_first"].as_f64()
        }
        class => run["tokens"]["tokens"][class.strip_prefix("tokens_")?].as_f64(),
    }
}

/// One `timing` figure of every agent of a run, or None when any agent lacks it.
fn per_agent(run: &Value, pointer: &str) -> Option<Vec<f64>> {
    let agents = run["transcript"]["agents"].as_object()?;
    let values: Option<Vec<f64>> = agents
        .values()
        .map(|a| a.pointer(pointer)?.as_f64())
        .collect();
    values.filter(|v| !v.is_empty())
}

fn sum(run: &Value, pointer: &str) -> Option<f64> {
    per_agent(run, pointer).map(|v| v.iter().sum())
}

/// The sample coefficient of variation (standard deviation with n − 1, over the mean), the
/// statistic of #178's evidence record §4; None below two values.
fn variation(values: &[Option<f64>]) -> Option<f64> {
    let present: Vec<f64> = values.iter().flatten().copied().collect();
    let n = present.len() as f64;
    let mean = present.iter().sum::<f64>() / n;
    let squares: f64 = present.iter().map(|x| (x - mean).powi(2)).sum();
    (present.len() >= 2 && mean > 0.0).then(|| (squares / (n - 1.0)).sqrt() / mean)
}

/// Each repetition's value (null when missing), the median, range and n.
fn stats(values: &[Option<f64>]) -> Value {
    let present: Vec<f64> = values.iter().flatten().copied().collect();
    let low = present.iter().copied().reduce(f64::min);
    let high = present.iter().copied().reduce(f64::max);
    json!({"values": values, "median": median(present.clone()),
        "range": low.zip(high).map(|(low, high)| [low, high]),
        "n": present.len(), "missing": values.len() - present.len()})
}

/// The evidence label of one metric on one fixture (#5 amendment, decision 5).
fn label(git: &[Option<f64>], falinks: &[Option<f64>]) -> &'static str {
    let complete = |values: &[Option<f64>]| {
        values
            .iter()
            .copied()
            .collect::<Option<Vec<f64>>>()
            .filter(|v| !v.is_empty())
    };
    let (Some(git), Some(falinks)) = (complete(git), complete(falinks)) else {
        return "incomplete";
    };
    let below = |a: &[f64], b: &[f64]| a.iter().all(|x| b.iter().all(|y| x < y));
    if below(&git, &falinks) || below(&falinks, &git) {
        "consistent"
    } else {
        "mixed"
    }
}

/// #228 decision 5: properties of the treatment, stated rather than measured.
const TREATMENT: [&str; 2] = [
    "The Falinks arm's enrollment fixes the file set: no new production files, and one test file per source file.",
    "Formatter access differs: the Git arm runs gofmt and cargo fmt directly, while the Falinks arm has only an enrolled gofmt job and no rustfmt job (#224). Formatting is normalized before measurement and is not a metric.",
];

/// One fixture's deterministic code-quality metrics from `quality` (#230), per arm, with the
/// usual statistics and labels. Each repetition that lacks a value names why in
/// `missing_reasons`; each lint count gives its per-lint breakdown per repetition, and
/// `failing_properties` (#231) the failing property IDs.
fn code_quality(fixture: &str, slots: &BTreeMap<&str, Vec<Option<&Value>>>) -> Value {
    let rust = crate::fixture(fixture).is_ok_and(|f| f["language"] == "rust");
    let lint = if rust {
        "clippy_findings"
    } else {
        "vet_findings"
    };
    let metrics = [
        "production_lines_changed",
        "test_lines_added",
        "test_functions",
        lint,
        "failing_properties",
    ];
    let mut out = serde_json::Map::new();
    let mut labels = serde_json::Map::new();
    let mut values = BTreeMap::new();
    for arm in ARMS {
        let mut entry = serde_json::Map::new();
        for metric in metrics {
            let cells: Vec<(Option<f64>, Value)> = slots[arm]
                .iter()
                .map(|run| {
                    let Some(run) = run else {
                        return (None, json!("no run: absent or interrupted"));
                    };
                    let quality = &run["quality"];
                    let cell = &quality["metrics"][metric];
                    let reason = [&cell["missing"], &quality["missing"]]
                        .into_iter()
                        .find(|r| r.is_string())
                        .cloned()
                        .unwrap_or_else(|| json!("not measured: run quality on this batch"));
                    match cell["value"].as_f64() {
                        Some(value) => (Some(value), Value::Null),
                        None => (None, reason),
                    }
                })
                .collect();
            let (present, reasons): (Vec<Option<f64>>, Vec<Value>) = cells.into_iter().unzip();
            let mut figure = stats(&present);
            figure["missing_reasons"] = json!(reasons);
            let breakdown = match metric {
                "failing_properties" => Some("properties"),
                _ if metric == lint => Some("lints"),
                _ => None,
            };
            if let Some(key) = breakdown {
                figure[key] = slots[arm]
                    .iter()
                    .map(|r| {
                        r.map_or(Value::Null, |r| {
                            r["quality"]["metrics"][metric][key].clone()
                        })
                    })
                    .collect();
            }
            entry.insert(metric.into(), figure);
            values.insert((arm, metric), present);
        }
        let runs = slots[arm].iter().flatten();
        entry.insert(
            "versions".into(),
            runs.clone().map(|r| json!({"run_id": r["run_id"], "versions": r["quality"]["versions"], "go_note": r["quality"]["go_note"]})).collect(),
        );
        out.insert(arm.into(), Value::Object(entry));
    }
    for metric in metrics {
        labels.insert(
            metric.into(),
            label(&values[&("git", metric)], &values[&("falinks", metric)]).into(),
        );
    }
    let measured = ARMS.iter().flat_map(|arm| slots[arm].iter().flatten());
    let reference = measured
        .clone()
        .map(|r| &r["quality"]["reference_production_lines_changed"])
        .find(|r| !r.is_null());
    let measurements: Vec<&Value> = measured
        .map(|r| &r["quality"]["measurement"])
        .filter(|m| !m.is_null())
        .fold(Vec::new(), |mut seen, m| {
            if !seen.contains(&m) {
                seen.push(m);
            }
            seen
        });
    out.insert("labels".into(), Value::Object(labels));
    out.insert(
        "reference_production_lines_changed".into(),
        json!(reference),
    );
    out.insert("treatment_properties".into(), json!(TREATMENT));
    out.insert("measurements".into(), json!(measurements));
    Value::Object(out)
}

/// `runs` are every recorded result (pilot included) in batch order; all are listed. The
/// metrics use scored runs only. A record naming an earlier one in `replaces` supersedes it. A
/// replaceable outcome is an interruption, not a task outcome: its repetition lacks every
/// metric and, until replaced, it is named in `gaps`. Without `show_code`,
/// `reverted_peer_text` gives a line count in place of the lines (#228 decision 28).
pub fn report(runs: &[Value], show_code: bool) -> Value {
    let replaced: Vec<&Value> = runs.iter().filter_map(|r| r.get("replaces")).collect();
    let current: Vec<&Value> = runs
        .iter()
        .filter(|r| r["scored"] == true && !replaced.contains(&&r["run_id"]))
        .collect();
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
    let mut successes = BTreeMap::from([("git", 0), ("falinks", 0)]);
    let mut gaps = Vec::new();
    for fixture in SCORED_FIXTURES {
        let mut pairs: BTreeMap<&str, BTreeMap<&str, &Value>> = BTreeMap::new();
        for run in current.iter().filter(|r| r["fixture"] == fixture) {
            let (Some(pair), Some(arm)) = (run["pair"].as_str(), run["arm"].as_str()) else {
                continue;
            };
            pairs.entry(pair).or_default().insert(arm, run);
        }
        // Each repetition's run per arm: None when it is absent or interrupted.
        let slots: BTreeMap<&str, Vec<Option<&Value>>> = ARMS
            .map(|arm| {
                let slots = pairs
                    .values()
                    .map(|arms| arms.get(arm).copied().filter(|r| !replaceable(r)));
                (arm, slots.collect())
            })
            .into();
        let succeeded = |r: &Option<&Value>| r.is_some_and(|r| r["outcome"] == "success");
        let arm_successes: BTreeMap<&str, u64> = ARMS
            .map(|arm| {
                (
                    arm,
                    slots[arm].iter().filter(|r| succeeded(r)).count() as u64,
                )
            })
            .into();
        let (mut complete, mut times) = (0, BTreeMap::<&str, Vec<f64>>::new());
        for (git, falinks) in slots["git"].iter().zip(&slots["falinks"]) {
            complete += usize::from(git.is_some() && falinks.is_some());
            if let (Some(g), Some(f)) = (
                git.filter(|_| succeeded(git)),
                falinks.filter(|_| succeeded(falinks)),
            ) {
                times.entry("git").or_default().extend(elapsed(g));
                times.entry("falinks").or_default().extend(elapsed(f));
            }
        }
        let qualifying = times.get("git").map_or(0, Vec::len);
        let git = median(times.remove("git").unwrap_or_default());
        let falinks = median(times.remove("falinks").unwrap_or_default());
        // The time difference over jointly successful pairs; none means no time comparison.
        let reduction = git.zip(falinks).map(|(g, f)| 1.0 - f / g);
        for (arm, n) in &arm_successes {
            *successes.get_mut(arm).unwrap() += n;
        }
        let names = METRICS
            .iter()
            .map(|m| m.to_string())
            .chain(CLASSES.iter().map(|c| format!("tokens_{c}")));
        let mut metrics = serde_json::Map::new();
        for metric in names {
            let values = |arm: &str| -> Vec<Option<f64>> {
                slots[arm]
                    .iter()
                    .map(|r| r.and_then(|r| value(r, &metric)))
                    .collect()
            };
            let (git, falinks) = (values("git"), values("falinks"));
            metrics.insert(
                metric.clone(),
                json!({"git": stats(&git), "falinks": stats(&falinks), "label": label(&git, &falinks)}),
            );
        }
        let scored = |arm: &str| -> Vec<&Value> {
            runs.iter()
                .filter(|r| r["scored"] == true && r["fixture"] == fixture && r["arm"] == arm)
                .collect()
        };
        let mut costs: serde_json::Map<String, Value> = ARMS
            .iter()
            .map(|arm| (arm.to_string(), cost(&scored(arm), arm_successes[arm])))
            .collect();
        let falinks_runs = &slots["falinks"];
        let per_run = |f: &dyn Fn(&Value) -> Option<f64>| -> Value {
            stats(
                &falinks_runs
                    .iter()
                    .map(|r| r.and_then(f))
                    .collect::<Vec<_>>(),
            )
        };
        // Falinks only: the Git arm has no `sandbox_instructions` (#206 decision 5).
        let key = "sandbox_excluded_cost_usd";
        let (missing, dollars, per_success) =
            cost_line(&scored("falinks"), arm_successes["falinks"], key);
        costs["falinks"]["sandbox_excluded"] = json!({"label": SANDBOX_LABEL,
            "per_run": per_run(&|r| r["tokens"][key].as_f64()), "missing": missing,
            "cost_usd": usd(dollars), "cost_per_success_usd": per_success});
        // A run with a transcript and no refusal of an op counts 0 for it.
        let key_refusals = |r: &Value| r["transcript"]["request_key_refusals"].as_object().cloned();
        let ops: BTreeSet<String> = falinks_runs
            .iter()
            .flatten()
            .filter_map(|r| key_refusals(r))
            .flat_map(|refusals| refusals.into_iter().map(|(op, _)| op))
            .collect();
        let refusals = json!({
            "total": per_run(&|r| key_refusals(r).map(|m| m.values().filter_map(Value::as_f64).sum())),
            "ops": ops.iter().map(|op| {
                let count = per_run(&|r| key_refusals(r).map(|m| m.get(op).and_then(Value::as_f64).unwrap_or(0.0)));
                (op.clone(), count)
            }).collect::<serde_json::Map<_, _>>(),
        });
        let edits: serde_json::Map<String, Value> = FORMS
            .iter()
            .map(|form| {
                let mut entry = serde_json::Map::new();
                for count in EDIT_COUNTS {
                    entry.insert(
                        count.into(),
                        per_run(&|r| r["edits"]["forms"][form][count].as_f64()),
                    );
                }
                for key in RECOVERY {
                    let figure =
                        per_run(&|r| r["transcript"]["refusal_recovery"][form][key].as_f64());
                    entry.insert(format!("recovery_{key}"), figure);
                }
                let reasons: Vec<Value> = falinks_runs
                    .iter()
                    .flatten()
                    .map(|r| {
                        let form = &r["edits"]["forms"][form];
                        json!({"run_id": r["run_id"], "rejected": form["rejected_reasons"],
                            "host_refused": form["host_refused_reasons"]})
                    })
                    .filter(|r| {
                        r["rejected"].as_array().is_some_and(|a| !a.is_empty())
                            || r["host_refused"].as_array().is_some_and(|a| !a.is_empty())
                    })
                    .collect();
                entry.insert("reasons".into(), json!(reasons));
                (form.to_string(), Value::Object(entry))
            })
            .collect();
        let reverted: Vec<Value> = falinks_runs
            .iter()
            .flatten()
            .flat_map(|r| {
                let entries = r["edits"]["reverted_peer_text"].as_array().cloned();
                entries.into_iter().flatten().map(|mut entry| {
                    entry["run_id"] = r["run_id"].clone();
                    entry["line_count"] = json!(entry["lines"].as_array().map_or(0, Vec::len));
                    if !show_code {
                        entry.as_object_mut().map(|e| e.remove("lines"));
                    }
                    entry
                })
            })
            .collect();
        if complete < 3 {
            gaps.push(format!("{fixture}: {complete} of 3 complete pairs"));
        }
        if qualifying == 0 {
            gaps.push(format!("{fixture}: no jointly successful pairs"));
        }
        fixtures.insert(
            fixture.into(),
            json!({
                "pairs": pairs.iter().map(|(pair, arms)| json!({"pair": pair,
                    "arms": arms.iter().map(|(arm, r)| (arm.to_string(), json!({"run_id": r["run_id"],
                        "outcome": r["outcome"], "elapsed_seconds": elapsed(r), "concurrent": concurrent(r)})))
                        .collect::<serde_json::Map<_, _>>()}))
                    .collect::<Vec<_>>(),
                "complete_pairs": complete, "successes": arm_successes,
                "qualifying_pairs": qualifying,
                "median_seconds": {"git": git, "falinks": falinks}, "median_reduction": reduction,
                "metrics": metrics, "label_note": LABEL_NOTE,
                "cost": costs,
                "code_quality": code_quality(fixture, &slots),
                "falinks_edits": edits,
                "falinks_request_key_refusals": refusals,
                "reverted_peer_text": reverted,
            }),
        );
    }
    for run in current.iter().filter(|r| replaceable(r)) {
        // As `replaceable` reads it: no outcome is an infrastructure failure.
        let outcome = run["outcome"].as_str().unwrap_or("infrastructure_failure");
        let id = run["run_id"].as_str().unwrap_or_default();
        gaps.push(format!("unreplaced interruption {id}: {outcome}"));
    }
    for run in runs.iter().filter(|r| r["outcome"] == "run_invalid") {
        let id = run["run_id"].as_str().unwrap_or_default();
        gaps.push(format!("invalid run {id}: nested agent"));
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
    let transitions: u64 = current
        .iter()
        .filter(|r| r["arm"] == "falinks" && !replaceable(r))
        .filter_map(|r| r["transitions"].as_u64())
        .sum();
    json!({
        "qualification": "Exploratory: three repetitions per fixture and no p-values. No winner is computed; the decision is written on the ticket. Cost is API-equivalent list price, not billed cost.",
        "runs": runs.iter().map(|r| json!({"run_id": r["run_id"], "pair": r["pair"], "arm": r["arm"],
            "scored": r["scored"], "outcome": r["outcome"], "replaces": r.get("replaces"),
            "failure_reason": r.get("failure_reason"), "concurrency": r.get("concurrency"),
            "nested_agent": r.get("nested_agent"), "tokens": r.get("tokens")})).collect::<Vec<_>>(),
        "concurrent_runs": runs.iter().filter(|r| concurrent(r)).map(|r| &r["run_id"]).collect::<Vec<_>>(),
        "concurrency_note": "Times of concurrent_runs were measured while another shard's run was active.",
        "fixtures": fixtures,
        "successes": successes,
        "gaps": gaps,
        "safety_failures": unsafe_runs,
        "nested_agent_hits": {"git": nested("git"), "falinks": nested("falinks")},
        "falinks_transitions": transitions,
        "regrouping_benefit": if transitions == 0 { "unestablished: no transitions" } else { "observed transitions; benefit needs review" },
        "replaced_runs": replaced,
    })
}

/// Track-T report (#178, #182): one agent against two Git agents, per fixture. Each metric
/// gives every repetition's value per team size, the median, range and n, the evidence label,
/// and the paired difference (two agents minus one) per repetition; completion time also gets
/// its sample coefficient of variation per team size. Like `report`, it computes no winner.
pub fn track_t_report(runs: &[Value]) -> Value {
    let replaced: Vec<&Value> = runs.iter().filter_map(|r| r.get("replaces")).collect();
    let current: Vec<&Value> = runs
        .iter()
        .filter(|r| !replaced.contains(&&r["run_id"]))
        .collect();
    let size = |r: &Value| r["team_size"].as_u64().map(|s| s.to_string());
    let mut fixtures = serde_json::Map::new();
    let mut gaps = Vec::new();
    for fixture in FIXTURES {
        let mut pairs: BTreeMap<&str, BTreeMap<String, &Value>> = BTreeMap::new();
        for run in current.iter().filter(|r| r["fixture"] == fixture) {
            let (Some(pair), Some(size)) = (run["pair"].as_str(), size(run)) else {
                continue;
            };
            pairs.entry(pair).or_default().insert(size, run);
        }
        // A fixture this batch did not run (`--fixtures`).
        if pairs.is_empty() {
            continue;
        }
        // Each repetition's run per team size: None when it is absent or interrupted.
        let slots: BTreeMap<&str, Vec<Option<&Value>>> = SIZES
            .map(|s| {
                let slots = pairs
                    .values()
                    .map(|sizes| sizes.get(s).copied().filter(|r| !replaceable(r)));
                (s, slots.collect())
            })
            .into();
        let complete = slots["1"]
            .iter()
            .zip(&slots["2"])
            .filter(|(one, two)| one.is_some() && two.is_some())
            .count();
        if complete < 5 {
            gaps.push(format!("{fixture}: {complete} of 5 complete pairs"));
        }
        let names = METRICS
            .iter()
            .chain(&TRACK_T_METRICS)
            .map(|m| m.to_string())
            .chain(CLASSES.iter().map(|c| format!("tokens_{c}")));
        let mut metrics = serde_json::Map::new();
        let mut cv = serde_json::Map::new();
        for metric in names {
            let values = |s: &str| -> Vec<Option<f64>> {
                slots[s]
                    .iter()
                    .map(|r| r.and_then(|r| value(r, &metric)))
                    .collect()
            };
            let (one, two) = (values("1"), values("2"));
            let paired: Vec<Option<f64>> = one
                .iter()
                .zip(&two)
                .map(|(one, two)| Some(two.as_ref()? - one.as_ref()?))
                .collect();
            if metric == "completion_seconds" {
                cv = SIZES
                    .iter()
                    .zip([&one, &two])
                    .map(|(s, v)| (s.to_string(), json!(variation(v))))
                    .collect();
            }
            metrics.insert(
                metric.clone(),
                json!({"1": stats(&one), "2": stats(&two), "label": label(&one, &two),
                    "paired_differences": paired}),
            );
        }
        let successes: BTreeMap<&str, u64> = SIZES
            .map(|s| {
                let ok = slots[s]
                    .iter()
                    .flatten()
                    .filter(|r| r["outcome"] == "success");
                (s, ok.count() as u64)
            })
            .into();
        // Every run of a team size, failed, interrupted and replaced runs included.
        let costs: serde_json::Map<String, Value> = SIZES
            .iter()
            .map(|s| {
                let all: Vec<&Value> = runs
                    .iter()
                    .filter(|r| r["fixture"] == fixture && size(r).as_deref() == Some(*s))
                    .collect();
                (s.to_string(), cost(&all, successes[s]))
            })
            .collect();
        fixtures.insert(
            fixture.into(),
            json!({
                "pairs": pairs.iter().map(|(pair, sizes)| json!({"pair": pair,
                    "team_sizes": sizes.iter().map(|(s, r)| (s.clone(), json!({"run_id": r["run_id"],
                        "outcome": r["outcome"], "elapsed_seconds": r["timing"]["elapsed_seconds"],
                        "slowest_agent": r["transcript"]["slowest_agent"],
                        "agents": r["transcript"]["agents"], "usage_status": r["usage"]["status"],
                        "developments": r["developments"]})))
                        .collect::<serde_json::Map<_, _>>()}))
                    .collect::<Vec<_>>(),
                "complete_pairs": complete, "successes": successes,
                "completion_variation": cv,
                "metrics": metrics, "label_note": TRACK_T_NOTE,
                "cost": costs,
            }),
        );
    }
    for run in current.iter().filter(|r| replaceable(r)) {
        let outcome = run["outcome"].as_str().unwrap_or("infrastructure_failure");
        let id = run["run_id"].as_str().unwrap_or_default();
        gaps.push(format!("unreplaced interruption {id}: {outcome}"));
    }
    for run in runs.iter().filter(|r| r["outcome"] == "run_invalid") {
        let id = run["run_id"].as_str().unwrap_or_default();
        gaps.push(format!("invalid run {id}: nested agent"));
    }
    // Runs a nested agent made invalid (#178 C7), per team size.
    let nested: serde_json::Map<String, Value> = SIZES
        .iter()
        .map(|s| {
            let hits = runs
                .iter()
                .filter(|r| size(r).as_deref() == Some(*s) && !r["nested_agent"].is_null());
            (s.to_string(), json!(hits.count()))
        })
        .collect();
    json!({
        "qualification": "Exploratory: five repetitions per team size and fixture, Git arm only, and no p-values. No winner is computed; the decision is written on the ticket. Cost is API-equivalent list price, not billed cost.",
        "fixtures": fixtures,
        "gaps": gaps,
        "nested_agent_hits": nested,
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
        for (f, fixture) in SCORED_FIXTURES.iter().enumerate() {
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

    fn uniform() -> Vec<Value> {
        batch([[700.0; 3], [800.0; 3], [1100.0; 3]])
    }

    fn find<'a>(runs: &'a mut [Value], id: &str) -> &'a mut Value {
        runs.iter_mut().find(|r| r["run_id"] == id).unwrap()
    }

    /// Five track-T pairs on go-indep-4; `secs` are (one agent, two agents) per repetition.
    fn track_t_runs(secs: [(f64, f64); 5]) -> Vec<Value> {
        let mut runs = vec![];
        for (rep, (one, two)) in secs.into_iter().enumerate() {
            for (size, secs) in [(1, one), (2, two)] {
                let id = format!("go-indep-4-{}-git-{size}", rep + 1);
                let mut run = run(&id, "go-indep-4", rep as u32 + 1, "git", "success", secs);
                run["scored"] = json!(false);
                run["team_size"] = json!(size);
                runs.push(run);
            }
        }
        runs
    }

    #[test]
    fn variation_is_the_sample_coefficient_of_variation() {
        // #178's evidence record §4: go-relationships, Git arm, 13.1%.
        let cv = variation(&[Some(79.6), Some(73.8), Some(61.3)]).unwrap();
        assert!((cv - 0.131).abs() < 0.0005, "{cv}");
        assert_eq!(variation(&[Some(1.0), None]), None);
    }

    #[test]
    fn track_t_compares_team_sizes_per_repetition() {
        let mut runs = track_t_runs([
            (300.0, 200.0),
            (310.0, 190.0),
            (290.0, 210.0),
            (305.0, 205.0),
            (295.0, 195.0),
        ]);
        let two = find(&mut runs, "go-indep-4-1-git-2");
        two["transcript"] =
            json!({"agents": {"A": {"busy_seconds": 30.0}, "B": {"busy_seconds": 10.0}}});
        two["developments"] = json!({"arrival_seconds": [12.5], "drafts_shared_after_first": 3});
        let out = track_t_report(&runs);
        let fixture = &out["fixtures"]["go-indep-4"];
        let time = &fixture["metrics"]["completion_seconds"];
        assert_eq!(time["1"]["n"], 5);
        assert_eq!(time["2"]["median"], 200.0);
        assert_eq!(time["label"], "consistent");
        assert_eq!(
            time["paired_differences"],
            json!([-100.0, -120.0, -80.0, -100.0, -100.0])
        );
        assert!(fixture["completion_variation"]["1"].is_f64());
        let metric = |name: &str| &fixture["metrics"][name]["2"]["values"][0];
        // 30 over a mean of 20.
        assert_eq!(metric("load_balance"), 1.5);
        assert_eq!(metric("first_development_seconds"), 12.5);
        assert_eq!(metric("drafts_after_first_development"), 3.0);
        assert_eq!(out["gaps"], json!([]));
        assert!(out["fixtures"].get("go-page").is_none());
        // A missing repetition leaves the cell incomplete and is named.
        runs.retain(|r| r["run_id"] != "go-indep-4-5-git-1");
        let out = track_t_report(&runs);
        let time = &out["fixtures"]["go-indep-4"]["metrics"]["completion_seconds"];
        assert_eq!(time["label"], "incomplete");
        assert_eq!(time["paired_differences"][4], Value::Null);
        assert_eq!(out["gaps"], json!(["go-indep-4: 4 of 5 complete pairs"]));
    }

    #[test]
    fn medians_handle_odd_even_and_empty() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![4.0, 1.0]), Some(2.5));
        assert_eq!(median(vec![]), None);
    }

    #[test]
    fn failed_checks_count_until_the_first_pass() {
        let attempts = |passed: &[bool]| {
            let attempts: Vec<Value> = passed.iter().map(|p| json!({"passed": p})).collect();
            value(
                &json!({"validation_attempts": attempts}),
                "failed_checks_before_pass",
            )
        };
        assert_eq!(attempts(&[false, false, true, false]), Some(2.0));
        assert_eq!(attempts(&[false]), Some(1.0));
        assert_eq!(attempts(&[]), Some(0.0));
        assert_eq!(value(&json!({}), "failed_checks_before_pass"), None);
    }

    #[test]
    fn the_report_computes_no_winner() {
        let out = report(&uniform(), false);
        for key in [
            "verdict",
            "continuation_signal",
            "fixtures_meeting_threshold",
        ] {
            assert!(out.get(key).is_none(), "{key}");
        }
        let page = &out["fixtures"]["go-page"];
        assert!(page.get("meets_threshold").is_none());
        // The time difference stays as a number.
        assert!((page["median_reduction"].as_f64().unwrap() - 0.2).abs() < 1e-9);
        assert_eq!(out["gaps"], json!([]));
        // Code quality per arm, labelled like every metric, with no winner either.
        let quality = &page["code_quality"];
        assert_eq!(quality["labels"]["vet_findings"], "incomplete");
        assert!(quality["git"].is_object() && quality.get("winner").is_none());
        assert_eq!(out["regrouping_benefit"], "unestablished: no transitions");
    }

    #[test]
    fn non_overlapping_ranges_print_consistent_with_the_note() {
        let out = report(
            &batch([[700.0, 800.0, 900.0], [800.0; 3], [1100.0; 3]]),
            false,
        );
        let rust = &out["fixtures"]["rust-errors"];
        let time = &rust["metrics"]["completion_seconds"];
        assert_eq!(time["label"], "consistent", "{time}");
        assert_eq!(
            time["falinks"],
            json!({"values": [700.0, 800.0, 900.0], "median": 800.0, "range": [700.0, 900.0],
                "n": 3, "missing": 0})
        );
        assert_eq!(
            out["fixtures"]["go-relationships"]["metrics"]["completion_seconds"]["label"],
            "consistent"
        );
        let note = rust["label_note"].as_str().unwrap();
        assert!(
            note.contains("2/20 of the time per cell at n = 3"),
            "{note}"
        );
        assert!(note.contains("not proof of a meaningful or generalizable advantage"));
        // Equal values overlap.
        assert_eq!(rust["metrics"]["success"]["label"], "mixed");
        assert_eq!(rust["metrics"]["refused_operations"]["label"], "incomplete");
    }

    #[test]
    fn overlapping_ranges_print_mixed() {
        let out = report(
            &batch([[900.0, 1000.0, 1100.0], [800.0; 3], [800.0; 3]]),
            false,
        );
        let time = &out["fixtures"]["rust-errors"]["metrics"]["completion_seconds"];
        assert_eq!(time["label"], "mixed", "{time}");
    }

    #[test]
    fn a_run_missing_usage_or_transcripts_prints_incomplete() {
        let mut runs = uniform();
        for run in runs.iter_mut() {
            run["tokens"] = json!({"cost_usd": 1.0, "tokens": {"output": 10}});
            run["transcript"] = json!({"round_trips": {"count": 20, "model_seconds": 50.0}});
        }
        let out = report(&runs, false);
        let metrics = &out["fixtures"]["go-page"]["metrics"];
        assert_eq!(metrics["cost_usd"]["label"], "mixed");
        assert_eq!(metrics["round_trips"]["label"], "mixed");
        let page = find(&mut runs, "go-page-1-f");
        page["tokens"] = json!({"status": "missing", "cost_usd": null, "tokens": null});
        page["transcript"] = Value::Null;
        let out = report(&runs, false);
        let metrics = &out["fixtures"]["go-page"]["metrics"];
        for metric in ["cost_usd", "tokens_output", "model_seconds", "round_trips"] {
            assert_eq!(metrics[metric]["label"], "incomplete", "{metric}");
            // Shown as missing, never imputed.
            assert_eq!(metrics[metric]["falinks"]["missing"], 1, "{metric}");
            assert_eq!(metrics[metric]["falinks"]["n"], 2, "{metric}");
            assert_eq!(
                metrics[metric]["falinks"]["values"][1],
                Value::Null,
                "{metric}"
            );
        }
        assert_eq!(metrics["cost_usd"]["git"]["missing"], 0);
    }

    #[test]
    fn a_failed_run_counts_in_cost_per_successful_task() {
        let mut runs = uniform();
        for run in runs.iter_mut() {
            run["tokens"] = json!({"cost_usd": 1.0, "tokens": {"output": 10}});
        }
        find(&mut runs, "go-page-0-f")["outcome"] = "task_failure".into();
        let out = report(&runs, false);
        let page = &out["fixtures"]["go-page"];
        let falinks = &page["cost"]["falinks"];
        // Three runs cost $3 and two succeeded.
        assert_eq!(falinks["cost_per_success_usd"], 1.5);
        assert_eq!(falinks["tokens"]["output"], 30);
        assert_eq!(
            page["metrics"]["success"]["falinks"]["values"],
            json!([0.0, 1.0, 1.0])
        );
        // A failed run is never timed as a success.
        assert_eq!(
            page["metrics"]["completion_seconds"]["falinks"]["missing"],
            1
        );
        assert_eq!(page["qualifying_pairs"], 2);
        find(&mut runs, "go-page-0-f")["tokens"] = json!({"status": "missing", "cost_usd": null});
        let falinks = &report(&runs, false)["fixtures"]["go-page"]["cost"]["falinks"];
        assert_eq!(falinks["missing"], 1);
        assert_eq!(falinks["cost_per_success_usd"], Value::Null);
    }

    #[test]
    fn per_form_edit_counts_follow_each_recorded_ledger() {
        let mut runs = uniform();
        // Each Falinks run's edits as `adapters/claude/arm.rs` records them from its ledger.
        for (rep, accepted) in [3, 5, 4].into_iter().enumerate() {
            let mut content: serde_json::Map<String, Value> = EDIT_COUNTS
                .iter()
                .map(|c| (c.to_string(), json!(0)))
                .collect();
            content.insert("accepted".into(), json!(accepted));
            content.insert("rejected".into(), json!(1));
            content.insert(
                "rejected_reasons".into(),
                json!(["a.go: hunk 2: old text matches 0 places"]),
            );
            content.insert("host_refused_reasons".into(), json!([]));
            let run = find(&mut runs, &format!("go-page-{rep}-f"));
            run["edits"] = json!({"forms": {"content": content},
                "reverted_peer_text": [{"agent": 1, "id": "b3", "path": "a.go", "peer_id": "a2", "lines": ["x"]}]});
            run["transcript"] = json!({"refusal_recovery": {"content":
                {"refusals": 1, "unresolved": 0, "round_trips": rep + 2, "model_seconds": 4.0}}});
        }
        find(&mut runs, "go-page-2-f")
            .as_object_mut()
            .unwrap()
            .remove("edits");
        let out = report(&runs, false);
        let page = &out["fixtures"]["go-page"];
        let content = &page["falinks_edits"]["content"];
        assert_eq!(
            content["accepted"],
            json!({"values": [3.0, 5.0, null], "median": 4.0, "range": [3.0, 5.0], "n": 2, "missing": 1})
        );
        assert_eq!(content["rejected"]["values"], json!([1.0, 1.0, null]));
        assert_eq!(
            content["recovery_round_trips"]["values"],
            json!([2.0, 3.0, 4.0])
        );
        assert_eq!(content["reasons"][1]["run_id"], "go-page-1-f");
        assert_eq!(
            content["reasons"][1]["rejected"][0],
            "a.go: hunk 2: old text matches 0 places"
        );
        // A form the ledger never saw is missing, never zero.
        assert_eq!(page["falinks_edits"]["replace"]["accepted"]["n"], 0);
        let reverted = page["reverted_peer_text"].as_array().unwrap();
        assert_eq!(reverted.len(), 2);
        assert_eq!(reverted[0]["run_id"], "go-page-0-f");
        assert_eq!(reverted[0]["peer_id"], "a2");
    }

    /// Usage as `tokens` gives it: one session per agent, with its first call's cache figures.
    fn usage(arm: &str, measured: f64, cold_start: f64, sandbox_excluded: Option<f64>) -> Value {
        let (read, sandbox) = if arm == "git" {
            (4777, Value::Null)
        } else {
            (0, json!(3100.0))
        };
        let session = json!([{"first_call": {"cache_read": read, "cache_write": 4000}, "sandbox_tokens": sandbox}]);
        json!({"cost_usd": measured, "cold_start_cost_usd": cold_start,
            "sandbox_excluded_cost_usd": sandbox_excluded, "tokens": {"output": 10},
            "agents": {"A": {"sessions": session}, "B": {"sessions": session}}})
    }

    #[test]
    fn cold_start_cost_is_printed_beside_the_measured_cost_with_first_call_figures() {
        let mut runs = uniform();
        for run in runs.iter_mut() {
            run["tokens"] = match run["arm"].as_str() {
                Some("git") => usage("git", 1.0, 1.1, None),
                _ => usage("falinks", 3.0, 3.0, Some(2.9)),
            };
        }
        find(&mut runs, "go-page-1-g")["tokens"]["cold_start_cost_usd"] = json!(1.2);
        let out = report(&runs, false);
        let page = &out["fixtures"]["go-page"];
        let cold = &page["metrics"]["cold_start_cost_usd"];
        assert_eq!(
            cold["git"],
            json!({"values": [1.1, 1.2, 1.1], "median": 1.1, "range": [1.1, 1.2], "n": 3, "missing": 0})
        );
        assert_eq!(cold["label"], "consistent");
        let git = &page["cost"]["git"];
        assert_eq!(git["cost_per_success_usd"], 1.0);
        // $3.40 over three successes.
        assert_eq!(git["cold_start_cost_per_success_usd"], 1.1333);
        assert_eq!(git["label"], "API-equivalent list price, not billed cost");
        // Each agent's first call, per run.
        assert_eq!(git["first_calls"].as_array().unwrap().len(), 6);
        assert_eq!(
            git["first_calls"][0],
            json!({"run_id": "go-page-0-g", "agent": "A", "cache_read": 4777, "cache_write": 4000})
        );
        assert_eq!(page["cost"]["falinks"]["first_calls"][1]["cache_read"], 0);
        // The sandbox line is the Falinks arm's only.
        assert!(git.get("sandbox_excluded").is_none());
    }

    #[test]
    fn the_sandbox_line_carries_its_label_and_shows_a_session_without_the_attachment_as_missing() {
        let mut runs = uniform();
        for run in runs.iter_mut() {
            run["tokens"] = usage("falinks", 3.0, 3.0, Some(2.9));
        }
        let sandbox = |runs: &[Value]| {
            report(runs, false)["fixtures"]["go-page"]["cost"]["falinks"]["sandbox_excluded"]
                .clone()
        };
        let line = sandbox(&runs);
        assert_eq!(
            line["label"],
            "API-equivalent list price, not billed cost; estimate ±15%"
        );
        assert_eq!(line["per_run"]["values"], json!([2.9, 2.9, 2.9]));
        assert_eq!(line["cost_per_success_usd"], 2.9);
        // A session without the attachment leaves its run's value missing, never zero.
        find(&mut runs, "go-page-2-f")["tokens"]["sandbox_excluded_cost_usd"] = Value::Null;
        let line = sandbox(&runs);
        assert_eq!(line["per_run"]["values"], json!([2.9, 2.9, null]));
        assert_eq!(line["per_run"]["missing"], 1);
        assert_eq!(line["missing"], 1);
        assert_eq!(line["cost_per_success_usd"], Value::Null);
    }

    #[test]
    fn request_key_refusals_are_counted_per_op() {
        let mut runs = uniform();
        find(&mut runs, "go-page-0-f")["transcript"] =
            json!({"request_key_refusals": {"handle": 2, "ack": 1}});
        find(&mut runs, "go-page-1-f")["transcript"] = json!({"request_key_refusals": {}});
        let refusals = &report(&runs, false)["fixtures"]["go-page"]["falinks_request_key_refusals"];
        // The third run has no transcript: missing, never zero.
        assert_eq!(refusals["ops"]["handle"]["values"], json!([2.0, 0.0, null]));
        assert_eq!(refusals["ops"]["ack"]["values"], json!([1.0, 0.0, null]));
        assert_eq!(refusals["total"]["values"], json!([3.0, 0.0, null]));
        assert_eq!(refusals["ops"].as_object().unwrap().len(), 2, "{refusals}");
    }

    #[test]
    fn no_joint_successes_means_no_time_difference() {
        let mut runs = uniform();
        for run in runs
            .iter_mut()
            .filter(|r| r["fixture"] == "go-page" && r["arm"] == "git")
        {
            run["outcome"] = "timeout".into();
        }
        let out = report(&runs, false);
        let page = &out["fixtures"]["go-page"];
        assert_eq!(page["qualifying_pairs"], 0);
        assert_eq!(page["median_reduction"], Value::Null);
        assert_eq!(page["metrics"]["completion_seconds"]["label"], "incomplete");
        assert_eq!(page["metrics"]["success"]["label"], "consistent");
        assert_eq!(out["gaps"], json!(["go-page: no jointly successful pairs"]));
    }

    #[test]
    fn replacements_supersede_infrastructure_failures_and_stay_listed() {
        let mut runs = uniform();
        runs[0]["outcome"] = "infrastructure_failure".into();
        let mut replacement = runs[0].clone();
        replacement["run_id"] = "rust-errors-0-g-r1".into();
        replacement["replaces"] = runs[0]["run_id"].clone();
        replacement["outcome"] = "success".into();
        runs.push(replacement);
        let out = report(&runs, false);
        assert_eq!(out["fixtures"]["rust-errors"]["qualifying_pairs"], 3);
        assert_eq!(out["replaced_runs"], json!(["rust-errors-0-g"]));
        assert_eq!(out["gaps"], json!([]));
    }

    #[test]
    fn runs_overlapping_a_sibling_shard_are_flagged_both_ways() {
        let mut runs = uniform();
        // go-page-0-g started while rust-errors-0-g ran; rust-errors-0-g saw nothing itself.
        let page = runs
            .iter()
            .position(|r| r["run_id"] == "go-page-0-g")
            .unwrap();
        runs[page]["concurrency"] = json!({"start": ["rust-errors-0-g"], "stop": []});
        let out = report(&runs, false);
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
        let mut runs = uniform();
        runs.truncate(16);
        runs[15]["outcome"] = "usage_limit".into();
        let out = report(&runs, false);
        let gaps = out["gaps"].as_array().unwrap();
        assert_eq!(gaps.len(), 2, "{gaps:?}");
        assert!(
            gaps[1]
                .to_string()
                .contains(runs[15]["run_id"].as_str().unwrap())
        );
        runs[3]["outcome"] = "safety_failure".into();
        let out = report(&runs, false);
        assert_eq!(out["safety_failures"], json!([runs[3]["run_id"]]));
        let safety = &out["fixtures"]["rust-errors"]["metrics"]["safety_incidents"];
        assert_eq!(safety["falinks"]["values"], json!([0.0, 1.0, 0.0]));
    }

    #[test]
    fn unreplaced_interruptions_in_either_arm_are_named_gaps() {
        let f0 = SCORED_FIXTURES[0];
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
            let mut runs = uniform();
            find(&mut runs, &id)["outcome"] = outcome;
            let out = report(&runs, false);
            let fixture = &out["fixtures"][f0];
            assert_eq!(fixture["pairs"].as_array().unwrap().len(), 3);
            assert_eq!(fixture["complete_pairs"], 2);
            // An interruption is not a task outcome: its repetition lacks every metric.
            assert_eq!(fixture["metrics"]["success"]["label"], "incomplete", "{id}");
            let reason = json!(format!("unreplaced interruption {id}: {named}"));
            assert!(
                out["gaps"].as_array().unwrap().contains(&reason),
                "{id}: {out}"
            );
        }
    }

    #[test]
    fn replaced_interruptions_of_either_kind_leave_no_gap() {
        let mut runs = uniform();
        for (i, outcome) in [(0, "infrastructure_failure"), (3, "usage_limit")] {
            let mut replacement = runs[i].clone();
            replacement["run_id"] = format!("{}-r1", runs[i]["run_id"].as_str().unwrap()).into();
            replacement["replaces"] = runs[i]["run_id"].clone();
            runs[i]["outcome"] = outcome.into();
            runs.push(replacement);
        }
        let out = report(&runs, false);
        assert_eq!(out["gaps"], json!([]), "{out}");
        assert_eq!(out["replaced_runs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_usage_limited_pilot_run_leaves_no_gap() {
        let mut runs = uniform();
        let mut pilot = runs[0].clone();
        pilot["run_id"] = "pilot-g".into();
        pilot["pair"] = "pilot".into();
        pilot["scored"] = false.into();
        pilot["outcome"] = "usage_limit".into();
        runs.insert(0, pilot);
        assert_eq!(report(&runs, false)["gaps"], json!([]));
    }
}
