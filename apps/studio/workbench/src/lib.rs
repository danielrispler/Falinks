//! Workbench git (#328, module 7; #461, decision 8): the places a session can start in, read live
//! from `git worktree list --porcelain` with no stored copy, and the `claude` command a studio
//! terminal runs.
// #468 adds creating and removing tasks here, beside `places`.

use serde::Serialize;
use std::{path::Path, process::Command};

/// A main checkout or a task's worktree: its folder, and its branch, `None` at a detached `HEAD`.
#[derive(Debug, PartialEq, Serialize)]
pub struct Place {
    pub path: String,
    pub branch: Option<String>,
}

/// The repository's main checkout, its tasks, the worktrees under `.claude/worktrees/` in `git`'s
/// order, and how many `agent-*` worktrees there were hidden.
#[derive(Debug, PartialEq, Serialize)]
pub struct Places {
    pub main: Place,
    pub tasks: Vec<Place>,
    pub hidden: usize,
}

/// The places of the repository that holds `repo`, which may be any of its worktrees.
pub fn places(repo: &Path) -> Result<Places, String> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git worktree list: {e}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree list: {}", error.trim()));
    }
    // Each worktree is a block of lines; the main checkout's comes first.
    let text = String::from_utf8_lossy(&output.stdout);
    let mut listed = text.split("\n\n").filter_map(|block| {
        let mut place = Place {
            path: String::new(),
            branch: None,
        };
        for line in block.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                place.path = path.into();
            } else if let Some(branch) = line.strip_prefix("branch ") {
                place.branch = Some(branch.trim_start_matches("refs/heads/").into());
            } else if line == "bare" || line.starts_with("prunable") {
                // A bare repository has no files, and a prunable worktree's folder is gone.
                return None;
            }
        }
        (!place.path.is_empty()).then_some(place)
    });
    let main = listed.next().ok_or("git listed no main checkout")?;
    let under = format!("{}/.claude/worktrees/", main.path);
    let (agents, tasks): (Vec<Place>, Vec<Place>) = listed
        .filter(|place| place.path.starts_with(&under))
        .partition(|place| place.path[under.len()..].starts_with("agent-"));
    Ok(Places {
        main,
        tasks,
        hidden: agents.len(),
    })
}

/// The command a studio terminal runs for a new session, or for `resume`, an ended session's id. It
/// sets `CLAUDE_CODE_PLUGIN_DIRS` to the studio plugin at `plugin` alone, as the README loads it,
/// so a profile that sets it too never loads the hooks twice. The daemon runs it under a login
/// shell with the terminal's `STUDIO_TERMINAL_ID`.
pub fn claude(plugin: &Path, resume: Option<&str>) -> Vec<String> {
    let mut argv = vec![
        "env".into(),
        format!("CLAUDE_CODE_PLUGIN_DIRS={}", plugin.display()),
        "claude".into(),
    ];
    if let Some(id) = resume {
        argv.extend(["--resume".into(), id.into()]);
    }
    argv
}
