# Studio research: form factor and stack (#283)

**Answer.** For v0, build a **menu-bar app with Tauri 2 and a TypeScript frontend**: an icon in the macOS menu bar, a small popover on click, and system notifications when an agent needs Daniel. It adds no window to the Dock or the window switcher. Next to it, keep a **Claude Code status line** script as a free per-session view. Daniel writes almost all v0 code in TypeScript; Rust stays a thin shell.<br>
**Evidence: moderate.** Capabilities come from official docs (Tauri, Claude Code, VS Code, Zed, Apple). Build speed and maintenance cost are my inference; nothing here was built or timed.<br>
**Hypotheses.** Supports "Too many windows": a menu-bar popover and notifications replace looking at windows. Weakens any plan that starts with a full desktop window or a web dashboard tab: both add a window.<br>
**For Daniel.** Tauri's [System Tray guide](https://v2.tauri.app/learn/system-tray/) and [Notification plugin](https://v2.tauri.app/plugin/notification/) (15 min). Claude Code's [status line page](https://code.claude.com/docs/en/statusline), "How status lines work" (10 min).

---

## How to read this report

- A claim with a link is **established** by that source.
- A sentence that starts with **Inference:** is my own reasoning.
- "Window" means anything that shows up in Cmd-Tab or needs arranging next to terminals, editor, GitHub and chats ([`problems.md`](../problems.md), "Too many windows").

## Form factors

Each one in a line, then the table.

- **Desktop app (full window).** Adds one window. Wrong direction for v0.
- **Menu-bar app.** One icon, a popover on demand, notifications. Adds no window. Tauri ([tray](https://v2.tauri.app/learn/system-tray/)) and SwiftUI ([`MenuBarExtra`](https://developer.apple.com/documentation/swiftui/menubarextra), macOS 13+) both support it.
- **Terminal UI.** Needs its own terminal pane or tab: another place to look. Cannot raise system notifications by itself. **Inference:** only worth it if Daniel lives in one tiled terminal.
- **Editor panel.** VS Code extensions can render a web view in the sidebar or panel ([Webview views](https://code.visualstudio.com/api/extension-guides/webview)). Zed extensions offer themes, languages, debuggers, snippets and MCP servers, not custom panels ([Zed extensions](https://zed.dev/docs/extensions)). Ties the app to one editor and only shows when that editor is in front.
- **Claude Code status line.** A script whose output shows under the prompt; it gets session JSON on stdin, re-runs on events with a 300 ms debounce, can print several lines and clickable links, and can also run on a `refreshInterval` timer ([status line](https://code.claude.com/docs/en/statusline)). Zero windows, zero UI code. Limit: it shows only inside the session it belongs to, so it cannot be the cross-session view.
- **Claude Code hooks.** Push events when Claude needs input ([hooks guide](https://code.claude.com/docs/en/hooks-guide#get-notified-when-claude-needs-input)). They are a **data source** for the menu-bar app, not a UI; see [data-sources.md](https://github.com/danielrispler/Falinks/blob/d8f26439dd49114a026cd330c5f95cb9ee085785/docs/studio/research/data-sources.md).
- **Local web dashboard.** A browser tab: one more window, easy to lose among tabs. Notifications need browser permission and an open tab. **Inference:** good later for rich views, not for v0.

| Form factor | New windows | Notifications | Live updates | Sees all sessions |
|---|---|---|---|---|
| Desktop app | 1 | yes | yes | yes |
| Menu-bar app | 0 (popover) | yes | yes | yes |
| Terminal UI | 1 pane | no (needs helper) | yes | yes |
| VS Code panel | 0, but VS Code only | VS Code toasts | yes | yes |
| Zed panel | not possible | no | no | no |
| Status line | 0 | no | per event, 300 ms | no, one session |
| Web dashboard | 1 tab | only with tab open | yes | yes |

## Stacks

- **Tauri 2 + TypeScript.** UI in HTML/TS; tray and menu are callable from JS (`TrayIcon.new`, `Menu.new` in `@tauri-apps/api`), and notifications from JS (`sendNotification` in `@tauri-apps/plugin-notification`) on macOS, Windows and Linux ([tray](https://v2.tauri.app/learn/system-tray/), [notification](https://v2.tauri.app/plugin/notification/)). Linux trays lack mouse events but keep the menu ([tray](https://v2.tauri.app/learn/system-tray/)). **Inference:** Daniel's Rust is limited to setup and a few commands (for example running `claude agents --json`, watching files), which a TS developer can copy from docs.
- **Pure Rust UI (egui, iced, Slint, gpui, Dioxus).** All UI in Rust: Daniel would learn Rust and a UI framework at once. None ships a tray; they pair with the separate `tray-icon` crate, which on macOS needs the event loop on the main thread ([tray-icon](https://github.com/tauri-apps/tray-icon)). gpui is built for Zed itself. Dioxus desktop renders through a web view, like Tauri, but in Rust syntax. **Inference:** slowest path to v0 for Daniel; revisit only if the engine and UI must share Rust types tightly.
- **Native Swift (SwiftUI `MenuBarExtra`).** Best macOS fit with the least code for a menu-bar app ([MenuBarExtra](https://developer.apple.com/documentation/swiftui/menubarextra)). But it is a third language in the repo, macOS only, and `AGENTS.md` allows Rust plus Go, with a TS allowance for the studio only. **Inference:** a port to Linux later means a rewrite.

| Stack | Effort for a TS dev | Other platforms later | Maintenance |
|---|---|---|---|
| Tauri + TS | low: TS UI, thin Rust | macOS, Windows, Linux | two toolchains (Cargo, npm) |
| egui / iced / Slint / gpui / Dioxus | high: Rust UI | yes, tray via `tray-icon` | one toolchain, young UI crates |
| Swift | medium: new language | macOS only | Xcode, third language |

## Recommendation for v0

1. **Menu-bar Tauri app, TS frontend.** One tray icon whose title or badge shows "N agents waiting". Click opens a small popover listing sessions and their state. A system notification fires when a session needs input.
2. **Feed it from hooks plus `claude agents --json`**, as in the data-sources report. No credentials.
3. **Ship a status line script too.** It costs one short script and gives per-session state with no window.
4. **Skip** a full window, TUI, web dashboard and editor panel for v0.

Why: it is the only option that is cross-session, has notifications, adds no window, and lets Daniel write mostly TypeScript. Tauri keeps Linux open for later, matching [ADR 1](../../adr/0001-develop-on-macos-support-all-oses.md).

**To check before building (not verified here):** hiding the Dock icon. Tauri exposes an activation policy setting for macOS (`ActivationPolicy::Accessory`) in its Rust API; confirm the exact call in the Tauri docs. Choosing Tauri also triggers the `AGENTS.md` "Implementation language" amendment (map #266, "Not yet specified").

---

## Addendum: after #285 changed the main job

**Answer.** Tauri still fits, but the surface changes from a tray menu to a **small floating panel**. It stays open while Daniel types a guess and comes up without taking focus from the terminal. Desktop notifications can carry buttons but no typed replies, so they only open the panel. **Cards come from what the working agent already does (Option B).** A `PreToolUse` hook sees the exact next tool call before it runs, which is the natural thing to guess about. A separate `claude -p` turns that into a short Hebrew card (B+A). The working agent is never asked to explain itself (not C).<br>
**Evidence: moderate.** Hook payloads, notification actions and window flags come from official docs. Panel focus behavior and the dismiss rules are partly unconfirmed and need a hands-on test.<br>
**Hypotheses.** Supports "Move learning into waiting time": hooks give a live "about to happen" signal for guesses. "Too many windows" still holds, with one small panel.<br>
**For Daniel.** The [hooks reference](https://code.claude.com/docs/en/hooks), the `PreToolUse`, `PostToolBatch` and common input fields sections (20 min).

Context: [#285 resolution](https://github.com/danielrispler/Falinks/issues/285#issuecomment-6412880294). The main job is short guesses and cards during waits, in Hebrew with English code terms. Status is shown thinly. v0 is a throwaway prototype of the guess-and-card loop.

### 1. Does the popover vanish on blur and lose a half-typed guess?

- **Confirmed:** Tauri's tray gives an icon, a native menu and click events ([tray guide](https://v2.tauri.app/learn/system-tray/)). A native menu cannot hold a text field, so the guess box must be a webview window the app shows on tray click.
- **Inference (unconfirmed):** such a window closes only when the app's own code hides it, for example on a focus-lost event. So losing the draft is our choice, not Tauri's. Two fixes, both cheap: do not hide on blur, and keep the draft in app state.
- **Panel option:** [`tauri-nspanel`](https://github.com/ahkohd/tauri-nspanel) (community, macOS only, branch `v2.1`, about 430 stars, used by Cap, Handy and screenpipe) turns a Tauri window into an `NSPanel`. It supports floating level (`is_floating_panel`, `PanelLevel::Floating`) and `can_become_key_window`. **Unconfirmed:** its README does not mention the non-activating style. It installs from a Git branch, not from a crates.io release. Maturity is fair for a prototype, risky for v1.

### 2. Can it appear without stealing keyboard focus?

- **Confirmed:** Tauri window config has `focus` ("Whether the window will be initially focused or not"), `focusable`, `alwaysOnTop` and `visibleOnAllWorkspaces` ([config reference](https://v2.tauri.app/reference/config/)). None has a macOS note.
- **Unconfirmed:** whether `focus: false` on macOS really leaves the terminal as the key window when the app is activated. A non-activating `NSPanel` is the standard macOS way to do this. Whether `tauri-nspanel` sets that mask needs a test.
- **Inference:** show the card without focus, and let a click or a global shortcut move focus into the guess box. Daniel decides when to type.

### 3. Notification actions on macOS desktop

- **Confirmed:** the notification plugin supports `registerActionTypes` and `onAction` on desktop. "On desktop only the identifier and title are used". "On desktop, the actions become buttons". On macOS, "two or more actions are shown in an 'Options' menu". Without an `onAction` listener, desktop notifications show no actions ([JS reference](https://v2.tauri.app/reference/javascript/notification/)).
- **Consequence:** there is no text-reply field on desktop, because the `input` flag is used on Android only. A notification can offer "Guess" or "Later" buttons, but the guess itself is typed in the panel.

### 4. Who writes the card?

**Confirmed hook facts** ([hooks reference](https://code.claude.com/docs/en/hooks)):
- `PreToolUse` and `PostToolUse` fire "on every tool call inside the agentic loop", so mid-turn. `PreToolUse` carries `tool_name`, `tool_input` and `tool_use_id` *before* the tool runs. `PostToolUse` adds `tool_response` and an optional `duration_ms`.
- `PostToolBatch` fires once per batch, "before the next model request".
- `TaskCreated` carries `task_subject` and `task_description`.
- Every hook gets `session_id`, `transcript_path`, `cwd` and `permission_mode` (which includes `"plan"`).
- Hooks can be `"async": true`, which "runs in the background without blocking".
- The transcript "is written asynchronously and may lag the in-memory conversation". It can be read live, but it runs a little behind. Its format is internal (see [data-sources.md](https://github.com/danielrispler/Falinks/blob/d8f26439dd49114a026cd330c5f95cb9ee085785/docs/studio/research/data-sources.md)).
- **Unconfirmed:** the docs do not name TodoWrite or ExitPlanMode. **Inference:** an unscoped matcher should catch them like any other tool. A logging hook will confirm it.

**The options:**
- **A, a separate `claude -p` per card.** It costs usage and seconds of delay. On its own it does not know what comes next.
- **B, read what the agent already emits.** It costs nothing and slows nothing. `PreToolUse` gives the exact next action, such as the file and the edit or the command, *before* it happens. That makes it the best input for a guess: "the agent is about to edit `auth.rs`. What do you think it will change, and why?" The answer is shown after `PostToolUse`. The plan text and tasks give the bigger "what next". Raw tool input is not a Hebrew card, though.
- **C, the agent writes the card itself** (a skill, a `CLAUDE.md` rule or an MCP tool). **Inference:** each card costs the agent context and turns, and adds a side goal that can pull it off task. Hook `additionalContext` also lands in the agent's context, so even nudging it is not free. The card would also give the answer before Daniel guesses, which defeats the guess. The #274 report found that active engagement helps the human. It gives no evidence that making the agent explain helps the task. I did not find a primary source that measures C's cost; this point stays unconfirmed.

**Recommendation for v0: B feeding A.**
1. An async `PreToolUse` hook (plus `TaskCreated` and plan text when present) sends the next action to the app.
2. The app asks Daniel for a guess right away, from raw facts only. This needs no model.
3. In parallel, one `claude -p` call writes the Hebrew card from those facts and the diff from `PostToolUse`. It is shown after the guess.
4. The working agent is not touched.

Pick only significant actions (edits, new files, commands), not every `Read`, so A's cost stays small. This is a guess at a good filter.

### Hands-on tests for v0

- **Wait timing:** how long the gap is between `PreToolUse` and `PostToolUse`, and between turns, against how long a guess takes.
- **Hebrew bidi** (mixed right-to-left and left-to-right text) with English code terms, in the WKWebView panel (`dir="auto"`, code spans) and in macOS notifications.
- **No-focus show:** `focus: false` against `tauri-nspanel`, with the terminal keeping keystrokes.
- **Draft survives blur** and Space switches.
- **Hook coverage:** whether TodoWrite and ExitPlanMode reach `PreToolUse`, and whether the async hook adds no delay.
- **Choosing the call:** a run makes dozens of tool calls, and many (reading a file, a search) are too shallow to guess about. A card per call would flood Daniel. v0 must find how to pick the one call per wait worth a card, and test it on real runs. Not researched yet.
