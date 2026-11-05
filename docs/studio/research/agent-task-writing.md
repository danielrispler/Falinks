# What makes a good task for a coding agent

Research report for [#281](https://github.com/danielrispler/Falinks/issues/281), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-11-05.

## Card

- **Answer:** A good task gives the agent what it cannot find in the repo (the symptom, the expected behavior, the error text, a preference between approaches) plus a check that says pass or fail; the repo itself it reads well. Agents almost never ask when a task is vague, so the gap stays hidden. People get better by letting the agent interview them into a short spec first, and by checking the result against it.
- **Evidence:** Moderate on what a task needs: benchmark studies on SWE-bench Verified measure it, with LLM-simulated users. Weak on how people improve: one older study of novice prompt writers and vendor advice, no trial.
- **Supports:** "Evidence, not promises" (a check and its output are the core of a good task). Partly "Guessing, not testing" (writing "what done looks like" first is a prediction; untested as such). **Complicates:** "Non-blocking" (the gaps that matter are cheapest to fix before the run, so this step should block).
- **For Daniel:** Claude Code's [Best practices](https://code.claude.com/docs/en/best-practices), sections "Tell Claude what it can't see" and "Give Claude a check to run" (15 min). Then sections 1 to 4 of [Ambig-SWE](https://arxiv.org/html/2502.13069v3) (30 min).

## Words used here

- **Task:** the text a person gives a coding agent to start a piece of work: a prompt, an issue or a ticket.
- **Underspecified:** a task that leaves out information needed to solve it correctly, so several reasonable solutions exist and only some are right.
- **SWE-bench / SWE-bench Verified:** a benchmark of real GitHub issues from Python projects; an agent must write a fix that passes hidden tests. Verified is a 500-issue subset that people screened for clear issues and fair tests.
- **Resolve rate:** the share of tasks where the agent's fix passes the tests.
- **Simulated user:** in the studies below, a second LLM that holds the full issue text and answers the agent's questions. It stands in for a person; no study here used real people.
- **Acceptance criteria:** the conditions that must hold for a task to count as done, ideally checkable.
- **Context file:** a file such as `AGENTS.md` or `CLAUDE.md` that the agent reads at the start of every session.

Labels: **Established** means a primary source reports it. **Inference** means my reading of the sources, not something a study tested.

## 1. What vendors say a good task contains

**Point: tell the agent what it cannot see, and give it a check that comes back pass or fail.**

Established, from Claude Code's [Best practices](https://code.claude.com/docs/en/best-practices) page:

- Claude reads the repo well, so the task should hold only what is not in the repo, or what takes digging every session. Its test before writing something down: "Could it find this by reading the code? Would it actually go and look? If both are yes, leave it out."
- "Write a specific prompt": name the file, describe the symptom, and say what "fixed" looks like. Its example replaces "fix the login bug" with "users report login fails after the session times out. look at token refresh in src/auth/, write a failing test that reproduces it, then fix it". A preference between two reasonable approaches must be stated, since the code cannot show it.
- Hand over the thing itself instead of describing it: the file, the screenshot, the error output.
- Scope: Claude "tends to read a request conservatively and build the smallest thing that satisfies it". Say "be ambitious" with the parts you want when you want more.
- Vague prompts are fine when exploring; the cost is more correction later.
- "Give Claude a check to run": without one, "looks done" is all it has, and "that makes you the test suite". A check can be a test file, a build's exit code, a linter, a script that diffs against a fixture, or a screenshot beside the mockup. Ask for evidence: the command and its output.
- Before an unattended run, five questions: does it know what done looks like; can it check its own work; what may it touch; what stops it; where does the result go and who signs off.
- "Brainstorm with Claude first": when you do not know what you want, ask it to "Interview me until you could write the spec, then write it to SPEC.md", then build from that spec in a fresh session and review the diff against it.

Established, from Anthropic's [Prompting best practices](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/claude-prompting-best-practices), "Be clear and direct": "Show your prompt to a colleague with minimal context on the task and ask them to follow it. If they'd be confused, Claude will be too." It also says giving the reason behind an instruction helps the model aim its answer.

These are vendor guidance, not measured results.

## 2. What the benchmark studies measured

**Point: missing information roughly halves an agent's success, and the most valuable missing piece is the error itself.**

SWE-bench Verified (established, from OpenAI's August 2024 announcement as quoted in search results; the page itself returned 403): 93 Python developers screened 1,699 SWE-bench issues. Each was rated 0 to 3 on two criteria: is the issue underspecified, and do the tests reject valid solutions. 500 passed. The exact share rejected for vague issues is something I could not confirm (see "Sources checked"). So a large part of real GitHub issues were judged too vague to solve from the text alone, even by experienced developers.

[Ambig-SWE](https://arxiv.org/html/2502.13069v3) (Vijayvargiya, Zhou, Yerukola, Sap, Neubig, CMU; arXiv 2502.13069, v3 revised February 2026). Established:

- GPT-4o rewrote each of the 500 Verified issues into a short, vague version. Agents ran three ways: full issue, vague issue, and vague issue plus a simulated user who holds the full issue.
- Asking recovers much of the loss: with interaction, Claude Sonnet 4 reached 89% of its full-issue resolve rate, Claude Sonnet 3.5 and Haiku 3.5 up to 80%, Deepseek-v2 59%, Llama 3.1 70B 54% (section 3.2). The abstract reports gains "up to 74%" over the vague, non-interactive setting.
- Agents do not ask on their own: "Without explicit prompting, models almost never interact, even for severely underspecified inputs" (section 4.2). Told strongly to check, Claude Sonnet 4 told vague from full issues with 89% accuracy; Qwen 3 Coder stayed at 50%, chance (Table 2).
- Useful questions were specific and about the task. Questions about things the code already shows added little. Asking raised success but did not cut the number of steps.

[Asking What Matters](https://arxiv.org/html/2604.14624) (Vijayvargiya, Viswanathan, Neubig; arXiv 2604.14624, April 2026). Established:

- Removing information cut agent success from 43.8% to 23.7% across 700 instances.
- In 112 highly underspecified Verified issues, the missing information fell into six kinds: error information (65% of issues), implementation details (37%), expected behavior (33%), external references (15%), reproduction steps (12%), version or environment (3%).
- Error information mattered most to success, then implementation details, then expected behavior. The authors say the confidence intervals overlap, so the order is a trend, not a proven ranking.

Inference: the two lists agree. What makes a task good is the observed error or symptom, what should happen instead, and enough pointers to reproduce it. File locations help less than you would expect, because the agent can find them; Ambig-SWE's results on location hints were mixed across models.

## 3. Context files are not the task

**Point: a long standing file does not replace a specific task.**

[Evaluating AGENTS.md](https://arxiv.org/abs/2602.11988) (Gloaguen, Mündler-Sasahara, Müller, Raychev, Vechev, ETH Zurich; arXiv 2602.11988, v3 September 2026). Established: LLM-written context files did not raise success (about −0.5 points on SWE-bench Lite, −2 on the authors' CTXbench of 138 tasks; neither significant) and raised cost by about 20%. Developer-written files gave about +2.4 points on CTXbench, not significant. Agents did follow the files' instructions; repository overviews did not help. The authors advise keeping such files to what the agent needs and cannot find, such as non-standard conventions.

Inference: this matches section 1's "tell it what it can't see". Repeated rules belong in a context file; what this task needs belongs in the task.

## 4. Acceptance criteria and examples

**Point: requirements practice says the same thing in older words: state behavior as examples that can be checked.**

Established: Dan North's ["Introducing BDD"](https://dannorth.net/blog/introducing-bdd/) (Better Software, March 2006) writes stories as "As a [X], I want [Y], so that [Z]", where the "so that" line forces the value into the open, and acceptance criteria as scenarios: "Given some initial context, When an event occurs, Then ensure some outcomes." "A story's behaviour is simply its acceptance criteria," and each scenario can be run as a test.

Inference: a Given/When/Then line is close to a failing test written in prose. It gives the agent both the expected behavior and the check from section 1. The "so that" line is the "reason behind the instruction" Anthropic recommends.

## 5. How people get better at it

**Point: little direct evidence. The best-supported moves are to let the agent ask, and to write down what done looks like before the run.**

- Established, older and indirect: Zamfirescu-Pereira, Wong, Hartmann and Yang, ["Why Johnny Can't Prompt"](https://dl.acm.org/doi/10.1145/3544548.3581388) (CHI 2023), watched non-experts design chatbot prompts. They tried changes ad hoc rather than systematically, overgeneralized from single cases, and expected the model to understand like a person. This is about chatbot prompts and GPT-3-era models, not coding agents. I read only the abstract-level summary (see "Sources checked").
- Established: agents can close the gap if told to ask (Ambig-SWE), and Claude Code recommends exactly that ("Interview me until you could write the spec").
- I found no study that trains people to write better agent tasks and measures the result.

Inference, for the studio:

1. The interview is the practice loop. Each question the agent asks names a piece the person left out. Over time the person can see which kinds they keep leaving out; the six categories in section 2 are a ready checklist to count them.
2. Writing "done looks like" before the run is a prediction. Comparing it with the result is the same feedback loop as the guess-and-card idea, applied to the task instead of the change.
3. A task that came back wrong is the best teacher: the useful question is "which of the six kinds was missing?", not "was the agent bad?".

## 6. This repo's own ticket rules

[`docs/agents/ticket-authoring.md`](https://github.com/danielrispler/Falinks/blob/f856a09/docs/agents/ticket-authoring.md) at f856a09 requires `## Reading` (decisions and research the agent cannot infer, with a reason each) and `## Touches` (files, freeze, session). Ticket #281 itself adds "Question" and "Where to start".

Inference: `## Reading` is a strong version of "tell it what it can't see": settled decisions live in comments, not code. Two things the sources stress are not required fields: the check that proves the work is done, and the observed symptom or expected behavior. The file mentions a "stop condition" only in passing, and "verification" only at resolution. That is a possible gap, not a finding; the engine's `ci-checks` rule in `AGENTS.md` covers the check for engine work in general.

## 7. The hypotheses, one by one

- **Evidence, not promises:** supported. asking for evidence ("the command it ran and its output") is the vendor's own advice, and a pass/fail check is what lets the agent finish without you.
- **Guessing, not testing:** partly supported, by inference. Writing what done looks like before the run is a guess the result can confirm or refute. No source tests it as a learning tool.
- **Non-blocking:** complicated. The missing pieces cost most when found late (an unattended run "spent the whole night building on the wrong assumption", in Claude Code's words). The task-writing step, at least, should block, briefly.
- **Move learning into waiting time / Small and visual / A game / Control-room style / Learning content while waiting:** not addressed by these sources.
- **"It's hard to write a good task for an agent"** (Working well): explained. It is hard because the person must notice what is only in their head, and the agent will not tell them it is missing unless asked.

## Open questions

- Do people who use the interview step write better first tasks after a few weeks? No study; Daniel's own question records could show it.
- Do the six missing-information kinds, found in bug-fix issues, also fit feature tasks and research tickets?
- All interaction results use a simulated user. Real people answer slower and less completely.

## Sources checked

Checked by fetching the source on 2026-11-05:

- Claude Code [Best practices](https://code.claude.com/docs/en/best-practices): quotes in section 1, including the five questions and the interview prompt.
- Anthropic [Prompting best practices](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/claude-prompting-best-practices) (the "be clear and direct" URL redirects there): the colleague test quote.
- [Ambig-SWE](https://arxiv.org/html/2502.13069v3): authors, date, construction, 89/80/59/54% relative performance, "up to 74%", the "almost never interact" quote, Table 2 detection figures. The full-vs-vague drop is only in Figure 3, so I give no number for it.
- [Asking What Matters](https://arxiv.org/html/2604.14624): authors, date, 43.8% to 23.7%, the six categories and their shares, the caveat on overlapping intervals.
- [Evaluating AGENTS.md](https://arxiv.org/abs/2602.11988) and its HTML version: authors, dates, −0.5/−2 points, ~20% cost, +2.4 points, 138 tasks. The fetch noted the paper's own figures are not fully consistent (an intro margin of 7% against about 4.4 points in section 4.2).
- Dan North, ["Introducing BDD"](https://dannorth.net/blog/introducing-bdd/): templates, quote, dates.
- [`docs/agents/ticket-authoring.md`](https://github.com/danielrispler/Falinks/blob/f856a09/docs/agents/ticket-authoring.md): read in the repo.

Not checked against the primary source, resting on search summaries or memory:

- OpenAI's "Introducing SWE-bench Verified" page returned 403. The figures 93 developers, 1,699 samples, 0–3 labels and 500 kept come from search-result quotes of that page. The share of issues flagged as underspecified (I recall about 38%) is from memory and is left out of the text.
- "Why Johnny Can't Prompt": the ACM page returned 403 and the PDF could not be read. Authors, venue and the findings come from search-result summaries of the abstract.
- The authors' follow-up on SWE-bench Verified's retirement (February 2026) was seen only in search results and is not used.
