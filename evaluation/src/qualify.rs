//! Step A's mechanical qualification steps (#498 amendment 1 and 4, #509), under the frozen
//! qualification rule: the static checks with the reference swap, the `git-1` shared-helper
//! check over Go's call graph, and the qualify decision.
use crate::collisions::class;
use crate::{
    Result, bail, blobs, check, fixture, frozen, git, initial, protected, put_files, read,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

/// A function `(directory, "", name)` or a method `(directory, receiver type, name)`.
type Key = (String, String, String);

fn show(key: &Key) -> String {
    let (dir, ty, name) = key;
    match ty.as_str() {
        "" => format!("{dir}.{name}"),
        _ => format!("{dir}.{ty}.{name}"),
    }
}

/// One requirement as the spec states it: who owns it and the functions and methods it names,
/// its entry functions.
struct Requirement {
    id: String,
    owner: String,
    entries: BTreeSet<Key>,
}

/// The fixture's requirements, each `T<n> Package \`dir\` ... \`func ...\` ...`, with the owner
/// the allocation gives it: the IDs before the first `:` of each agent's allocation.
fn requirements(fixture: &Value) -> Result<Vec<Requirement>> {
    let mut owners = BTreeMap::new();
    for (agent, text) in fixture["allocation"].as_object().into_iter().flatten() {
        let owned = text.as_str().unwrap_or_default().split(':').next().unwrap();
        for word in owned.split(|c: char| !c.is_alphanumeric()) {
            if word.len() > 1 && word.starts_with('T') && word[1..].parse::<u32>().is_ok() {
                owners.insert(word.to_string(), agent.clone());
            }
        }
    }
    let mut out = vec![];
    for text in fixture["requirements"].as_array().into_iter().flatten() {
        let text = text.as_str().unwrap_or_default();
        let id = text.split(' ').next().unwrap().to_string();
        let package = text
            .split_once("Package `")
            .and_then(|(_, rest)| rest.split_once('`'))
            .map(|(p, _)| p)
            .ok_or(format!("{id} names no package"))?;
        let entries = (text.split('`').skip(1).step_by(2))
            .filter_map(|span| span.strip_prefix("func "))
            .map(|header| header_key(package, header))
            .collect();
        let owner = owners.get(&id).ok_or(format!("{id} has no owner"))?.clone();
        out.push(Requirement { id, owner, entries });
    }
    Ok(out)
}

/// The key of a declaration header after `func `: `Name(...` or `(r *Type) Name(...`.
fn header_key(dir: &str, header: &str) -> Key {
    let (ty, rest) = match header.trim_start().strip_prefix('(') {
        Some(rest) => {
            let (receiver, rest) = rest.split_once(')').unwrap_or((rest, ""));
            let ty = receiver.split_whitespace().last().unwrap_or_default();
            (ty.trim_start_matches('*').split('[').next().unwrap(), rest)
        }
        None => ("", header),
    };
    let name = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (dir.into(), ty.into(), name)
}

/// `source` with comments blanked and strings and runes emptied, so nothing inside them counts.
fn code(source: &str) -> String {
    let (mut out, mut chars) = (String::new(), source.chars().peekable());
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                chars.by_ref().find(|c| *c == '\n');
                out.push('\n');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = if c == '\n' { ' ' } else { c };
                    if c == '\n' {
                        out.push('\n');
                    }
                }
                out.push(' ');
            }
            '"' | '\'' | '`' => {
                let mut escaped = false;
                for d in chars.by_ref() {
                    if d == c && (c == '`' || !escaped) {
                        break;
                    }
                    escaped = c != '`' && d == '\\' && !escaped;
                }
                out.push_str("\"\"");
            }
            _ => out.push(c),
        }
    }
    out
}

/// What a reference in a body can reach: a package's function, or any method of that name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Function(String, String),
    Method(String),
}

