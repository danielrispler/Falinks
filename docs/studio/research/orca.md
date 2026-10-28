# Orca, the agent IDE

> **Answer:** Orca, by Stably AI (`stablyai/orca`), is the largest parallel-agent workbench. It is one desktop app with a terminal per agent, a git worktree per task, a browser, diffs, pull requests, usage limits, agent-to-agent orchestration and a phone app. It answers "which agent needs me" and "too many windows" better than any tool in the baseline report. It does nothing to help Daniel understand a change while it happens, and by default it starts agents with permission prompts switched off.
> **Evidence:** Moderate for what Orca does: I read its docs and source at one pinned commit, and its issue tracker. Weak for whether it helps: a few Hacker News comments and a Better Stack guide, no study. I could not get the video's transcript, only its description and chapter titles.
> **Hypotheses:** Supports the calm side of *Serious control-room style* (colour only on "Needs You" and "Done") and *Non-blocking* (mark-unread, a catch-up feed), for attention only. Partly supports *Evidence, not promises* (marks on AI-written lines, PR checks). Says nothing on *Move learning into waiting time*, *Guessing, not testing*, *A game that rewards understanding, not volume* or *Learning content while waiting*, and its "Ship 100x" pitch pulls toward volume.
> **For Daniel:** [The video](https://youtu.be/8QgBcI40VC8), 7 min; the chapters "Everything is a terminal" (0:37) and "Orchestration across Claude and Codex" (3:47) matter most. Orca's [Agents & sessions](https://www.onorca.dev/docs/model/agents-sessions) page, 5 min: status glyphs, the Agent Dashboard and the permission default.

Research date: 2026-10-28. Star counts, issue counts and release dates come from the GitHub API on that day. Orca's docs and source are read at commit [`0a349b3`](https://github.com/stablyai/orca/tree/0a349b3197db2aceafdd74d988f1641265adaf2c), the head of `main` that day. Ticket: [#318](https://github.com/danielrispler/Falinks/issues/318). Map: [#266](https://github.com/danielrispler/Falinks/issues/266). Baseline: the [existing-tools report](https://github.com/danielrispler/Falinks/blob/fe647d25890d11e443d606596d3e9890feeab013/docs/studio/research/existing-tools.md) ("baseline" below). This report extends it and does not repeat it.

**How to read this.** Each section opens with its point in one line. A sentence with a link is established by that source. A sentence marked **Inference** is my own reading, which the sources do not state. A sentence marked **Video only** comes from the video's description or chapter titles and nowhere else. The problem and hypothesis names are the ones in [`problems.md`](https://github.com/danielrispler/Falinks/blob/660c11a/docs/studio/problems.md). The main job Orca is judged against is the [#285 resolution](https://github.com/danielrispler/Falinks/issues/285#issuecomment-6412880294): keep Daniel's understanding in step with the agents' work, during the work.

---

## Words used in this report

The baseline defines coding agent, hook, transcript, git worktree, ADE and Electron. Four more terms come up here.

- **OSC sequence:** a hidden code a program prints into its terminal to tell the terminal something, such as a window title. The user never sees it.
- **Program Status Protocol (OSC 7501):** an open terminal protocol in which a program reports its own state: idle, working, done, blocked or error ([spec](https://www.superlogical.com/rex/docs/build/program-status)). Claude Code supports it since 2.1.295 (section 5).
- **Bypass mode ("Yolo"):** running an agent with its permission prompts switched off, for Claude Code the `--dangerously-skip-permissions` flag.
- **Orchestration:** one agent (the coordinator) hands tasks to other agents (workers) and collects their results.

---

## 1. Which Orca this is

The video's Orca is the open-source app from Stably AI. Several products share the name, so the identity was checked link by link.

- The video's description links only to `https://www.onorca.dev` (YouTube page, read 2026-10-28).
- onorca.dev's docs point to the repository `github.com/stablyai/orca`, and its footer names Stably AI ([docs home](https://www.onorca.dev/docs)). The repository's README links back to onOrca.dev ([README](https://github.com/stablyai/orca/blob/0a349b3197db2aceafdd74d988f1641265adaf2c/README.md)). The repository's homepage field, `orca.dev`, redirects to onorca.dev.
- Not this Orca: the GNOME screen reader, which owns `/usr/bin/orca` on Linux, and an unrelated Plotly Homebrew cask called `orca`. Orca's own install docs warn about both ([Install](https://www.onorca.dev/docs/install)).

**Basic facts** (GitHub API, 2026-10-28, unless linked):

| Fact | Value |
| --- | --- |
| Maker | Stably AI, San Francisco, "Backed by Y Combinator" ([home page](https://www.onorca.dev/)). Y Combinator lists Stably AI in its Winter 2022 batch, founders Jinjing Liang and Neil Parker ([YC](https://www.ycombinator.com/companies/stably)) |
| Licence and price | MIT, free. An Enterprise page offers rollout help and self-hosting, with no price listed ([Enterprise](https://www.onorca.dev/enterprise)) |
| Repository | Created 2026-03-17. 108,406 stars, 6,848 forks. 4,151 open issues and 3,803 open pull requests |
| Releases | 990 GitHub releases from 2026-03-21 to 2026-10-28, counting release candidates and mobile builds. 22 stable desktop releases in the last 30 days. Latest: v1.4.243 on 2026-10-28 |
| Self-description at launch | "Show HN: Open Source 'Conductor + Ghostty'", 2026-03-27, 21 points ([HN](https://news.ycombinator.com/item?id=47549197)) |

**Inference:** by stars, Orca is almost four times cmux, the largest tool in the baseline (about 28,400). The baseline's ADE table (its section 3.2) missed the category's largest member.

---

## 2. The video: what I could read

I could not get the transcript. I read the title, description and chapter list, and checked each feature they name against Orca's own sources.

**What failed.** The page has an English auto-generated caption track, but downloading it returned an empty file. My session's permission policy blocked a call to YouTube's transcript endpoint. Two public transcript sites returned an error or only their landing page. `yt-dlp` is not installed, and I installed nothing.

**What I have.** "The Best Agent IDE I've ever used (Orca)", Better Stack, published 2026-10-23, 7 min 5 s, 111,567 views on 2026-10-28. The description, quoted: "Orca is a free, open-source app that runs Claude Code, Codex and any other coding agent side by side, with git worktrees, a built-in browser, usage tracking and multi-agent orchestration wrapped around a plain terminal. It's the biggest improvement to my agent workflow I've seen. If you juggle more than one coding agent, give it a try."

| Chapter | What Orca's own sources say | Checked |
| --- | --- | --- |
| 0:00 Every coding agent in one window | "Works with any CLI agent — if it runs in a terminal, it runs in Orca." The README names 34 agents ([README](https://github.com/stablyai/orca/blob/0a349b3197db2aceafdd74d988f1641265adaf2c/README.md)) | Yes |
| 0:37 Everything is a terminal | An agent session is "one CLI agent running in one terminal in one worktree" ([Agents & sessions](https://www.onorca.dev/docs/model/agents-sessions)). An experimental Chat UI can also talk to some agents directly, without a terminal ([Chat UI](https://www.onorca.dev/docs/agents/native-chat)) | Yes, with that exception |
| 1:39 Worktrees and usage tracking | One worktree per task. Usage and rate-limit resets for ten providers in the status bar ([Usage](https://www.onorca.dev/docs/agents/usage-tracking)) | Yes |
| 2:32 Built-in browser and annotations | Design Mode: click a UI element in a built-in Chromium window to send its HTML, CSS and a screenshot into the agent's prompt (README) | Yes, from the README only |
| 3:18 Git, PRs and file previews | GitHub, Linear and Jira in the app; Markdown, PDF and image previews (README) | Yes |
| 3:47 Orchestration across Claude and Codex | Runs, tasks, workers, messages and decision gates, behind an Experimental setting ([Orchestration](https://www.onorca.dev/docs/cli/orchestration)) | Yes. It is experimental |
| 5:18 Why I switched from Herdr | **Video only.** The presenter used Herdr before Orca. Section 8 covers Herdr | Not checkable |
| 5:42 The mobile app | iOS and Android companion, in beta ([Mobile](https://www.onorca.dev/docs/mobile)) | Yes |
| 5:56 Should you switch? / 6:12 Everything else | **Video only.** Content unknown | No |

"It's the biggest improvement to my agent workflow I've seen" is the presenter's opinion (**video only**). I do not know what the video says about weaknesses, the permission default, or anything beyond these titles.

Better Stack also published a written guide by Stanley Ulili, updated 2026-10-26 ([guide](https://betterstack.com/community/guides/ai/orca-ai/)). It does not mention the video or Herdr, so I treat it as a separate secondary source. Its limits section is useful: worktrees do not isolate ports, databases or services; parallel implementations add cost and review time; and "two agents producing code does not show that either implementation is correct" (paraphrase from a summarising fetch, not a verbatim read).

---

## 3. What Orca is, in one screen

Orca is an ADE in the baseline's sense, with more attached: orchestration, a phone app, remote servers and a CLI that agents call.

- **Pitch.** "Ship 100x with the agent IDE" ([home page](https://www.onorca.dev/)); "The AI Orchestrator for 100x builders" (README). The docs say it is for people who "read diffs, care about commits, and keep a worktree tidy" ([What is Orca?](https://www.onorca.dev/docs)).
- **Signature move.** "Running the same task across multiple agents in parallel is Orca's killer move. Same prompt, three branches, pick the winner." Its argument: "Where three agents agree, the answer is probably right. Where they split, you've found the hard part" ([recipe](https://www.onorca.dev/docs/recipes/parallel-agents)).
- **Status.** Glyphs on every agent tab: spinner (working), amber question mark (waiting on you), emerald (done), red (blocked, interrupted or failed), grey (idle) ([Agents & sessions](https://www.onorca.dev/docs/model/agents-sessions)).
- **Agent Dashboard** (Experimental). A board with the columns Needs You, Working, Done and Idle. Idle holds "quiet agents that have not reported completion for about 30 minutes" and is hidden by default. Cards show the last message or the task summary. "Needs You cards tint amber …; Done cards tint green; other states stay neutral so tint means 'look here'" (same page).
- **Notifications.** A system notification, sound and chip when an agent goes from working to idle; a header bell; a Dock badge; and "mark unread" to come back later ([Notifications](https://www.onorca.dev/docs/notifications)). An **Agents feed** lists finished turns, blocking questions and reply previews across all worktrees as "the catch-up surface when you've been away" ([Agents feed](https://www.onorca.dev/docs/activity)).
- **Review.** Annotate AI Diff: comment on any diff line, then send all comments to the agent as one batch, because "sending comments one at a time causes the agent to swing back and forth" ([Annotate AI Diff](https://www.onorca.dev/docs/review/annotate-ai-diff)). **Attribution** marks AI-written lines in the diff gutter, and human edits flip a line back to human. It stays local and is not committed ([Attribution](https://www.onorca.dev/docs/review/attribution)).
- **Agent-written status.** Each worktree has a free-text comment that agents update through `orca worktree set --comment …`. Orca recommends this "for keeping human collaborators in the loop without forcing chat" ([Worktree checkpoints](https://www.onorca.dev/docs/cli/worktree-checkpoints)).
- **History.** Orca scans the transcripts that Claude, Codex and other CLIs leave on disk, lists past sessions across agents, and resumes one in a click ([Session history](https://www.onorca.dev/docs/agents/session-history)).
- **Permission default.** "Orca launches every supported agent with its full-autonomy permission flag pre-applied — Claude with `--dangerously-skip-permissions`, Codex with `--dangerously-bypass-approvals-and-sandbox`, Gemini with `--yolo` … The intent is that the worktree itself is the sandbox" ([Agents & sessions](https://www.onorca.dev/docs/model/agents-sessions)). Settings → Agents offers **Manual** "to keep each agent's own approval flow" ([Settings](https://www.onorca.dev/docs/settings)). Claude Code's own docs warn that bypass mode skips prompts "including for writes to protected paths such as `.git` and `.claude`", and say to use it "only … in isolated environments like containers or VMs" ([permissions](https://code.claude.com/docs/en/permissions)). **Inference:** a git worktree is not a sandbox; the agent still runs as Daniel on the same machine.

---

## 4. How it ships and installs

Orca is an Electron desktop app with a native installer, plus a phone app and an optional headless server. It is not an `npx` web app and not an editor extension. Input for [#319](https://github.com/danielrispler/Falinks/issues/319).

- **Desktop.** Electron 43.8.1 (`package.json` at the pinned commit). The docs say macOS builds are signed and notarized, and may still ask for confirmation "that's normal for Electron-based apps" ([Install](https://www.onorca.dev/docs/install)).
- **Packages.** macOS `.dmg` (Apple Silicon and Intel), Windows `.exe`, Linux AppImage, `.deb` and `.rpm` for x64 and arm64. Homebrew `brew install --cask stablyai/orca/orca`; AUR `stably-orca-bin` (README, Install).
- **Updates.** Auto-update on the stable channel. Release candidates ship "often daily", reached by Shift-clicking Check for Updates. The AppImage updates itself; `.deb` and `.rpm` hand you a copy-paste install command (Install).
- **First launch.** It asks for access to the home directory, and offers to import `~/.claude`, `~/.codex` and Ghostty settings (Install).
- **CLI.** `orca` (`orca-ide` on Linux, because the screen reader owns `orca`). Agents running inside Orca call it to create worktrees, drive the browser and report progress ([CLI reference](https://www.onorca.dev/docs/cli/reference)).
- **Phone.** iOS App Store and an Android APK, both in beta. Pairing goes through Orca Relay, which needs an Orca account sign-in, or over the local network ([Mobile](https://www.onorca.dev/docs/mobile)). Phone and desktop never connect directly through Relay. Each opens an outbound connection to a relay server that splices them ([cloud/README](https://github.com/stablyai/orca/blob/0a349b3197db2aceafdd74d988f1641265adaf2c/cloud/README.md)). I did not check whether the relayed frames are end-to-end encrypted.
- **Elsewhere.** SSH hosts, a self-hosted always-on server (`orca serve`), and per-task cloud VMs on your own provider account ([Ways to run](https://www.onorca.dev/docs/ways-to-run)).
- **Telemetry.** On by default, anonymous, sent to PostHog Cloud in the US. It never sends prompts, agent output, paths or repo names. Opt out in Settings or with `DO_NOT_TRACK=1` ([Privacy & Telemetry](https://www.onorca.dev/docs/telemetry)).

**Inference for #319:** Orca and Herdr (section 8) are the two ends of the shipping choice. One is a heavy all-in-one Electron app with daily releases. The other is a single binary that lives in the terminal you already use. Neither ships as `npx` or as an editor panel.

---

## 5. How Orca knows what agents do

Orca uses the baseline's hooks and transcripts, plus a newer channel: the agent's own OSC 7501 status. It also writes into Daniel's global Claude settings and reads his Claude login.

- **Hooks, first.** Orca writes managed hooks into `~/.claude/settings.json` for up to 15 events, depending on the Claude Code version. They include `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Stop`, `SubagentStart`, `Elicitation` and `SessionEnd` ([`claude-managed-hook-events.ts`](https://github.com/stablyai/orca/blob/0a349b3197db2aceafdd74d988f1641265adaf2c/src/main/claude/claude-managed-hook-events.ts), [`hook-settings.ts`](https://github.com/stablyai/orca/blob/0a349b3197db2aceafdd74d988f1641265adaf2c/src/main/claude/hook-settings.ts)). The hooks carry state, prompt, tool name and input, the assistant's reply, the session ID and the model ([agent-hooks README](https://github.com/stablyai/orca/blob/0a349b3197db2aceafdd74d988f1641265adaf2c/src/main/agent-hooks/README.md)). A setting removes them ([Hooks & memory](https://www.onorca.dev/docs/agents/hooks-memory)).
- **OSC 7501, as the fallback.** When hooks are off, missing, or silent for 2 minutes on a state the program has left, Orca reads the agent's own Program Status Protocol reports from the terminal stream (agent-hooks README). Claude Code's changelog for 2.1.295: "Added Program Status Protocol (OSC 7501) support: terminals that implement it can show whether Claude Code is working, waiting on you, or done" ([CHANGELOG](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)). Orca's table, dated 2026-10-17, says Codex, Grok, pi and oh-my-pi send it too. Orca says Claude's reports are the "richest" but carry "no prompt and no session id".
- **Window titles.** Orca also reads the terminal title, the older way Claude signals activity ([Agents & sessions](https://www.onorca.dev/docs/model/agents-sessions)).
- **Status line.** Orca installs its own Claude status line only into an empty slot. If you set one yourself, it leaves yours alone ([`statusline-install.ts`](https://github.com/stablyai/orca/blob/0a349b3197db2aceafdd74d988f1641265adaf2c/src/main/claude/statusline-install.ts)).
- **Your Claude login.** For usage limits, "Orca requests `https://api.anthropic.com/api/oauth/usage` with the token from your existing Claude Code login" ([Usage](https://www.onorca.dev/docs/agents/usage-tracking)).

**Same trouble as the baseline's section 6.3.** Open issues ask Orca to "Ask before writing hooks into ~/.claude/settings.json and ~/.codex/hooks.json, or add an opt-out" ([#33874](https://github.com/stablyai/orca/issues/33874)) and to ship the hooks as a plugin instead ([#15496](https://github.com/stablyai/orca/issues/15496)). Other open issues: Orca writes to `~/.claude` even when `CLAUDE_CONFIG_DIR` points elsewhere ([#24873](https://github.com/stablyai/orca/issues/24873)), and other tools that rewrite `settings.json` "silently wipe Orca's managed Claude hooks" ([#16771](https://github.com/stablyai/orca/issues/16771)).

**Inference:** OSC 7501 is new to this research set; neither the baseline nor the data-sources report mentions it. A control room can read it only if it owns the terminal, as Orca and Herdr do. A separate panel next to Daniel's terminal cannot see it. The data-sources work ([#282](https://github.com/danielrispler/Falinks/issues/282)) may want a note.

---

## 6. Problem by problem: what Orca adds to the baseline

Orca is strong on watching and windows, and adds one useful review idea. It adds nothing on understanding during the work. Ratings are my **inference**. Rows where Orca matches the baseline's ADEs are marked "as baseline".

| Problem in `problems.md` | What Orca does | Compared with the baseline |
| --- | --- | --- |
| What the agents are doing now, which phase | Glyphs on every tab, the Dashboard board, the Agents feed, and agent-written worktree comments with a phase (`todo`, `in-progress`, `in-review`, `completed`) | Better than the baseline ADEs. The phase is the agent's own claim, not evidence |
| Progressing or stuck | Red means blocked or failed. Idle means no completion report for about 30 minutes | **Gap remains.** Nothing tells a loop from slow progress. OSC 7501 has an optional `progress` field (0–100) that a detector could use one day (**inference**) |
| Missing the moment an agent waits | Needs You column, amber glyph, phone pushes | Solved, as baseline. But by default Orca removes most permission prompts, so fewer such moments exist at all. Desktop notification sources in the code are `agent-task-complete` and `terminal-bell` ([types](https://github.com/stablyai/orca/blob/0a349b3197db2aceafdd74d988f1641265adaf2c/src/shared/notification-settings-types.ts)); I did not check whether a permission wait fires one |
| Waiting is boring | Nothing | **Inference:** the pitch fills waits by starting more agents, which is more supervision, not learning |
| Not understanding what changed | Annotate AI Diff, attribution gutter, Mermaid and Markdown rendering | End-of-run review, as baseline. Attribution is new: it shows *which lines* the AI wrote |
| Hard to review, unsure whether to trust | PR checks and check-failure notifications; attribution; "where they split, you've found the hard part" | Partial. What the agent was refused or skipped is still not gathered |
| Forgetting previous runs | Session history across all agents' transcripts, searchable, with the first prompt | Better than the baseline: one place for every agent. Still not summarised across runs |
| Project moving faster than my understanding | Nothing | **Inference:** "Ship 100x" and racing three agents make this worse |
| Hard to write a good task | Start a worktree from a Linear, GitHub or Jira item; orchestration task specs | As baseline (Emdash) |
| Am I working well, am I improving | Usage limits and estimated cost | Cost only, as baseline (ccusage) |
| Too many windows | Terminal, editor, browser, diff, PRs, issue trackers and phone, all in one app | **Best in this research set.** This is Orca's core strength |
| Knowing what to use when | An Orca skills registry for agents ([CLI overview](https://www.onorca.dev/docs/cli/overview)); I did not read it | Not addressed for Daniel |

---

## 7. Where Orca sits in the baseline's categories

Orca belongs in the baseline's section 3.2 (ADEs), with Emdash, Superset and Conductor, and stretches into three others.

- **3.2 ADE.** It is the same shape: one agent per worktree, a diff viewer, a terminal and a pull-request flow. It called itself "Conductor + Ghostty" at launch (section 1). It stops at "review the diff", like the rest.
- **3.3 Terminal-first.** Every agent is a real terminal. The account that posted Orca's Show HN said it had "also actually tried cmux" and preferred Orca's tab layout ([HN comment](https://news.ycombinator.com/item?id=47549379)).
- **3.4 Remote control.** A phone app like Happy's, plus an always-on server.
- **A new row: agent-run orchestration.** A coordinator agent dispatches tasks to worker agents, which must report `worker_done` with an outcome and the files they changed, and ask questions through `orca orchestration ask` instead of the terminal ([Orchestration](https://www.onorca.dev/docs/cli/orchestration)). Herdr offers the same idea (section 8). The baseline has no tool of this kind.

**Additions to the baseline's form-factor table (its section 6.5):** Orca belongs under Desktop ADE and Phone; Herdr under Terminal.

**The baseline's churn lesson (its 6.4) holds here too.** Orca ships stable builds almost daily, and the docs already list retired orchestration commands that now "perform no effects". **Inference:** composing v1 on Orca's internals would be fragile. Its documented CLI is the steadier surface.

**Independent opinion is thin and positive.** One user: "I used herdr before now Orca has fully replaced it … I use it every day" ([HN](https://news.ycombinator.com/item?id=49926533)). Another: Orca "leveled up everything around" the CLI ([HN](https://news.ycombinator.com/item?id=49980232)). Its issue tracker shows the weight of a large Electron app. Examples: a terminal daemon whose memory grows to "300-400+ MB per process" ([#12728](https://github.com/stablyai/orca/issues/12728)), and a Linux case where an "unbounded agent node fleet makes systemd-oomd kill the entire GNOME session" ([#16630](https://github.com/stablyai/orca/issues/16630)). Both are open.

---

## 8. Herdr, the other tool the video names

Herdr is a terminal multiplexer for coding agents, like tmux with agent states. The baseline missed it. The video's chapter "Why I switched from Herdr" is the only other tool the chapter titles name.

- "the runtime your coding agents live on"; "one rust binary, no electron — runs in whatever terminal you already use" ([README](https://github.com/herdrdev/herdr)). Install with `curl … | sh`, `brew install herdr` or `mise`.
- "every pane is marked working, blocked, or idle. when an agent stops and needs an answer, herdr says so." Terminals keep running when you detach or lose SSH. Agents drive Herdr through a CLI and a socket API, and can "prompt each other, and wait until another agent is genuinely blocked" (README).
- Apache-2.0, Rust, created 2026-03-27, 46,253 stars, v0.9.5 on 2026-10-14 (GitHub API).

**A word trap.** Herdr's "never hunt for the stuck one" means *blocked on you*. In `problems.md`, "stuck" means *not progressing*. **Inference:** Herdr does not detect a looping agent either.

Other names came up next to Orca, in Hacker News comments and Better Stack's guide list: Paseo, Cyrus and T3 Code. They are not from the video, and I did not check them.

---

## 9. What is worth borrowing, and what is not

All of this is **inference**, offered to the decision tickets. Nothing here is decided.

**Worth borrowing:**

1. **Tint means "look here".** Only cards that need action get colour; the rest stay neutral. This matches the situational-awareness finding (grey when normal, colour only when off). The alarm-fatigue report would go further and keep green "Done" neutral too.
2. **Mark unread.** A one-click "come back to this" on any item is the cheapest form of the *Non-blocking* inbox. For the studio, the item would be a card Daniel has not understood yet, not an agent.
3. **Attribution as a map for cards.** Knowing which lines the AI wrote, and which Daniel wrote, tells the studio where an explanation is needed. The cognitive-load report already asks for less help in code Daniel knows.
4. **Batch the feedback.** Orca sends review notes as one batch so the agent does not "swing back and forth". The same may hold for Daniel's guesses and questions: collect them, and let them reach the agent at a pause.
5. **Fill an empty slot, never take a used one.** Orca's status line installer leaves Daniel's own status line alone. A v0 that adds hooks should do the same, or ship as a Claude Code plugin, which Orca's own users are asking for (#15496).
6. **Disagreement as a pointer.** "Where they split, you've found the hard part." A card could point at the spot where two attempts differ. This costs a second agent run, and it is Orca's claim, not tested.
7. **OSC 7501, if the studio ever hosts the terminal.** It is a status channel that needs no hooks and no settings writes.

**Not worth borrowing:**

- Bypass mode as the default. [#285](https://github.com/danielrispler/Falinks/issues/285#issuecomment-6412880294) makes risky changes into approvals through Claude Code permission rules. I did not check whether those rules still prompt under `--dangerously-skip-permissions`. If Daniel uses Orca, he should switch Agent Permissions to **Manual** first.
- Writing global agent settings without asking (#33874).
- The volume frame ("100x", race three agents). It is the opposite of the main job.

**Does Orca make the control room unnecessary?** No (**inference**). The main job, understanding during the work, is untouched. Orca could be the "watching" layer that #285 says to borrow, but it replaces Daniel's terminal with its own. It also writes his global Claude settings and reads his Claude login. If Daniel tries it, that login read is the kind of access [#284](https://github.com/danielrispler/Falinks/issues/284), rule 4, asked him to note.

---

## 10. What Orca says about each hypothesis

Like the baseline, these are signals of what a large tool chose to build, not verdicts.

- **Move learning into waiting time.** Not tried. Orca fills waits with more agents. Signal: **untested**.
- **Small and visual before text.** The board cards are small: an agent, its state, one line. Design Mode and Mermaid rendering are visual. None of it explains a change. Signal: **mixed**, as in the baseline.
- **Non-blocking.** Mark-unread, the catch-up feed, and phone replies all let Daniel keep working. Signal: **supports**, for attention. It is untried for understanding.
- **Guessing, not testing.** Nothing. Signal: **untested**.
- **Evidence, not promises.** PR checks, the attribution gutter, and `worker_done` reports with an outcome and the changed files. But the worktree comment and `worker_done` are the agent's own words. Signal: **partly supports**; refused and skipped actions are still not shown.
- **A game that rewards understanding, not volume.** No game. The pitch rewards volume. Signal: **untested**, with the market leaning the other way.
- **Serious control-room style.** The calm board (colour only on "look here") fits the calm side. Signal: **supports the calm side**, not the telemetry look.
- **Learning content while waiting.** Nothing. Signal: **untested**.

---

## Open questions

1. **What does the video actually say?** Only its description and chapter titles were read. Daniel watching it (7 min) closes this, especially "Should you switch?" (5:56).
2. **Does a permission wait reach Daniel in Orca?** The notification types in the code are "task complete" and "terminal bell". In Manual mode, whether an approval request raises a system notification is untested. *For #285's rule that approvals always break through:* this matters if Orca is the borrowed watching layer.
3. **Do Claude Code ask rules still prompt in bypass mode?** Claude Code's docs say bypass mode "skips permission prompts, except for the actions no mode auto-approves". I did not read that list. *For #285, item 5.*
4. **Can a separate panel read Orca's state?** `orca terminal list --json` and `orca status --json` exist; I did not check whether they carry each agent's working or waiting state. *For #282:* if they do, Orca could be a data source, like `claude agents --json`.
5. **Do Orca's global hooks and a v0 hook clash?** Both would add `PreToolUse` hooks to `~/.claude/settings.json`. I did not test coexistence. *For #315.*

---

## Sources checked

Fetched and checked against the source, on 2026-10-28:

- YouTube oEmbed for https://youtu.be/8QgBcI40VC8: title and channel. The watch page's embedded data: the description (quoted), chapters, publish date 2026-10-23, length 425 s, 111,567 views. **Transcript not obtained** (section 2).
- Orca repository through the GitHub API: metadata (stars, forks, created date, licence), open issue and pull request counts from the search API, all 990 releases, and the README. At commit `0a349b3`: the docs sources under `docs/site/content/docs/` for index, install, ways-to-run, model/agents-sessions, notifications, activity, agents/hooks-memory, agents/session-history, agents/usage-tracking, agents/native-chat, cli/orchestration, cli/worktree-checkpoints, review/annotate-ai-diff, review/attribution, recipes/parallel-agents, mobile, telemetry and settings (the Agents and Notifications sections). Also `src/main/agent-hooks/README.md`, `src/main/claude/claude-managed-hook-events.ts`, `hook-settings.ts`, `hook-service.ts`, `statusline-install.ts`, `src/shared/notification-settings-types.ts`, `package.json` (Electron version) and `cloud/README.md`.
- onorca.dev: the home page, Enterprise page and live Agents & sessions page (it matches the pinned source). Every onorca.dev link above returned HTTP 200. `orca.dev` returns a 302 to onorca.dev.
- Orca issue [#33874](https://github.com/stablyai/orca/issues/33874), read in full. Issues #15496, #24873, #16771, #12728 and #16630: titles and states from GitHub search only.
- Hacker News through the Algolia API: the Show HN thread [47549197](https://news.ycombinator.com/item?id=47549197) with its comments, and comments 49926533, 49980232 and 49615716.
- Claude Code `CHANGELOG.md`, entry 2.1.295 (OSC 7501 line, quoted). Claude Code [permissions docs](https://code.claude.com/docs/en/permissions): the `bypassPermissions` row and warning, quoted.
- Program Status Protocol [spec page](https://www.superlogical.com/rex/docs/build/program-status): states, blocked kinds and fields.
- Herdr: repository metadata through the GitHub API, latest release, and the README (quoted).

Read through a summarising fetch tool, not verbatim:

- Better Stack's written guide on Orca (author, date and limits list).
- The Y Combinator page for Stably AI (batch Winter 2022, founders).
- The onorca.dev home and Enterprise pages (headline, "Backed by Y Combinator", no listed price).

Not checked, or resting on others' claims:

- Everything the video says beyond its description and chapter titles.
- Orca's OSC 7501 support table for agents other than Claude Code (Orca's own testing).
- Whether a permission wait triggers a desktop notification; whether ask rules prompt in bypass mode; whether Orca's CLI exposes per-agent state; whether Relay traffic is end-to-end encrypted.
- The iOS App Store and Android APK links (from the README, not opened).
- Paseo, Cyrus and T3 Code (names only).
- I ran neither Orca nor Herdr.
