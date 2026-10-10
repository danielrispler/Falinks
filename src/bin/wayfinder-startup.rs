//! Read the live Falinks map and ordered ticket metadata; never mutate the tracker.
//!
//! Usage: `cargo run -q --bin wayfinder-startup -- [map-number]` (default: 1).
use serde_json::{Value, json};
use std::process::Command;

const REPO: &str = "danielrispler/Falinks";

/// Open children in map order. Unknown blocker counts are errors, never eligible.
fn open_tickets(pages: &Value) -> Result<Vec<Value>, String> {
    let mut rows = Vec::new();
    for ticket in pages.as_array().into_iter().flatten().flat_map(|page| page.as_array().into_iter().flatten()) {
        if ticket["state"] != "open" {
            continue;
        }
        let blockers = ticket["issue_dependencies_summary"]["blocked_by"]
            .as_u64()
            .ok_or_else(|| format!("Unknown blockers: {}", ticket["title"].as_str().unwrap_or_default()))?;
        let assignees: Vec<&Value> = ticket["assignees"].as_array().into_iter().flatten().map(|p| &p["login"]).collect();
        rows.push(json!({
            "number": ticket["number"], "title": ticket["title"], "url": ticket["html_url"],
            "assignees": assignees, "blocked_by": blockers, "frontier": blockers == 0 && assignees.is_empty(),
        }));
    }
    Ok(rows)
}

fn gh(args: &[&str]) -> Result<Value, String> {
    let output = Command::new("gh").args(args).output().map_err(|e| format!("gh: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}

fn run() -> Result<(), String> {
    let map = match std::env::args().nth(1) {
        None => 1,
        Some(arg) => arg.parse::<u64>().ok().filter(|n| *n >= 1).ok_or("map issue number must be positive")?,
    };
    let number = map.to_string();
    let issue = gh(&["issue", "view", &number, "--repo", REPO, "--json", "title,url,body,labels"])?;
    if !issue["labels"].as_array().into_iter().flatten().any(|label| label["name"] == "wayfinder:map") {
        return Err("The selected issue is not a Wayfinder map".into());
    }
    let tickets = open_tickets(&gh(&["api", "--paginate", "--slurp", &format!("repos/{REPO}/issues/{map}/sub_issues")])?)?;
    let text = |key: &str| issue[key].as_str().unwrap_or_default().to_string();
    println!("# {}\n{}\n\n{}", text("title"), text("url"), text("body"));
    println!("\n## Open tickets (map order)\n");
    for ticket in tickets {
        println!("{ticket}");
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free() -> Value {
        json!({"number": 17, "title": "Free", "html_url": "https://example.com/17", "state": "open",
               "assignees": [], "issue_dependencies_summary": {"blocked_by": 0}})
    }

    fn with(mut ticket: Value, key: &str, value: Value) -> Value {
        ticket[key] = value;
        ticket
    }

    #[test]
    fn selects_frontier_in_map_order_across_pages() {
        let claimed = with(with(free(), "number", json!(11)), "assignees", json!([{"login": "dev"}]));
        let blocked = with(with(free(), "number", json!(12)), "issue_dependencies_summary", json!({"blocked_by": 2}));
        let later = with(free(), "number", json!(18));
        let rows = open_tickets(&json!([[{"state": "closed"}, claimed, free()], [blocked, later]])).unwrap();
        let numbers = |frontier_only: bool| -> Vec<u64> {
            rows.iter().filter(|r| !frontier_only || r["frontier"] == true).map(|r| r["number"].as_u64().unwrap()).collect()
        };
        assert_eq!(numbers(false), [11, 17, 12, 18]);
        assert_eq!(numbers(true), [17, 18]);
        assert_eq!(rows[0]["assignees"], json!(["dev"]));
        assert!(open_tickets(&json!([[]])).unwrap().is_empty());
    }

    #[test]
    fn unknown_blockers_never_make_a_ticket_eligible() {
        for unknown in [json!({}), json!({"blocked_by": null}), json!({"blocked_by": -1}), json!({"blocked_by": true})] {
            let ticket = with(free(), "issue_dependencies_summary", unknown);
            assert!(open_tickets(&json!([[ticket]])).is_err());
        }
    }
}