/// Each non-test Go file's top-level functions and methods, with what their bodies reference:
/// `name` (the own package's), `q.name` (the package `q` imports, when `q` is an import name
/// for a package of `module`) and any other `.name` (a method).
fn declarations(tree: &BTreeMap<String, Vec<u8>>, module: &str) -> BTreeMap<Key, BTreeSet<Target>> {
    let mut functions: BTreeMap<Key, BTreeSet<Target>> = BTreeMap::new();
    for (path, bytes) in tree {
        if !path.ends_with(".go") || path.ends_with("_test.go") {
            continue;
        }
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        let source = String::from_utf8_lossy(bytes);
        let code = code(&source);
        let imports = import_names(&source, module);
        // A top-level declaration starts a line with `func`; its body runs to the matching brace.
        let mut rest = code.as_str();
        while let Some(at) = rest.find("\nfunc ") {
            rest = &rest[at + "\nfunc ".len()..];
            let key = header_key(dir, rest);
            let Some(open) = rest.find('{') else { break };
            let mut depth = 0;
            let end = rest[open..]
                .char_indices()
                .find(|(_, c)| {
                    depth += match c {
                        '{' => 1,
                        '}' => -1,
                        _ => 0,
                    };
                    depth == 0
                })
                .map_or(rest.len(), |(i, _)| open + i);
            let targets = functions.entry(key).or_default();
            targets.extend(references(&rest[open..end], dir, &imports));
            rest = &rest[end..];
        }
    }
    functions
}

/// The import names `source` gives packages of `module`: an alias, or the last path element.
/// Dot and blank imports name nothing.
fn import_names(source: &str, module: &str) -> BTreeMap<String, String> {
    let (mut block, mut names) = (false, BTreeMap::new());
    for line in source.lines() {
        let line = line.split("//").next().unwrap().trim();
        let spec = if block {
            block = line != ")";
            line
        } else if let Some(rest) = line.strip_prefix("import") {
            block = rest.trim() == "(";
            rest.trim()
        } else {
            continue;
        };
        let (name, quoted) = match spec.split_whitespace().collect::<Vec<_>>()[..] {
            [p] => (None, p),
            [n, p] => (Some(n), p),
            _ => continue,
        };
        let Some(dir) = quoted
            .trim_matches('"')
            .strip_prefix(module)
            .and_then(|p| p.strip_prefix('/'))
        else {
            continue;
        };
        let name = name.unwrap_or_else(|| dir.rsplit('/').next().unwrap());
        if name != "." && name != "_" {
            names.insert(name.to_string(), dir.to_string());
        }
    }
    names
}

fn references(body: &str, dir: &str, imports: &BTreeMap<String, String>) -> Vec<Target> {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    // Identifiers as themselves, `.` as ".", anything else as "".
    let mut tokens: Vec<&str> = vec![];
    let mut at = 0;
    for (i, c) in body.char_indices() {
        if i < at {
            continue;
        }
        if ident(c) {
            at = body[i..]
                .find(|c: char| !ident(c))
                .map_or(body.len(), |n| i + n);
            tokens.push(&body[i..at]);
        } else if c == '.' {
            tokens.push(".");
        } else if !c.is_whitespace() {
            tokens.push("");
        }
    }
    let word = |t: &str| t.starts_with(ident);
    let mut out = vec![];
    for (i, &t) in tokens.iter().enumerate() {
        if !word(t) {
            continue;
        }
        let before = |n: usize| i.checked_sub(n).map(|j| tokens[j]);
        out.push(match (before(1), before(2), before(3)) {
            (Some("."), Some(q), third)
                if word(q) && third != Some(".") && imports.contains_key(q) =>
            {
                Target::Function(imports[q].clone(), t.into())
            }
            (Some("."), _, _) => Target::Method(t.into()),
            _ => Target::Function(dir.into(), t.into()),
        });
    }
    out
}

