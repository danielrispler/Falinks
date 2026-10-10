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
            "Usage: wayfinder-startup [map-number] | --check\nDefault map: 1. This command only reads GitHub."
        );
        return Ok(());
    }
    require(
        args.len() <= 1,
        "usage: wayfinder-startup [map-number] | --check",
    )?;
    let map = args
        .first()
        .map(|number| number.parse::<u64>())
        .transpose()?
        .unwrap_or(1);
    require(map > 0, "map issue number must be positive")?;
    let map = map.to_string();
    let issue = gh(&[
        "issue",
        "view",
        &map,
        "--repo",
        REPO,
        "--json",
        "title,url,body,labels",
    ])?;
    require(
        issue["labels"]
            .as_array()
            .is_some_and(|labels| labels.iter().any(|label| label["name"] == "wayfinder:map")),
        "The selected issue is not a Wayfinder map",
    )?;
    let tickets = open_tickets(&gh(&[
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
    for ticket in tickets {
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
