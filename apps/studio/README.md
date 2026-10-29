# Studio

The control room's app (map #266, spec #328), for macOS. This version's pane follows the Claude Code session you prompted last: its last step, live, and a card on the first Edit or Write of each wait. A card shows its question; tap it for the answer. A fake card writer writes the cards for now, and they are kept only until the app quits.

The pane also shows the session's state: working, your turn, or needs you, the only one in colour. When a session newly needs you and the window is not focused, macOS shows a notification. macOS asks once for permission to show them.

- `plugin/`: the Claude Code plugin. On ten hook events it runs `studio-hook`.
- `hook/`: `studio-hook`, which appends each hook payload as one line to the event log, and the log's path.
- `core/`: the studio core, plain Rust with no I/O. The app replays the whole log through it at start, then feeds it each new line live. Only live lines request cards and notifications.
- `understanding/`: the understanding layer's I/O: the card writer interface and its fake.
- `src-tauri/`: the app's Rust shell. `ui/`: its TypeScript pane.

## Build

Needs Node and the Rust toolchain.

    cd apps/studio
    npm ci
    npm run tauri build

This builds `target/release/bundle/macos/Studio.app` with an ad-hoc signature (`codesign -dv` shows `Signature=adhoc`), and `target/release/studio-hook`, which the plugin runs. Open the app with `open target/release/bundle/macos/Studio.app`.

`cargo test` runs the core's replay tests. It leaves out the shell, which needs `dist/` from `npm run build`.

## Load the plugin

Add the plugin folder to `CLAUDE_CODE_PLUGIN_DIRS` in your shell profile:

    export CLAUDE_CODE_PLUGIN_DIRS=/Users/danielrispler/work/Falinks/apps/studio/plugin

Each `claude` session then loads the plugin in place, as `falinks-studio@inline` in `claude plugin list`. Nothing is written to `~/.claude/settings.json`. `claude plugin install` would write `enabledPlugins` and `extraKnownMarketplaces` there.

The hooks run the `studio-hook` built in this checkout, so build before you start a session. Edits to the plugin take effect at the next session start or `/reload-plugins`.

## The event log

`~/Library/Application Support/dev.falinks.studio/events.jsonl`, one hook payload per line, with two added fields:

- `studio_terminal_id`: the session's `STUDIO_TERMINAL_ID`, when set;
- `studio_received_ms`: when the hook received the event, in Unix milliseconds.

The hooks are async, so Claude Code never waits for them, and they append whether the app runs or not. Two events that fire within a few milliseconds can land in either order.

The log is never rotated. It holds tool inputs and results, file contents included.