/// The call graph: each function's edges to the declared functions its references can reach.
/// shortcut: syntactic, so a method reference reaches every method of that name and a local
/// shadowing a function name counts; upgrade to go/types if a qualification turns on it.
fn graph(tree: &BTreeMap<String, Vec<u8>>, module: &str) -> BTreeMap<Key, BTreeSet<Key>> {
    let functions = declarations(tree, module);
    let keys: Vec<Key> = functions.keys().cloned().collect();
    functions
        .iter()
        .map(|(key, targets)| {
            let edges = keys
                .iter()
                .filter(|k| *k != key)
                .filter(|(d, ty, name)| {
                    targets.iter().any(|t| match t {
                        Target::Function(td, tn) => ty.is_empty() && td == d && tn == name,
                        Target::Method(tn) => !ty.is_empty() && tn == name,
                    })
                })
                .cloned()
                .collect();
            (key.clone(), edges)
        })
        .collect()
}

/// What each requirement reaches from its entry functions without passing through a function
/// another requirement names, and the edges it crosses: (reached, edges).
fn reach(
    graph: &BTreeMap<Key, BTreeSet<Key>>,
    requirements: &[Requirement],
    index: usize,
) -> (BTreeSet<Key>, BTreeSet<(Key, Key)>) {
    let own = &requirements[index].entries;
    let others: BTreeSet<&Key> = (requirements.iter().enumerate())
        .filter(|(i, _)| *i != index)
        .flat_map(|(_, r)| &r.entries)
        .filter(|k| !own.contains(*k))
        .collect();
    let (mut seen, mut edges) = (own.clone(), BTreeSet::new());
    let mut queue: Vec<Key> = own.iter().cloned().collect();
    while let Some(from) = queue.pop() {
        for to in graph.get(&from).into_iter().flatten() {
            edges.insert((from.clone(), to.clone()));
            if !others.contains(to) && seen.insert(to.clone()) {
                queue.push(to.clone());
            }
        }
    }
    (seen, edges)
}

fn module(fixture: &Value) -> Result<String> {
    Ok(fixture["files"]["go.mod"]
        .as_str()
        .and_then(|m| m.lines().find_map(|l| l.strip_prefix("module ")))
        .ok_or("the fixture has no go.mod module")?
        .trim()
        .to_string())
}

/// Qualification step 3: the functions the spec does not name that requirements of different
/// developers reach, each with the requirements that reach it.
pub fn shared_helpers(fixture: &Value, tree: &BTreeMap<String, Vec<u8>>) -> Result<Value> {
    let requirements = requirements(fixture)?;
    let graph = graph(tree, &module(fixture)?);
    let named: BTreeSet<&Key> = requirements.iter().flat_map(|r| &r.entries).collect();
    let mut reached: BTreeMap<Key, Vec<&Requirement>> = BTreeMap::new();
    for (index, requirement) in requirements.iter().enumerate() {
        for key in reach(&graph, &requirements, index).0 {
            if !named.contains(&key) {
                reached.entry(key).or_default().push(requirement);
            }
        }
    }
    let helpers: Vec<Value> = reached
        .into_iter()
        .filter(|(_, by)| by.iter().any(|r| r.owner != by[0].owner))
        .map(|(key, by)| json!({"function": show(&key), "requirements": by.iter().map(|r| &r.id).collect::<Vec<_>>()}))
        .collect();
    Ok(json!(helpers))
}

/// The trap milestone's `release_on_call`: its defining package and function.
fn trap(fixture: &Value) -> Result<&Value> {
    Ok(&(fixture["milestones"].as_array().into_iter().flatten())
        .find(|m| m["release_on_call"].is_object())
        .ok_or("the fixture has no trap milestone")?["release_on_call"])
}

