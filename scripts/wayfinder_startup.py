#!/usr/bin/env python3
"""Read the live Falinks map and ordered ticket metadata; never mutate the tracker."""

import argparse
import json
import subprocess
import sys


REPO = "danielrispler/Falinks"


def open_tickets(pages):
    rows = []
    for page in pages:
        for ticket in page:
            if ticket["state"] != "open":
                continue
            blockers = ticket.get("issue_dependencies_summary", {}).get("blocked_by")
            if type(blockers) is not int or blockers < 0:
                raise ValueError(f"Unknown blockers: {ticket['title']}")
            assignees = [person["login"] for person in ticket["assignees"]]
            rows.append({"number": ticket["number"], "title": ticket["title"],
                         "url": ticket["html_url"], "assignees": assignees,
                         "blocked_by": blockers, "frontier": blockers == 0 and not assignees})
    return rows


def check():
    free = {"number": 17, "title": "Free", "html_url": "https://example.com/17",
            "state": "open", "assignees": [], "issue_dependencies_summary": {"blocked_by": 0}}
    claimed = {**free, "number": 11, "assignees": [{"login": "dev"}]}
    blocked = {**free, "number": 12, "issue_dependencies_summary": {"blocked_by": 2}}
    later = {**free, "number": 18}
    rows = open_tickets([[{"state": "closed"}, claimed, free], [blocked, later]])
    assert [row["number"] for row in rows] == [11, 17, 12, 18]
    assert [row["number"] for row in rows if row["frontier"]] == [17, 18]
    assert rows[0]["assignees"] == ["dev"]
    assert open_tickets([[]]) == []
    for unknown in ({}, {"blocked_by": None}, {"blocked_by": -1}, {"blocked_by": True}):
        try:
            open_tickets([[{**free, "issue_dependencies_summary": unknown}]])
        except ValueError:
            continue
        raise AssertionError("Unknown blockers must not make a ticket eligible")
    print("Startup checks passed")


def gh(*args):
    return json.loads(subprocess.check_output(["gh", *args], text=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("map", type=int, nargs="?", default=1, help="map issue number (default: 1)")
    parser.add_argument("--check", action="store_true", help="run offline selection checks")
    args = parser.parse_args()
    if args.check:
        check()
        return
    if args.map < 1:
        parser.error("map issue number must be positive")
    issue = gh("issue", "view", str(args.map), "--repo", REPO,
               "--json", "title,url,body,labels")
    if not any(label["name"] == "wayfinder:map" for label in issue["labels"]):
        raise ValueError("The selected issue is not a Wayfinder map")
    tickets = open_tickets(gh("api", "--paginate", "--slurp",
                             f"repos/{REPO}/issues/{args.map}/sub_issues"))
    print(f"# {issue['title']}\n{issue['url']}\n\n{issue['body']}")
    print("\n## Open tickets (map order)\n")
    for ticket in tickets:
        print(json.dumps(ticket, ensure_ascii=False, separators=(",", ":")))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
