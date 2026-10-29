//! The worktree fingerprint (#328, module 5): the files `git status --porcelain` lists in a
//! worktree, with their content. A step changed the worktree when a file listed before or after it
//! has other content, or now exists or is gone. Ignored files are never listed, and a commit alone
//! leaves every file's content as it was.

use similar::TextDiff;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// The files `git status` listed, by path from the worktree's root, with their content: `None` for
/// a file that is gone.
type Record = BTreeMap<String, Option<Vec<u8>>>;

/// Each worktree's latest record, by the worktree's root.
#[derive(Default)]
pub struct Fingerprints {
    records: HashMap<PathBuf, Record>,
}

impl Fingerprints {
    /// Records the worktree that holds `folder` and returns the diff of the files that changed since
    /// its previous record. `None` when nothing changed, when this is the worktree's first record, or
    /// when `folder` is in no git worktree.
    pub fn take(&mut self, folder: &Path) -> Option<String> {
        let root = git(folder, &["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(String::from_utf8(root).ok()?.trim_end());
        // Untracked files one by one rather than their folders; a rename as a deletion and an addition.
        let status = [
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ];
        let record: Record = git(&root, &status)?
            .split(|&byte| byte == 0)
            // Each entry is `XY path`, with the path from the root.
            .filter_map(|entry| entry.get(3..))
            .map(|path| {
                let path = String::from_utf8_lossy(path).into_owned();
                let content = fs::read(root.join(&path)).ok();
                (path, content)
            })
            .collect();
        let previous = self.records.insert(root.clone(), record)?;
        let record = &self.records[&root];
        let paths: BTreeSet<&String> = previous.keys().chain(record.keys()).collect();
        let diff: String = paths
            .into_iter()
            .filter_map(|path| {
                // A file one record does not list was clean then: committed before, on disk now.
                let before = previous
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| git(&root, &["show", &format!("HEAD:{path}")]));
                let after = record.get(path).cloned();
                let after = after.unwrap_or_else(|| fs::read(root.join(path)).ok());
                (before != after).then(|| unified(path, before, after))
            })
            .collect();
        (!diff.is_empty()).then_some(diff)
    }
}

/// `git`'s output in `folder`, or `None` when it fails. It takes no optional locks, so it never
/// holds up the agent's own `git` commands in the same worktree.
fn git(folder: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(folder)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// One file's change as a unified diff, as `git diff` shows it.
fn unified(path: &str, before: Option<Vec<u8>>, after: Option<Vec<u8>>) -> String {
    let (before, after) = (before.unwrap_or_default(), after.unwrap_or_default());
    let (Ok(before), Ok(after)) = (str::from_utf8(&before), str::from_utf8(&after)) else {
        return format!("Binary file {path} changed\n");
    };
    TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}
