# Comprehension debt and AI

Research report for [#274](https://github.com/danielrispler/Falinks/issues/274), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-10-24.

## Card

- **Answer:** AI help lowers what people understand when they hand the thinking over, and people often misjudge how much they understand. What protects understanding is active engagement during the work: asking why, predicting, or solving a step before the answer appears. Reading explanations afterwards is not enough.
- **Evidence:** Moderate. The harm shows up consistently in several randomized trials (coding, math), but they are short, small or done with learners. Evidence for specific countermeasures is weak: small groups, underpowered tests, mostly correlation.
- **Supports:** "Guessing, not testing" (prediction before the answer is well established; the "no score" part is untested). Partly supports "Move learning into waiting time" (engagement during the work helps; no study tests learning while waiting on an agent) and "Small and visual before text" (small pieces yes, visual untested).
- **Weakens or complicates:** "Non-blocking" (the measures that work cost effort, and people skip optional ones). "Learning content while waiting" (passive material is the weakest form). "Evidence, not promises" is mixed: explanations raise trust without raising understanding.
- **For Daniel:** Anthropic's [research page](https://www.anthropic.com/research/AI-assistance-coding-skills) (10 min), then sections 6 and 7 of the [paper by Shen and Tamkin](https://arxiv.org/html/2601.20245v2) (40 min). Then [Balepur et al., "(Im)Paired Programming"](https://arxiv.org/html/2607.26375v1), the closest study to agent work (30 min).

## Words used here

Each term is defined once here, so later sections can be read on their own.

- **Comprehension debt:** the gap between how much code exists and how much of it any person really understands ([Osmani](https://addyosmani.com/blog/comprehension-debt/)). A practitioner term, not a research construct.
- **Cognitive debt and intent debt:** Margaret-Anne Storey's split. Cognitive debt is a team's shared understanding wearing away. Intent debt is missing written rationale: goals, constraints and the reasons for decisions ([Storey, 2026](https://arxiv.org/abs/2603.22106)).
- **Randomized controlled trial (RCT):** people are assigned to groups by chance, so a difference between groups can be blamed on the treatment.
- **Effect size (Cohen's d):** how big a difference is, in units of the spread of scores. About 0.2 is small, 0.5 medium and 0.8 large.
- **p-value:** how surprising the result would be if there were no real effect. Below 0.05 is the usual bar for "significant". It says nothing about size.
- **Underpowered:** too few participants to detect an effect of the expected size reliably.
- **Metacognition and calibration:** metacognition is judging your own knowledge. You are calibrated when your confidence matches your actual accuracy.
- **Illusion of explanatory depth:** believing you understand how something works in more detail than you do ([Rozenblit and Keil, 2002](https://pmc.ncbi.nlm.nih.gov/articles/PMC3062901/)).
- **Cognitive offloading:** handing mental work to a tool or to another person.
- **Overreliance:** accepting an AI's answer even when it is wrong.
- **Cognitive forcing function:** a design that makes a person think before accepting an AI's answer, for example by asking for their own answer first ([Buçinca et al., 2021](https://arxiv.org/abs/2102.09692)).

Labels used below: **Established** means a primary source reports it. **Inference** means it is my reading of the sources, not something a study tested.

## 1. Where the term comes from

**Point: "comprehension debt" is a 2025–2026 practitioner term; research measures nearby things, not the term itself.**

Established:

- Addy Osmani's post (14 March 2026) defines comprehension debt and says it is "also called cognitive debt". It argues that passing tests and approved reviews can hide it ([Osmani](https://addyosmani.com/blog/comprehension-debt/)).
- Margaret-Anne Storey's blog post (9 February 2026) describes a student team that, by week seven, could not make simple changes without breaking something. Nobody could explain why the design was as it was ([Storey, blog](https://margaretstorey.com/blog/2026/02/09/cognitive-debt/)). Its countermeasures: at least one person fully understands each AI-generated change before it ships, the reasons for changes are written down, and the team holds regular checkpoints. Her later preprint turns this into a "Triple Debt Model": technical, cognitive and intent debt ([Storey, arXiv](https://arxiv.org/abs/2603.22106)). It is a position paper with no new data.
- Storey grounds the idea in Peter Naur's 1985 essay "Programming as Theory Building" ([scan](https://pages.cs.wisc.edu/~remzi/Naur.pdf)): a program is a theory held in its programmers' heads, and the code captures only part of it. I could not read the scan's text, so this relies on Storey's summary.
- "Cognitive debt" became popular through the MIT Media Lab preprint "Your Brain on ChatGPT" ([Kosmyna et al., 2025](https://arxiv.org/abs/2506.08872)). Its participants wrote essays with an LLM, with web search, or with no tools. LLM users showed the weakest brain connectivity on EEG, often could not quote their own essays, and felt the least ownership. It is a preprint with 54 participants, only 18 of whom did the final session. Treat it as weak evidence.
- Who first coined "comprehension debt" is unsettled. Blogs credit Jason Gorman in 2025, but I found no primary post.

Inference: Osmani quotes unattributed figures: delegators "below 40%" and conceptual askers "above 65%". These match the interaction-pattern clusters in Anthropic's study (section 2), so that is almost certainly their source.

What the research measures instead: a quiz right after a task, the ability to extend your own code, depth of learning, and how well people judge their own knowledge. No study measures a team's or a supervisor's "theory" of a system over months.

## 2. The primary Anthropic study

**Point: in a randomized trial, AI help reduced how much developers learned about a new library, with no clear time saving.**

The primary source is the paper "How AI Impacts Skill Formation" by Judy Hanwen Shen and Alex Tamkin ([arXiv 2601.20245](https://arxiv.org/abs/2601.20245), submitted 28 January 2026, revised 1 February 2026). Anthropic's [research page](https://www.anthropic.com/research/AI-assistance-coding-skills) (29 January 2026) is the authors' own summary. The study was pre-registered ([OSF](https://osf.io/w49e7)), and the annotated screen-recording transcripts are public ([GitHub](https://github.com/safety-research/how-ai-impacts-skill-formation)).

Design (established):

- 52 mostly junior developers, 26 per group, recruited from a crowd-work platform and paid $150. All used Python weekly and had used AI coding assistants, and none had used Trio, a Python library for asynchronous programming.
- A warm-up, then 35 minutes to build two features with Trio, then a quiz without AI. Participants knew the quiz was coming.
- The AI group had a GPT-4o chat assistant in a sidebar. It could see their code and write full solutions. The control group coded by hand.
- The quiz had 14 questions worth 27 points: conceptual, code reading and debugging. Code writing was left out to avoid noise from syntax slips.
- Four pilots shaped the design. In the first pilots, 25–35% of the no-AI group used AI anyway, so the main study used a platform with screen recording to check compliance.

Results (established):

- Quiz: the AI group scored lower, by 4.15 of 27 points. That is Cohen's d = 0.738 and p = 0.010, a medium-to-large effect. The research page shows about 50% against 67% and calls it "nearly two letter grades". The paper calls it a 17% difference.
- Time: the AI group finished about two minutes faster, which was not significant (p = 0.391). Screen recordings show why: up to 11 minutes went into composing queries.
- The biggest gap was in debugging questions. The control group met more errors, including Trio-specific ones (median 3 against 1). The authors argue that fixing those errors alone built the skill, but they did not test that directly.
- Self-reports: the control group reported learning more. The AI group found the task easier. One AI participant wrote: "I feel like I got lazy."

The paper's six interaction patterns (established, but correlational and with tiny groups):

| Pattern | n | What they did | Quiz |
| --- | --- | --- | --- |
| AI delegation | 4 | Let the AI write the code (fastest, about 19.5 min) | 39% |
| Progressive AI reliance | 4 | A few questions, then delegated everything | 35% |
| Iterative AI debugging | 4 | Used the AI to debug or verify (slowest, about 31 min) | 24% |
| Generation, then comprehension | 2 | Generated code, then asked follow-up questions about it | 86% |
| Hybrid code and explanation | 3 | Asked for code together with explanations | 68% |
| Conceptual inquiry | 7 | Asked only concept questions and fixed errors alone | 65% |

The authors stress that these patterns show association, not cause.

Limitations the authors state: a small sample; one task and one chat interface; learning measured over about an hour, not months; no comparison with help from a human; crowd workers with no job reason to learn Trio. They expect agentic tools such as Claude Code to cut engagement further, and they call their chat setting a likely lower bound on the harm. The authors work at Anthropic, and the paper recommends learning modes, including Claude Code's.

Inference: "Iterative AI debugging" is the closest pattern to supervising an agent: the AI produces, the human asks the AI to check. It scored lowest. If that holds up, having the agent check its own work does nothing for the human's understanding.

## 3. With an agent, the harm repeats, and users know

**Point: the one study of a code-editing agent found the same loss of understanding, and users saw it but still preferred the agent.**

Established, from [Balepur et al., "(Im)Paired Programming"](https://arxiv.org/abs/2607.26375) (preprint, 29 July 2026):

- 54 computer-science students built a small web game in 50 minutes. One group used an agent that edits code and proposes diffs (Aider with GPT-4.1). The other used a restricted chatbot that only gave syntax help and snippets of five lines or fewer.
- Then everyone had 20 minutes to extend their own code with the chatbot only, followed by comprehension questions about their code.
- The agent group finished the first task better and faster but scored much lower on comprehension (p < 0.002, d > 0.8). On the extension task the two groups did about the same: the agent's head start did not carry over.
- Self-reports, on a 1–5 scale: the agent group rated its understanding lower (3.3 against 4.2), its mental effort far lower (1.9 against 3.7) and the code less its own. Yet most still preferred the agent. One participant: "I didn't have to think at all."
- Review style mattered. Auto-accepting every edit went with the lowest comprehension (0.615, 6 users). Accepting file by file went with the highest (0.777, 7 users), though still below the chatbot group. Pasting the task text as the prompt, the most common strategy (25 users), went with low comprehension. Shorter agent code (fewer lines, and even fewer comments) went with better comprehension.
- The authors' design suggestions: discourage low-effort prompting, make review active rather than passive, optimize agents for readable code, and make the value of understanding visible.

Limitations: students, one task, LLM-generated questions, exploratory analysis.

Inference: this is the most important finding for the control room. Here the problem was not an illusion. People knew they understood less, and they chose the easy path anyway. A tool that only informs Daniel of the gap may not be enough; it also has to make engagement cheap.

## 4. The same pattern elsewhere

**Point: across math class, novice programmers and professional engineers, AI raises performance with help and lowers skill without it.**

Established:

- **High-school math, field RCT.** About 1,000 Turkish students had GPT-4 tutors for four 90-minute sessions ([Bastani et al., PNAS 2025](https://doi.org/10.1073/pnas.2422633122); I read the [SSRN preprint](https://papers.ssrn.com/abstract=4895486)). With access, practice scores rose 48% with a plain ChatGPT-style tutor and 127% with a guarded tutor. Once access was removed, the plain-tutor group scored 17% below students who never had AI. The guarded tutor was given the correct solutions and told to give hints, not answers; it removed the harm, but it gave no gain on the exam. The authors' analysis suggests students in the plain group were "simply copying answers".
- **Novice programmers.** 21 students in a first programming course, observed with eye tracking ([Prather et al., ICER 2024](https://arxiv.org/abs/2405.17739)). Students who already knew what to write used AI to go faster. Students who struggled got new problems: suggestions interrupted their thinking, misled them, and gave "the illusion of progress". Some "finished with an illusion of competence".
- **Engineers at Anthropic.** A survey of 132 engineers, 53 interviews and 200,000 Claude Code transcripts ([Anthropic, December 2025](https://www.anthropic.com/research/how-ai-is-transforming-work-at-anthropic)). Engineers worried about skills atrophying. The report names a "paradox of supervision": using Claude well needs supervision, and supervision needs the very coding skills that may atrophy from AI overuse. More than half said they could fully delegate only 0–20% of their work. One engineer sometimes doesn't ask Claude on purpose, to stay sharp. This is self-report from one company.
- **Students using Claude.** An analysis of 574,740 student conversations found Claude mostly doing higher-order thinking ("Creating" 39.8%, "Analyzing" 30.2%), and about 47% of conversations were "direct", seeking answers with little back-and-forth ([Anthropic Education Report, April 2025](https://www.anthropic.com/news/anthropic-education-report-how-university-students-use-claude)). This shows usage, not learning outcomes.

## 5. The illusion of understanding

**Point: people judge their own understanding badly, and outside answers usually make the judgement worse, but not always.**

The base result (established, [Rozenblit and Keil, 2002](https://pmc.ncbi.nlm.nih.gov/articles/PMC3062901/)):

- People rated their understanding of how devices work, then wrote a step-by-step explanation, then rated again. Ratings fell: in one table, from 3.89 to 3.10 after explaining and to 2.49 after a diagnostic question, on a 7-point scale.
- The illusion was large for explanations of how things work, small for facts, and absent for procedures and stories. An explicit warning shrank it but did not remove it.
- Two of the proposed causes fit code closely. People confuse what they can see in front of them with what they hold in their head. And people rarely produce explanations, so they get little feedback on what they can't explain.

Inference: a diff on screen is exactly this kind of "environmental support". Reading it feels like understanding it.

Outside answers and the illusion (established):

- **Web search.** Nine experiments showed that searching the web makes people rate their own knowledge higher, even in unrelated topics ([Fisher, Goddu and Keil, 2015](https://doi.org/10.1037/xge0000070)). The trigger was the act of searching: reading the same material without searching did not inflate ratings. Even a search rigged to return nothing useful did.
- **Chatbots.** A 2025 poster with 102 students found that getting explanations from ChatGPT inflated the illusion more than reading the same text without knowing its source ([Aslanov and Felmer](https://events.gwdg.de/event/1099/contributions/4117/)). It is a poster only, so weak evidence.
- **AI summaries.** Seven experiments with over 10,000 people compared learning from an LLM summary with learning from web links ([Melumad and Yun, PNAS Nexus 2025](https://academic.oup.com/pnasnexus/article/4/10/pgaf316/8303888)). Summary users spent less effort, wrote sparser and less original advice, and blind readers found that advice less helpful. But summary users also reported learning less, so this is shallower learning rather than an illusion. No retention test was used.
- **Reasoning tests.** People solving logic problems with ChatGPT scored about three points higher but overestimated their score by about four ([Fernandes et al., CHI 2025](https://arxiv.org/abs/2409.16708)). People with more AI literacy were more confident and less accurate about themselves.
- **Speed.** Experienced open-source developers took 19% longer with AI but believed it had made them 20% faster ([METR, July 2025](https://metr.org/blog/2025-07-10-early-2025-ai-experienced-os-dev-study/)). METR's own follow-up calls its newer data unreliable and warns again that self-reported speedups can be "quite unreliable" ([METR, February 2026](https://metr.org/blog/2026-02-24-uplift-update/)).
- **Grades.** In the math trial, students with the plain tutor did not think they had learned less, and students with the guarded tutor thought they had done significantly better on the exam when they had not ([Bastani et al.](https://papers.ssrn.com/abstract=4895486)).
- **Critical thinking.** A survey of 319 knowledge workers found that more confidence in the AI went with less critical thinking, and more self-confidence with more ([Lee et al., CHI 2025](https://doi.org/10.1145/3706598.3713778)). Critical thinking shifted toward checking, integrating and "task stewardship". This is self-report.
- **Science.** Messeri and Crockett argue that AI tools can give scientists an "illusion of explanatory depth", taking the tool's understanding for their own ([Nature, 2024](https://doi.org/10.1038/s41586-024-07146-0)). It is a Perspective: an argument, not new data. I read summaries only, since the article is paywalled.

The counterweight (established): in both coding studies, AI users rated their own learning or understanding lower, not higher ([Shen and Tamkin](https://arxiv.org/html/2601.20245v2); [Balepur et al.](https://arxiv.org/html/2607.26375v1)).

Inference: the illusion seems strongest about things nobody checks right away, like speed or an exam weeks later, and weaker right after a quiz on the same material. For a control room, both cases matter. Daniel's sense of "I get it" cannot be trusted alone. And Daniel's sense of falling behind, the pain in `problems.md`, may be accurate, but it is also a self-report that nothing measures.

## 6. What protects understanding

**Point: every countermeasure with evidence makes the person produce something, such as a question, a prediction or a step, before or right after seeing the AI's answer.**

| Countermeasure | Source | Strength |
| --- | --- | --- |
| Tutor gives hints, not answers, and knows the right solution | [Bastani et al.](https://doi.org/10.1073/pnas.2422633122), field RCT of about 1,000 | Moderate: prevented harm, gave no gain |
| Ask concept questions; ask follow-up questions about generated code; ask for code with explanations | [Shen and Tamkin](https://arxiv.org/abs/2601.20245), section 6 | Weak: correlation, 2–7 people per pattern |
| Say what each step should do before its code is revealed ("Lead-and-Reveal") | [Kazemitabaar et al., IUI 2025](https://arxiv.org/abs/2410.08922) | Weak: best of seven techniques, but p = 0.058 in study 1 and no difference in study 2 |
| Review agent edits file by file; never auto-accept | [Balepur et al.](https://arxiv.org/abs/2607.26375) | Weak: correlation, 6–7 people per group |
| The AI asks questions instead of giving explanations | [Danry et al., CHI 2023](https://doi.org/10.1145/3544548.3580672), 204 people, logic task | Moderate for that task; untested on code |
| Make people decide first, or delay the AI's answer | [Buçinca et al., 2021](https://arxiv.org/abs/2102.09692), 199 people | Moderate: reduced overreliance, least liked |
| Write an explanation, then re-rate your understanding | [Rozenblit and Keil](https://pmc.ncbi.nlm.nih.gov/articles/PMC3062901/) | Strong for exposing the illusion; it is a probe, not a teaching method |
| Do some work without AI on purpose | [Anthropic engineers](https://www.anthropic.com/research/how-ai-is-transforming-work-at-anthropic) | Anecdote |

Details worth knowing (established):

- **Explanations alone can backfire.** Showing AI explanations made people more likely to accept the AI's suggestion whether or not it was right ([Bansal et al., CHI 2021](https://doi.org/10.1145/3411764.3445717)). An explanation can work as a signal of competence rather than a source of understanding.
- **What works is disliked.** In Buçinca's study, the designs that cut overreliance most got the least favorable ratings, and they helped people who enjoy effortful thinking more than others.
- **Friction has a price.** Lead-and-Reveal took about 1.8 times as long as just showing the code. Trace-and-Predict, where learners predict variable values, took about 2.7 times as long and had the highest workload ([Kazemitabaar et al.](https://arxiv.org/html/2410.08922)). In study 2, Lead-and-Reveal was the only condition where people's rating of their own ability tracked their real ability (r = 0.26, a weak but significant link).
- **Learning modes exist but are untested.** The Anthropic authors recommend learning modes. Claude Code has two built-in styles ([docs](https://code.claude.com/docs/en/output-styles)). "Explanatory" adds short `Insight` notes about its choices. "Learning" adds the notes and also leaves small design decisions for the user to write, marked `TODO(human)`, then waits. Neither was tested in the study.

Inference: by the research above, the "Learning" style's active part is the part likely to help; `Insight` notes alone resemble the passive explanations that did not.

## 7. Guessing before the answer

**Point: guessing before you see the answer is a well-established learning effect, and wrong guesses help most.**

Established:

- **Pretesting.** In five experiments, people who tried to answer questions before reading a text later remembered it better than people given extra reading time. This held even for the questions they got wrong at first ([Richland, Kornell and Kao, 2009](https://doi.org/10.1037/a0016496)). The benefit depends on getting the answer afterwards.
- **Prediction and surprise.** Making a prediction before seeing an outcome made people surprised when they were wrong (measured by pupil size), and that surprise tracked how much they learned. False predictions did the most work ([Brod, Hasselhorn and Bunge, 2018](https://doi.org/10.1016/j.learninstruc.2018.01.013); I read the abstract and the institute's press release).
- **Explaining exposes the gap.** Rozenblit and Keil's method, explain and then re-rate, is itself a guess-then-check loop (section 5).
- **On code, the evidence is thin.** Kazemitabaar's Trace-and-Predict was promising but not significant and was the most effortful technique. Lead-and-Reveal is also a form of guessing, at the level of "what should this step do", and it was cheaper.

Inference: a light "what do you think this change will do?" question is the research's best-supported shape for the comprehension job. It targets the illusion directly, and wrong guesses are useful rather than failures. The "no score to fail" part is not tested here; it belongs to the "Gamification and motivation" and "Learning that sticks" rows of the research table in `problems.md`.

## 8. The hypotheses, one by one

**Point: this research favors active, small, in-the-moment engagement, and it is wary of anything passive or purely optional.**

These verdicts are input for the map, not its verdicts. Hypotheses this research does not touch are marked so.

| Hypothesis in `problems.md` | What this research says |
| --- | --- |
| Move learning into waiting time | Partly supports. Engagement during the work preserves learning ("conceptual inquiry", "generation, then comprehension"). No study tests learning while an agent runs. METR saw developers fill agent waits with unrelated tasks, so waiting time is contested. |
| Small and visual before text | Partly supports "small": file-by-file review and shorter code went with better comprehension, and step-by-step reveal worked best. "Visual" is untested here. |
| Non-blocking | Complicates. The countermeasures that work carry friction, people pick the low-effort path even when they know the cost, and forcing functions are disliked. A purely optional inbox risks never being opened (inference). |
| Guessing, not testing | Supports, moderately. Prediction and pretesting are well established, and explaining exposes the illusion. On code, evidence is thin. "No score" is untested. |
| Evidence, not promises | Mixed, and mostly outside this report. Explanations and confidence in the AI raise acceptance without raising understanding. Evidence may build trust without understanding unless it also shows what was not checked (inference). |
| A game that rewards understanding, not volume | Indirect support for measuring calibration, since self-assessment with AI is poorly calibrated. The game itself is outside this report. |
| Serious control-room style | Not addressed. |
| Learning content while waiting | Weakens the passive form. Reading syntheses gave shallower learning, and passive explanations did little. Videos would need an active step to count (inference). |

## 9. How `docs/agents/learning.md` compares

**Point: the repo's guide already does several things the research supports, but it puts the main explanation at the end and makes it optional, which is where the research is least encouraging.**

All of this section is inference. `learning.md` has never been measured.

What matches the research:

- **"Design together" before implementation.** Daniel and the agent agree the problem, design and scope first. That is stating intent up front (Storey's intent debt, Osmani's advice) and resembles Lead-and-Reveal at the design level: Daniel decides what each step should do before the code exists.
- **Predictions before reveal.** The guide asks for "an opportunity to reason about a scenario before revealing its outcome". That is the pretesting and prediction effect (section 7).
- **Open-ended questions about explanation, prediction and failure cases** in the HTML knowledge check. That is generating explanations, which is the Rozenblit and Keil probe.
- **Micro-worlds** where Daniel changes inputs and predicts outcomes. That is active tracing.
- **The session-opening brief** with state, settled decisions and open questions. That guards against cognitive and intent debt at the project level.
- **"Distinguish recorded facts from assumptions."** That fits the warning that explanations can raise trust on their own.

Where it diverges:

- **The explanation comes at the end.** "After meaningful tickets, suggest `$dsh-explain-diff`." It is a long HTML page after the work, the format `problems.md` calls the sharpest pain. The research's best patterns happen during the work.
- **Explanations are optional and never block.** That keeps the flow, but Balepur's users chose the low-effort path while knowing the cost, and Buçinca's most effective designs were the least liked. An optional explanation may simply go unread.
- **No measure of understanding over time.** Continuity records the gaps "Daniel raises", which is self-report. The research shows self-report with AI is unreliable. The guide has no explain-then-re-rate step and no record of predictions against outcomes.
- **Implementation is fully delegated.** At the code level that is the "AI delegation" pattern, the low-scoring one. Whether it matters depends on which understanding Daniel needs (open question 1). The guide's bet is that design-level understanding is the one that counts.

## Open questions

**What the research could not settle, and what each means for the map.**

1. **Which understanding does a supervisor of agents need?** The Anthropic quiz measured skill with a library: debugging, reading code, concepts. Balepur measured comprehension of one's own code. Neither measures the "theory" of a system (Naur) or its intent (Storey), which is what Daniel, who does not write the code, mostly needs. *For the control room's main job:* decide which kind of understanding it protects before choosing features. Code-level and design-level understanding call for different tools.
2. **Long-term effects.** Every coding study is about an hour. The math trial covered four sessions, and the essay preprint four months with 18 people. Nobody knows whether the harm grows, fades or levels off over months of agent use. *For verdicts:* mark long-term claims "untested".
3. **Deferred versus in-the-moment.** No study compares understanding an agent's change while it happens with understanding it later from a queue. Spacing research may favor some delay; that belongs to the "Learning that sticks" row of the research table in `problems.md`. *For "Non-blocking" and "Move learning into waiting time":* their verdicts need that research and the wait-learning ticket, [#279](https://github.com/danielrispler/Falinks/issues/279), as well as this report.
4. **How much friction will Daniel accept?** The research says effective engagement costs effort and people avoid it. Daniel's own rule is "never block". *For v1:* a decision ticket should name the effort budget, for example one question per change, and the prototype should test whether Daniel answers it.
5. **Can interaction patterns be read from data?** The Anthropic patterns came from hand-annotated screen recordings. Balepur labeled prompts with an LLM and logged accept and reject actions. *For data sources* ([#282](https://github.com/danielrispler/Falinks/issues/282)): whether Claude Code transcripts show query types and accept-or-reject behavior well enough to detect a delegation pattern is an open technical question, not a finding.
6. **How to measure the app itself** (in the map's "Not yet specified"). The research offers ready-made measures: a short quiz after a task, an extension task without the agent, explain-then-re-rate, and predicted-against-actual calibration. *For measurement:* reuse these rather than inventing new ones.
7. **Conflict of interest.** The key study comes from Anthropic, and it recommends Anthropic's own learning mode. Its main result agrees with independent studies (Bastani, Balepur, Prather), which lowers the concern. Its pattern analysis has no independent replication.
