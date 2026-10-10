//! Read the live map and ordered ticket metadata. Only `--claim` writes: it assigns a ticket and sets up its worktree.
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
#[derive(Debug, PartialEq)]
pub enum Workspace {
    /// A worktree already has the ticket branch checked out; use it as is.
    Reuse,
    /// The ticket branch exists without a worktree; check it out.
    CheckoutBranch,
    /// Neither exists; branch from up-to-date `origin/main`.
    Create,
}
#[derive(Debug, PartialEq)]
pub struct ClaimPlan {
    pub assign: bool,
    pub branch: String,
    pub path: String,
    pub workspace: Workspace,
}
/// Decide whether `--claim` may take `issue` and how to set up its worktree.
/// `worktrees` is `git worktree list --porcelain`; `branches` lists local branch names one per line.
pub fn claim_plan(
    issue: &Value,
    open_blockers: &[u64],
    caller: &str,
    root: &str,
    worktrees: &str,
    branches: &str,
) -> Result<ClaimPlan> {
    let number = issue["number"].as_u64().ok_or("missing issue number")?;
    let title = issue["title"].as_str().ok_or("missing issue title")?;
    let slug = title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    require(issue["state"] == "OPEN", &format!("#{number} is closed"))?;
    let assignees = issue["assignees"]
        .as_array()
        .ok_or("missing issue assignees")?
        .iter()
        .map(|person| person["login"].as_str().ok_or("missing assignee login"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let others = assignees
        .iter()
        .filter(|login| **login != caller)
        .copied()
        .collect::<Vec<_>>();
    require(
        others.is_empty(),
        &format!("#{number} is assigned to {}", others.join(", ")),
    )?;
    let blockers = open_blockers
        .iter()
        .map(|n| format!("#{n}"))
        .collect::<Vec<_>>();
    require(
        blockers.is_empty(),
        &format!("#{number} is blocked by {}", blockers.join(", ")),
    )?;
    let prefix = format!("issue-{number}-");
    let ours = |name: &str| name.starts_with(&prefix);
    let assign = assignees.is_empty();
    let mut path = "";
    for line in worktrees.lines() {
        if let Some(worktree) = line.strip_prefix("worktree ") {
            path = worktree;
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/")
            && ours(branch)
        {
            return Ok(ClaimPlan {
                assign,
                branch: branch.into(),
                path: path.into(),
                workspace: Workspace::Reuse,
            });
        }
    }
    let (branch, workspace) = match branches.lines().map(str::trim).find(|name| ours(name)) {
        Some(existing) => (existing.to_owned(), Workspace::CheckoutBranch),
        None => (format!("{prefix}{slug}"), Workspace::Create),
    };
    Ok(ClaimPlan {
        assign,
        path: format!("{root}/.claude/worktrees/{branch}"),
        branch,
        workspace,
    })
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
fn stdout(program: &str, args: &[&str]) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args);
    let output = command_output(command, Duration::from_secs(120))?;
    require(
        output.status.success(),
        &format!(
            "{program} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    Ok(String::from_utf8(output.stdout)?)
}
fn gh(args: &[&str]) -> Result<Value> {
    Ok(serde_json::from_str(&stdout("gh", args)?)?)
}
fn blockers_of(number: impl std::fmt::Display) -> Result<Vec<u64>> {
    open_blockers(&gh(&[
        "api",
        "--paginate",
        "--slurp",
        &format!("repos/{REPO}/issues/{number}/dependencies/blocked_by"),
    ])?)
}
/// The only write path: assign the ticket, then create or reuse its worktree.
fn claim(number: u64) -> Result<()> {
    let id = number.to_string();
    let issue = gh(&[
        "issue",
        "view",
        &id,
        "--repo",
        REPO,
        "--json",
        "number,state,title,assignees",
    ])?;
    let caller = gh(&["api", "user"])?["login"]
        .as_str()
        .ok_or("missing caller login")?
        .to_owned();
    let common = stdout(
        "git",
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let root = std::path::Path::new(common.trim())
        .parent()
        .ok_or("git common dir has no parent")?
        .to_str()
        .ok_or("non-UTF-8 repo path")?
        .to_owned();
    let plan = claim_plan(
        &issue,
        &blockers_of(number)?,
        &caller,
        &root,
        &stdout("git", &["worktree", "list", "--porcelain"])?,
        &stdout(
            "git",
            &[
                "branch",
                "--list",
                &format!("issue-{number}-*"),
                "--format=%(refname:short)",
            ],
        )?,
    )?;
    if plan.assign {
        stdout(
            "gh",
            &[
                "issue",
                "edit",
                &id,
                "--repo",
                REPO,
                "--add-assignee",
                "@me",
            ],
        )?;
    }
    match plan.workspace {
        Workspace::Reuse => {}
        Workspace::CheckoutBranch => {
            stdout("git", &["worktree", "add", &plan.path, &plan.branch])?;
        }
        Workspace::Create => {
            stdout("git", &["fetch", "origin", "main"])?;
            // --no-track: a plain `git push` must not target main.
            stdout(
                "git",
                &[
                    "worktree",
                    "add",
                    "--no-track",
                    "-b",
                    &plan.branch,
                    &plan.path,
                    "origin/main",
                ],
            )?;
        }
    }
    println!("{}", plan.path);
    Ok(())
}
const USAGE: &str = "usage: wayfinder-startup [map-number] | --claim <issue> | --check";
fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args == ["--check"] {
        check()?;
        println!("Startup checks passed");
        return Ok(());
    }
    if args == ["--help"] {
        println!(
            "{USAGE}\nDefault map: newest open issue labelled wayfinder:map. The map listing only reads GitHub.\n--claim <issue> is the only write path: it refuses closed, blocked or otherwise-assigned tickets, assigns the issue to you, creates or reuses its worktree under .claude/worktrees/ on branch issue-<n>-<slug> from origin/main, and prints the path."
        );
        return Ok(());
    }
    if let [flag, number] = args.as_slice()
        && flag == "--claim"
    {
        return claim(number.parse()?);
    }
    require(args.len() <= 1, USAGE)?;
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
            blockers_of(&ticket["number"])?
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
#[cfg(test)]
mod claim_tests {
    use super::*;
    const ROOT: &str = "/repo";
    const WORKTREES: &str = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n";
    fn ticket() -> Value {
        json!({"number":47,"state":"OPEN","title":"wayfinder-startup --claim: assign and set up worktree","assignees":[]})
    }
    #[test]
    fn eligible_ticket_is_assigned_and_gets_a_new_worktree_from_main() {
        let plan = claim_plan(&ticket(), &[], "dev", ROOT, WORKTREES, "").unwrap();
        assert_eq!(
            plan,
            ClaimPlan {
                assign: true,
                branch: "issue-47-wayfinder-startup-claim-assign".into(),
                path: "/repo/.claude/worktrees/issue-47-wayfinder-startup-claim-assign".into(),
                workspace: Workspace::Create,
            }
        );
    }
    fn refusal(issue: &Value, blockers: &[u64]) -> String {
        claim_plan(issue, blockers, "dev", ROOT, WORKTREES, "")
            .unwrap_err()
            .to_string()
    }
    #[test]
    fn ticket_assigned_to_another_user_is_refused() {
        let mut issue = ticket();
        issue["assignees"] = json!([{"login":"other"}]);
        let error = refusal(&issue, &[]);
        assert!(error.contains("#47 is assigned to other"), "{error}");
    }
    #[test]
    fn ticket_already_assigned_to_caller_is_not_reassigned() {
        let mut issue = ticket();
        issue["assignees"] = json!([{"login":"dev"}]);
        let plan = claim_plan(&issue, &[], "dev", ROOT, WORKTREES, "").unwrap();
        assert!(!plan.assign);
    }
    #[test]
    fn blocked_ticket_is_refused_naming_open_blockers() {
        let error = refusal(&ticket(), &[46, 48]);
        assert!(error.contains("#47 is blocked by #46, #48"), "{error}");
    }
    #[test]
    fn existing_worktree_for_the_ticket_is_reused_wherever_it_lives() {
        let worktrees = format!(
            "{WORKTREES}\nworktree /repo/.claude/worktrees/issue-470-other\nHEAD def\nbranch refs/heads/issue-470-other\n\nworktree /elsewhere/agent-1\nHEAD 123\nbranch refs/heads/issue-47-older-title\n"
        );
        let plan = claim_plan(&ticket(), &[], "dev", ROOT, &worktrees, "").unwrap();
        assert_eq!(plan.workspace, Workspace::Reuse);
        assert_eq!(plan.path, "/elsewhere/agent-1");
        assert_eq!(plan.branch, "issue-47-older-title");
    }
    #[test]
    fn existing_branch_without_worktree_is_checked_out() {
        let plan = claim_plan(
            &ticket(),
            &[],
            "dev",
            ROOT,
            WORKTREES,
            "main\nissue-470-other\nissue-47-older-title\n",
        )
        .unwrap();
        assert_eq!(plan.workspace, Workspace::CheckoutBranch);
        assert_eq!(plan.branch, "issue-47-older-title");
        assert_eq!(plan.path, "/repo/.claude/worktrees/issue-47-older-title");
    }
    #[test]
    fn closed_ticket_is_refused() {
        let mut issue = ticket();
        issue["state"] = json!("CLOSED");
        let error = refusal(&issue, &[]);
        assert!(error.contains("#47 is closed"), "{error}");
    }
}