/// Each dependency `T<n> uses T<m>'s \`function\`` on the trap's function, and whether T<n>'s
/// code in `tree` reaches a call to it.
fn trap_callers(fixture: &Value, tree: &BTreeMap<String, Vec<u8>>) -> Result<Vec<(String, bool)>> {
    let call = trap(fixture)?;
    let target: Key = (
        call["package"].as_str().unwrap().into(),
        String::new(),
        call["function"].as_str().unwrap().into(),
    );
    let requirements = requirements(fixture)?;
    let graph = graph(tree, &module(fixture)?);
    let mut out = vec![];
    for dependency in fixture["dependencies"].as_array().into_iter().flatten() {
        let dependency = dependency.as_str().unwrap_or_default();
        if !dependency.ends_with(&format!("'s `{}`", target.2)) {
            continue;
        }
        let user = dependency.split(' ').next().unwrap();
        let index = requirements
            .iter()
            .position(|r| r.id == user)
            .ok_or(format!("{dependency}: no such requirement"))?;
        let edges = reach(&graph, &requirements, index).1;
        out.push((user.to_string(), edges.iter().any(|(_, to)| *to == target)));
    }
    Ok(out)
}

fn text_tree(files: &Value) -> BTreeMap<String, Vec<u8>> {
    (files.as_object().into_iter().flatten())
        .map(|(k, v)| {
            (
                k.clone(),
                v.as_str().unwrap_or_default().as_bytes().to_vec(),
            )
        })
        .collect()
}

/// The Go tests that failed in a check's oracle output.
fn failed_tests(result: &Value) -> BTreeSet<String> {
    (result["oracle"]["checks"].as_array().into_iter().flatten())
        .filter_map(|c| c["output"].as_str())
        .flat_map(str::lines)
        .filter_map(|l| l.trim_start().strip_prefix("--- FAIL: "))
        .map(|l| l.split([' ', '/']).next().unwrap().to_string())
        .collect()
}

/// Qualification step 1's mechanical checks on Step A's fixture: a cross-developer dependency
/// on the trap's function, the reference reaching a call to it from every user the dependency
/// list names, and the reference swap in both directions. The oracle runs on the initial tree
/// plus the corrected reference, and on the initial tree plus the reference overlaid with
/// `old_reference` except the trap function's own file (the old reference with only the
/// defining function corrected).
pub fn static_checks(name: &str) -> Result<Value> {
    frozen()?;
    let (fixture, protected) = (fixture(name)?, protected(name)?);
    let mut tree = text_tree(&fixture["files"]);
    tree.extend(text_tree(&protected["reference"]));
    let callers = trap_callers(&fixture, &tree)?;
    let owners: BTreeMap<String, String> = requirements(&fixture)?
        .into_iter()
        .map(|r| (r.id, r.owner))
        .collect();
    let call = trap(&fixture)?;
    let defining = (requirements(&fixture)?.into_iter())
        .find(|r| {
            r.entries.iter().any(|(d, ty, n)| {
                ty.is_empty()
                    && d == call["package"].as_str().unwrap()
                    && n == call["function"].as_str().unwrap()
            })
        })
        .ok_or("no requirement names the trap's function")?;
    let cross: Vec<&String> = (callers.iter())
        .map(|(id, _)| id)
        .filter(|id| owners[*id] != defining.owner)
        .collect();
    let package = call["package"].as_str().unwrap();
    // #501's tags: `cases` for C2, `side_cases` for the side report.
    let trap = &protected["trap"];
    let tmp = tempfile::Builder::new().prefix("falinks-swap-").tempdir()?;
    let run = |label: &str, overlay: &Value| -> Result<BTreeSet<String>> {
        let repo = tmp.path().join(label);
        initial(&repo, &fixture)?;
        put_files(&repo, overlay)?;
        git(&repo, &["add", "."], None)?;
        git(&repo, &["commit", "-m", label], None)?;
        let private = tmp.path().join(format!("{label}-private"));
        std::fs::create_dir(&private)?;
        let result = check(&repo, &fixture, &private, Duration::from_secs(120), &[])?;
        if result["visible"]["passed"] != true {
            bail!("{label}: the visible checks failed");
        }
        Ok(failed_tests(&result))
    };
    let corrected = run("corrected", &protected["reference"])?;
    let mut swapped = protected["reference"].clone();
    for (file, text) in protected["old_reference"].as_object().into_iter().flatten() {
        if !file.starts_with(&format!("{package}/")) {
            swapped[file] = text.clone();
        }
    }
    let swap = run("swap", &swapped)?;
    let tagged = |key: &str| -> Vec<&String> {
        let listed = trap[key].as_array().cloned().unwrap_or_default();
        swap.iter().filter(|t| listed.contains(&json!(t))).collect()
    };
    let checks = json!({
        "cross_developer_dependents": cross,
        "reference_calls": callers.iter().map(|(id, calls)| json!({"requirement": id, "calls": calls})).collect::<Vec<_>>(),
        "corrected_reference_failures": corrected,
        "swap_failures": swap,
        "swap_c2_failures": tagged("cases"),
        "swap_side_failures": tagged("side_cases"),
    });
    let passed = !cross.is_empty()
        && callers.iter().all(|(_, calls)| *calls)
        && corrected.is_empty()
        && !checks["swap_c2_failures"].as_array().unwrap().is_empty();
    Ok(json!({"passed": passed, "checks": checks,
        "not_mechanical": "whether the dependents are absolute enough (trap file, Oracle) was judged when the trap was fitted"}))
}

