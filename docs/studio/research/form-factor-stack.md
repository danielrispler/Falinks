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
