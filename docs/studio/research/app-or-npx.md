# Shipping the studio: desktop app, npx web app or something else

> **Answer:** Keep #283's Tauri floating panel and change how it ships: the hooks as a Claude Code plugin (one install command, no hand-edited settings), and the panel built once into a local app that starts at login, with no Apple signing while only Daniel uses it. Not an `npx` web app: a web page floats only after a click and cannot show itself without taking focus, and none of the 17 surveyed tools makes `npx` its main install. Watch Claude Code mods: a pane inside the terminal with no extra window, but today they cannot lay out Hebrew right to left.
> **Evidence:** Moderate. Install methods come from each tool's own README or docs, capabilities from official docs (Tauri, Electron, Apple, MDN, npm, Claude Code), and v0 confirmed the panel on Daniel's Mac. The daily cost of starting and updating each form is my inference; nothing new was built or timed, and no mod was run.
> **Hypotheses:** Supports "Too many windows": the panel adds no Dock or app-switcher entry, while a web app adds a tab or a window. Weakens a web dashboard as the main surface; a later debrief page stays open. For "I miss the moment an agent is waiting", only a desktop app's own notifications reach Daniel in every app; Claude Code's terminal notifications reach the desktop by default only in Ghostty, Kitty and iTerm2.
> **Read yourself:** Claude Code's [Mods overview](https://code.claude.com/docs/en/plugins/mods/overview), down to "How a mod works" (about 15 min). Tauri's [macOS code signing](https://v2.tauri.app/distribute/sign/macos/) (about 10 min), for what shipping to other people would cost.

Research date: 2026-10-28. Ticket: [#319](https://github.com/danielrispler/Falinks/issues/319). Map: [#266](https://github.com/danielrispler/Falinks/issues/266). It revisits the [form factor and stack report](https://github.com/danielrispler/Falinks/blob/107a2cbfe5042a5c1455db9fb7c6d46b0200bfce/docs/studio/research/form-factor-stack.md) (#283, with its addendum).

**How to read this.** A claim with a link is established by that source. A sentence that starts with **Inference:** is my own reasoning. In the big table, a cell marked * is inference. The problem and hypothesis names are the ones in [`problems.md`](https://github.com/danielrispler/Falinks/blob/660c11a/docs/studio/problems.md).

---

## Words used in this report

- **`npx`:** a command that comes with Node.js. `npx some-tool` downloads a package from the npm registry and runs it, without a separate install step.
- **Local web app:** a small server on Daniel's own machine, opened in a browser at an address such as `http://localhost:4981`.
- **PWA (progressive web app):** a web app that the browser can "install", so it gets its own window and Dock icon. Safari calls these "web apps" ("Add to Dock").
- **Gatekeeper and notarization:** macOS checks apps that came from the internet. An app passes when it is signed with an Apple "Developer ID" and has been sent to Apple's notary service, which scans it. This costs a paid Apple developer account.
- **Login item:** an app that macOS starts when you log in.
- **Claude Code plugin:** a folder that adds skills, hooks and other parts to Claude Code, installed with `claude plugin install`.
- **Mod:** a newer kind of Claude Code plugin whose JavaScript or TypeScript code runs inside Claude Code and can draw a pane in its interface.
- **Non-activating panel:** a macOS window that floats on top but does not take keyboard focus from the app you are typing in. v0 uses one.

---

## 1. What v0 settled, and why this ticket remains

#283 chose a Tauri 2 app with a TypeScript frontend, shown as a small floating panel ([report](https://github.com/danielrispler/Falinks/blob/107a2cbfe5042a5c1455db9fb7c6d46b0200bfce/docs/studio/research/form-factor-stack.md)). The v0 spike then answered its open questions on Daniel's Mac ([#315 results](https://github.com/danielrispler/Falinks/issues/315#issuecomment-6418344200)):

- The panel shows without changing the front app and without becoming the key window. A click makes it key, so Daniel can type.
- `ActivationPolicy::Accessory` hides the Dock icon. This was checked on a debug binary, not a bundled `.app`.
- A half-typed guess survives clicking away, switching Spaces, and hide/show.
- `tauri-nspanel` is now on crates.io as version 2.1, so it no longer installs from a Git branch.

How v0 starts today ([spike README](https://github.com/danielrispler/Falinks/blob/6d59b76666c7fd9de2df3a77553a8186c87a8868/apps/studio/v0-spike/README.md)): run `cargo run` in the panel folder, then start each Claude Code session with `claude --settings <absolute path into a worktree>/hook/settings.json`.

**Inference:** that is fine for a spike and wrong for daily use. The panel stops when its terminal closes. The hooks exist only in sessions started with that flag. The path points into a worktree that will be deleted. So the question is no longer "can Tauri do it", but "how does it reach Daniel every day, and is there a cheaper form that does the job as well".

---

## 2. How agent tools ship today

Most tools ship a desktop app. Terminal tools ship through Homebrew, an install script or npm. No tool makes `npx` its main install.

These are the 17 tools in section 6.5 of the [existing-tools report](https://github.com/danielrispler/Falinks/blob/fe647d25890d11e443d606596d3e9890feeab013/docs/studio/research/existing-tools.md#65-form-factors-in-use). Each row comes from the tool's own README or install page, read on 2026-10-28. "Main" is the first or recommended install the source names.

| Tool | Main install | Other routes | Source |
| --- | --- | --- | --- |
| Pixel Agents | VS Code extension (VS Code Marketplace, Open VSX) | `npx pixel-agents` starts a local server for a browser; `npm install --global pixel-agents` | [README](https://github.com/pixel-agents-hq/pixel-agents) |
| Agents Observe | Claude Code plugin: `claude plugin marketplace add simple10/agents-observe`, then `claude plugin install agents-observe`. "The plugin will auto start the MCP server"; the dashboard is at `http://localhost:4981` | Clone and run with `just` | [README](https://github.com/simple10/agents-observe) |
| disler's Multi-Agent Observability | Clone, copy its `.claude` folder into the project, `just start`; web client at `http://localhost:5173` | none | [README](https://github.com/disler/claude-code-hooks-multi-agent-observability) |
| Emdash | Desktop app: `brew install --cask emdash`, or a DMG | Windows MSI, Linux AppImage, DEB, RPM | [README](https://github.com/generalaction/emdash) |
| Superset (its README now says **Paradev**) | Desktop app, DMG for macOS | Linux AppImage ("experimental"); CLI by `curl … \| sh` or `brew install paradevai/tap/superset`; iPhone app | [README](https://github.com/superset-sh/superset) |
| Conductor | Mac app ("Download Conductor") | none found | [conductor.build](https://www.conductor.build) |
| Nimbalyst | Desktop app: DMG, `.exe`, `.deb`, AppImage. "Nimbalyst automatically checks for updates" | iOS app | [README](https://github.com/nimbalyst/nimbalyst) |
| xum (formerly mux) | Desktop app: "Signed and notarized DMG" | `npx @coder/xum run …`, `npx @coder/xum server --port 3000` "for remote/mobile access", Nix. Its docs say it runs agents "right from your browser or desktop" | [install page](https://xum.cdr.dev/install), [intro](https://xum.cdr.dev/) |
| Google Antigravity | Desktop app from `antigravity.google/download` (DMG, `.exe`, `.tar.gz`) | CLI by `curl … \| bash`; IDE extensions in the VS Code and Visual Studio marketplaces; Remote Control in a browser | [getting started](https://antigravity.google/docs/getting-started) |
| ChatGPT desktop app (Codex app, pets) | Desktop app: "the Codex app merged into the ChatGPT desktop app for macOS and Windows" | Linux preview `.deb`/`.rpm` | [What's new](https://learn.chatgpt.com/codex/whats-new) |
| Claude desktop app | Desktop download for macOS and Windows | Linux (beta) through apt | [Desktop docs](https://code.claude.com/docs/en/desktop) |
| Claude Code agent view | Part of Claude Code: `curl -fsSL https://claude.ai/install.sh \| bash`, which auto-updates | `brew install --cask claude-code` (no auto-update), WinGet, npm, apt/dnf/apk | [Setup](https://code.claude.com/docs/en/setup) |
| Claude Squad | `brew install claude-squad` | `curl … install.sh \| bash` | [README](https://github.com/smtg-ai/claude-squad) |
| ccmanager | `npm install -g ccmanager` | `npx ccmanager` ("Or run without installing") | [README](https://github.com/kbwo/ccmanager) |
| agent-deck | `curl … install.sh \| bash` | `brew install asheshgoplani/tap/agent-deck`, `go install`; `agent-deck web` serves a web UI on `127.0.0.1:8420` | [README](https://github.com/asheshgoplani/agent-deck) |
| cmux | DMG; "cmux auto-updates via Sparkle" | `brew tap manaflow-ai/cmux`, `brew install --cask cmux` | [README](https://github.com/manaflow-ai/cmux) |
| Happy | Desktop app download, and phone apps in the App Store and Google Play | `npm install -g happy` | [README](https://github.com/slopus/happy) |

**The count.**

- **Main install:** a downloaded desktop app for 10 tools (Emdash, Superset, Conductor, Nimbalyst, xum, Antigravity, ChatGPT, Claude desktop, cmux, Happy). A terminal program for 4 (Claude Code, Claude Squad, ccmanager, agent-deck). A local web dashboard for 2 (Agents Observe, disler's tool). An editor extension for 1 (Pixel Agents).
- **Any route at all:** a desktop download 10, Homebrew 6, an install script piped to the shell 5, npm or `npx` 5, a local web UI 5, a phone store 3, an editor extension 2, a Claude Code plugin 1.
- **`npx` is never the main install.** It starts a local server as a second route in Pixel Agents and xum, and runs a TUI in ccmanager. Outside section 6.5, the existing-tools report names Vibe Kanban, whose install is `npx vibe-kanban` ([README](https://github.com/BloopAI/vibe-kanban)); the company behind it shut down on 2026-04-10 ([existing-tools report, 3.2](https://github.com/danielrispler/Falinks/blob/fe647d25890d11e443d606596d3e9890feeab013/docs/studio/research/existing-tools.md#32-parallel-agent-workbenches-ades-many-tools-one-shape)).
- **The two local web dashboards** are started by a Claude Code plugin with Docker (Agents Observe) or by a clone and a script (disler's tool), not by `npx`.
- **No tool ships a small floating panel** like v0. The nearest is the Codex pet, which floats but lives inside the ChatGPT app ([Pets](https://learn.chatgpt.com/codex/pets), via the existing-tools report). The apps that ship the same `tauri-nspanel` technique (Handy, Cap, screenpipe and others in its [showcase](https://github.com/ahkohd/tauri-nspanel)) are not agent tools.

**Inference:** the field ships big workbenches as desktop apps and observers as local web pages. v0 is neither: it is a small companion. So copying the desktop-app route makes sense, but not its weight (installers, auto-update servers, signing) while Daniel is the only user.

---

## 3. Daily convenience, form by form

### 3.1 A Tauri app that Daniel builds for himself

- **Install.** Tauri builds a macOS app bundle with `cargo tauri build --bundles app` ([macOS bundle](https://v2.tauri.app/distribute/macos-application-bundle/)). Daniel copies the `.app` to Applications once.
- **Gatekeeper.** Apple's developer support says "Gatekeeper usually only kicks in the app is quarantined", and that "most Unix-y tools don't quarantine their downloads" ([Apple Developer Forums, DTS](https://developer.apple.com/forums/thread/666452)). Browsers mark downloads as quarantined. **Inference:** an app that Daniel builds on his own Mac is not a download, so it should open without notarization. v0's debug binary already ran this way. To confirm with the built `.app`.
- **Start.** Tauri's autostart plugin will "automatically launch your application at system startup", on macOS through `MacosLauncher::LaunchAgent` ([autostart](https://v2.tauri.app/plugin/autostart/)). Another route: a hook runs `open -g` on the app, and `-g` means "Do not bring the application to the foreground" (`man open` on macOS).
- **Update.** **Inference:** `git pull` and rebuild. No update server, no signing key.
- **What breaks.** **Inference:** Rust and Tauri crate updates, and `tauri-nspanel`, a community crate for macOS only. Nothing breaks from Node versions or ports, because nothing listens on a port.

### 3.2 The same app, shipped to other people

This is the cost #283 did not cover. It applies only if the studio gets other users.

- **Apple account.** The Apple Developer Program costs "99 USD per membership year" ([enroll](https://developer.apple.com/programs/enroll/)). Tauri's docs: "Notarization is required when using a Developer ID Application certificate", and a free account "will not be able to notarize" ([macOS code signing](https://v2.tauri.app/distribute/sign/macos/)).
- **Unsigned apps got harder to open.** "In macOS Sequoia, users will no longer be able to Control-click to override Gatekeeper when opening software that isn't signed correctly or notarized" ([Apple, 2024-08-06](https://developer.apple.com/news/?id=saqachfa)). An ad-hoc signature "does not prevent MacOS from requiring users to" approve the app in Privacy & Security ([Tauri](https://v2.tauri.app/distribute/sign/macos/)). xum shows the cost: its non-`main` builds are not notarized, so users must run `xattr -cr` and re-sign by hand ([xum install](https://xum.cdr.dev/install)).
- **Auto-update.** Tauri's updater "needs a signature to verify that the update is from a trusted source. This cannot be disabled." It can read a static JSON file from GitHub Releases ([updater](https://v2.tauri.app/plugin/updater/)). Electron: "Your application must be signed for automatic updates on macOS", and "There is no built-in support for auto-updater on Linux" ([autoUpdater](https://www.electronjs.org/docs/latest/api/auto-updater)).
- **Homebrew.** Homebrew 5.0.0 deprecated casks without code signing and the `--no-quarantine` flag, and will disable casks in Homebrew/homebrew-cask that fail Gatekeeper checks in September 2026 ([Homebrew 5.0.0](https://brew.sh/2025/11/12/homebrew-5.0.0/)). A personal tap can hold casks ([taps](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap)); the post does not say whether the Gatekeeper rule covers taps.

**Inference:** about a day of setup plus the yearly fee. Worth it only when someone other than Daniel installs the app.

### 3.3 A local web app started with `npx`

- **How it starts.** `npx` installs the package "to a folder in the npm cache" and prints a prompt first, unless `--yes` is given ([npm docs](https://docs.npmjs.com/cli/v11/commands/npx)). The docs do not say when a cached copy is checked for a newer version. ccusage tells users to run `npx ccusage@latest` ([README](https://github.com/ccusage/ccusage)). **Inference:** authors add `@latest` to be sure users get updates.
- **What breaks, in the tools' own words.**
  - Node versions. Pixel Agents: "verify Node.js 20+". xum needs specific patched Node versions, because older ones "are affected by CVE-2026-111497" ([Pixel Agents](https://github.com/pixel-agents-hq/pixel-agents), [xum](https://xum.cdr.dev/install)).
  - Ports. Pixel Agents picks a new free port on each start, so a bookmark breaks unless you pass `--port`. Agents Observe "auto-assigns a free port if 4981 is taken" ([Pixel Agents](https://github.com/pixel-agents-hq/pixel-agents), [Agents Observe](https://github.com/simple10/agents-observe)).
  - Security. Pixel Agents puts a token in the URL: "Treat that URL as a secret: the token is a bearer capability". Agents Observe once listened on all network interfaces with no login (existing-tools report, 2.2).
  - Extra runtimes. Agents Observe: "If docker, node, or bash are not installed on your host, the plugin will fail to properly load."
- **Notifications.** Permission is given per origin, and `Notification.requestPermission()` "should only be called when handling a user gesture". A notification created by a page has a lifetime "tied to the lifetime of the page" ([MDN, Notifications API](https://developer.mozilla.org/en-US/docs/Web/API/Notifications_API)). A notification that outlives the page needs a service worker, and push messages arrive "from a server" through a subscription endpoint ([MDN, Push API](https://developer.mozilla.org/en-US/docs/Web/API/Push_API)). `localhost` counts as a secure context, so the APIs are allowed there ([MDN, Secure contexts](https://developer.mozilla.org/en-US/docs/Web/Security/Secure_Contexts)). **Inference:** for a local-only app, the tab must stay open for notifications to work.
- **Floating.** The only web window that floats is Document Picture-in-Picture: "an always-on-top window that can be populated with arbitrary HTML content", one per tab, which "never outlives the opening window" ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/Document_Picture-in-Picture_API)). Opening it needs a user action: it rejects "without transient activation" ([MDN, requestWindow](https://developer.mozilla.org/en-US/docs/Web/API/DocumentPictureInPicture/requestWindow)). Chrome shipped it in version 116, and "the Picture-in-Picture window position cannot be set by the website" ([Chrome docs](https://developer.chrome.com/docs/web-platform/document-picture-in-picture)). MDN marks it "not Baseline". **Inference:** it cannot pop up on its own when a hook fires, and closing the tab closes it.
- **Start.** **Inference:** someone runs the command after each reboot, or writes a LaunchAgent by hand.

### 3.4 An installed web app (PWA)

- Safari's "Add to Dock" makes a web app that "functions independently of Safari". It needs macOS Sonoma 14 or later, can be added "as a login item", and shows unread notifications as a red badge on its Dock icon ([Apple Support](https://support.apple.com/en-us/104996)).
- The Badging API sets that badge, and MDN marks it "Limited availability" ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/Badging_API)).
- **Inference:** it fixes the lost-tab problem, but not focus. It is an ordinary app window with a Dock icon, so it appears in Cmd-Tab and takes focus when it opens. I found no web API that shows a window without focus. The local server must still run, as in 3.3.

### 3.5 A Claude Code plugin, and mods

**The plugin as packaging.** A plugin carries hooks in `hooks/hooks.json`, in "the same shape as `hooks` in `settings.json`". `${CLAUDE_PLUGIN_ROOT}` resolves to the plugin's installed folder, and `${CLAUDE_PLUGIN_DATA}` is a data folder "kept across plugin updates" and deleted on uninstall ([plugin reference](https://code.claude.com/docs/en/plugins-reference)). A marketplace can be a local directory, and `claude plugin install <name> --marketplace <source>` adds it and installs in one command ([install plugins](https://code.claude.com/docs/en/plugins/install)). User scope writes only an `enabledPlugins` entry to `~/.claude/settings.json` (same page). Local and third-party marketplaces do not auto-update by default; `claude plugin update` updates one plugin (same page). Agents Observe uses its plugin to start its own server (section 2).

**Inference:** this replaces v0's `--settings` flag and absolute path. It also avoids what the existing-tools report warned about: tools that rewrote `~/.claude/settings.json` or injected hooks into projects (Pixel Agents, Emdash). One limit carries over from the [data-sources report](https://github.com/danielrispler/Falinks/blob/d8f26439dd49114a026cd330c5f95cb9ee085785/docs/studio/research/data-sources.md): sessions that Falinks launches do not see Daniel's own hooks, so the app still needs a second way in for those.

**Mods as the interface.** A mod is a plugin whose JavaScript or TypeScript runs inside Claude Code. It can "draw an interface you can use: a pane beside the transcript or a band above the prompt, with tabs, buttons, and text fields". Mods are on by default from v2.1.287 in the terminal, and also work in the Desktop app's Code tab and the VS Code extension ([Mods overview](https://code.claude.com/docs/en/plugins/mods/overview)). This Mac runs Claude Code 2.1.308 (`claude --version`). What a mod offers the guess-and-card loop ([reference](https://code.claude.com/docs/en/plugins/mods/reference), [API](https://code.claude.com/docs/en/plugins/mods/api), [interface](https://code.claude.com/docs/en/plugins/mods/interface)):

- A `tool.call` event before each tool runs, and the settings hook events as `classic.PostToolUse` and similar.
- `$.model.complete` calls a model with "the session's credentials, so they bill to the user's plan". **Inference:** it could write the card in place of a separate `claude -p` process.
- `$.store`, a key-value store "that every session on the machine shares".
- Focus: "A pane that opens while the user is typing doesn't take their keystrokes", and a focus request is granted "only while the prompt is empty". A text field keeps the user's text across redraws "as long as the hook passes the same `value` each time".
- It adds no window at all.

The limits matter more:

- **Hebrew.** The `Box` and `Text` props cover layout, spacing, colour and type, with no direction or alignment ([reference, Elements](https://code.claude.com/docs/en/plugins/mods/reference#elements)). An open feature request from a Hebrew user, [anthropics/claude-code#99956](https://github.com/anthropics/claude-code/issues/99956) (2026-10-06), reports that "Hebrew stays left-aligned", and that direction marks inside the text "only reorder runs within a line". Hebrew cards are a decision of [#285](https://github.com/danielrispler/Falinks/issues/285#issuecomment-6412880294).
- **Reach.** `$.ui.notify` sends "a notification from their terminal". By default "Claude Code sends a desktop notification only in Ghostty, Kitty, and iTerm2" ([terminal config](https://code.claude.com/docs/en/terminal-config)). In the Desktop app and VS Code it answers `no-surface` ([API](https://code.claude.com/docs/en/plugins/mods/api)). This research session ran with `TERM_PROGRAM=zed`. **Inference:** if Daniel runs Claude Code in Zed's terminal, a mod's notification would not reach the desktop, and its pane is seen only while that terminal is in front.
- **Trust.** "Mods aren't sandboxed" and run with the user's permissions (overview). For Daniel's own code this is acceptable.
- **Language rule.** **Inference:** a mod is TypeScript that is not a frontend. `apps/studio/AGENTS.md` allows "a TypeScript frontend", so a mod would need that wording widened. The same applies to an Electron main process.

---

## 4. What the main job needs, form by form

The main job ([#285](https://github.com/danielrispler/Falinks/issues/285#issuecomment-6412880294)) is short guesses and cards during waits, in Hebrew. So the form must appear without taking focus, keep a half-typed guess, reach Daniel when an agent is blocked on him, update live from hooks, and show Hebrew right to left. The [interruptions report](https://github.com/danielrispler/Falinks/blob/4dc167c4ba23a1398653eb128551001398a61fc4/docs/studio/research/interruptions-attention.md) asks for one calm, visible panel, not pop-ups.

Cells marked * are my inference.

| | Tauri panel (v0) | Electron panel | `npx` web app in a tab | Installed web app (PWA) | Claude Code mod |
| --- | --- | --- | --- | --- | --- |
| **First install** | Build once with Cargo, copy the `.app`; no signing for own use* | npm project, package the app*; same signing story as Tauri for others | `npx <pkg>`; needs Node (Pixel Agents: 20+) | Open the local page, Safari "Add to Dock" (macOS 14+); server still needed* | `claude plugin install … --marketplace <path>`; no build step |
| **Start each day** | Login item via the autostart plugin, or a hook runs `open -g` | `app.setLoginItemSettings`; Electron warns it "may silently fail" when the app is not signed and notarized | Run the command after each reboot*; the port may change (Pixel Agents) | Login item (Apple); the server starts separately* | Starts with every Claude Code session |
| **Update** | `git pull` and rebuild*; for others, an updater with a mandatory signature | `autoUpdater` needs a signed app on macOS, none on Linux | `npx <pkg>@latest`*; cache refresh undocumented | Reload the page* | A plugin from a local path loads in place; `/reload-plugins` |
| **Focus** | Non-activating floating panel, proven by v0 (`tauri-nspanel`, macOS only) | Documented: `type: 'panel'` adds the non-activating mask; `showInactive()` "Shows the window but doesn't focus on it". Not tried | A tab takes focus; the only float is Document Picture-in-Picture (a click to open, Chrome 116+ and not Baseline, closes with its tab) | An ordinary app window that takes focus* | Never takes keystrokes while Daniel types; visible only while that terminal is in front* |
| **Keeps a half-typed guess** | Yes (v0) | Yes, unless reloaded* | Until the tab closes or reloads* | Same as a tab* | Yes across redraws; `$.store` across restarts |
| **Notifications** | System notifications with buttons, no typed reply (#283) | Has a notification module (not checked here) | Needs permission and an open page; push needs a server | Notifications and a Dock badge in Safari web apps | Through the terminal; desktop only in Ghostty, Kitty, iTerm2 by default |
| **Live updates from hooks** | Tails a file today (v0) | Same* | Local server plus WebSocket, as Agents Observe does | Same as a tab | In-process events, no file |
| **Hebrew right to left** | Webview with CSS `direction: rtl`; Daniel saw v0's panel and it looks good | Chromium webview, same* | Browser, same* | Browser, same* | No direction prop; open request #99956 |
| **Adds a window** | None in Dock or Cmd-Tab (v0) | None with `setActivationPolicy('accessory')` | One more tab | One more window and Dock icon | None |
| **Effort for Daniel** | Done for v0: a 209-line Rust shell plus HTML | Rewrite the shell in TypeScript*; ships Chromium and Node in the app | Write a server, lose floating and focus* | Same as `npx`, plus a manifest* | TypeScript only, smallest*, but Hebrew is blocked |
| **Other platforms later** | Tauri builds for Windows and Linux; the panel crate is macOS only; Windows notifications "Only works for installed apps" | Windows and Linux; `panel` is macOS only; `alwaysOnTop` "Not supported on Wayland" | Any OS with Node and a browser | Any desktop browser that installs web apps* | Wherever Claude Code runs; no plugins in Desktop WSL sessions |
| **Other users later** | $99 a year, notarization, updater key | Same | npm publish; users need the right Node | The local server still ships through npm* | One plugin install, for Claude Code users only |

Sources for the table, beyond sections 3.1 to 3.5: Electron's [window options](https://www.electronjs.org/docs/latest/api/structures/base-window-options), [BaseWindow](https://www.electronjs.org/docs/latest/api/base-window) and [app](https://www.electronjs.org/docs/latest/api/app) pages; Electron works "by embedding Chromium and Node.js into its binary" ([docs](https://www.electronjs.org/docs/latest/)); Tauri's [notification plugin](https://v2.tauri.app/plugin/notification/); the v0 panel's [`index.html`](https://github.com/danielrispler/Falinks/blob/6d59b76666c7fd9de2df3a77553a8186c87a8868/apps/studio/v0-spike/panel/dist/index.html) and [`main.rs`](https://github.com/danielrispler/Falinks/blob/6d59b76666c7fd9de2df3a77553a8186c87a8868/apps/studio/v0-spike/panel/src/main.rs); [#315 progress](https://github.com/danielrispler/Falinks/issues/315#issuecomment-6418665855) for "it looks good".

**Inference:** only the two desktop panels pass on focus and Hebrew together. Of those, Tauri is already built and proven. Electron documents the non-activating panel without a community crate, which is a real plus, but it would mean redoing working code.

---

## 5. Hybrids and "something else"

- **One local server, several thin shells.** Pixel Agents runs one server behind both its VS Code panel and its browser page, and running both at once "is supported" ([README](https://github.com/pixel-agents-hq/pixel-agents)). **Inference:** v0's panel is plain HTML and JavaScript with no build step, so the Tauri app could later serve the same page on `localhost` for a large, non-urgent view, such as the weekly debrief from the [measuring-improvement report](https://github.com/danielrispler/Falinks/blob/8550db7cc6c9eed83bccc7d0f3aa45518d1b61f3/docs/studio/research/measuring-improvement.md). The panel stays the main surface.
- **A menu-bar app that opens a web view.** This was #283's first answer. **Inference:** a tray icon is still a cheap handle to reopen the panel, and nothing in this research argues against it.
- **A Claude Code plugin that starts the app.** Agents Observe does this for its server. **Inference:** a `SessionStart` hook in the studio plugin can run `open -g -a` on the app, so the app is up whenever a session starts, without a login item.
- **An editor extension.** #283 found that Zed extensions cannot add custom panels, and a VS Code panel shows only while VS Code is in front ([report](https://github.com/danielrispler/Falinks/blob/107a2cbfe5042a5c1455db9fb7c6d46b0200bfce/docs/studio/research/form-factor-stack.md)). Nothing found here changes that.
- **A Claude Code mod.** The strongest "something else" (section 3.5). It needs no app, no window and no Rust. It loses on Hebrew today and on reaching Daniel outside the terminal.

---

## 6. Does this keep or change #283's recommendation?

**It keeps the core:** Tauri 2 with a TypeScript frontend, one small floating panel that keeps a half-typed guess, notifications that only open the panel, and cards driven by hooks.

**It changes four things:**

1. **Delivery.** v0's two manual commands become a plugin for the hooks, plus a locally built app that starts at login or from the plugin. #283 did not cover delivery.
2. **The web dashboard.** #283 called it "good later for rich views, not for v0". That still holds, and now has sources: a web page cannot float or show without focus, and its notifications need an open page.
3. **The in-terminal view.** #283 suggested a status line script as a free per-session view. Mods now go further, with a pane, a text field and model calls inside the terminal. They replace the status line idea once they can show Hebrew.
4. **Shipping to others.** #283 did not price it: a $99 yearly Apple account, notarization, an updater signing key, and Homebrew's Gatekeeper rule.

---

## 7. What this could mean for the studio (to test, not decided)

1. **Keep the Tauri panel as the card surface.** It is the only form that has passed focus, draft and Hebrew on Daniel's Mac.
2. **Move the hooks into a plugin in the repo**, installed from a local path with one `claude plugin install … --marketplace <path>` command. Test that the hooks fire in the terminal Daniel really uses.
3. **Build the panel into a `.app` once** and start it with the autostart plugin or a `SessionStart` hook. Test that a locally built `.app` opens with no Gatekeeper prompt, that the Dock icon stays hidden in the bundle, and that its notifications appear.
4. **Do not sign, notarize or add an updater** until someone other than Daniel installs it.
5. **Serve a local web page from the same app only for heavy, non-urgent views**, such as the weekly debrief. Never for interruptions.
6. **Recheck mods when they gain a text direction prop** (#99956). Then try a mod pane for waits that Daniel spends in the terminal, next to the floating panel, and keep whichever he uses.

---

## Limits

- The count rests on READMEs and install pages read on 2026-10-28. Tools change fast; Superset's README already says Paradev.
- Nothing was built, installed or timed. No mod was run. The daily-cost cells are inference.
- The Electron panel's focus behaviour comes from its docs only.
- Linux and Windows were looked at only through the docs above; [docs/platforms.md](https://github.com/danielrispler/Falinks/blob/main/docs/platforms.md) covers the engine, not the studio.
- Which terminal Daniel uses daily is unknown. This session's `TERM_PROGRAM=zed` is a hint, not a fact.

## Sources checked

Fetched and checked against the source on 2026-10-28:

- READMEs, through the GitHub API: Pixel Agents (`npx pixel-agents`, free port per start, `--port`, Node 20+, token warning, extension and standalone together), Agents Observe (plugin install commands, auto-start, port 4981 and auto-assign, Docker and Node requirement), disler's tool (clone, `just start`, port 5173, uv, Bun, API key), Emdash (`brew install --cask emdash`, DMG, MSI, AppImage, DEB, RPM), Superset/Paradev (DMG, AppImage "experimental", CLI by curl or tap, App Store), Nimbalyst (DMG, `.exe`, `.deb`, AppImage, auto-update), xum (release downloads), Claude Squad (`brew install claude-squad`, script), ccmanager (`npm install -g`, `npx ccmanager`), agent-deck (script, tap, `go install`, `agent-deck web` on `127.0.0.1:8420`), cmux (DMG, Sparkle, tap and cask), Happy (desktop, stores, `npm install -g happy`), Vibe Kanban (`npx vibe-kanban`), ccusage (`npx ccusage@latest`).
- xum install page, https://xum.cdr.dev/install: "Signed and notarized DMG", the `xattr`/`codesign` steps for non-`main` builds, `npx @coder/xum`, the Node versions and CVE.
- Antigravity getting started, https://antigravity.google/docs/getting-started: app downloads, the `agy` install script, extension marketplaces.
- Conductor home page: a Mac app download; no format stated.
- OpenAI, https://learn.chatgpt.com/codex/whats-new: the merge into the ChatGPT desktop app "for macOS and Windows".
- Claude Code docs: [Setup](https://code.claude.com/docs/en/setup) (install routes, auto-update per route), [Desktop](https://code.claude.com/docs/en/desktop) (platforms), [plugin reference](https://code.claude.com/docs/en/plugins-reference) (hooks, plugin variables, monitors), [install plugins](https://code.claude.com/docs/en/plugins/install) (local marketplaces, `--marketplace`, scopes, auto-update defaults), [Mods overview](https://code.claude.com/docs/en/plugins/mods/overview), [reference](https://code.claude.com/docs/en/plugins/mods/reference), [API](https://code.claude.com/docs/en/plugins/mods/api) and [interface](https://code.claude.com/docs/en/plugins/mods/interface) (versions, panes, focus rules, `$.store`, `$.model.complete`, `$.ui.notify`, element props), [terminal config](https://code.claude.com/docs/en/terminal-config) (which terminals get desktop notifications).
- [anthropics/claude-code#99956](https://github.com/anthropics/claude-code/issues/99956): open, 2026-10-06, no text direction for mods.
- Tauri: [updater](https://v2.tauri.app/plugin/updater/) (mandatory signature, static JSON), [autostart](https://v2.tauri.app/plugin/autostart/) (LaunchAgent; its source adds the plugin only for macOS, Windows and Linux targets), [macOS code signing](https://v2.tauri.app/distribute/sign/macos/) ($99, notarization, ad-hoc), [macOS bundle](https://v2.tauri.app/distribute/macos-application-bundle/) (build command), [notification](https://v2.tauri.app/plugin/notification/) (Windows installed-only note), [start](https://v2.tauri.app/start/) (system webview, "less than 600KB").
- [`tauri-nspanel` README](https://github.com/ahkohd/tauri-nspanel): `tauri-nspanel = "2.1"`, macOS only, showcase list.
- Electron: [window options](https://www.electronjs.org/docs/latest/api/structures/base-window-options) (`type: 'panel'`, `focusable`, `alwaysOnTop` on Wayland), [BaseWindow](https://www.electronjs.org/docs/latest/api/base-window) (`showInactive`), [app](https://www.electronjs.org/docs/latest/api/app) (login items, activation policy), [autoUpdater](https://www.electronjs.org/docs/latest/api/auto-updater), [intro](https://www.electronjs.org/docs/latest/) (embeds Chromium and Node.js).
- Apple: [enroll](https://developer.apple.com/programs/enroll/) (99 USD a year), [Sequoia Gatekeeper news](https://developer.apple.com/news/?id=saqachfa) (2024-08-06), [Gatekeeper and runtime protection](https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web), [DTS forum answer](https://developer.apple.com/forums/thread/666452) (quarantine), [Safari web apps](https://support.apple.com/en-us/104996) (Sonoma 14, login item, Dock badge). `man open` on this Mac for `-g`.
- MDN: [Notifications API](https://developer.mozilla.org/en-US/docs/Web/API/Notifications_API), [Push API](https://developer.mozilla.org/en-US/docs/Web/API/Push_API), [Secure contexts](https://developer.mozilla.org/en-US/docs/Web/Security/Secure_Contexts), [Document Picture-in-Picture](https://developer.mozilla.org/en-US/docs/Web/API/Document_Picture-in-Picture_API), [requestWindow](https://developer.mozilla.org/en-US/docs/Web/API/DocumentPictureInPicture/requestWindow), [Badging API](https://developer.mozilla.org/en-US/docs/Web/API/Badging_API). Chrome's [Document PiP page](https://developer.chrome.com/docs/web-platform/document-picture-in-picture) (Chrome 116, user gesture, position).
- npm: [`npx`](https://docs.npmjs.com/cli/v11/commands/npx) (prompt, npm cache).
- Homebrew: [5.0.0 post](https://brew.sh/2025/11/12/homebrew-5.0.0/) (unsigned casks deprecated, September 2026), [taps](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap).
- In this repo: the #283 report at `107a2cb`, the #315 comments, the v0 spike at `6d59b76`, the existing-tools, interruptions, data-sources and measuring-improvement reports.

Rests on memory, an earlier report, or not checked:

- That macOS does not quarantine an app Daniel builds himself, and that Apple Silicon's linker signs it ad hoc. Apple's answer says Gatekeeper "usually" checks only quarantined apps; the rest is memory. To test with a built `.app`.
- Whether macOS notifications from a Tauri app need the bundled `.app`. Tauri's page notes this only for Windows.
- Electron's notification module, and Electron app size: not checked.
- Whether Safari's "Add to Dock" works for an `http://localhost` page, and whether Chrome can start an installed web app at login.
- Whether Zed's terminal shows Claude Code's desktop notifications or lays out Hebrew right to left. The Claude Code docs list only Ghostty, Kitty and iTerm2.
- The Vibe Kanban shutdown date and the Codex pet: from the existing-tools report, not rechecked.
- Whether the September 2026 Homebrew rule has taken effect, and whether it covers personal taps.
