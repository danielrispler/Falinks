# v0 guess-and-card loop (#315), throwaway

Run the panel:

    cd apps/studio/v0-spike/panel && cargo run                      # fake card
    cd apps/studio/v0-spike/panel && CARD_WRITER=claude cargo run   # real `claude -p` card writer

Run Claude Code with the hooks, from any project:

    claude --settings /Users/danielrispler/work/Falinks/.claude/worktrees/issue-315-studio-prototype-the-guess/apps/studio/v0-spike/hook/settings.json

`V0_TEST_GUESS=<text>` submits a guess automatically, for tests with nobody at the keyboard.
