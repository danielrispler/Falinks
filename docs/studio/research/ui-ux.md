# Studio UI/UX: what to change, against herdr and calm developer tools

> **Answer:** The studio draws fast but looks slow and busy. It shows the same facts several times and many lines per row. It gives no feedback on a click, and it puts the thing Daniel wants far away. Before pass 2 adds more to the screen, fix five things: one selection everywhere, a rail that opens on "now", one-line rows with shape-coded status, instant click feedback, and keys to move around.
> **Evidence:** Strong for the studio itself: Daniel's screenshots and recording, the code at `2c7fd47`, and measurements in headless Chrome. There, an update costs 1.3 ms and a session switch 33 ms; Chrome is not WKWebView. Moderate for competitors: herdr's source, images and user comments, and Conductor's changelog and user comments. No study tests any of it on coding agents.
> **Hypotheses:** Supports "Serious control-room style" in its calm form: grey when normal, colour only for needs you (#267, #269). Supports "Small and visual before text" for status glyphs paired with a word. Weakens adding features each pass without a layout to hold them.
> **Read or watch yourself:** herdr's default look and its "t3" sidebar, `local-evidence/2026-11-09-ui-ux/herdr-images/01-og-herdr-default.png` and `02-tile-t3-default-sidebar.png` (2 min). Your own recording's frames `measure/rec/ev/f126.png` and `f127.png` (1 min): the rail and the terminal show different sessions. Linear's [A calmer interface for a product in motion](https://linear.app/now/behind-the-latest-design-refresh) (10 min).

Research report for [#563](https://github.com/danielrispler/Falinks/issues/563), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-11-09.

It merges three inputs:

- a UI designer (visual);
- a UX designer (interaction, wayfinding and speed);
- a product manager (jobs, market, tickets; who also merged them).

Their full notes are in `local-evidence/2026-11-09-ui-ux/`: `ui-expert.md`, `ux-expert.md` and `pm.md`. They live on Daniel's machine and are not committed. The same holds for every image and measurement named here.

## How to read this

- **Evidence labels:**
  - **[real]**: Daniel's own screenshots or recording.
  - **[fixture]**: the real UI code in headless Chrome, fed by the core's scrubbed replay fixtures. This is **not** WKWebView.
  - **[code]**: read from the source at `2c7fd47`.
  - **Inference**: our own reading, which no source states.
- **Hurt:** high, medium or low.
- **Work:** S (a day or less), M (a few days), L (one to two weeks).
- **Evidence files:**
  - Daniel's shots are in `local-evidence/2026-11-09-ui-ux/`: `full app.png`, `half app.png`, `Screenshot 2026-11-09 at 17.26.58.png` (a card) and `Screenshot 2026-11-09 at 17.27.07.png` (the picker). His recording is `Screen Recording 2026-11-09 at 17.27.16.mov` (20.9 s).
  - Measurement scripts, raw data, fixture screenshots and recording frames are under `measure/`.
  - herdr images are under `herdr-images/`.
- Studio terms follow `docs/studio/CONTEXT.md`.

## Contents

1. Daniel's four complaints, named
2. The jobs the screen must do
3. Problems
4. "Slow", with numbers
5. Bugs found along the way
6. What herdr does better, and what not to copy
7. What Conductor and Orca teach
8. Top changes
9. Design directions
10. Pass 2 tickets the findings would change
11. Open evidence
12. Sources checked

---

## 1. Daniel's four complaints, named

| Complaint | What it is, concretely | Problems |
| --- | --- | --- |
| **Dense and cluttered** | Rows of three lines. The same fact in two to four places. Eight controls before the rail's first line of story. Bold and borders everywhere. A rail that reads as a log. | D1–D8 |
| **Hard to understand where everything is** | The rail and the terminal can show different sessions. The card sits below the fold. The rail hides at normal widths. The strip says "Fali" five times. There is no board and almost no keys. | W1–W8 |
| **Outdated** | Web-default controls. No hover, focus or motion. A flat window with a native title bar. No named sizes or spacing. | O1–O4 |
| **Slow** | Mostly **looks** slow. Nothing answers a click until the shell replies. Things jump. A running session looks frozen. Focus and text selection are lost on every update. One real delay: a question takes up to 1.2 s to dock. | S1–S5 |

---

## 2. The jobs the screen must do

From the main job ([#285](https://github.com/danielrispler/Falinks/issues/285#issuecomment-6412880294)), the scope decision ([#320](https://github.com/danielrispler/Falinks/issues/320#issuecomment-6465695482)) and the attention research (#267, #269, #270, #271). In order:

1. **Tell me where to look.** Which session needs me, which finished, and that the rest are fine. One glance, no reading.
2. **Help me understand this session's last turn.** What it did, what is open for me, one question. More only on request.
3. **Let me work in the terminal.** It is the main work surface. Never take its room or its keys.
4. **Housekeeping.** Start, rename, close, delete; tasks; open a file. Rare, so one step away, not always shown.

Jobs 1 and 3 are always on screen. Job 2 is for the selected session. Job 4 is hidden until asked for.

`pm.md`, section 2, checks every element on screen against these jobs. Its result: the elements that serve job 4, or no job, are the clutter. Section 3 names them.

---

## 3. Problems

Each problem merges what the three experts found. Its sources are in brackets: UI-P*n*, UX-P*n* and PM refer to the notes in `local-evidence/2026-11-09-ui-ux/`.

### Dense and cluttered

**D1. Rows of three lines.** (UI-P1, PM)

- A session row has up to 7 items on 3 lines: dot, name, context share, state word, "N done", step line, and "Session · Turn · Step" (`list.ts:86-116`).
- Row pitch is about 70 px [real: `full app.png`]. herdr's row is one line and 30 px (`herdr-app@b721ded defaults/mods/agents/main.js:333-361`).
- The eye scans three lines per row to find one fact: who needs me.
- **Hurt:** high. **Work:** M.

**D2. Finished subagents fill the list.** (UI-P2, UX-P14, PM)

- In `full app.png`, 6 of about 14 rows are subagents that completed or were killed, such as "Build #500 setup pinning · completed · SubagentHandback". Only one session is running.
- `Subagent::faded` folds helpers only. #535 fixes it.
- **Hurt:** high. **Work:** S.

**D3. The same fact in two to four places.** (UI-P7, UX-P18, PM)

- The session name "herdr grilling" shows four times: top bar, terminal header, list row, and Claude Code's own footer [real].
- The context shows three times, and the numbers disagree: "101%" in the top bar and the row, "77% left" in Claude's line (see Bugs).
- The state shows three times: "RUNNING · 1", the row's "Running", and the rail's "Running".
- The times show in the rail and on the row.
- **Hurt:** medium. **Work:** S.

**D4. Tools before content in the rail.** (UI-P6, UX-P10, PM)

- Before the first line of story, the rail shows about 164 px of chrome [real: `full app.png`, y = 64–228]:
  - three clocks;
  - "Designing · no estimate";
  - "Open all" and "Close all";
  - "Running";
  - "Rename", "Close" and "Delete".
- Delete sits beside Rename at the same weight.
- In the picker, a bordered "Remove" sits beside every task [real: picker shot].
- **Hurt:** medium. **Work:** S.

**D5. Text without meaning.** (UI-P8, UI-P15, UX-P14, PM)

- **Non-messages:**
  - "no estimate", which shows until 20 ended sessions of a kind exist;
  - "Step 0s";
  - "No step yet";
  - "SubagentHandback", a tool name.
- **Raw text:**
  - a raw Markdown line, "Yes, screenshots of \*\*our app…";
  - a temp path wrapped over five lines (`/private/tmp/claude-501/…/scratchpad/map266.md`);
  - the terminal header's "169 × 38" grid size.
- All of these show in `full app.png`.
- **Hurt:** medium. **Work:** S.

**D6. Clocks tick while everything is normal.** (PM, UI-P4)

- Each session row and the rail tick every second (`times.ts:78-86`). In `full app.png` that is up to 6 moving numbers.
- #267 keeps motion for the off-nominal ("misplaced salience"). Meanwhile, the one thing that should move, a running session, has a static dot (see S3).
- **Hurt:** medium. **Work:** S.

**D7. The rail reads as a log.** (UI-P13, UI-P15, PM)

- "Responses (7)" is open in full, with one "Ask about this" under each [real].
- The rail's reading text is 15.5 px against the terminal's 12.5 px, so the side panel is the loudest region.
- #526's turn block (summary open; learning and full response folded) is the planned fix.
- **Hurt:** medium. **Work:** S, with #526.

**D8. Cards and boxes.** (UI-P3, UI-P9, UI-P17, PM)

- After the tap, a card holds up to 11 parts. #271 suggests about four new things per card.
- A quiz is 5 boxes inside a box [real: card shot].
- 18 rules use bold, so bold marks nothing. The loudest word in the list is a subagent's "killed".
- 17 solid 1 px borders. Linear's 2026 refresh: "reduce visual noise with fewer separators".
- **Hurt:** medium. **Work:** M.

### Hard to understand where everything is

**W1. The rail and the terminal show different sessions.** (UX-P1)

- In the recording, a click on the row "Falinks" changes the rail and the top bar to "Falinks". The terminal stays on "herdr grilling 169 × 38" [real: `measure/rec/ev/f126.png` vs `f127.png`].
- The cause: a session the studio did not start has no pane of its own, so the studio shows its login shell. Here that shell runs another session's `claude` (`panes.ts:124-137`).
- This is the largest single cause of "where am I".
- **Hurt:** high. **Work:** M.

**W2. A question in a view that is not shown goes to the terminal.** (UX-P2)

- The answer hook waits for the card only while that view shows. Otherwise it hands the question to the terminal (`answer.ts:1-4, 54-58`).
- That terminal may be hidden, or not in the studio at all (W1).
- A subagent's question docks only in that subagent's own view.
- **Hurt:** high. **Work:** M.

**W3. The rail has no "now", and the card sits below the fold.** (UX-P3, UI-P5)

- The order is: actions, decisions, intention, step, responses, brief, and only then cards (`index.html:61-87`).
- In the recording, Daniel scrolls from 7.97 s to 15.0 s to reach a card: **7 of the 21 seconds** [real].
- Mockup C ([#463](https://github.com/danielrispler/Falinks/issues/463#issuecomment-6632622381)) made the current intention the biggest, boxed item: 16.5 px, weight 500, a bright edge. That anchor was not shipped: the intention is 15.5 px with no box (`style.css:137`). Nothing leads the eye.
- **Hurt:** high. **Work:** S.

**W4. No rail at the default size or at half width.** (UX-P4)

- The layout keeps the terminal at 100 columns or more and gives up the list first, then the rail (`layout.ts:19-23`). The full layout needs about 1,550 px.
- At the app's default 960 × 640 (`tauri.conf.json:12`) and at half of Daniel's screen, the rail is hidden behind ⌘2 [fixture: `fixture-05…png`; real: `half app.png`].
- The understanding layer is the first thing to disappear.
- **Hurt:** high. **Work:** M.

**W5. The strip cannot tell sessions apart.** (UI-P16, UX-P8, PM)

- At half width, the strip shows five dots labelled "Fali" and a bare "67" [real: `half app.png`].
- The label is the issue number of an `issue-<n>-…` folder, else the folder's first 4 letters (`list.ts:15-19`). Main-checkout sessions all read "Fali".
- **Hurt:** high at half width. **Work:** S.

**W6. Status is grey on grey.** (UI-P4, UX-P6, UX-P12, PM)

- "Your turn", "idle" and "ended" share the same faint dot (`style.css:87-89`).
- Nothing marks a turn that ended while Daniel looked elsewhere: there is no "unseen" state (#556 is planned).
- The selected row is about 7 grey levels lighter than the rest. A frame diff at threshold 12 did not even register the change [real].
- There is no global "1 needs you" count when the list is a strip.
- herdr shows the state by shape: a spinner for working, a ring for idle, a dot for done, a "Needs you" pill.
- **Hurt:** high. **Work:** S.

**W7. No home for tasks.** (UX-P7, UX-P17, UI-P18, PM)

- `CONTEXT.md` defines a **Board**, but no board exists: no view, no code (`grep -i board ui/`). Daniel could not take the screenshot of it that #563 asked for.
- Tasks show only inside the "New session" popover. There, paths are cut at the part that differs ("…/Falinks/.clau"), and the placeholder is cut [real: picker shot].
- **Hurt:** medium. **Work:** L for a board; S for the picker.

**W8. Almost no keys, and none shown.** (UX-P9, UX-P11)

- Six window keys exist: ⌘1, ⌘2, Esc and three zoom keys (`settings.ts:60-78`).
- There is no key for the next session, the next "needs you", a new session or settings. There are no arrow keys in the list, no palette and no help.
- Settings exist only as files. Their errors appear in the rail's notice line.
- Daniel uses Zed every day, and the studio already uses Zed's keymap format.
- **Hurt:** high for a Zed user. **Work:** M.

### Outdated

**O1. Web-default controls.** (UI-P10, UI-P18)

- 8 separate button styles with square 4 px corners.
- Native `<details>` triangles ("▶ Ask about this", "▼ Responses (7)").
- Upper-case 11 px tracked headings.
- Default WebKit scrollbars.
- macOS's blue focus ring in the picker: the only blue in the app [real].
- **Hurt:** high for "outdated". **Work:** M.

**O2. No hover, no focus, no motion.** (UI-P11, UI-P19)

- The stylesheet has 2 hover rules. List rows have none. There is no `:focus-visible` and there are 0 transitions.
- Switching sessions swaps content with no fade [real: recording].
- herdr and Zed put hover on every row.
- **Hurt:** medium. **Work:** S.

**O3. A flat window.** (UI-P12)

- The native title bar and the top bar take 64 px of chrome.
- List, terminal and rail are three flat columns split by 1 px lines. The list and the rail differ in tone by only 1.08:1.
- herdr frames the terminal as an inset rounded pane on a darker frame. The work area reads as "the thing" (`herdr-images/01`).
- **Hurt:** medium. **Work:** M.

**O4. No named scales.** (UI-P14)

- 10 text sizes, 28 padding values, 27 margin values, 26 raw px sizes.
- Zed names 4 text sizes and uses a 4 px spacing scale (`crates/ui/src/styles/typography.rs`, `spacing.rs`).
- **Hurt:** low on its own, but a root cause of D8 and O1. **Work:** M.

### Slow

**S1. No feedback on a click.** (UX-P5, UI-P11)

- A row click waits for `invoke("select")` before anything moves (`selection.ts:20-30`).
- A quiz tap waits for `invoke("guess")`, which fsyncs the store, then for a second `invoke("cards")` (`cards.ts:12-15`).
- No hover, and a faint selection (W6).
- This is the main reason the app looks slow.
- **Hurt:** high. **Work:** S.

**S2. Things jump.** (UX-P15)

- After a switch, the terminal replays at the old size, then fits. Its content jumps about two lines one frame later [real: `measure/rec/ev/f337.png` → `f338.png`].
- The card grows on the tap.
- The picker flashes "Reading worktrees…".
- **Hurt:** medium. **Work:** M.

**S3. A running session looks frozen.** (UI-P4)

- The running dot is static white. A session that works looks the same as one that hung.
- herdr's only animation is a 900 ms spinner on working agents.
- **Hurt:** medium. **Work:** S.

**S4. Focus and text selection are lost on every update.** (UX-P13)

- The list is rebuilt whole twice per event (`list.ts:199-215`, `selection.ts:32-36`), up to about 5 times a second.
- The responses are re-rendered (`story.ts:85-90`), so selected text in a running session's response disappears at the next step.
- Measured: 39 nodes replaced per update, 92 KB per event with 25 sessions [fixture].
- **Hurt:** medium. **Work:** M.

**S5. A question docks up to 1.2 s late.** (UX-P16)

- A 1 s poll (`answer.ts:48-63`) runs on top of the 200 ms read loop. This is the one **real** delay found.
- **Hurt:** low to medium. **Work:** S (push an event instead).

---

## 4. "Slow", with numbers

**Plainly: the studio is mostly not slow at drawing. It looks slow.** The causes are S1–S4 and W3: no feedback, jumps, a frozen-looking dot, and hunting for the card.

**Drawing** [fixture: headless Chrome on Daniel's Mac, **not WKWebView**]:

| Measurement | Median | p95 |
| --- | --- | --- |
| One update (`sessions` + `selected`), 25 sessions | 1.3 ms | 3.0 ms |
| One update, about 67 sessions | 3.2 ms | 3.7 ms |
| Session switch, click to the next frame | 33 ms | 35 ms |
| ⌘2 rail open, key to frame | 16 ms | 97 ms |
| Picker open | 17 ms | 53 ms |
| First rows after load | 147 ms | 1,163 ms (cold) |
| Payload per update, 25 sessions | 92 KB | 95 KB |
| Layout shift over 120 updates | CLS 0.00007 | — |

Raw data: `measure/raw/results.json` and `scale.txt`. Method: `measure/measure.mjs`, `measure/dumper/`.

**From hook to screen** [code]:

| What | Time to screen |
| --- | --- |
| A step | about 100 ms on average, about 220 ms at worst: the 200 ms read loop, then 1–3 ms to draw, then a frame |
| A question, plan or permission | **up to 1.2 s** (S5) |
| A new card | seconds (`claude -p`) |
| Plan usage | up to 10 s |

**In the real app** [real: the recording, every frame diffed by region, `measure/rec/diffs.json`]:

- macOS did not record the clicks. So each figure is the time from the cursor coming to rest to the first change on screen, which is an upper bound.
- Those gaps are 250 to 600 ms, and part of each is Daniel's own hand.
- No real delay over about 250 ms is proven.
- 7 of the 21 seconds went to scrolling the rail to reach a card.

**Polls and timers** [code]:

| Timer | Interval |
| --- | --- |
| Shell read loop (rebuilds the whole list on any change) | 200 ms |
| UI answer watch | 1 s |
| UI clocks | 1 s |
| Settings watch | 500 ms |
| Build watch | 2 s |
| PR lookup | 5 s loop, each branch at most every 300 s |
| UI usage | 10 s |

The full table is in `ux-expert.md`, section 3a.

**Not measured:** WKWebView itself. Section 11 lists what would replace the Chrome numbers.

---

## 5. Bugs found along the way

These are defects, kept apart from the design findings. Each needs a ticket or a check, not a design choice.

| Bug | Evidence | Ticket |
| --- | --- | --- |
| **"context 101%"** in the top bar, and "116%" and "107%" on rows, while Claude Code's own line in the same window says "77% left \| 231.4k tok" | [real] `full app.png` | #519 is closed: "an honest context share", merged before the screenshot. **First check whether the app running at 17:25 was built after pass 1.** The rows also show all three clocks, where #517 decision 4 and `list.ts` at main show the turn time only. That also points to an older build. If the build is current, reopen #519. |
| Finished and killed subagents stay in the list | [real] 6 of about 14 rows | #535 (open) |
| Two rows both called "Falinks", both "Your turn · No step yet" | [real] `full app.png` | Possibly #562 (one conversation as several rows). Check them against its reproduction. |
| **The rail and the terminal show different sessions** | [real] `measure/rec/ev/f126.png` vs `f127.png`; `panes.ts:124-137` | **None yet.** It needs one (W1). |
| A quiz card on a change to `.DS_Store`, a file Finder writes; the brief says "nothing beyond macOS housekeeping" | [real] `Screenshot 2026-11-09 at 17.26.58.png` | **None yet.** The card trigger should ignore files like this. **Inference:** a card that needs no attention trains Daniel to skip cards, as false alarms do (#269). |
| A question takes up to 1.2 s to dock | [code] `answer.ts:48-63`, `main.rs:554` | None yet (S5). |

---

## 6. What herdr does better, and what not to copy

Read at `herdrdev/herdr-app@b721ded`, plus herdr.dev's images, its 30 showcase tiles, its guide and four demo videos. Users' quotes are traced in [`herdr.md`](https://github.com/danielrispler/Falinks/blob/2c7fd47/docs/studio/research/herdr.md), section 3.

**Better:**

1. **One-line rows, 30 px, regular weight.** A name, one dim fact and one glyph (`defaults/mods/agents/main.js:333-361`; `herdr-images/01`, `02`).
2. **Status by shape and motion, not hue.** A spinner (900 ms) for working, a ring for idle, a dot for done, a "Needs you" pill (`main.js:202-210`). In the "t3" layout, only the group title takes the status colour (`herdr-images/02`).
3. **A global "needs you" count,** seen without opening the list: a status-bar chip "4 agents working · 1 needs you" (`herdr-images/03`), or a summary strip (`herdr-images/05`).
4. **Done until seen.** A finished turn stays "done" until the person really looks: the terminal is visible and the window is active (`src/model.rs:460-479, 525-535`; `src/window.rs:2048`). Alerts follow one rule: needing you always alerts; finishing alerts only if you were not looking (`model.rs:481-503`).
5. **Keys cover moving around** (`defaults/keys.macos.json`):
   - `cmd-]` and `cmd-[` go to the next and previous agent, in list order;
   - `cmd-,` opens settings;
   - `cmd-/` opens a filterable key list;
   - `cmd-shift-p` opens the palette.
6. **A framed work area.** The sidebar sits on a darker `chrome`. The terminal is an inset pane with radius 10 and an 8 px gap. Borders are 9% white. Colours are derived from one palette (`src/settings.rs:303-353`).
7. **Hover on everything,** with secondary actions shown on hover. The pin replaces the age on hover (`main.js:359-360`).
8. **Feedback on failure:** a toast "`<name> failed`" that can carry an action (`src/window.rs:2738-2750`).
9. **What users praise** (herdr note, section 3):
   - "One glance shows which agent needs you";
   - "everything is clickable" ([smashah, HN 48826541](https://news.ycombinator.com/item?id=48826541));
   - "no integration just show me the terminal output" ([kristjansson, HN 49203351](https://news.ycombinator.com/item?id=49203351));
   - from the UI expert's reading of the threads: "the most polished tool in its category" (shepherdjerred, HN 50298436 thread).

**Not to copy:**

- **Several status colours.** herdr uses yellow, red and blue, or blue, amber and green. The studio's rule, colour only for "needs you", rests on #269. Take the shapes and the motion, not the hues.
- **Skins, mods, layouts, themes.** In the "command center" tile, every row has a coloured badge, so the row that needs you does not stand out (`herdr-images/06`). Daniel's agent edits the studio's source directly.
- **Spaces, tabs and free splits.** They add places to get lost. #529 already caps panes at two.
- **Dropping "what it is doing".** herdr's row has no activity line. The studio's one line is its situational-awareness value (#267). Keep it, but only for running and needs-you rows.
- **A program icon per row.** For Claude Code sessions it says nothing. Status-only rows (#560) may need a mark.
- **Text under AA contrast for real content.** herdr's `dim` is 3.49:1.
- **`cmd-1…9` as-is.** In the studio, ⌘1 and ⌘2 mean list and rail. Pick one meaning.
- **The 100 ms flush jitter** users complained about ([HN 50319113](https://news.ycombinator.com/item?id=50319113)), and herdr-app's UI-thread stalls ([#748](https://github.com/herdrdev/herdr-app/issues/748)). Native code does not make an app fast by itself.

---

## 7. What Conductor and Orca teach

**Conductor** (a Mac app for parallel Claude Code and Codex agents):

- **Its pitch is the glance.** "See at a glance what they're working on, then review and merge their changes" ([home page](https://www.conductor.build)).
- **It groups work by status.** It organises workspaces as "backlog, in progress, in review, and done" ([0.35.0, 11 Feb 2026](https://www.conductor.build/changelog/0.35.0-workspace-status)).
- **It moved detail to hover.** It shipped a "new, simplified sidebar" where "Hover any workspace to see metadata and get easy access to next actions" ([0.44.0, 24 Mar 2026](https://www.conductor.build/changelog/0.44.0-new-sidebar-rebuilt-composer-codex-checkpoints)).
- **Users call it clean:**
  - "a beautiful clean UI, great UX" ([HN 50370579](https://news.ycombinator.com/item?id=50370579));
  - "a nice clean simple extension of how Claude code already works" ([HN 44627897](https://news.ycombinator.com/item?id=44627897)).
- **Its designer** aims to be "functional, visually subtle, and chromatically warm" ([HN 44629675](https://news.ycombinator.com/item?id=44629675)).
- **The complaint is about wrapping Claude Code:** "I don't think I'm looking for an interface to replace CC … I just want a better way to manage the sessions" ([HN 44635832](https://news.ycombinator.com/item?id=44635832)).
- **It is winding down.** It joined Meta on 4 Nov 2026. The local app gets "critical bug fixes until early 2027" ([announcement](https://www.conductor.build/announcement)). It is a reference, not a tool to adopt.

**Orca** ([#318 report](https://github.com/danielrispler/Falinks/blob/dd8b3521239e5554bf5e9b055550b9f011d0b028/docs/studio/research/orca.md)):

- "Needs You cards tint amber …; other states stay neutral so tint means 'look here'".
- Idle is hidden by default.
- An Agents feed is the catch-up surface.
- One user went back to herdr: "Orca has a lot of UI bugs, buttons that don't work, menus that don't disappear" ([HN 50371064](https://news.ycombinator.com/item?id=50371064)).

**Together** (**inference**):

1. The glance is table stakes. The studio has it, buried in noise.
2. Users call a tool "clean" when it adds little on top of Claude Code. The studio's rail adds the most, so it must earn its room.
3. Detail belongs on hover or one click away.
4. No competitor does the understanding job (job 2). That is the studio's room, and its clutter risk.

---

## 8. Top changes

In order. Each fixes several problems.

| # | Change | Fixes | Work |
| --- | --- | --- | --- |
| 1 | **One selection everywhere.** The terminal always shows the selected session. A session outside the studio says so ("runs in another terminal; Open here resumes it") instead of showing the shell. A question for a hidden view raises its row; it is not silently handed to a terminal. | W1, W2, D3 | M |
| 2 | **The rail opens on "now".** First the open question or the newest card. Then the current intention as the boxed anchor from mockup C, then the turn summary (#526). History (responses, decisions, checks) comes after, folded. One header line. Rename, Close and Delete go into a "⋯" menu. One "Ask about this" per turn, on hover. | W3, D4, D7 | S–M |
| 3 | **One-line rows, status by shape.** About 30 px; regular weight; a glyph at the end: a spinner for running, a dot for your turn, a ring for idle, a "Needs you" pill. A second muted line only for running and needs-you rows. Finished subagents fold (#535). An unseen mark (#556). A "1 needs you" chip in the top bar. Strip labels from the session's name. | D1, D2, D6, W5, W6, S3 | M |
| 4 | **Instant feedback.** Hover on every row and button. A clear edge bar on the selected row. Paint the selection and a quiz tap on pointerdown, then reconcile (Linear's pattern, `ux-expert.md` section 6). Hide a new pane until it has fitted. | S1, S2, O2 | S |
| 5 | **A keyboard spine, shown in the app.** ⌘] and ⌘[ for the next and previous session. One key for the next needs-you, then the next unseen. ↑ and ↓ in the list; 1–4 to answer. ⌘N for a new session; ⌘, for settings. A key list and a palette built from `keymap.json`. | W8 | M |
| 6 | **Say each fact once, cleanly.** The name in one place. Clocks at minute level, and only on the row being looked at. Cut "no estimate", "Step 0s", the grid size and raw Markdown. Shorten paths to their last 2 segments, with the full path on hover. | D3, D5, D6 | S |
| 7 | **Keep the rail at normal widths.** Narrow the rail, or let the terminal drop under 100 columns, before hiding the rail. If it must hide, keep a slim header with the state and the open question. | W4 | M |
| 8 | **A quiet frame.** The terminal as an inset rounded pane. The list and the rail on a darker frame. Fills instead of borders. One button component. Named sizes and spacing. Sentence-case headings. Thin scrollbars. A grey focus ring. | O1, O3, O4, D8 | M |

**Next after these:**

- Stop the double full redraw and the response re-render (S4).
- Push open questions instead of polling them every 1 s (S5).
- Give tasks a home (W7).

---

## 9. Design directions

Three product bets, in words and rough wireframes. Mockups come after Daniel picks one.

All three share a floor: changes 1, 4, 6 and 8 above. That means one selection, instant feedback, each fact once, and the quiet frame from `ui-expert.md`, section 6.

### A. A calm triage list

**The bet:** the biggest pain is not knowing where to look. The list becomes the calm home of the app.

- One-line rows with shape-coded status.
- Colour only on the "Needs you" group title and its pill.
- A top-bar "1 needs you" chip.
- A second section, "Places", holds tasks and the main checkout, with #559's counts. This is the board, reduced to what the list needs.
- The rail shows only the latest turn: its summary and its one question. Doctor, skills and settings go in a palette (⌘⇧P).

```
+----------------------+------------------------------+--------------------+
| herdr grilling   ⋯   |                              | Needs you · 1      |
| ▌Adding ticket nums  |                              |  #552 CI   [Needs] |
|                      |         terminal             |   Approve plan?    |
| Turn summary         |     (inset, the stage)       | Running · 1        |
|  Four edits applied… |                              |  herdr grilling ◠  |
| ? one question  A B C|                              | Your turn · 4      |
| › more               |                              |  engine         ●  |
|                      |                              | › Places · 3       |
| [dock: Allow Bash?]  |                              | › Idle · 3         |
+----------------------+------------------------------+--------------------+
  ◠ running (spins)   ● your turn   ○ idle   ▌ the anchor   [1 needs you] in top bar
```

- **Risk:** the depth Daniel uses (decisions, checks, older cards) moves one click further.
- **Pass 2 fit:** #522 and #556 become one cue. #527 and #528 go to the palette. #529 and #542 still compete for width.

### B. Terminal first, story on demand

**The bet:** Daniel lives in the terminal, and understanding is pulled, not pushed. The studio feels like Zed with agents in the sidebar.

- One left sidebar, as in an editor: sessions by state, with tasks as their parents. It folds with ⌘B.
- The terminal fills the window. The split (#529) and the bottom panel (#542) work as in Zed.
- The story slides in on ⌘2, or by itself at a turn's end when Daniel is not typing. Esc closes it.
- The keyboard spine (change 5) is central.

```
+---------------+---------------------------------------------------+
| Sessions      |  terminal              |  terminal (split, #529)  |
|  ● #552 [!]   |                        |                          |
|  ◠ herdr gr.  |                        |                          |
|  ● engine     +------------------------+--------------------------+
| Tasks         |  shell panel (ctrl-`, #542)                       |
|  issue-559    +---------------------------------------------------+
+---------------+     ⌘2 → the story slides in over the right edge
```

- **Risk:** it bets most against the main job (#285). A rail that is usually closed is easy to forget. #268 warns that supervisors who watch less understand less.
- **Pass 2 fit:** best for #529, #542 and #557. Worst for #526: a summary nobody opens.

### C. The story rail as a turn feed

**The bet:** understanding is the product. Give the reading column one structure. This builds on #526 and the approved story-rail mockup (#517 decision 11).

- The rail is a feed of turns, newest first. Each turn is one block:
  - its prompt, in one line;
  - the summary, open;
  - the learning question;
  - its card, if any;
  - the checks it ran;
  - the full response, folded.
- The separate "Last 30 minutes", "Older", "Responses" and "Questions" sections go away. Everything belongs to its turn. #535's subagent history becomes entries in the feed.
- One header line: verdict, phase, mode. Times and model on hover.

```
+--------------------------------+---------------------------+-------------+
| herdr grilling · Designing · ⋯ |                           | Needs you 1 |
| ── turn 4 · now ─────────────  |                           |  #552  [!]  |
| ▌summary: four edits applied…  |        terminal           | Running 1   |
|  ? Why is the trap rule risky? |                           |  herdr  ◠   |
|    A   B   C                   |                           | Your turn 4 |
|  card ›  checks ✓2 ›  reply ›  |                           |  engine ●   |
| ── turn 3 ────────────────────|                           |             |
|  summary …            (folded) |                           |             |
| [dock: Allow Bash?  Yes  No]   |                           |             |
+--------------------------------+---------------------------+-------------+
```

- **Risk:** the rail is the widest column, and #529 and #542 want the same width. It needs W4's rule on which side gives way.
- **Pass 2 fit:** #526 is its core. #528 opens in the rail's place.

### The PM's lean (to grill, not decided)

- A and C do not compete: A is the list, C is the rail. **A + C** gives a calm list and one structured rail. This is the recommendation to test first.
- **B** is the real alternative. It answers Daniel's "a plain terminal, like in any other IDE" (#542).
- The question that decides it: *when you glance at the studio, do you come to find out who needs you, or to understand what happened?*

---

## 10. Pass 2 tickets the findings would change

| Ticket | Today | What the findings change |
| --- | --- | --- |
| #535 finished subagents leave the list | open | **Do first.** It removes 6 of 14 rows in `full app.png` (D2). |
| #562 one conversation, one row | open | **Do first.** Also check the two "Falinks" rows (Bugs). |
| #526 turn summary and learning | adds a turn block | It must **replace** "Responses (n)" and the repeated "Ask about this", and sit at the **top** of the rail (W3, D7). In direction C it is the core. |
| #522 notify on your turn | bold orange "your turn" | Design it with #556. The orange is a second colour state, which breaks "colour only for needs you" (#267, #269). The shape and the unseen mark can carry "your turn" instead. That needs Daniel's call: #517 decision 7 chose orange. The notification part stands. |
| #556 unseen mark and sounds | adds a mark | It becomes the core "finished, not seen" cue (W6). Add the top-bar "needs you" count. |
| #529 split terminals | a second pane | Two 100-column panes need about 1,500 px of terminal alone, so the rail hides more often (W4). It needs the width rule from change 7 first. |
| #542 docked terminal panel | a bottom band | The same width and height pressure. In direction B it is central. In A and C it waits. |
| #528 skills browser, #527 Doctor menu | new view, new button | No navigation exists for new views. Put them in a palette (change 5), not the top bar. |
| #559 ahead/behind counts | numbers on "board rows" | No board exists (W7). The rows are in the picker popover. Give tasks a home first (direction A's "Places"). |
| #560 status-only rows | a new row kind | Status by shape (change 3) gives these rows the same glyphs. They need a clear "status only" mark. |
| #541 auto mode | a mode label in the rail header | Keep it. It belongs on the one header line, where it earns its room as a safety fact. |
| #557 file links, #558 fast-failure notice | links; a notice | Unchanged. Use a toast with an action for #558 (herdr's pattern). |
| #521, #544, #555 | invisible | Unchanged. |

**New tickets the findings suggest:**

1. one selection everywhere (W1, W2);
2. the rail opens on "now" (W3, D4);
3. instant feedback (S1, S2);
4. the keyboard spine and palette (W8);
5. no cards on `.DS_Store` and similar OS files (Bugs);
6. push open questions instead of polling (S5);
7. check the running build against #519 (Bugs).

---

## 11. Open evidence

Still needed from Daniel:

1. **A recording with clicks shown.** In ⌘⇧5, choose Options → "Show Mouse Clicks". This turns the 250–600 ms upper bounds into exact click-to-response times.
2. **A recording that starts a new session,** from "New session" to the first prompt. The current recording lacks it.
3. **Optional:** a Safari Web Inspector "Timelines" capture of the real app (Develop menu → Studio) during 3 session switches. It would replace the Chrome numbers with WKWebView ones.
4. **Optional:** the five timed tasks in `ux-expert.md`, section 8. Do them before the changes, then again after, to compare.
5. **A shot with a docked question** (none exists). Today the dock findings rest on code only.
6. **Which build was running at 17:25** (Bugs, the context share).

---

## 12. Sources checked

**The studio:**

- Code at `2c7fd47`:
  - `apps/studio/ui/*.ts`, `index.html` and `style.css`;
  - `src-tauri/src/main.rs`, `settings.rs`, `restart.rs`, `terminal.rs` and `tauri.conf.json`;
  - parts of `core/src/lib.rs`, `understanding/src/store.rs` and `workbench/src/lib.rs`.
- The 20 fixtures in `core/tests/fixtures/`, replayed by `measure/dumper`.
- `docs/studio/CONTEXT.md`; the story-rail mockup `docs/studio/mockups/517-story-rail.html`; #463's resolution (mockup C's tokens).
- Daniel's four screenshots and his recording, `local-evidence/2026-11-09-ui-ux/`.
- Tickets, through `gh issue view`: #563, map #266, the #517 spec (Implementation Decisions 1–22), #519 (closed 2026-11-09 14:19 UTC), and the 17 pass-2 tickets.

**Studio research**, at their Findings permalinks:

- #267, #269, #270 and #271: cards and their studio sections;
- #318 Orca: sections 3, 6, 7 and 9;
- `herdr.md` at `2c7fd47`: sections 1, 3 and 4.

**herdr:**

- `herdrdev/herdr-app@b721ded`:
  - `src/settings.rs`, `model.rs`, `window.rs`, `commands.rs` and `sound.rs`;
  - `defaults/mods/agents`, `help`, `toast` and `workspace-chrome`;
  - `defaults/layouts/main.json` and `defaults/keys.macos.json`;
  - `defaults/AGENTS.md` and `docs/`.
- herdr.dev: `og-herdr.png`, the 30 showcase tiles, `live/herdr-look.css` (its tokens match the source), and the /guide, /tui, /core, /blog and /gallery pages.
- Three videos (the TUI README / v0.4.0 release, v0.1.0, and the session navigator) plus the connecting-machines demo, viewed as frames.
- herdr-app issues found by `gh api` title search (UI, sidebar, cluttered, design, slow, lag, font, density, theme). Bodies read for #112, #617, #748, #13 and #214.

**Other tools:**

- Conductor, each checked with WebFetch against the source:
  - the home page;
  - the 0.35.0 and 0.44.0 changelog pages;
  - the announcement;
  - `llms-full.txt` (the Checks tab).
- Hacker News comments, read as full text through the Algolia API: 44627897, 44629675, 44635832, 44934481, 47871018, 48418811, 50370579 and 50371064, plus the herdr threads 49201003 and 50298436.
- Zed at `483b746`: `crates/ui/src/styles/typography.rs`, `spacing.rs`, `elevation.rs`; `list_item.rs`. Also the [key bindings](https://zed.dev/docs/key-bindings) and [command palette](https://zed.dev/docs/command-palette) docs.
- Linear:
  - [How we redesigned the Linear UI (part II)](https://linear.app/now/how-we-redesigned-the-linear-ui) (28 Mar 2024);
  - [A calmer interface for a product in motion](https://linear.app/now/behind-the-latest-design-refresh) (12 Mar 2026);
  - [the 25 Mar 2021 changelog](https://linear.app/changelog/2021-03-25-keyboard-shortcuts-help);
  - [reverse-linear-sync-engine](https://github.com/wzhudev/reverse-linear-sync-engine).

**Measurements:**

- Headless Chrome through playwright-core, scripts in `measure/` (results in `measure/raw/`).
- WCAG contrast ratios computed from the hex values.
- The recording diffed frame by frame (`measure/rec/`).

**Checked and not used:**

- A claimed Conductor line, "one notification per workspace to reduce noise", was found at no source.
- The New Stack's Conductor review did not load.
- releasebot.io's copy of Conductor's changelog is a secondary source.
- Linear's ⌘K and "peek" are described only by third-party pages.
- Some `linear.app` docs URLs returned 404.

**Not done:**

- WKWebView was not measured.
- herdr, Zed and Conductor were not installed or run.
- No shot of a docked question or of a board exists.

**Rests on memory:** nothing.
