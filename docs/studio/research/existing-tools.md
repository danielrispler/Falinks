# Existing agent-supervision tools

> **Answer:** Existing tools handle one job well: showing which agent needs you now. None of them tells you whether an agent is stuck, and none helps you understand a change while it happens. The agent makers themselves (Anthropic, OpenAI, Google) are taking over the supervision layer, so a v1 can build on their supported interfaces and spend its own effort on understanding.
>
> **Evidence:** Moderate. I confirmed that 22 tools exist and are maintained, and read the docs, source code and issue trackers of 16 of them. Independent user reports are thin: a few Hacker News threads, and almost none for Pixel Agents or agents-observe. I ran no tool.
>
> **Hypotheses:** Supports *Non-blocking* and *Evidence, not promises*. Mixed for *Small and visual before text* and *Serious control-room style*. No tool tries *Move learning into waiting time*, *Guessing, not testing*, *A game that rewards understanding, not volume* or *Learning content while waiting*: open ground, still untested.
>
> **For Daniel:** Claude Code's [agent view docs](https://code.claude.com/docs/en/agent-view), down to "Peek and reply" (15 min). Google's [Antigravity launch post](https://developers.googleblog.com/en/build-with-google-antigravity-our-new-agentic-development-platform/), the "Verify with Artifacts, not logs" section (10 min).

Research date: 2026-10-24. Star counts, release dates and last-push dates come from the GitHub API on that day. Ticket: [#277](https://github.com/danielrispler/Falinks/issues/277). Map: [#266](https://github.com/danielrispler/Falinks/issues/266).

**How to read this.** Each section opens with its point in one line. A sentence with a link is established by that source. A sentence marked **Inference** is my own reading, which the sources do not state. The problem and hypothesis names are the ones in [`problems.md`](https://github.com/danielrispler/Falinks/blob/660c11a/docs/studio/problems.md).

---

## Words used in this report

The same few terms come up in every tool, so they are defined once here.

- **Coding agent:** a program such as Claude Code or Codex that reads code, runs commands and edits files for you, in a loop, until a task is done.
- **Hook:** a script that the agent itself runs at fixed moments, for example before every tool call, when it needs permission, or when it stops. Claude Code documents its hooks and the events they receive. A supervision tool installs a hook to be told what the agent is doing.
- **Transcript (JSONL):** the log file in which Claude Code records each conversation, one JSON record per line, under `~/.claude/projects/`. The format is internal and can change between versions.
- **Screen scraping:** reading what an agent prints in its terminal and guessing its state from the text.
- **Git worktree:** a second working copy of the same repository, on its own branch. Parallel agents each get one so they do not edit the same files.
- **ADE (agentic development environment):** a desktop app built around running several agents at once, usually one per worktree, with a diff viewer and a terminal.
- **TUI:** a text user interface, an interactive app that runs inside a terminal.
- **Artifact (in Antigravity):** a structured deliverable the agent produces for a human, such as a plan, a task list, a screenshot or a browser recording.
- **Electron:** a way of building desktop apps out of web pages. Many tools here use it. Some users find it slow.

---

## 1. The four named tools, confirmed

All four exist and are actively maintained. Two of them have since changed shape, and a second, unrelated project uses the name "Pixel Agents".

| Asked as | Exact name and owner | Home page | Form factor | Last activity |
| --- | --- | --- | --- | --- |
| Pixel Agents | **Pixel Agents**, repo `pixel-agents-hq/pixel-agents` (the old `pablodelucca/pixel-agents` URL redirects there) | No website. The [repo](https://github.com/pixel-agents-hq/pixel-agents) and the [VS Code Marketplace listing](https://marketplace.visualstudio.com/items?itemName=pablodelucca.pixel-agents) | VS Code extension, plus a browser version started from the command line | v1.5.0 on 2026-10-17. About 9,900 stars |
| agents-observe | **Agents Observe**, repo [`simple10/agents-observe`](https://github.com/simple10/agents-observe) | No website. The dashboard runs locally at port 4981 | Claude Code plugin plus a local web dashboard | v0.9.12 on 2026-07-22, last push 2026-09-04. About 700 stars. One author wrote 876 of its commits ([contributors](https://github.com/simple10/agents-observe/graphs/contributors)) |
| Emdash | **Emdash** by General Action, repo [`generalaction/emdash`](https://github.com/generalaction/emdash) | [emdash.com](https://emdash.com) (emdash.sh redirects there) | Desktop app (an ADE) for macOS, Windows and Linux | v1.2.9 on 2026-10-23. About 6,100 stars. In Y Combinator's [Winter 2026 batch](https://www.ycombinator.com/companies/emdash) |
| Antigravity | **Google Antigravity** | [antigravity.google](https://antigravity.google) | Now a family of products: the Antigravity App (desktop), the Antigravity CLI (`agy`), editor extensions and an SDK. The original IDE is being phased down | App v2.26.0 on 2026-10-22 ([changelog](https://antigravity.google/docs/changelog/)) |

Two traps for the hands-on hour:

- **Another "Pixel Agents".** In September 2026 a second, unrelated project launched on Hacker News as ["Pixel Agents – A pixel-art mission control for your Claude Code agents"](https://news.ycombinator.com/item?id=49730005). It has since moved to `mateovalle/agent-campus` ([redirect note](https://github.com/mateovalle/pixel-agents)). The one in `problems.md` is almost certainly the far larger `pixel-agents-hq` project. **Inference**, from its size and its earlier launch date.
- **Antigravity is no longer one app.** At Google I/O on 2026-05-19, Google released [Antigravity 2.0](https://antigravity.google/blog/introducing-google-antigravity-2), "a completely separate desktop application" in which "there is no IDE". The [docs overview](https://antigravity.google/docs/overview/) calls this app the successor to the Agent Manager. The original IDE still exists, but Google plans to remove its Agent Manager in a future release ([same post](https://antigravity.google/blog/introducing-google-antigravity-2)).

---

## 2. The four named tools in depth

### 2.1 Pixel Agents: ambient and playful, now reliable for "needs me"

Pixel Agents turns each Claude Code session into an animated character in a small pixel-art office. It is good at catching your eye when an agent is waiting. It tells you nothing about what the agent changed.

**What it does.** Characters "walk to their desks, sit down, type when they're editing files, read when they're searching, and flag you visually when they're stuck waiting for input" ([README](https://github.com/pixel-agents-hq/pixel-agents)). It shows speech bubbles for permission requests, plays optional sounds when a turn ends, has an office layout editor, and shows sub-agents and Agent Teams as their own characters (same README).

**What it reads.** There are two paths ([README, "How it works"](https://github.com/pixel-agents-hq/pixel-agents)):

- *Hooks mode*, the default: a hook script receives Claude Code events such as `SessionStart`, `PreToolUse`, `PermissionRequest` and `Stop`, and forwards them to the Pixel Agents server. Installing it edits `~/.claude/settings.json`.
- *Heuristic mode*, the fallback: it scans the JSONL transcripts under `~/.claude/projects/`. It also reads transcripts in hooks mode, for details that the hook event lacks.

**What it does well.**

- It is the only tool in this survey designed to make waiting pleasant. **Inference:** it is the only one that touches the problem "Waiting is boring". It does so with entertainment, not learning.
- Its "needs me" signal became trustworthy over time. Its own February 2026 README listed this limitation: "Claude Code's JSONL transcript format does not provide clear signals for when an agent is waiting for user input or when it has finished its turn. The current detection is based on heuristics (idle timers, turn-duration events) and often misfires" ([README at commit 3f1067e](https://github.com/pixel-agents-hq/pixel-agents/blob/3f1067ebf8d4d12b974dcc5da2b20f56a8521850/README.md)). Version 1.3.0 then switched to hooks "for sub-second status updates, replacing filesystem polling" ([CHANGELOG](https://github.com/pixel-agents-hq/pixel-agents/blob/main/CHANGELOG.md)).
- It is very popular: about 9,900 stars in eight months (GitHub API).

**What it does badly.**

- It breaks when Claude Code's internal formats change. Issue [#93](https://github.com/pixel-agents-hq/pixel-agents/issues/93) reports that sub-agents vanished because Claude Code 2.1.71 stopped writing a transcript record that Pixel Agents relied on. Its most-discussed issue, [#7](https://github.com/pixel-agents-hq/pixel-agents/issues/7), is "Agents don't respond to claude code" (26 comments).
- It writes to your Claude settings. Version 1.4.1 had to add "Never overwrite user settings": the hook installer now refuses to rewrite `~/.claude/settings.json` when the file holds shapes it did not write, and makes a backup first ([CHANGELOG](https://github.com/pixel-agents-hq/pixel-agents/blob/main/CHANGELOG.md)).
- The browser version's URL carries an access token. The README warns: "Treat that URL as a secret: the token is a bearer capability", and it can end up in browser history ([README](https://github.com/pixel-agents-hq/pixel-agents)).
- It supports only Claude Code. Codex, Gemini and Cursor are on the roadmap ([README](https://github.com/pixel-agents-hq/pixel-agents)).
- Its game plans reward activity, not understanding. The roadmap's "Actually a game" stage lists "rate-limit and token-budget health bars, scores" ([README, Roadmap](https://github.com/pixel-agents-hq/pixel-agents)). **Inference:** this points toward the volume-rewarding game that `problems.md` wants to avoid.
- I found no independent reviews, on Hacker News or elsewhere, that judge whether it helps in daily work. Its stars show interest, not usefulness.

### 2.2 Agents Observe: the full event log, for debugging

Agents Observe records every hook event from Claude Code into a local database and shows the events as a live, searchable stream. It is a good flight recorder. It is not a calm overview.

**What it does.** It offers a live stream of tool calls (each call merged with its result), a tree showing which agent spawned which sub-agent, filters, full-text search, full event payloads, a session history with replay, and token and cost statistics ([README](https://github.com/simple10/agents-observe)).

**What it reads.** Hook events, sent by a small script to a local server, which stores them in SQLite and pushes them to a React dashboard over a live connection. It also reads the session transcript for session names and token counts ([README](https://github.com/simple10/agents-observe)). The README names Claude Code and Codex. A Codex adapter exists in the source ([`hooks/scripts/lib/agents`](https://github.com/simple10/agents-observe/tree/main/hooks/scripts/lib/agents)), but the README's roadmap still lists "Add support for Codex" as not done. It credits disler's [Multi-Agent Observability System](https://github.com/disler/claude-code-hooks-multi-agent-observability) as its inspiration.

**Its philosophy, in its own words.** "Tool calls are the ground truth. The assistant's text output is a summary. The actual tool calls … tell you what Claude is really doing" ([README, "Why observability matters"](https://github.com/simple10/agents-observe)). Google states the opposite view (section 2.4). The two views are compared in section 6.2.

**What it does well.**

- It answers "what exactly did the agent run?" better than any other tool here, including inside sub-agents.
- Its history persists across sessions, and its author argues that "Sessions are ephemeral, but patterns aren't" (README).

**What it does badly.**

- Setup is heavy: it needs Docker, Node.js and Bash, and "if docker, node, or bash are not installed on your host, the plugin will fail to properly load" ([README](https://github.com/simple10/agents-observe)). Issue [#27](https://github.com/simple10/agents-observe/issues/27) reports that it cannot yet run without Docker.
- An observer can break the agent it observes. Issue [#8](https://github.com/simple10/agents-observe/issues/8) reports that its `WorktreeCreate` hook "prevents **all** worktree creation" for anyone with the plugin enabled. Issue [#7](https://github.com/simple10/agents-observe/issues/7) reports that registering hook event names that Claude Code does not know made "the entire plugin's hooks fail to load". Both are closed.
- Its dashboard was once open to the whole network. Issue [#22](https://github.com/simple10/agents-observe/issues/22) found it listening on all network interfaces, with no login, while serving tool inputs and outputs. It was fixed in v0.9.12 by listening only on the local machine.
- It shows everything, so it is not calm. **Inference:** a raw event stream is the opposite of "a card of a few lines". This tool belongs behind a "show me the evidence" button, not on the main screen.
- It is maintained by essentially one person, and the last push was seven weeks before this research (GitHub API). I found no independent user reviews.

### 2.3 Emdash: a workbench for many agents, with review built in

Emdash is an open-source desktop app that runs many coding agents side by side, each in its own git worktree. It shows their status, and you review and merge their work in one place. It reduces window juggling. It leaves understanding to its diff view.

**What it does.** "Each task runs in its own Git worktree," so you can try several fixes, review the diffs and merge what works ([README](https://github.com/generalaction/emdash)). It pulls tasks from issue trackers (Linear, GitHub, Jira, GitLab, Asana and others), shows diffs from several agents side by side, monitors GitHub Actions checks, opens pull requests, runs agents on remote machines over SSH, and keeps sessions alive in tmux ([docs index](https://emdash.com/docs)). It supports many command-line agents: the README names Claude Code, Codex, Cursor, OpenCode, Amp, Devin, Qwen Code, Droid and GitHub Copilot "and more".

**What it reads.**

- Agent status comes from hooks that Emdash writes into each agent's configuration ([README](https://github.com/generalaction/emdash)). Issue [#1944](https://github.com/generalaction/emdash/issues/1944) lists exactly what it writes: `Notification` and `Stop` hooks into `.claude/settings.local.json`, a `notify` entry into `.codex/config.toml`, and plugins for OpenCode and Pi.
- The source turns these events into five states: idle, working, awaiting input, error and completed. Only a permission prompt, an idle prompt or a question from the agent counts as "awaiting input" ([`agent-events.ts`](https://github.com/generalaction/emdash/blob/e21cd3d84fd025546bdd4c4aef879336c7a91a20/apps/emdash-desktop/src/core/primitives/agents/api/agent-events.ts), [`agent-status-service.ts`](https://github.com/generalaction/emdash/blob/e21cd3d84fd025546bdd4c4aef879336c7a91a20/apps/emdash-desktop/src/main/core/agent-status/agent-status-service.ts)).
- Agents run inside an embedded terminal. The source also contains a runtime for the [Agent Client Protocol](https://agentclientprotocol.com), a standard way for an app to talk to an agent directly, without a terminal in between ([`packages/core/src/runtimes`](https://github.com/generalaction/emdash/tree/e21cd3d84fd025546bdd4c4aef879336c7a91a20/packages/core/src/runtimes)).
- App state lives in a local SQLite database. The README says Emdash does not send your code or chats to its own servers. Telemetry is optional.

**What it does well.**

- It addresses "Too many windows". On Hacker News, users liked switching agents "without juggling terminals", for example: "Super useful, much better to avoid flipping between agents" ([Show HN thread, Feb 2026](https://news.ycombinator.com/item?id=47140322)).
- It is very actively developed: over 10,000 commits and a release the day before this research (GitHub API).

**What it does badly.**

- It injected hook files into your project without a way to opt out. Issue [#1944](https://github.com/generalaction/emdash/issues/1944) noted that "there is currently **no setting that disables this injection**". It was closed as completed on 2026-09-01.
- It slows down at scale. One user reported: "I've loaded 78 tasks and the UI is crawling to a halt" ([comment](https://news.ycombinator.com/item?id=47151688)). Another noted that it is an Electron app ([comment](https://news.ycombinator.com/item?id=47148773)).
- Because it wraps terminals, its status is only as rich as the hooks: working, waiting or done. One commenter pointed out that it does not use "any agent SDK or --json outputs. You just embed a terminal window" ([comment](https://news.ycombinator.com/item?id=47148054)). The Agent Client Protocol runtime above may have changed this since February. I did not check how widely it is used.
- It does not address whether one human can review the output. The sharpest comment in its launch thread: "Are we really landing good PRs with 100% cognitive focus from 5-10 agents? Chances are, I'm making mistakes" ([comment](https://news.ycombinator.com/item?id=47142801)).

### 2.4 Google Antigravity: artifacts instead of logs, inside a closed product

Antigravity is Google's agent platform. Its central idea is that agents report through **artifacts**, such as plans, task lists, screenshots and browser recordings, so you review deliverables instead of raw tool calls. It is the tool closest to "Evidence, not promises" and "Small and visual before text". It is also the least open to being composed with other tools.

**What it does.**

- At launch in November 2025, the IDE had a "Manager Surface", "a dedicated interface where you can spawn, orchestrate, and observe multiple agents" ([Google Developers Blog, 2025-11-20](https://developers.googleblog.com/en/build-with-google-antigravity-our-new-agentic-development-platform/)).
- The 2.0 App, from May 2026, adds subagents, background tasks, scheduled tasks, projects that group conversations, isolated git worktrees, and JSON hooks. The hooks fire before and after each tool call, before and after each model call, and when the agent stops ([2.0 post](https://antigravity.google/blog/introducing-google-antigravity-2), [feature deep dive](https://antigravity.google/blog/google-io-2026-feature-deep-dive)).
- Its notification features are recent. The changelog shows Remote Control with push notifications in v2.9.1 and a Notifications settings page in v2.26.0 ([changelog](https://antigravity.google/docs/changelog/)).

**The artifact idea, in Google's words.** "Delegating work to an agent requires trust, but scrolling through raw tool calls is tedious." Artifacts let you check the agent's logic "at a glance", and you can comment on an artifact "similar to commenting on a doc", which the agent takes in "without stopping its execution flow" ([launch post](https://developers.googleblog.com/en/build-with-google-antigravity-our-new-agentic-development-platform/)). The docs list the artifact types as "rich Markdown plans (implementation plans), code diffs, architecture diagrams, images, and browser recordings", and describe review at milestones instead of watching every step ([Artifacts](https://antigravity.google/docs/artifacts/)).

**The walkthrough is an end-of-run recap.** "The agent creates walkthrough artifacts when it completes task implementation". It recaps the changes and "often contain[s] screenshots and screen recordings" ([Walkthrough](https://antigravity.google/docs/walkthrough/)). **Inference:** this is exactly the "arrives at the end" pattern that `problems.md` names as the sharpest pain, though richer in visuals than a markdown page.

**`/learn` teaches the agent, not you.** It turns a correction or a lesson from the session into a saved rule or skill "so future sessions benefit automatically" ([/learn](https://antigravity.google/docs/commands/learn/)).

**What it reads.** Its own agent's internal state, which only Google's apps show. **Inference:** the documented JSON hooks are the only route by which an outside control room could get Antigravity events. I did not verify what those hooks receive.

**What it does well.**

- It has the clearest public argument for trust through curated evidence. Vendor coverage and launch-thread comments praise the Agent Manager concept. One commenter predicted: "The core ideas of an Agent Manager view will be copied and improved by others" ([HN launch thread](https://news.ycombinator.com/item?id=45967814)).
- Its feedback does not block: you comment on a plan while the agent keeps working (launch post, above).

**What it does badly, from public reports.**

- **Background agents are rarely watched.** A security firm showed a prompt injection (instructions hidden in a web page the agent read) that made Antigravity read a `.env` file and send the credentials out through its browser. The firm argues that because "the majority of agents running at any given time will be running in the background", it is "highly plausible that an agent is not caught and stopped before it performs a malicious action". It also notes that the onboarding defaults let the model decide when a human review is needed ([PromptArmor](https://www.promptarmor.com/resources/google-antigravity-exfiltrates-data), [HN discussion](https://news.ycombinator.com/item?id=46048996)).
- **Autonomy without a brake.** A widely discussed December 2025 user report said that the agent, running in "Turbo" mode, deleted the contents of a whole drive while trying to clear a cache ([HN thread linking the Reddit post](https://news.ycombinator.com/item?id=46103532)). It is a single user's account, not confirmed by Google.
- **The product moves under its users.** A May 2026 post with 771 points on HN says the 2.0 update "nuked the IDE, the actual Antigravity I had been using for months", replaced it with "a single conversational prompt box", and wiped chat history and settings ([0xsid blog](https://www.0xsid.com/blog/antigravity-bait-n-switch)).
- **Terms of service risk for other tools.** An HN thread from September 2026 discusses accounts losing access for using Antigravity through third-party tools. It quotes a clause against "using the service in connection with products not provided by us". The thread does not settle the question ([HN](https://news.ycombinator.com/item?id=49548452)). **Inference:** this matters for any plan to read Antigravity data from a Falinks studio app, and for the map's "Productization" note.

---

## 3. Other tools in the same space

The space has four kinds of tool. Each one below was verified to exist through its repository or official docs, with its last activity date from the GitHub API on 2026-10-24. For xum, opcode, Sculptor, ccmanager, agent-deck and Omnara, I checked only the repository description and activity, not the docs or source.

### 3.1 First-party supervision views: the baseline is now built in

Since May 2026, all three big agent makers ship their own "which agent needs me" screen. Any v1 should measure itself against these first.

**Claude Code agent view** (Anthropic, research preview since v2.1.139, announced [2026-05-11](https://claude.com/blog/agent-view-in-claude-code)). This is the most relevant tool for Daniel, who already uses Claude Code. It is "one screen for all your background sessions: what's running, what needs your input, and what's done" ([docs](https://code.claude.com/docs/en/agent-view)). Rows are grouped as Pinned, Ready for review, Needs input, Working and Completed. "Ready for review" means a session has an open pull request that needs review or has failing checks. You can peek at a session's latest output or pending question, and reply without attaching to it. It sends a system notification when a session needs input, finishes or fails (same page). Details that matter for a control room:

- Each row's one-line summary "is generated by a Haiku-class model", a small fast model. It is refreshed at the end of each turn, and in between it is updated at most every 15 seconds from the session's own output (same page). **Inference:** the status line is itself a model's summary, a "promise" rather than "evidence" in the terms of `problems.md`.
- "`claude agents --json` is the supported way to read session state from outside Claude Code, for example from a status bar". The files under `~/.claude/jobs/` "are not a stable interface" (same page). This is the cleanest documented data source in the whole survey.
- Its documented limitations: rate limits multiply with parallel sessions, sessions stop if the machine shuts down, and deleting a session deletes its worktree (same page, "Limitations").

**Other Claude Code features that touch Daniel's problems.** None of these is a supervision screen, but each meets a problem in `problems.md`:

- *Session recap*: after you step away, "Claude Code shows a one-line recap of what happened in the session so far", capped at 400 characters ([docs](https://code.claude.com/docs/en/interactive-mode#session-recap)). It meets "I forget what happened".
- *`/insights`*: an HTML report on "what you work on, friction points such as misunderstood requests or buggy code, and suggestions for using Claude Code more effectively" ([docs](https://code.claude.com/docs/en/costs#analyze-your-usage-patterns)). It meets "I don't know whether I'm working well" and "knowing what to use when".
- *Output styles*: Explanatory adds short "Insight" blocks explaining the choices behind the code, and Learning also leaves small pieces of code for you to write ([docs](https://code.claude.com/docs/en/output-styles)). **Inference:** this is the only built-in feature I found that explains a change while the work happens, the core of "Move learning into waiting time". It lives inside the chat transcript, not in a separate calm surface, and it is not designed for a tired reader.
- The *desktop app* runs parallel sessions with a visual diff view, line comments sent back to Claude, and CI status for pull requests ([docs](https://code.claude.com/docs/en/desktop)).
- An official page compares the five ways to run Claude in parallel and says how to check on each ([Run agents in parallel](https://code.claude.com/docs/en/agents)). **Inference:** it is a ready-made input for the "knowing which tool to use when" research in [#278](https://github.com/danielrispler/Falinks/issues/278).

**ChatGPT desktop app, formerly the Codex app** (OpenAI). "On July 9, the Codex app merged into the ChatGPT desktop app for macOS and Windows" ([What's new](https://learn.chatgpt.com/codex/whats-new)). It has an Activity view that "brings together chats you recently engaged with and work that needs your attention", a review pane for diffs with inline comments, and worktrees (same page). Its optional animated **pet** floats above other windows and shows Running, Needs input, Ready or Blocked, where Blocked means "A chat failed or encountered a system error" ([Pets](https://learn.chatgpt.com/codex/pets)). Notifications for finished turns, permissions and questions can each be switched on or off ([Notifications](https://learn.chatgpt.com/codex/notifications)). **Inference:** a large vendor shipping an ambient animated companion lends some support to the Pixel Agents style of signal.

**GitHub Agent HQ, "mission control"** (October 2025). "A unified command center that follows you wherever you work", meant to assign, steer and track agents across GitHub, VS Code, mobile and the CLI ([GitHub blog](https://github.blog/news-insights/company-news/welcome-home-agents/)). **Inference:** it is aimed mainly at cloud agents working on GitHub repositories, so it matters less for local Claude Code work.

### 3.2 Parallel-agent workbenches (ADEs): many tools, one shape

These are desktop apps that run one agent per worktree and add a diff viewer, a terminal and a pull-request flow. They differ mostly in polish and platform. All of them stop at "review the diff".

| Tool | Home | What stands out | Status on 2026-10-24 |
| --- | --- | --- | --- |
| Conductor | [conductor.build](https://www.conductor.build) | Mac app for parallel Claude Code, Codex and Cursor agents. "See at a glance what they're working on, then review and merge" (home page). By Charlie Holtz and Jackson de Campos, Y Combinator Summer 2024 ([YC](https://www.ycombinator.com/companies/conductor)). I found no public source code. AlternativeTo lists it as proprietary ([listing](https://alternativeto.net/software/conductor/about), a secondary source) | Active, according to its site |
| Superset | [superset.sh](https://superset.sh), [repo](https://github.com/superset-sh/superset) | "Agent Monitoring: track every agent from the sidebar, with working indicators, completion chimes, and dock badges". Installs its own status hooks ([agent integration docs](https://docs.superset.sh/agent-integration)). Has an iPhone app | desktop-v1.40.0 on 2026-10-23. About 15,500 stars |
| Vibe Kanban | [repo](https://github.com/BloopAI/vibe-kanban) | A kanban board for planning, then workspaces for agents. Its pitch: engineers "spend most of their time planning and reviewing coding agents" (README) | **Sunsetting.** The company behind it shut down on 2026-04-10. The project continues as "open source and community maintained" ([announcement](https://www.vibekanban.com/blog/shutdown)) |
| Nimbalyst (formerly Crystal) | [nimbalyst.com](https://nimbalyst.com), [repo](https://github.com/nimbalyst/nimbalyst) | Visual editors for markdown, mockups, Mermaid and Excalidraw diagrams. Red/green review inside the rendered document. A session kanban. An iOS app that pushes "when an agent is waiting" (README). Crystal's repo now says "Crystal is now Nimbalyst" ([stravu/crystal](https://github.com/stravu/crystal)) | Pushed 2026-10-23. About 2,000 stars |
| xum (formerly Coder's mux) | [repo](https://github.com/coder/xum) | "A desktop app for isolated, parallel agentic development" (repo description). The old `coder/mux` URL redirects here | Pushed 2026-10-24. About 2,100 stars |
| opcode (formerly Claudia) | [opcode.sh](https://opcode.sh), [repo](https://github.com/winfunc/opcode) | GUI for Claude Code sessions, custom agents and background agents (repo description) | Pushed 2026-09-18. About 22,400 stars |
| Sculptor | [repo](https://github.com/imbue-ai/sculptor) | "Build product with grounded, parallel coding agents" (repo description) | Pushed 2026-10-23. About 240 stars |

**Inference:** Nimbalyst is the closest to "Small and visual before text", because it shows a change inside a rendered document or diagram instead of as raw lines. It still shows the change after the fact, not while it happens.

### 3.3 Terminal-first tools: for people who refuse a new app

These keep the terminal as the main surface and add a sidebar, notifications or a session list.

- **cmux** ([cmux.com](https://cmux.com), [repo](https://github.com/manaflow-ai/cmux)). A native macOS terminal built on Ghostty. When an agent waits, "its pane gets a blue ring and the tab lights up in the sidebar", and a sidebar shows each workspace's git branch, pull request status and "the latest notification text". It was built because "Claude Code's notification body is always just 'Claude is waiting for your input' with no context". It reads terminal notification escape sequences, special codes a program prints to ask the terminal for a notification, and offers a command that agents' hooks can call. Its stance: "cmux is a primitive, not a solution" ([README](https://github.com/manaflow-ai/cmux)). v0.65.2 on 2026-10-16. About 28,400 stars.
- **Claude Squad** ([repo](https://github.com/smtg-ai/claude-squad)). A TUI that runs agents in tmux sessions, each in its own worktree. Its status detection is screen scraping. It hashes the pane's text to see whether anything changed, and it detects a permission prompt by searching for the literal text "No, and tell Claude what to do differently" ([`tmux.go`](https://github.com/smtg-ai/claude-squad/blob/ce1ffb4392b01f38e2c4599c7c84d2a93973b138/session/tmux/tmux.go)). v1.0.20 on 2026-08-20. About 8,700 stars.
- **ccmanager** ([repo](https://github.com/kbwo/ccmanager)) and **agent-deck** ([repo](https://github.com/asheshgoplani/agent-deck)). Session-manager TUIs for many command-line agents. Both pushed in October 2026.

### 3.4 Observers, remote control, and usage counters

- **Multi-Agent Observability System** by disler ([repo](https://github.com/disler/claude-code-hooks-multi-agent-observability)). The project agents-observe credits as its inspiration: hook events go to a server, then SQLite, then a live web view. It asks for an `ANTHROPIC_API_KEY` in its setup (README). Last pushed 2026-02-08, and its repo shows no license.
- **Happy** ([happy.engineering](https://happy.engineering), [repo](https://github.com/slopus/happy)). Started as a phone client for Claude Code and Codex, with end-to-end encryption. It is now growing into its own multi-model harness and desktop app (README). Pushed 2026-10-23.
- **Omnara** ([repo](https://github.com/omnara-ai/omnara)). Once a mobile "command center" for agents, it now describes itself as "the open-source managed agent platform" (repo description). An example of how fast tools change direction.
- **ccusage** ([ccusage.com](https://ccusage.com), [repo](https://github.com/ccusage/ccusage)). Reads local usage data from Claude Code, Codex, OpenCode and others, and prints daily and per-session token and cost reports (README). About 19,200 stars.
- **OpenTelemetry export.** Claude Code can export metrics, events and traces in the OpenTelemetry format, an open standard that monitoring systems accept ([Monitoring docs](https://code.claude.com/docs/en/monitoring-usage)). It suits team dashboards. It is mentioned here only as a data source; [#282](https://github.com/danielrispler/Falinks/issues/282) covers data sources.

---

## 4. Problem-by-problem coverage

Status ("needs me") is well covered. "Stuck or progressing" and comprehension are barely covered.

The ratings are my **inference**, from the docs and source cited above. None comes from measuring the tools.

| Problem in `problems.md` | Who addresses it | How well |
| --- | --- | --- |
| What the agents are doing now, which phase | Every tool. Richest: agents-observe (each tool call). Calmest: agent view rows, cmux sidebar | Good for "doing". Weak for "phase": no tool knows a task's plan, except Antigravity's task-list artifacts |
| Progressing or stuck | Nobody, really. "Working" stays on until the agent stops. Blocked in Codex means failed ([Pets](https://learn.chatgpt.com/codex/pets)). Agent view shows how long a blocked session has waited (agent view docs, version history v2.1.207) | **Gap.** I found only small single-purpose loop detectors in a web search. None is part of the tools above |
| Missing the moment an agent waits for approval | Agent view, Codex notifications, Antigravity notifications, Emdash, Superset, cmux, Pixel Agents bubbles, and phone pushes from Happy, Nimbalyst and Superset | **Solved,** and mostly through hooks |
| Waiting is boring, nothing to do | Pixel Agents (entertainment). The Codex pet, somewhat | Only as entertainment. No tool offers learning while you wait |
| Not understanding what changed | Diff viewers with comments (Claude desktop, Codex, Conductor, Emdash, Superset, Vibe Kanban). Antigravity walkthroughs with screenshots. Nimbalyst's rendered diffs | Covered as end-of-run review, which is the pattern `problems.md` calls the sharpest pain |
| Hard to review, unsure whether to trust | CI-check display (agent view's "Ready for review", Conductor, Emdash, Claude desktop). Antigravity artifacts. agents-observe raw events | Partial. Checks that passed are shown. What the agent was refused or skipped is not gathered anywhere |
| Forgetting previous runs | Session history (agents-observe, Emdash, Nimbalyst). Claude Code's session recap is for the current session only | Weak. History is stored, but not summarized across runs |
| Project moving faster than my understanding | Claude Code Explanatory and Learning styles, partially | **Gap** |
| Hard to write a good task | Antigravity `/grill-me`, kanban planning tools (Vibe Kanban, Nimbalyst), issue-tracker import (Emdash) | Partial |
| Am I working well, am I improving | Claude Code `/insights`. ccusage, for cost only | Partial, and only from Anthropic |
| Too many windows | ADEs (Emdash, Superset, Conductor, Nimbalyst), cmux, the Claude desktop app's panes | Good: this is what the ADE category exists for |
| Knowing what to use when | Claude Code `/insights` suggestions. The official "Run agents in parallel" comparison page | **Gap,** beyond Anthropic's own feature tips |

---

## 5. What the tools read to know what agents do

Every tool uses one of five sources. The field has moved from guessing (transcripts, screen text) to being told (hooks, official status commands), because guessing broke.

| Source | Used by | Strength | Weakness |
| --- | --- | --- | --- |
| Hook events | Pixel Agents (default), agents-observe, disler's tool, Emdash, Superset, cmux (through its notify command) | Fast and exact for start, tool call, permission and stop | The tool must edit the agent's settings, and a faulty hook can break the agent ([agents-observe #8](https://github.com/simple10/agents-observe/issues/8), [#7](https://github.com/simple10/agents-observe/issues/7)) |
| Transcript files (JSONL) | Pixel Agents (fallback and details), agents-observe (names, tokens), ccusage, `/insights` | Full history, including what the model said | An internal format that changes without notice ([Pixel Agents #93](https://github.com/pixel-agents-hq/pixel-agents/issues/93)). "Does not provide clear signals" for waiting or done ([early README](https://github.com/pixel-agents-hq/pixel-agents/blob/3f1067ebf8d4d12b974dcc5da2b20f56a8521850/README.md)) |
| Terminal screen text | Claude Squad (pane hash and literal prompt text), cmux (escape sequences) | Works with any terminal agent | Breaks whenever the agent's wording changes ([`tmux.go`](https://github.com/smtg-ai/claude-squad/blob/ce1ffb4392b01f38e2c4599c7c84d2a93973b138/session/tmux/tmux.go)) |
| The vendor's own state | Agent view, the ChatGPT/Codex app, the Antigravity App, the Claude desktop app | Richest: knows pull requests, worktrees and summaries | Closed. Only agent view documents an outside interface (`claude agents --json`) |
| Git and GitHub | Agent view's pull request badge, cmux sidebar, Conductor checks, Emdash CI view | Stable, and evidence rather than claims | Says nothing about the agent's reasoning |

**Inference:** a v1 that reads hooks plus `claude agents --json` plus git would stand on the three most stable sources. Transcripts are useful for depth, but should be treated as a format that may change. Depth on this belongs to [#282](https://github.com/danielrispler/Falinks/issues/282).

---

## 6. Cross-cutting lessons

### 6.1 "Needs me" is solved. "Stuck" and "understood" are not.

Every serious tool can now tell you that an agent is waiting. **Inference:** the scarce things are (a) knowing whether a "working" agent is making progress and (b) understanding the change. These are the two areas where `problems.md` puts its sharpest pains.

Practitioners say review is the bottleneck. Simon Willison: "the natural bottleneck on all of this is how fast I can review the results" ([2025-10-05](https://simonwillison.net/2025/Oct/5/parallel-coding-agents/)). Vibe Kanban's README makes the same point, and so does the HN comment quoted in section 2.3.

### 6.2 Two theories of trust: raw events or curated artifacts

agents-observe says "Tool calls are the ground truth" ([README](https://github.com/simple10/agents-observe)). Google says "Verify with Artifacts, not logs", because "scrolling through raw tool calls is tedious" ([launch post](https://developers.googleblog.com/en/build-with-google-antigravity-our-new-agentic-development-platform/)).

**Inference:** both are right about different moments. Curated artifacts suit the glance. Raw events suit the investigation when something looks wrong. "Evidence, not promises" needs both layers, and a curated artifact written by the agent is still the agent's own claim unless checks back it up. Falinks' engine evidence (what was checked, passed and refused) would be a third kind, produced by something other than the agent itself.

### 6.3 Observers change what they observe, and they hold sensitive data

Tools that install hooks have broken agents (agents-observe [#8](https://github.com/simple10/agents-observe/issues/8)), rewritten settings files (Pixel Agents v1.4.1 [CHANGELOG](https://github.com/pixel-agents-hq/pixel-agents/blob/main/CHANGELOG.md)), and written into project folders without a way to opt out (Emdash [#1944](https://github.com/generalaction/emdash/issues/1944)). Dashboards have exposed tool inputs and outputs to the network (agents-observe [#22](https://github.com/simple10/agents-observe/issues/22)) or depended on a secret in the URL (Pixel Agents README).

**Inference:** a Falinks control room should read without writing where it can, and listen only on the local machine by default. This also bears on the map's open item "Daniel's own data".

### 6.4 The category is being absorbed by vendors, and third-party tools churn

In 2026 alone:

- Anthropic shipped agent view (May).
- Google replaced the Antigravity IDE with a standalone app (May).
- OpenAI folded the Codex app into ChatGPT and added an Activity view and pets (July to September).
- Bloop shut down the company behind Vibe Kanban (April).
- Crystal became Nimbalyst, Coder's mux became xum, and Omnara changed its mission.

All of this is cited in section 3. **Inference:** composing v1 "mostly from existing tools" is safest when it means composing from documented interfaces (hooks, `claude agents --json`, git, OpenTelemetry), not from a particular third-party app that may be gone next year.

### 6.5 Form factors in use

| Form factor | Examples |
| --- | --- |
| Editor panel | Pixel Agents (VS Code), Antigravity IDE extensions |
| Local web dashboard | agents-observe, disler's tool, Pixel Agents standalone |
| Desktop ADE | Emdash, Superset, Conductor, Nimbalyst, xum, Antigravity App, ChatGPT/Codex app, Claude desktop |
| Terminal | Claude Code agent view, Claude Squad, ccmanager, agent-deck, cmux (a terminal app itself) |
| Floating companion | Codex pet |
| Phone | Happy, Superset, Nimbalyst, Antigravity Remote Control |

**Inference:** nobody has built the "serious control room" as a calm, glanceable second screen. The closest are agent view (a list) and Pixel Agents (a game). This bears on the form-factor research still to come.

---

## 7. What the tools say about each hypothesis

Tools show what builders tried and users starred, not what works for a tired human. So these are signals for the hypotheses, not verdicts.

- **Move learning into waiting time.** No supervision tool does this. The nearest is Claude Code's Explanatory and Learning styles, which explain as the work happens but inside the transcript ([output styles](https://code.claude.com/docs/en/output-styles)). Antigravity's walkthrough comes at the end ([Walkthrough](https://antigravity.google/docs/walkthrough/)). Verdict signal: **untested, open ground**.
- **Small and visual before text.** Partly present. Agent view compresses each session into one model-written line. Antigravity offers screenshots and recordings. Nimbalyst renders diffs inside documents and diagrams. Pixel Agents and the Codex pet are purely visual status. None of these is a small visual explanation of a change. Signal: **mixed**. The visual layer exists for status and is missing for understanding.
- **Non-blocking.** The inbox pattern is now standard for attention: agent view's "Needs input" and "Ready for review" groups ([docs](https://code.claude.com/docs/en/agent-view)), and Codex's Activity view ([What's new](https://learn.chatgpt.com/codex/whats-new)). Antigravity's comments do not stop the agent (launch post). No tool queues *things not yet understood*. Signal: **supports** the pattern. Applying it to comprehension is untried.
- **Guessing, not testing.** No tool found. Claude Code's Learning style asks you to write code, which is doing, not predicting. Signal: **untested**.
- **Evidence, not promises.** The strongest vendor convergence: CI checks on pull requests everywhere, Antigravity's artifacts argument, and agents-observe's raw tool calls. What was *refused* or *skipped* is shown nowhere. Signal: **supports**, with a clear gap that Falinks could fill.
- **A game that rewards understanding, not volume.** The only game-like tool, Pixel Agents, plans health bars for rate limits and token budgets, which reward activity ([roadmap](https://github.com/pixel-agents-hq/pixel-agents)). Signal: **untested**. The existing example leans the wrong way.
- **Serious control-room style.** The vendors use the vocabulary: "mission control", "command center", "Manager". The most-starred visual monitor, though, is the playful one, with about 9,900 stars against about 700 for the serious event dashboard (GitHub API). Stars measure curiosity, not daily value. Signal: **mixed**.
- **Learning content while waiting.** No tool found. Signal: **untested**.

---

## Open questions

These are the questions the research could not settle, and what each means for the map's later decisions.

1. **Do status displays help people at all?** No tool publishes usage data or a study of its users. GitHub stars and HN comments show interest, not benefit. *For the hypothesis verdicts:* these signals stay weak until Daniel's own hands-on hour ([#284](https://github.com/danielrispler/Falinks/issues/284)) and the human-factors research add evidence.
2. **Can "stuck" be detected from the data available?** No tool tries beyond "working" and "failed". Whether hooks plus git activity are enough to tell slow progress from a loop is unknown. *For data sources ([#282](https://github.com/danielrispler/Falinks/issues/282)):* this is a concrete question to carry over.
3. **How stable is `claude agents --json`?** It is documented as the supported interface, but agent view is a research preview whose "interface and keyboard shortcuts may change" ([docs](https://code.claude.com/docs/en/agent-view)). *For v1:* building on it is the cheapest way to answer "which agent needs me". The risk is churn, not removal. That risk is my inference.
4. **Can a third-party app read Antigravity or Codex activity?** Antigravity has JSON hooks, and Codex has a `notify` setting ([notifications](https://learn.chatgpt.com/codex/notifications)). I did not check what data they carry or what the terms of service allow; the ToS thread above is unresolved. *For data sources and productization:* v1 may have to be Claude-Code-first.
5. **What does Daniel actually feel with these tools?** User reports for Pixel Agents and agents-observe are nearly absent. *For the main-job decision:* the hands-on hour is the only first-hand evidence on the calm-versus-noise question.
6. **Main job (inference, not a decision).** "Which agent needs me" is already free and built in. So the control room's distinctive job may be comprehension: what changed, why, and whether to trust it, in small pieces. Monitoring would then be a composed-in layer. This is offered as input to the first decision ticket, not as its answer.

---

## Optional checklist for the hands-on hour (#284)

This is a suggestion only. [#284](https://github.com/danielrispler/Falinks/issues/284) says that using it is a choice.

**Before you start (5 minutes).**

- Pick two to four tools. A balanced set:
  - **Claude Code agent view**, the baseline you already have: run `claude agents`.
  - **Pixel Agents**, playful and ambient.
  - **One workbench**: Emdash, or Superset on a Mac.
  - **The Antigravity App**, for the artifacts idea.
  - agents-observe only if Docker is already installed.
- Know what each tool changes on your machine:
  - Pixel Agents' hooks edit `~/.claude/settings.json`, and it backs the file up first.
  - agents-observe installs a Claude Code plugin with hooks and runs a Docker container.
  - Emdash writes hook files into each worktree's `.claude/settings.local.json`.
  - Antigravity needs a Google account sign-in.
- Per #284's rule 4, skip any tool that asks for another CLI's token or API key, and note it. disler's observability system asks for an `ANTHROPIC_API_KEY`.
- Use a throwaway repository or branch, not Falinks `main`. Avoid "skip permissions", "auto-accept" or "Turbo" modes (see section 2.4).

**One small scenario to repeat in each tool (10 to 15 minutes per tool).** Give the agent a small, real task that needs at least one permission prompt and takes a few minutes. Then note:

1. **Glance test.** Look away for two minutes, then look back for five seconds. Can you say what the agent is doing and which phase it is in? *(Blind during a run)*
2. **Approval test.** When the agent asked for permission, how did you find out, and how long did it take? Did the alert say *what* it wanted? *(Missing the approval moment)*
3. **Stuck test.** Could you tell "working hard" from "going in circles"? What would you have needed to see? *(Progressing or stuck)*
4. **Waiting test.** What did you do while it worked? Did the tool give you anything worth doing? *(Waiting is boring)*
5. **Two-sentence test.** When it finished, explain the change in two sentences *before* opening the diff. What did the tool give you to make that possible? *(Comprehension debt)*
6. **Trust test.** What evidence did you see: tests run, checks passed, things refused? Did you believe it? *(Evidence, not promises)*
7. **Exam test.** Did anything feel like an exam or block you from moving on? *(The sharpest pain)*
8. **Window count.** How many windows or tabs did you need? *(Too many windows)*

**After each tool (1 minute).** Write one line on what worked, one on what bothered you, and one on the problems it touched. Add one word for how it felt: calm, noisy, fun or tiring.

**At the end (5 minutes).** Which tool would you keep open tomorrow, and for which single job? That answer is direct evidence for the decision on the control room's main job.
