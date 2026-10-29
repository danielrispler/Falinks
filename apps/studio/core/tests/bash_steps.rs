//! Seam 1 (#328, Testing Decisions): Bash steps as card steps. The shell fingerprints the worktree
//! where the core asks it to and passes in what changed; these tests play the shell's part.

use studio_core::{CardRequest, Output, Step, Studio};

const LOG: &str = include_str!("fixtures/bash-steps.jsonl");
const REVERT: &str = "toolu_01Rv3Gt5Hy7Ju9Ki1Lo3Pa5S";
const FORMAT: &str = "toolu_01Qw2Er4Ty6Ui8Op0As2Df4G";
const NOTE: &str = "toolu_01Zx3Cv5Bn7Mq9Wa1Se3Rd5F";

const FORMAT_DIFF: &str = concat!(
    "--- a/apps/studio/core/src/lib.rs\n",
    "+++ b/apps/studio/core/src/lib.rs\n",
    "@@ -90,3 +90,3 @@\n",
    "     let plan_file = input[\"file_path\"]\n",
    "-        .as_str().is_some_and(|path| path.contains(\"/.claude/plans/\"));\n",
    "+        .as_str()\n",
);

/// The card requests after an app start that replays the first `replayed` lines of the log and
/// gets the rest live. The shell's fingerprint finds a change only after the steps in `changed`,
/// by tool use id, with their diffs.
fn restart_after(replayed: usize, changed: &[(&str, &str)]) -> Vec<CardRequest> {
    let lines: Vec<&str> = LOG.lines().collect();
    let mut studio = Studio::default();
    for line in &lines[..replayed] {
        studio.replay(line);
    }
    lines[replayed..]
        .iter()
        .flat_map(|line| {
            let fingerprinted = studio_core::fingerprint_folder(line).is_some();
            let change = changed
                .iter()
                .find(|(id, _)| fingerprinted && line.contains(id))
                .map(|(_, diff)| diff.to_string());
            studio.apply_with_change(line, change)
        })
        .map(|Output::Card(request)| request)
        .collect()
}

fn live(changed: &[(&str, &str)]) -> Vec<CardRequest> {
    restart_after(0, changed)
}

#[test]
fn the_waits_first_bash_step_that_changed_the_worktree_gets_its_card() {
    let note = "--- a/apps/studio/NOTES.md\n+++ b/apps/studio/NOTES.md\n@@ -0,0 +1 @@\n+live\n";

    let cards = live(&[(FORMAT, FORMAT_DIFF), (NOTE, note)]);

    assert_eq!(
        cards,
        [CardRequest {
            session_id: "e4a7c2f9-3b6d-4e1a-8c5f-2d9b7e0a6c13".into(),
            tool_use_id: FORMAT.into(),
            step: Step {
                tool: "Bash".into(),
                detail: Some("cargo fmt --manifest-path apps/studio/Cargo.toml".into()),
            },
            change: FORMAT_DIFF.into(),
        }]
    );
}

#[test]
fn bash_steps_that_changed_nothing_get_no_card() {
    assert_eq!(live(&[]), []);
}

#[test]
fn a_git_step_gets_no_card_even_when_it_changed_the_worktree() {
    let revert = "--- a/apps/studio/core/src/lib.rs\n+++ b/apps/studio/core/src/lib.rs\n";

    assert_eq!(live(&[(REVERT, revert)]), []);
}

#[test]
fn a_replayed_bash_step_requests_no_card() {
    let carded = |replayed| restart_after(replayed, &[(FORMAT, FORMAT_DIFF)]);

    assert_eq!(carded(LOG.lines().count()), []);
    assert_eq!(carded(7), []);
    let cards = carded(5);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].tool_use_id, FORMAT);
}

#[test]
fn the_shell_fingerprints_at_each_prompt_and_each_steps_end() {
    let fingerprinted = |log: &str| -> Vec<usize> {
        let lines = log.lines().enumerate();
        let at = lines.filter(|(_, line)| studio_core::fingerprint_folder(line).is_some());
        at.map(|(index, _)| index).collect()
    };

    assert_eq!(fingerprinted(LOG), [0, 2, 4, 6, 8]);
    assert_eq!(
        fingerprinted(include_str!("fixtures/failing-bash.jsonl")),
        [1, 3]
    );
    assert_eq!(
        studio_core::fingerprint_folder(LOG.lines().next().unwrap()).as_deref(),
        Some("/Users/danielrispler/work/Falinks/.claude/worktrees/issue-344-bash")
    );
}
