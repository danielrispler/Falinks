//! Read the live map and ordered ticket metadata; never mutate the tracker.
use falinks_host::{Result, command_output, require};
use serde_json::{Value, json};
use std::{
    env,
    process::{self, Command},
    time::Duration,
};
const REPO: &str = "danielrispler/Falinks";

pub fn open_tickets(pages: &Value) -> Result<Vec<Value>> {
    let mut rows = vec![];
    for page in pages
        .as_array()
        .ok_or("expected paginated ticket metadata")?
    {
        for ticket in page.as_array().ok_or("expected a ticket page")? {
            if ticket["state"] != "open" {
                continue;
            }
            let title = ticket["title"].as_str().ok_or("missing ticket title")?;
            let blockers = ticket["issue_dependencies_summary"]["blocked_by"]
                .as_u64()
                .ok_or_else(|| format!("Unknown blockers: {title}"))?;
            let assignees = ticket["assignees"]
                .as_array()
                .ok_or("missing ticket assignees")?
                .iter()
                .map(|person| {
                    person["login"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("missing assignee login")
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows.push(json!({"number":ticket["number"],"title":title,"url":ticket["html_url"],"assignees":assignees,"blocked_by":blockers,"frontier":blockers==0 && assignees.is_empty()}));
        }
    }
    Ok(rows)
}
fn list_maps(maps: &[&Value]) -> String {
    if maps.is_empty() {
        return " none".into();
    }
    maps.iter()
        .map(|map| {
            let closed = if map["state"] == "OPEN" {
                ""
            } else {
                " (closed)"
            };
            format!(
                "\n  #{} {}{closed}",
                map["number"],
                map["title"].as_str().unwrap_or("")
            )
        })
        .collect()
}
/// Pick the requested issue if open, else the newest open `wayfinder:map`; never a closed map.
pub fn select_map(requested: Option<&Value>, maps: &Value) -> Result<u64> {
    let maps = maps.as_array().ok_or("expected a map list")?;
    let mut open = maps
        .iter()
        .filter(|map| map["state"] == "OPEN")
        .collect::<Vec<_>>();
    open.sort_by_key(|map| std::cmp::Reverse(map["number"].as_u64()));
    if let Some(issue) = requested {
        let number = issue["number"].as_u64().ok_or("missing map number")?;
        if issue["state"] == "OPEN" {
            return Ok(number);
        }
        return Err(format!("Map #{number} is closed. Open maps:{}", list_maps(&open)).into());
    }
    match open.first() {
        Some(map) => map["number"]
            .as_u64()
            .ok_or_else(|| "missing map number".into()),
        None => Err(format!(
            "No open wayfinder:map issue; pass a map number. Recent maps:{}",
            list_maps(&maps.iter().collect::<Vec<_>>())
        )
        .into()),
    }
}
/// Open blocker numbers from paginated `dependencies/blocked_by` pages.
pub fn open_blockers(pages: &Value) -> Result<Vec<u64>> {
    let mut numbers = vec![];
    for page in pages.as_array().ok_or("expected paginated blockers")? {
        for blocker in page.as_array().ok_or("expected a blocker page")? {
            let number = blocker["number"].as_u64().ok_or("missing blocker number")?;
            if blocker["state"] == "open" {
                numbers.push(number);
            }
        }
    }
    Ok(numbers)
}
fn check() -> Result<()> {
    let free = json!({"number":17,"title":"Free","html_url":"https://example.com/17","state":"open","assignees":[],"issue_dependencies_summary":{"blocked_by":0}});
    let mut claimed = free.clone();
    claimed["number"] = json!(11);
    claimed["assignees"] = json!([{"login":"dev"}]);
    let mut blocked = free.clone();
    blocked["number"] = json!(12);
    blocked["issue_dependencies_summary"]["blocked_by"] = json!(2);
    let mut later = free.clone();
    later["number"] = json!(18);
    let rows = open_tickets(&json!([[{"state":"closed"},claimed,free],[blocked,later]]))?;
    assert_eq!(
        rows.iter()
            .map(|r| r["number"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [11, 17, 12, 18]
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r["frontier"] == true)
            .map(|r| r["number"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [17, 18]
    );
    assert_eq!(rows[0]["assignees"], json!(["dev"]));
    assert!(open_tickets(&json!([[]]))?.is_empty());
    for unknown in [
        json!({}),
        json!({"blocked_by":null}),
        json!({"blocked_by":-1}),
        json!({"blocked_by":true}),
    ] {
        let mut ticket = free.clone();
        ticket["issue_dependencies_summary"] = unknown;
        assert!(open_tickets(&json!([[ticket]])).is_err());
    }
    Ok(())
}
fn gh(args: &[&str]) -> Result<Value> {
    let mut command = Command::new("gh");
    command.args(args);
    let output = command_output(command, Duration::from_secs(60))?;
    require(
        output.status.success(),
        &String::from_utf8_lossy(&output.stderr),
    )?;
    Ok(serde_json::from_slice(&output.stdout)?)
}
fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args == ["--check"] {
        check()?;
        println!("Startup checks passed");
        return Ok(());
    }
    if args == ["--help"] {
        println!(
            "Usage: wayfinder-startup [map-number] | --check\nDefault map: newest open issue labelled wayfinder:map. This command only reads GitHub."
        );
        return Ok(());
    }
    require(
        args.len() <= 1,
        "usage: wayfinder-startup [map-number] | --check",
    )?;
    let requested = args
        .first()
        .map(|number| number.parse::<u64>())
        .transpose()?;
    require(requested != Some(0), "map issue number must be positive")?;
    let maps = gh(&[
        "issue",
        "list",
        "--repo",
        REPO,
        "--label",
        "wayfinder:map",
        "--state",
        "all",
        "--limit",
        "20",
        "--json",
        "number,title,state",
    ])?;
    let view = |number: u64| {
        gh(&[
            "issue",
            "view",
            &number.to_string(),
            "--repo",
            REPO,
            "--json",
            "number,state,title,url,body",
        ])
    };
    let (map, issue) = match requested {
        Some(number) => {
            let issue = view(number).map_err(|error| {
                let open = select_map(None, &maps)
                    .map(|newest| format!("; newest open map is #{newest}"))
                    .unwrap_or_default();
                format!("Map #{number} not found: {error}{open}")
            })?;
            (select_map(Some(&issue), &maps)?, issue)
        }
        None => {
            let map = select_map(None, &maps)?;
            (map, view(map)?)
        }
    };
    let mut tickets = open_tickets(&gh(&[
        "api",
        "--paginate",
        "--slurp",
        &format!("repos/{REPO}/issues/{map}/sub_issues"),
    ])?)?;
    println!(
        "# {}\n{}\n\n{}\n\n## Open tickets (map order)\n",
        issue["title"].as_str().unwrap_or(""),
        issue["url"].as_str().unwrap_or(""),
        issue["body"].as_str().unwrap_or("")
    );
    for ticket in &mut tickets {
        let blockers = if ticket["blocked_by"] == 0 {
            vec![]
        } else {
            open_blockers(&gh(&[
                "api",
                "--paginate",
                "--slurp",
                &format!(
                    "repos/{REPO}/issues/{}/dependencies/blocked_by",
                    ticket["number"]
                ),
            ])?)?
        };
        ticket["blocked_by"] = json!(blockers);
        println!("{ticket}");
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn startup_preserves_order_and_rejects_unknown_blockers() {
        super::check().unwrap();
    }
}
#[cfg(test)]
mod map_tests {
    use super::*;
    fn maps() -> Value {
        json!([
            {"number":1,"state":"CLOSED","title":"Map: old"},
            {"number":30,"state":"OPEN","title":"Map: newer"},
            {"number":20,"state":"OPEN","title":"Map: older"}
        ])
    }
    #[test]
    fn default_map_is_newest_open_map() {
        assert_eq!(select_map(None, &maps()).unwrap(), 30);
    }
    #[test]
    fn no_open_map_is_an_error_listing_recent_maps() {
        let only_closed = json!([{"number":1,"state":"CLOSED","title":"Map: old"}]);
        let error = select_map(None, &only_closed).unwrap_err().to_string();
        assert!(error.contains("No open wayfinder:map"), "{error}");
        assert!(error.contains("#1 Map: old (closed)"), "{error}");
    }
    #[test]
    fn explicit_closed_map_is_an_error_naming_open_maps() {
        let closed = json!({"number":1,"state":"CLOSED","title":"Map: old"});
        let error = select_map(Some(&closed), &maps()).unwrap_err().to_string();
        assert!(error.contains("#1 is closed"), "{error}");
        assert!(error.contains("#30 Map: newer"), "{error}");
        assert!(error.contains("#20 Map: older"), "{error}");
        assert!(!error.contains("Map: old ("), "{error}");
    }
    #[test]
    fn explicit_open_issue_is_used_without_map_label() {
        let spec = json!({"number":19,"state":"OPEN","title":"Spec: x"});
        assert_eq!(select_map(Some(&spec), &maps()).unwrap(), 19);
    }
    #[test]
    fn blockers_render_open_numbers_only() {
        let pages = json!([
            [{"number":46,"state":"open"},{"number":40,"state":"closed"}],
            [{"number":48,"state":"open"}]
        ]);
        assert_eq!(open_blockers(&pages).unwrap(), [46, 48]);
        assert!(open_blockers(&json!([[{"state":"open"}]])).is_err());
    }
}
