//! The event log (#328, module 2): one append-only JSON Lines file, the only source of session state.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command},
};

/// The log in the studio's data directory, which is Tauri's app data directory for the identifier
/// in `src-tauri/tauri.conf.json`. The hook writes it; the app reads it.
pub fn event_log(home: &Path) -> PathBuf {
    home.join("Library/Application Support/dev.falinks.studio/events.jsonl")
}

/// The root of the git worktree that holds `folder`, or its nearest existing parent, and the git
/// tree of its files as they are now, untracked ones included and ignored ones not (#486). `None`
/// outside a worktree or when `git` fails. It writes the tree through a throwaway index, so the
/// working tree, the real index and `git status` stay as they were.
pub fn record_tree(folder: &Path) -> Option<(String, String)> {
    let existing = folder.ancestors().find(|p| p.is_dir())?;
    let root = git(existing, &["rev-parse", "--show-toplevel"], None)?;
    let index = git(
        Path::new(&root),
        &["rev-parse", "--git-path", "index"],
        None,
    )?;
    let scratch = env::temp_dir().join(format!("studio-hook-{}.index", process::id()));
    // A copy of the real index keeps git's stat cache, so only changed files are hashed again.
    let _ = fs::copy(Path::new(&root).join(index), &scratch);
    let added = git(Path::new(&root), &["add", "-A"], Some(&scratch));
    let tree = added.and_then(|_| git(Path::new(&root), &["write-tree"], Some(&scratch)));
    let _ = fs::remove_file(&scratch);
    Some((root, tree?))
}

/// `git`'s trimmed output in `folder`, with `index` as its index when given, or `None` when it
/// fails. It takes no optional locks, so it never holds up the agent's own `git` commands.
fn git(folder: &Path, args: &[&str], index: Option<&Path>) -> Option<String> {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(folder);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let output = command.output().ok()?;
    let out = String::from_utf8(output.stdout).ok()?;
    output.status.success().then(|| out.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn the_log_lives_in_the_data_directory_of_the_apps_identifier() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../src-tauri/tauri.conf.json")).unwrap();
        let identifier = conf["identifier"].as_str().unwrap();
        let data = Path::new("/h/Library/Application Support").join(identifier);
        assert_eq!(super::event_log(Path::new("/h")), data.join("events.jsonl"));
    }
}