/// The tree of a run's final snapshot, from its controller's retained repository.
fn final_tree(run: &Path) -> Result<(Value, BTreeMap<String, Vec<u8>>)> {
    let result = read(run.join("controller/result.json"))?;
    let snapshot = result["final_snapshot"]
        .as_str()
        .ok_or("the run has no final snapshot")?;
    let tree = blobs(&run.join("controller/retained"), snapshot)?;
    Ok((result, tree))
}

/// `helpers RUN_DIR`: the shared helpers in a run's final tree.
pub fn helpers(run: &Path) -> Result<Value> {
    let (result, tree) = final_tree(run)?;
    let fixture = fixture(result["fixture"].as_str().unwrap_or_default())?;
    shared_helpers(&fixture, &tree)
}

/// The median of three or more counts, or null without any.
fn median(mut counts: Vec<u64>) -> Option<f64> {
    counts.sort();
    let n = counts.len();
    (n > 0).then(|| (counts[(n - 1) / 2] + counts[n / 2]) as f64 / 2.0)
}

/// `qualify --runs DIR`: the frozen qualification rule's decision for a `step-a-qualify` batch.
/// Step 4 reads Daniel's classes from the batch's `duplicates.json` (#502): each pair's `run_id` and
/// `class` (`duplicate`, `divergent` or `no`). Until every pair has one, the decision is
/// incomplete.
pub fn qualify(runs: &Path, static_result: Value) -> Result<Value> {
    let ledger = read(runs.join("batch.json"))?;
    let entries: Vec<&Value> = ledger["runs"].as_array().into_iter().flatten().collect();
    // The last run of each slot counts: replacements supersede the runs they replace.
    let last = |slot: &Value| {
        entries
            .iter()
            .rev()
            .find(|e| e["slot"] == slot["slot"])
            .copied()
    };
    let slots = crate::batch::step_a_qualify_slots();
    let done: Vec<&Value> = slots.iter().filter_map(last).collect();
    let runs_done = done.len() == slots.len();
    let of_size = |size: u64| done.iter().filter(move |e| e["team_size"] == size).copied();
    let mut solo = vec![];
    for entry in of_size(1) {
        let run = Path::new(entry["run_dir"].as_str().unwrap_or_default());
        let helpers = helpers(run).unwrap_or_else(|e| json!({"error": e.to_string()}));
        solo.push(json!({"run_id": entry["run_id"], "shared_helpers": helpers}));
    }
    let emergence = runs_done
        && solo.len() == 2
        && solo.iter().all(|r| {
            r["shared_helpers"]
                .as_array()
                .is_some_and(|h| !h.is_empty())
        });
    let classified = read(runs.join("duplicates.json")).unwrap_or(Value::Null);
    let pairs: Vec<&Value> = classified["pairs"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let unclassified = pairs
        .iter()
        .filter(|p| !matches!(class(p), Some("duplicate" | "divergent" | "no")))
        .count();
    let counts: Vec<u64> = of_size(2)
        .map(|e| {
            pairs
                .iter()
                .filter(|p| p["run_id"] == e["run_id"])
                .filter(|p| matches!(class(p), Some("duplicate" | "divergent")))
                .count() as u64
        })
        .collect();
    let gap = median(counts.clone());
    let steps = json!([
        {"step": 1, "name": "static checks", "passed": static_result["passed"], "detail": static_result},
        {"step": 2, "name": "runs", "passed": runs_done,
         "detail": done.iter().map(|e| json!({"slot": e["slot"], "run_id": e["run_id"], "outcome": e["outcome"]})).collect::<Vec<_>>()},
        {"step": 3, "name": "emergence", "passed": emergence, "detail": solo},
        {"step": 4, "name": "gap", "passed": if unclassified > 0 || classified.is_null() { Value::Null } else { (gap.unwrap_or(0.0) >= 1.0).into() },
         "detail": {"pairs_per_run": counts, "median": gap, "unclassified": unclassified}},
    ]);
    let failed = steps
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["passed"] == false);
    let decision = match failed {
        Some(step) => json!({"qualifies": false, "failed_step": step["name"]}),
        None if steps
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["passed"] == true) =>
        {
            json!({"qualifies": true})
        }
        None => {
            json!({"qualifies": null, "reason": "step 4 waits for every pair to be classified"})
        }
    };
    Ok(json!({"decision": decision, "steps": steps}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loans() -> (Value, BTreeMap<String, Vec<u8>>) {
        let fixture = fixture(crate::STEP_A_FIXTURE).unwrap();
        let mut tree = text_tree(&fixture["files"]);
        tree.extend(text_tree(
            &protected(crate::STEP_A_FIXTURE).unwrap()["reference"],
        ));
        (fixture, tree)
    }

    #[test]
    fn requirements_name_their_entry_functions_and_owners() {
        let requirements = requirements(&loans().0).unwrap();
        let owned = |agent: &str| -> Vec<&str> {
            (requirements.iter().filter(|r| r.owner == agent))
                .map(|r| r.id.as_str())
                .collect()
        };
        assert_eq!(owned("A"), ["T2", "T3", "T4", "T6", "T8"]);
        assert_eq!(owned("B"), ["T1", "T5", "T7", "T9"]);
        let shown: Vec<String> = requirements[1].entries.iter().map(show).collect();
        assert_eq!(
            shown,
            [
                "calendar.DaysBetween",
                "calendar.Parse",
                "calendar.Date.AddDays",
                "calendar.Date.String",
                "calendar.Date.Weekday"
            ]
        );
    }

    /// #509: the reference calls the trap's function wherever the dependency list says.
    #[test]
    fn the_reference_calls_days_between_wherever_the_dependencies_say() {
        let (fixture, tree) = loans();
        let callers = trap_callers(&fixture, &tree).unwrap();
        let expected: Vec<(String, bool)> = ["T5", "T6", "T8", "T9"]
            .map(|t| (t.to_string(), true))
            .to_vec();
        assert_eq!(callers, expected);
        // The initial skeleton calls it nowhere.
        let initial = text_tree(&fixture["files"]);
        assert!(
            trap_callers(&fixture, &initial)
                .unwrap()
                .iter()
                .all(|(_, c)| !c)
        );
    }

    /// Qualification step 3 on synthetic trees: only an unnamed function that requirements of
    /// both developers reach, each without passing another requirement's named function.
    #[test]
    fn a_shared_helper_needs_both_developers_and_no_named_function_between() {
        let (fixture, reference) = loans();
        assert_eq!(shared_helpers(&fixture, &reference).unwrap(), json!([]));
        let edit = |tree: &mut BTreeMap<String, Vec<u8>>, file: &str, old: &str, new: &str| {
            let text = String::from_utf8(tree[file].clone()).unwrap();
            assert_eq!(text.matches(old).count(), 1, "{old}");
            tree.insert(file.into(), text.replacen(old, new, 1).into_bytes());
        };
        // A kind check shared by DueDate (T4, A) and CanBorrow (T7, B), in a file of its own.
        let mut tree = reference.clone();
        tree.insert(
            "loan/kinds.go".into(),
            b"package loan\n\nimport \"example.com/lending/item\"\n\n// validKind reports a lendable kind.\nfunc validKind(k item.Kind) bool { return k >= item.Book && k <= item.Magazine }\n".to_vec(),
        );
        edit(
            &mut tree,
            "loan/due.go",
            "days, ok := periods[kind]\n\tif !ok {",
            "days := periods[kind]\n\tif !validKind(kind) {",
        );
        edit(
            &mut tree,
            "loan/limit.go",
            "if k < item.Book || k > item.Magazine {",
            "if !validKind(k) {",
        );
        assert_eq!(
            shared_helpers(&fixture, &tree).unwrap(),
            json!([{"function": "loan.validKind", "requirements": ["T4", "T7"]}])
        );
        // The same helper in a test file, or reached by A's requirements alone, does not count.
        let mut tested = tree.clone();
        let helper = tested.remove("loan/kinds.go").unwrap();
        tested.insert("loan/kinds_test.go".into(), helper.clone());
        assert_eq!(shared_helpers(&fixture, &tested).unwrap(), json!([]));
        let mut alone = reference.clone();
        alone.insert("loan/kinds.go".into(), helper);
        edit(
            &mut alone,
            "loan/due.go",
            "days, ok := periods[kind]\n\tif !ok {",
            "days := periods[kind]\n\tif !validKind(kind) {",
        );
        edit(
            &mut alone,
            "loan/renew.go",
            "case !ok:",
            "case !ok || !validKind(kind):",
        );
        assert_eq!(shared_helpers(&fixture, &alone).unwrap(), json!([]));
        // Calendar's own method, reached by B only through DaysBetween (named by T2), is not
        // shared; reached through a variable in B's code, it is.
        let mut direct = reference.clone();
        edit(
            &mut direct,
            "calendar/calendar.go",
            "func (d Date) time() time.Time {",
            "func (d Date) Time() time.Time {",
        );
        edit(
            &mut direct,
            "calendar/calendar.go",
            "return d.time().Weekday()",
            "return d.Time().Weekday()",
        );
        edit(
            &mut direct,
            "calendar/calendar.go",
            "n := int((to.time().Unix() - from.time().Unix()) / 86400)",
            "n := int((to.Time().Unix() - from.Time().Unix()) / 86400)",
        );
        assert_eq!(shared_helpers(&fixture, &direct).unwrap(), json!([]));
        edit(
            &mut direct,
            "loan/fee.go",
            "late := calendar.DaysBetween(due, returned) - 1",
            "late := int(returned.Time().Sub(due.Time()).Hours() / 24)",
        );
        assert_eq!(
            shared_helpers(&fixture, &direct).unwrap(),
            json!([{"function": "calendar.Date.Time", "requirements": ["T2", "T5"]}])
        );
    }

    /// The decision on a recorded qualification batch: two `git-1` final trees in retained
    /// repositories, three `git-2` runs and their classified pairs.
    #[test]
    fn the_qualify_decision_follows_the_frozen_rule() {
        let (fixture, reference) = loans();
        let dir = tempfile::tempdir().unwrap();
        let mut shared = reference.clone();
        shared.insert(
            "loan/kinds.go".into(),
            b"package loan\n\nimport \"example.com/lending/item\"\n\nfunc validKind(k item.Kind) bool { return k >= item.Book && k <= item.Magazine }\n".to_vec(),
        );
        for (file, old, new) in [
            (
                "loan/due.go",
                "days, ok := periods[kind]\n\tif !ok {",
                "days := periods[kind]\n\tif !validKind(kind) {",
            ),
            (
                "loan/limit.go",
                "if k < item.Book || k > item.Magazine {",
                "if !validKind(k) {",
            ),
        ] {
            let text = String::from_utf8(shared[file].clone()).unwrap();
            shared.insert(file.into(), text.replacen(old, new, 1).into_bytes());
        }
        let record = |slot: &Value, id: &str, tree: Option<&BTreeMap<String, Vec<u8>>>| {
            let run = dir.path().join(id);
            std::fs::create_dir_all(run.join("controller")).unwrap();
            let mut result = json!({"fixture": crate::STEP_A_FIXTURE});
            if let Some(tree) = tree {
                let files: serde_json::Map<String, Value> = (tree.iter())
                    .map(|(k, v)| (k.clone(), String::from_utf8_lossy(v).into_owned().into()))
                    .collect();
                let mut repo = fixture.clone();
                repo["files"] = files.into();
                result["final_snapshot"] = initial(&run.join("controller/retained"), &repo)
                    .unwrap()
                    .into();
            }
            crate::write(run.join("controller/result.json"), &result).unwrap();
            let mut entry = slot.clone();
            entry["run_id"] = id.into();
            entry["run_dir"] = run.to_string_lossy().into_owned().into();
            entry
        };
        let slots = crate::batch::step_a_qualify_slots();
        let solo_trees = [&shared, &shared];
        let entries: Vec<Value> = slots
            .iter()
            .enumerate()
            .map(|(i, s)| record(s, s["slot"].as_str().unwrap(), solo_trees.get(i).copied()))
            .collect();
        crate::write(dir.path().join("batch.json"), &json!({"runs": entries})).unwrap();
        let pairs = |classes: &[(usize, &str)]| {
            let pairs: Vec<Value> = classes
                .iter()
                .map(|(run, class)| json!({"run_id": slots[*run]["slot"], "class": class}))
                .collect();
            crate::write(dir.path().join("duplicates.json"), &json!({"pairs": pairs})).unwrap();
        };
        let passed = json!({"passed": true});
        // Medians of [1, 2, 0] and [1, 0, 0]: 1 qualifies, 0 does not.
        pairs(&[
            (2, "duplicate"),
            (3, "divergent"),
            (3, "duplicate"),
            (3, "no"),
            (4, "no"),
        ]);
        let decided = qualify(dir.path(), passed.clone()).unwrap();
        assert_eq!(decided["decision"], json!({"qualifies": true}), "{decided}");
        assert_eq!(
            decided["steps"][3]["detail"]["pairs_per_run"],
            json!([1, 2, 0])
        );
        pairs(&[(2, "divergent"), (3, "no")]);
        assert_eq!(
            qualify(dir.path(), passed.clone()).unwrap()["decision"],
            json!({"qualifies": false, "failed_step": "gap"})
        );
        // An unclassified pair leaves the decision open; a failed static check decides it.
        pairs(&[(2, "duplicate"), (3, "duplicate"), (4, "unsure")]);
        assert_eq!(
            qualify(dir.path(), passed.clone()).unwrap()["decision"]["qualifies"],
            Value::Null
        );
        assert_eq!(
            qualify(dir.path(), json!({"passed": false})).unwrap()["decision"]["failed_step"],
            "static checks"
        );
        // One git-1 tree without a shared helper fails emergence.
        let without = record(&slots[1], "replacement", Some(&reference));
        let mut ledger = read(dir.path().join("batch.json")).unwrap();
        ledger["runs"].as_array_mut().unwrap().push(without);
        crate::write(dir.path().join("batch.json"), &ledger).unwrap();
        assert_eq!(
            qualify(dir.path(), passed).unwrap()["decision"],
            json!({"qualifies": false, "failed_step": "emergence"})
        );
    }

    #[test]
    fn medians_of_pair_counts() {
        assert_eq!(median(vec![0, 2, 1]), Some(1.0));
        assert_eq!(median(vec![]), None);
        assert_eq!(median(vec![0, 1]), Some(0.5));
    }
}
