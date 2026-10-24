# Knowing which tool to use when

Research report for [#278](https://github.com/danielrispler/Falinks/issues/278), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). It informs the last "Working well" problem in [`problems.md`](https://github.com/danielrispler/Falinks/blob/660c11a/docs/studio/problems.md) and the decision on the control room's main job.

## Card

- **Answer:** Help built into the work and timed to its pause points works; catalogs you must remember to open mostly go unused, by people and agents alike. The best-supported mix is a short checklist for the known steps, plus a coach that sees which tools actually ran and quietly suggests the missing one, less often as skill grows. Daniel's ~100 skills and commands (9 sources) have only per-vendor routers that answer when asked, four skills claiming "review this PR", and nothing for waiting, approvals or Daniel's own growth.
- **Evidence:** Moderate. It is strong for checklists and tutoring (medicine, education), moderate for software command recommenders (small field studies), and missing for people choosing among AI-agent skills: no study of that exists yet.
- **Hypotheses:** Supports *Non-blocking*. Partly supports *Move learning into waiting time*, *Small and visual before text*, *Evidence, not promises* and *Serious control-room style*. Weakens *Learning content while waiting*. Says nothing on *Guessing, not testing* or *A game that rewards understanding, not volume*.
- **For Daniel:** Tovi Grossman, George Fitzmaurice and Ramtin Attar, ["A Survey of Software Learnability"](https://research.autodesk.com/app/uploads/2023/03/a-survey-of-software.pdf_recqWPTEYd6nsP0r8.pdf) (CHI 2009), about 40 minutes. Atul Gawande, ["The Checklist"](https://www.newyorker.com/magazine/2007/12/10/the-checklist) (The New Yorker, 2007), about 30 minutes.

## How to read this

Each section opens with its point in bold, so you can stop after the first sentence. Claims with a link come from that source. Paragraphs that begin with **Inference** are my reasoning, not something a source says. A few terms first:

- A **skill** is a folder with a `SKILL.md` file of instructions that Claude Code can load. Its short `description` tells the agent when to use it ([Claude Code docs, Skills](https://code.claude.com/docs/en/skills)).
- A **plugin** is a package of skills, commands, agents or servers, installed as one unit. Its skills get a name prefix such as `mattpocock-skills:tdd` ([Skills](https://code.claude.com/docs/en/skills)).
- A **slash command** is something you type at the start of a message, such as `/clear` or `/wayfinder` ([Commands](https://code.claude.com/docs/en/commands)).
- **Model-invoked** means the agent decides on its own to load a skill. **User-only** means only you can start it, by typing its name.

## 1. The problem has two layers

**"What to use when" is asked twice in Daniel's setup: once by Daniel, and once by the agent.**

Daniel picks a flow (grill, spec, research, review) and types or omits a skill name. Then the agent, reading a listing of skill descriptions, decides on its own whether to load any of them. Claude Code's docs say the description is what "Claude uses … to decide when to apply the skill" ([Skills](https://code.claude.com/docs/en/skills)). Both choices can go wrong separately. Section 9 covers the agent's layer; sections 3 to 8 cover the human one.

**Inference:** a control room that only teaches Daniel would leave the agent's layer untouched, and the reverse is also true. The research below suggests that the two layers need the same cure: put the right guidance where the choice is made, at the moment it is made.

## 2. Inventory: what Daniel has installed

**About 100 skills and commands sit on disk. The agent that wrote this report saw 66 entries in its listing, and most of the toolset serves "doing the work" and "writing the task", not Daniel's own awareness or growth.**

*Method.* As the ticket requires, I read only `SKILL.md` front matter (name and description), plugin manifests (`plugin.json`) and command description lines under `~/.claude/plugins/cache/` (installed plugins) and `~/.claude/skills/`. I did not read settings files. So I cannot tell which installed plugins are switched on. Where I say the agent "saw" a skill, I mean the skill listing given to this research agent, a background subagent in a Falinks worktree.

| Source | On disk | In this agent's listing | Notes |
| --- | --- | --- | --- |
| mattpocock-skills 1.3.1 | 34 skills (27 in its manifest) | 11 | 16 of the 27 are user-only, including the router `ask-matt`, `wayfinder`, `to-spec` and `retro` |
| caveman 3.2.0 | 22 skills, 1 command, 3 helper agents | 21 | always-on terse voice, plus task disciplines (`lean-build`, `surgical-patch`, `investigate-first`, `migration`) |
| ponytail 5.1.0 | 6 skills | 6 | always-on "smallest change" mode, plus review and audit reports |
| visual-explainer 0.12.0 | 1 skill, 8 commands | 8 | HTML diagrams, diff and plan reviews, slides, video |
| Daniel's own `dsh-*` | 3 skills | 2 | `dsh-explain-diff` says to invoke it "explicitly by name" |
| `explain-diff-html`, `find-skills` | 2 skills | 0 | |
| claude-plugins-official | 5 skills, 1 command, 2 MCP servers, 3 language servers | 0 | `claude-md-management`, `frontend-design`, `skill-creator`, `telegram`, `firebase`, `terraform`, Go, Rust and Swift LSPs |
| agents-observe 0.9.12 | 1 skill | 0 | "Real-time observability dashboard for Claude Code agents" |
| Skills synced from claude.ai | 15 skills | 3 | document formats, browser, computer use, deep research |
| Built into Claude Code | about 15 in the listing | about 15 | `/code-review`, `/security-review`, `/simplify`, `/loop`, `/init` and more |

The gap between "on disk" and "in the listing" has three causes that I can see. User-only skills are kept out of the listing on purpose: their "description [is] not in context" ([Skills, invocation table](https://code.claude.com/docs/en/skills)). Plugins can be installed but switched off. And the listing has a size budget (section 9). Settings decide the rest, and I was not allowed to read them.

### Grouped by the problem in `problems.md`

| Problem | What addresses it today | Coverage |
| --- | --- | --- |
| Don't know what agents are doing, or which phase | `agents-observe` dashboard; built-in `/tasks` and `/recap` | thin |
| Not sure if a run progresses or is stuck | `agents-observe`; `caveman-stats` (tokens only) | thin |
| Miss the moment an agent waits for approval | `telegram` channel (a messaging bridge, not an approval alert) | gap |
| Waiting is boring | nothing installed | gap |
| Don't understand what changed; review is hard; trust | six review tools (built-in `/code-review`, `/security-review`, `/simplify`; `mattpocock-skills:code-review`; `ponytail-review`; `caveman-review`) plus `ponytail-audit`; four diff explainers (`dsh-explain-diff`, `explain-diff-html`, `visual-explainer`, `visual-explainer:diff-review`); `pr` (before and after evidence); `verify-and-stop`; `fact-check` | crowded |
| Forget previous runs | `handoff`, `retro`, `revise-claude-md`, `claude-md-improver`, `domain-modeling` (ADRs), `project-recap`, `dsh-session-brief` | fair |
| Project moves faster than understanding | `teach`, `wait-what`, `grill-with-docs` | fair, all user-only |
| Long pages at the end | every explainer above produces an HTML page after the work | the tools *are* the pain |
| Hard to write a good task | `grilling`, `grill-me`, `grill-with-docs`, `to-spec`, `to-tickets`, `wayfinder`, `triage`, `to-questionnaire`, `prototype`, `research`, `deep-research`, `writing-for-agents` | crowded |
| Am I working well, am I improving? | `retro` (improves the environment, not Daniel); `ponytail-gain` (measures ponytail, not Daniel); `caveman-stats`, `caveman-evidence-review`, `caveman-learn` (cost); built-in `/insights`, `/skill-doctor`, `/usage` | gap for Daniel's own growth |
| Too many windows | `telegram`, Claude in Chrome, computer use | thin |
| Knowing what to use when | `ask-matt` (Matt's skills only, user-only), `caveman-help`, `ponytail-help`, `cavecrew` ("when to delegate"), `find-skills` (finds *more* skills to install), built-in `/powerup` | per-vendor only |

The rest of the toolset (`tdd`, `diagnosing-bugs`, `surgical-patch`, `safe-refactor`, `migration`, `codebase-design`, `wizard`, `frontend-design`, the document skills) helps the agent do the work. None of it is aimed at Daniel's understanding.

### Grouped by when they say to use them

| How it fires | Examples | What it needs from Daniel |
| --- | --- | --- |
| Always on, injected when a session or subagent starts | `caveman`, `ponytail` (both arrived that way in the session that wrote this report) | nothing, but the modes stack |
| Agent matches the request to the description | most caveman and ponytail skills, `tdd`, `diagnosing-bugs`, `code-review`, `research` | the right words in the prompt |
| User-only, by name | 16 mattpocock skills (`wayfinder`, `to-spec`, `to-tickets`, `implement`, `retro`, `handoff`, `teach`, `wait-what`, `ask-matt` and more), `megacave`, `ultracave`; `dsh-explain-diff` and `dsh-session-brief` by their own text | remembering that it exists |
| Named by a project rule | Falinks `AGENTS.md` and the map Notes: `research` for research tickets, `grilling` and `domain-modeling` for grilling tickets, `prototype` for prototype tickets, suggest `dsh-explain-diff` after meaningful tickets | nothing; the agent reads the rule |
| One-shot report | `ponytail-audit`, `ponytail-debt`, `ponytail-gain`, `caveman-help`, `/insights` | knowing when a report is due |
| Set up once | `setup-matt-pocock-skills`, `caveman-setup`, `caveman-init`, `git-guardrails-claude-code`, `setup-pre-commit`, Telegram `configure` | nothing after setup |
| Background delegation | `research`, `deep-research`, the three `cavecrew` agents, `caveman-explore` | nothing |

By phase of work, the descriptions sort into *before* (grilling, spec, wayfinder, research, prototype), *during* (tdd, diagnosing, patching, refactoring, migration, verification), *after* (the reviews, explainers, `pr`, `retro`), and *between sessions* (`handoff`, `revise-claude-md`, `/recap`). Only one source explains how its skills combine across phases: `ask-matt`'s map of a "main flow", "on-ramps" and "phase boundaries", and it covers Matt's skills only.

### Overlaps

- **One phrase, four claimants.** `caveman-review` lists "review this PR", `ponytail-review` lists "review my PR", `mattpocock-skills:code-review` lists "review a branch, a PR", and the built-in `/code-review` takes a PR number. Since the agent decides by matching descriptions ([Skills](https://code.claude.com/docs/en/skills)), which review runs depends on wording, not on what Daniel needs.
- **Diff explainers:** `dsh-explain-diff`, `explain-diff-html`, `visual-explainer` and its `diff-review` command all promise an HTML explanation of a change.
- **Diagnosis:** `diagnosing-bugs` and `investigate-first`. **Build discipline:** `tdd`, `lean-build` and the always-on `ponytail`. **Exploring code:** `caveman-explore`, the `cavecrew-investigator` agent and the built-in Explore agent. **Research:** `research`, the `deep-research` skill and the built-in `/deep-research` workflow ([Commands](https://code.claude.com/docs/en/commands)).
- **Handover:** `handoff`, `claude-handoff` (on disk, outside the manifest) and the built-ins `/compact`, `/fork` and `/background`.
- **Agent instruction files:** `claude-md-improver`, `revise-claude-md`, `caveman-compress`, `caveman-learn` ("trim a heavy CLAUDE.md") and `writing-for-agents`. **Skill authoring:** `skill-creator` (installed twice, once from the official marketplace and once synced) and `writing-for-agents`.
- **Voice:** two always-on modes (`caveman`, `ponytail`) both shape replies. `AGENTS.md` already needs a precedence rule: `learning.md` "overrides any terse or brevity output mode" during planning and explanations.

### Gaps

- Nothing for waiting, and nothing that alerts Daniel when an agent needs a decision.
- Nothing small or during the work for understanding a change. Every explainer is an after-the-fact page.
- Nothing records which tools Daniel or the agent chose, when, and how it went. `/skill-doctor` counts how often skills are used ([Skills, Find unused skills](https://code.claude.com/docs/en/skills)), but it counts usage, not whether a choice was right.
- No router spans all sources. The one real router, `ask-matt`, is user-only, so it helps only when Daniel remembers to ask.

**Inference:** the inventory already holds a performance-support artifact that works: the Falinks rules that name a skill per ticket type. It is short, it lives where the agent reads it, and it fires at a known pause point (claiming a ticket). Section 6 explains why that pattern holds up.

## 3. Catalogs and on-demand help: people rarely ask

**When help waits to be requested, most people do not request it at the right times, and they keep using the methods they already know.**

- A review of on-demand help in learning software found that effective help seeking goes with better learning, yet "learners are not using help facilities effectively" ([Aleven, Stahl, Schworm, Fischer and Wallace, 2003](https://doi.org/10.3102/00346543073003277)).
- In Cognitive Tutors, students both overused hints, "asking for hints more often or faster than appropriate", and avoided help "when it is likely needed". Early attempts to teach better help seeking had "some limited success" ([Koedinger and Aleven, 2007, manuscript](https://pslcdatashop.web.cmu.edu/KDDCup/FAQ/Koedinger-Aleven-EPR-07.pdf)).
- In professional software, "despite experience, many users with basic command knowledge do not progress to an efficient use", because efficient strategies are "suggested by neither tasks nor tools". The authors had promising results from teaching those strategies explicitly ([Bhavnani and John, 2000](https://doi.org/10.1207/S15327051HCI1523_3)). This is the long-known "paradox of the active user": people skip learning because they want to get the task done ([Carroll and Rosson, 1987, summarised by Nielsen Norman Group](https://www.nngroup.com/articles/paradox-of-the-active-user/)).
- Grossman, Fitzmaurice and Attar sorted the learning problems they watched in AutoCAD into five kinds: understanding the **task flow**, **awareness** that a tool exists, **locating** it, **understanding** how it works, and **transitioning** to a more efficient method the user already knows about but does not use. Awareness was "a major issue for the most novice users". A think-aloud study caught "no transition issues", because users cannot report what they do not notice. Only an expert watching and suggesting caught them ([Grossman, Fitzmaurice and Attar, 2009](https://research.autodesk.com/app/uploads/2023/03/a-survey-of-software.pdf_recqWPTEYd6nsP0r8.pdf)).

**Inference:** Daniel's sentence in `problems.md` ("knowing what to use when, how to combine it all correctly, and whether I'm doing it well") maps cleanly onto those categories. "What to use" is awareness. "How to combine" is task flow. "Whether I'm doing it well" is transition, the kind a person cannot see in their own work. The vendor `-help` skills and `ask-matt` are catalogs. The evidence says catalogs help with *locating* and *understanding*, once a person already knows to look.

## 4. Help built into the work beats help beside it

**Help works better when it is part of the screen where the work happens than when it sits in a separate manual or site.**

Gloria Gery's 1991 book defined an *electronic performance support system* (EPSS): an environment that gives "immediate, individualized on-line access" to information, guidance and tools at the moment of work, "with minimal support and intervention by others". This research could not open the book, so that wording comes from a secondary source ([Wikipedia, EPSS](https://en.wikipedia.org/wiki/Electronic_performance_support_systems)). Later work splits the idea three ways. *Intrinsic* support is part of the work interface itself. *Extrinsic* support is in the same system but beside the work. *External* support is outside the system, such as a manual or a web search.

A controlled study had employees complete a software task with intrinsic, extrinsic, external or no support. It found that "providing any kind of EPSS … is better than having none at all", and that designers improve performance, attitudes and use "by creating systems that integrate with the primary work interface" ([Nguyen, Klein and Sullivan, 2005](https://doi.org/10.1111/j.1937-8327.2005.tb00351.x)). The full text is a scanned PDF that this research could not read, so the effect sizes are unchecked.

**Inference:** for Daniel, "the primary work interface" is the agent session and its pause points (ticket claim, plan, approval, review). Help that shows up there is intrinsic. A YouTube video or a separate catalog page is external, which is the weakest kind in this framing.

## 5. Recommenders: personal, ambient, self-paced

**A recommender that compares your usage with others' can surface tools you never found. It works best as a quiet side panel that you consult when ready, not as a pop-up.**

- *CommunityCommands* recommended AutoCAD commands to each user from the usage of thousands of others (*collaborative filtering*: suggesting what similar users use). In a three-month study with professional users, its best algorithm gave "2.1 times as many good suggestions as existing techniques". It showed its suggestions in an "ambient" palette that "the user can refer to when convenient", explicitly to avoid the "harmful effects that notifications can have" ([Matejka, Li, Grossman and Fitzmaurice, 2009](https://doi.org/10.1145/1622176.1622214); [PDF](https://research.autodesk.com/app/uploads/2023/03/communitycommands-command-recommendations-for.pdf_recZiyrKCEIuGQLZh.pdf)). The study was small: 17 users finished it. Its design rules include "Self Paced" and "Minimal Cost for Poor Suggestions". A later deployment reached more than 1,100 users in a year ([Li and others, AAAI 2014](https://ojs.aaai.org/index.php/AAAI/article/view/19034)).
- For programmers, "developers often only use a small subset of the commands" of their IDE. Recommending new ones from usage history is "feasible", and "patterns of past discovery" help ([Murphy-Hill, Jiresal and Murphy, 2012](https://doi.org/10.1145/2393596.2393645)).
- *Ambient Help* showed context-matched help videos on a second screen while people worked. People found "more helpful information" with no "negative impact on the user's productivity" ([Matejka, Grossman and Fitzmaurice, 2011](https://doi.org/10.1145/1978942.1979349)).
- *Skillometers* are small displays that show what you would gain by "adopting a better technique". In a controlled experiment, they led to "earlier and faster learning" of shortcuts ([Malacria, Scarr, Cockburn, Gutwin and Grossman, 2013](https://doi.org/10.1145/2501988.2501996)).
- Eric Horvitz, whose Lumière research prototypes "served as the basis for components of the Office Assistant" in Office 97 ([Horvitz, Lumière project page](https://erichorvitz.com/lumiere.htm)), later wrote design principles for assistants that act on their own. Among them: "minimizing the cost of poor guesses about action and timing", and "scoping precision of service to match uncertainty", meaning it is better to do less when unsure ([Horvitz, 1999](https://doi.org/10.1145/302979.303030); [PDF](https://erichorvitz.com/chi99horvitz.pdf)).

**Inference:** a "which skill now?" suggestion should look like CommunityCommands, not like a pop-up: a short list beside the work, cheap to dismiss, consulted at a pause, and silent when unsure.

## 6. Checklists: for known pause points, if the team owns them

**Checklists reliably help with steps that are known, critical and easy to skip, at fixed pause points. They do little when imposed without real use.**

- Degani and Wiener's NASA field study of flight-deck checklists notes that "improper use, or nonuse" of the normal checklist is often cited in accidents. It finds that design, length and use are shaped by airline "culture", cockpit resource management and "production pressures", and it ends with design guidelines ([Degani and Wiener, 1993](https://doi.org/10.1177/001872089303500209); [NASA report, 1990](https://ntrs.nasa.gov/citations/19910017830)). The report is a scanned PDF that this research could not read beyond its abstract.
- In eight hospitals in eight countries, a 19-item surgical checklist "designed to improve team communication and consistency of care" went with deaths falling from 1.5% to 0.8% and complications from 11.0% to 7.0% ([Haynes and others, 2009](https://doi.org/10.1056/NEJMsa0810119)). An earlier ICU program in Michigan cut catheter infections by up to 66% over 18 months ([Pronovost and others, 2006](https://doi.org/10.1056/NEJMoa061115)). Gawande's article tells that story ([Gawande, 2007](https://www.newyorker.com/magazine/2007/12/10/the-checklist)).
- After an Ontario policy pushed every hospital to adopt a checklist, deaths (0.71% before, 0.65% after) and complications across 101 hospitals did not change significantly ([Urbach and others, 2014](https://doi.org/10.1056/NEJMsa1308261)). That study used administrative data, so it could not check whether teams actually ran the checklist.

**Inference:** a checklist encodes the "when" for routine situations: at this pause point, do these few things. It does not teach the "when" for new situations. Daniel's project rules (section 2) are already such a checklist, and they work because Daniel wrote them and the agent reads them every time. A checklist written for Daniel by someone else, or one with fifty items, would be closer to Ontario than to the eight hospitals.

## 7. Scaffolding has to fade

**Good help adapts to the learner and withdraws as skill grows. Help that suits a novice can slow down someone who has moved on.**

- Wood, Bruner and Ross coined *scaffolding* for what a tutor does: recruiting interest, reducing the "degrees of freedom" (making the task smaller), keeping direction, marking critical features, controlling frustration, and demonstrating ([Wood, Bruner and Ross, 1976](https://doi.org/10.1111/j.1469-7610.1976.tb00381.x); list as restated in [Stone, 1998](https://www.sfu.ca/~jcnesbit/EDUC220/ThinkPaper/Stone1998.htm), because the original was paywalled).
- A review of a decade of research names its three defining features: *contingency* (help fits the learner's current state), *fading* (help is withdrawn over time) and *transfer of responsibility* (the learner takes over). It adds that the few effectiveness studies "suggest that scaffolding is effective", but that measurement is the main problem ([van de Pol, Volman and Beishuizen, 2010](https://doi.org/10.1007/s10648-010-9127-6)).
- The *expertise reversal effect*: techniques "highly effective with inexperienced learners can lose their effectiveness and even have negative consequences" with experienced ones ([Kalyuga, Ayres, Chandler and Sweller, 2003](https://doi.org/10.1207/S15326985EP3801_4)).
- How much to give and when to hold back is still "a fundamental open problem", which Koedinger and Aleven call the *assistance dilemma* ([Koedinger and Aleven, 2007](https://doi.org/10.1007/s10648-007-9049-0)).

**Inference:** a coach that suggests the same skill every time will become noise. It needs a memory of what Daniel already uses well, so that it can stop suggesting it. That is the fading. Nothing in the inventory has such a memory.

## 8. Peers and coaches are the strongest known channel

**Watching someone skilled, or being watched by them, is the most effective way to discover tools. It is also rare. A tutor who gives feedback step by step comes close to that.**

- In interviews with 18 programmers, "peer observation and peer recommendation were rated as the most effective modes" of discovering tools. A diary study of 76 software users then found that people "discover tools from peers only once every few months" ([Murphy-Hill, Lee, Murphy and McGrenere, 2015](https://doi.org/10.1007/s10606-015-9230-9)). Grossman and colleagues built their study method on the same "over the shoulder" learning ([Grossman, Fitzmaurice and Attar, 2009](https://research.autodesk.com/app/uploads/2023/03/a-survey-of-software.pdf_recqWPTEYd6nsP0r8.pdf)).
- A review of tutoring experiments found human tutoring had an effect size of d = 0.79, and intelligent tutoring systems, which mostly give feedback at each step of a problem or finer, d = 0.76: "nearly as effective as human tutoring". *Effect size d* is the gap between groups measured in standard deviations, and 0.8 is conventionally large ([VanLehn, 2011](https://doi.org/10.1080/00461520.2011.611369)).

**Inference:** an agent's run is a stream of steps: a skill loaded, a tool called, a check run. That stream is the closest thing Daniel has to "watching a peer work". A coach that reads it at pause points could give step-level feedback ("this run skipped the test-first skill you use for this kind of ticket") without a human expert. No study has tested this for AI agents.

## 9. Knowing "when" is its own kind of knowledge, and it grows from outcomes

**Knowing a tool and knowing when to use it are different things. The second grows from seeing how earlier choices turned out, and people judge their own tool use poorly.**

- Reading researchers separated *declarative* knowledge (what a strategy is), *procedural* knowledge (how to do it) and *conditional* knowledge (when and why to use it) ([Paris, Lipson and Wixson, 1983](https://doi.org/10.1016/0361-476X%2883%2990018-8)). This research could not open the paper, so this is the standard summary of its three-way split, not a quote.
- A year-long study of children learning multiplication found four separate kinds of improvement: "introduction of new strategies, increasing use of the most efficient existing strategies, more efficient execution of each strategy, and more adaptive choices among strategies" ([Lemaire and Siegler, 1995](https://doi.org/10.1037/0096-3445.124.1.83)).
- Self-judgment is unreliable here. In a 2025 randomized trial, experienced open-source developers using AI took 19% longer, yet "still believed AI had sped them up by 20%" ([METR, 2025](https://metr.org/blog/2025-07-10-early-2025-ai-experienced-os-dev-study/)). METR's 2026 follow-up calls its newer data "an unreliable signal", partly because timing breaks down when developers run several agents at once ([METR, 2026](https://metr.org/blog/2026-02-24-uplift-update/)).
- People new to prompting worked "opportunistically, not systematically" and tended "to overgeneralize" from a few results ([Zamfirescu-Pereira and others, 2023](https://doi.org/10.1145/3544548.3581388)).

**Inference:** "Am I doing it well?" cannot be answered from Daniel's feeling, and it cannot be answered from token counts. It needs a record of choices joined to outcomes: which skills ran on which kind of ticket, and what happened next (checks passed, rework, reverts). Lemaire and Siegler's four kinds of change give a plain progress vocabulary: new tools adopted, better tools used more often, tools used better, and better choices.

## 10. The agent's side: more skills, worse choosing

**Agents also skip skills they have, and Claude Code thins the descriptions it shows the agent once there are too many. Fewer, focused skills, or guidance that is always in context, beat a large opt-in library.**

- Vercel's evaluation of a coding agent on Next.js tasks found that "in 56% of eval cases, the skill was never invoked". The skill alone scored 53%, the same as no docs. With explicit instructions to use it, the score rose to 79%. A short docs index placed directly in `AGENTS.md` scored 100%, which the authors put down to "no decision point". They also found results swung on "subtle wording changes" ([Vercel, Gao, 2026](https://vercel.com/blog/agents-md-outperforms-skills-in-our-agent-evals)). This is one vendor's evaluation on one framework.
- In Claude Code, the skill listing has a budget of "1% of the model's context window". When it overflows, Claude Code "drops descriptions starting with the skills you invoke least", which "removes the keywords Claude needs to match your request". User-only skills are left out of the listing entirely. `/skill-doctor` reports each skill's cost and use and "flags skills in the listing that have never been invoked" ([Skills](https://code.claude.com/docs/en/skills)).
- For tools in general, Anthropic's docs state that "Claude's ability to pick the right tool degrades once you exceed 30–50 available tools" ([Tool search tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool)).
- *SkillsBench* (87 tasks, 18 agent and model setups) found that hand-curated skills raised average pass rates from 33.9% to 50.5%. Focused skills with at most three modules beat bigger bundles, and four or more skills added only about 10 points. Skills the agent wrote for itself scored below no skills at all on all three dedicated harnesses, including Claude Code ([Li and others, 2026, v4](https://arxiv.org/abs/2602.12670); [full text](https://arxiv.org/html/2602.12670v4)). Its "Finding 4" says discovery "is usually not the bottleneck". But each task received only its own relevant skills, so the benchmark does not test choosing from a large library.

**Inference:** the agent in this session saw 66 entries, already past the 30–50 range where tool choice degrades. The listing and the tool list are not the same mechanism, so this is a warning sign, not a proof. The Vercel result and Falinks' own practice point the same way: for the few routings that matter most, a rule in `AGENTS.md` beats hoping the agent picks the skill.

## 11. Putting it together

**The research points to three pieces that work together. None of them is a bigger catalog.**

**Inference** (the whole section): this is my synthesis of sections 3 to 10, offered as input for the decision tickets, not as a decision.

1. **A short checklist per pause point**, written by Daniel and read by the agent: which skill at ticket claim, before a plan, after a change. Falinks already has the start of this in `AGENTS.md` and the map Notes (section 6).
2. **A smaller, sharper toolset for the agent:** retire overlapping skills, starting with the four review claimants and the four diff explainers, using `/skill-doctor` and the `/skills` menu ([Skills](https://code.claude.com/docs/en/skills)). Put the few must-route rules into always-loaded context (section 10).
3. **A coach that watches the run**, the control-room part: it shows which skills and checks actually ran, and at pause points it suggests the one tool that was missing, in a side panel, silently when unsure, and less often as Daniel adopts it (sections 5, 7 and 8).

Claude Code already ships pieces of each that need no new code. `/insights` writes a report on recent sessions covering "friction points" and "suggestions for using Claude Code more effectively" ([Costs, Analyze your usage patterns](https://code.claude.com/docs/en/costs)). `/powerup` gives "quick interactive lessons" ([Commands](https://code.claude.com/docs/en/commands)). Spinner tips can be replaced with your own text, which is an ambient, during-the-wait channel ([Settings reference, `spinnerTipsOverride`](https://code.claude.com/docs/en/settings-reference)). A `UserPromptSubmit` hook can add context next to each prompt, so the agent sees routing guidance at the moment of choice ([Hooks](https://code.claude.com/docs/en/hooks)).

### What this means for the hypotheses

| Hypothesis in `problems.md` | Effect | Why |
| --- | --- | --- |
| Move learning into waiting time | partly supports | just-in-time, in-context help works (sections 4 and 5); waits are natural pause points, though no study tested waits during agent runs |
| Small and visual before text | partly supports | checklists and recommendation panels are short; this research found nothing on "visual" specifically |
| Non-blocking | supports | ambient, self-paced help worked without hurting productivity (section 5); poorly timed help has a cost (Horvitz) |
| Guessing, not testing | not addressed | |
| Evidence, not promises | partly supports | self-judgment of tool use is unreliable (section 9), so feedback needs a record of what actually ran |
| A game that rewards understanding, not volume | not addressed | Skillometers show that displaying the benefit of a better technique speeds adoption, but they are not a game |
| Serious control-room style | partly supports | aviation checklists and fixed pause points fit a briefing and debrief rhythm; style itself was not studied |
| Learning content while waiting | weakens, for learning tools | external help is the weakest kind (section 4); peer and in-context discovery beat tutorials (section 8) |

## Open questions

- **No direct study.** Nobody has studied how people learn to choose among AI-agent skills. Everything human-side here comes from AutoCAD, IDEs, tutoring, cockpits and operating rooms. For the hypothesis verdicts, that means "partly supports" should stay "partly" until Daniel's own use tests it.
- **Does this problem sit above the others?** The inventory shows that most problems already have tools, often several. So the bottleneck looks like selection and combination, which argues for placing "what to use when" across the other problems as a layer, not as one more feature. That is an inference. The first decision ticket settles it with Daniel.
- **What does the coach read?** Seeing which skills ran needs session records. Claude Code keeps local session history, and `/insights` and `/skill-doctor` read it, but this research did not open any of it. Which records exist, their format, and what the map's rules allow are questions for [#282](https://github.com/danielrispler/Falinks/issues/282).
- **How do we know a choice was right?** No validated measure exists for "the right skill at the right time". Candidate signals (rework, reverts, failed checks after a skill was skipped) are untested. This bears on "Measuring the app itself" in the map.
- **How small is small enough?** SkillsBench suggests three or fewer skills per task, and the tool docs say 30–50 tools. No source gives a threshold for Claude Code's skill listing. Daniel could measure the current setup with `/skill-doctor`.
- **Why are some installed skills missing from the agent's listing?** Settings were off-limits, so I cannot say whether `dsh-explain-diff`, `explain-diff-html`, `find-skills`, `agents-observe` and the official plugins are switched off on purpose.
- **How should a coach fade without a model of Daniel's skill?** The assistance dilemma is open even in well-studied tutors. For v1, a simple rule (stop suggesting a skill once Daniel has used it unprompted a few times) is a guess to test, not a finding.
- **For v1:** the three pieces in section 11 can be composed from existing tools: rules files, `/skill-doctor`, `/insights`, spinner tips and hooks. Whether that is enough before building a coach panel is a decision for the v1 spec.
