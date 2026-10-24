# Wait-learning during agent work

Research report for [#279](https://github.com/danielrispler/Falinks/issues/279), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Researched 2026-10-24.

## Card

- **Answer:** Partly yes. A wait of about a minute can hold one small, optional item about the change Daniel just handed to the agent, shown right after he sends the task, next to a live view of the agent. Unrelated videos and interest-based material will most likely distract, because agent waits are work waits with a high mental load.
- **Evidence:** Moderate for the general rules: Carrie Cai's field studies had 20 to 25 people each, over two to eight weeks. Weak for agent work: those studies taught only vocabulary, left out work waits such as compiling code on purpose, and no study tests learning during agent runs.
- **Hypotheses:** Supports "Move learning into waiting time", but only for content about the current change. Also supports the "small" half of "Small and visual before text", and a prediction prompt at the start of a wait as in "Guessing, not testing". Weakens "Learning content while waiting" for videos and interest-based material. Complicates "Non-blocking": in Cai's studies, items that stayed until handled felt like a to-do list.
- **For Daniel:** Cai, Ren and Miller, ["WaitSuite"](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf) (ACM TOCHI, 2017), sections 3, 7.5 and 8 (about 45 minutes). Anthropic, ["Measuring AI agent autonomy in practice"](https://www.anthropic.com/research/measuring-agent-autonomy) (2026), the turn-duration part (about 10 minutes).

## How to read this report

Each section opens with its one-line point in bold. Claims marked **Established** come straight from a cited source. Claims marked **Inference** are my reasoning from those sources to agent work, and no study has tested them. Numbers are quoted as each source reports them.

## 1. Words used in this report

**This section defines the terms used throughout the report.**

- **Wait-learning:** Cai's term for "automatically detecting wait time and inviting people to learn while waiting" ([Cai, Ren and Miller 2017](https://doi.org/10.1145/3044534)).
- **Microlearning:** spreading learning into small units across the day, so that content comes back spaced out over time ([Cai et al. 2017, section 2.1](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). Wait-learning is microlearning that uses waits as the trigger.
- **Primary task:** the thing the person is really doing, such as chatting, or here, supervising an agent. The learning item is the **secondary task**.
- **Switch cost:** the extra time and errors that come from switching from one task to another and back ([Cai et al. 2017, section 2.2.3](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).
- **Problem state:** the in-between information that a task makes you hold in your head, such as "carry the one" in a long sum. Interruptions hurt more when both tasks hold a problem state ([Borst, Taatgen and van Rijn 2015](https://doi.org/10.1145/2702123.2702156)).
- **Competing demands:** how much of the mind the primary task still occupies during the wait. This is Cai's term ([Cai et al. 2017, section 3.1.3](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).
- **Access-to-wait ratio:** the time it takes to reach the learning item, divided by the length of the wait. A low ratio is good ([Cai et al. 2017, section 7.5.1](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).
- **Soft notification:** a fixed panel whose text changes gently. A pop-up is the opposite ([Cai et al. 2017, section 3.2.2](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).
- **Engagement rate:** the share of offered exercises that the person answered.
- **Turn:** in Claude Code, the span from when Claude starts working on a message to when it stops and hands control back ([Anthropic 2026](https://www.anthropic.com/research/measuring-agent-autonomy)).

## 2. What Cai's studies found

**Short, optional exercises placed beside the waiting task and shown at the start of the wait got used and taught something, with no measurable extra workload.**

Established. There are three studies, all by Cai and colleagues at MIT, and all teaching French or Spanish vocabulary.

**WaitChatter, CHI 2015.** WaitChatter added a small learning panel under the Google Chat input box. It showed a vocabulary exercise while the user waited for a reply ([Cai, Guo, Glass and Miller 2015](https://doi.org/10.1145/2702123.2702267), [PDF](https://sls.csail.mit.edu/publications/2015/Cai_CHI-2015.pdf)). Twenty people used it for two weeks and exchanged about 170 chats a day. On average they could recall 57 new words at the end. An exercise took a median of 1.83 seconds. The median gap between messages was 11 seconds, and the most common gap was 4 seconds. That is why the authors say exercises must be "lightweight". An exercise faded after 10 seconds if ignored, and the user could fetch another one with an arrow key.

**WaitSuite, ACM TOCHI 2017.** WaitSuite tested five kinds of wait in a two-week field study with 25 people ([Cai, Ren and Miller 2017](https://doi.org/10.1145/3044534), [PDF](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). The ticket calls it a CHI 2017 paper; the paper itself is the TOCHI journal article. Its Tables I and II report:

| App | Median wait | Median time to start the exercise | Engagement rate |
| --- | --- | --- | --- |
| ElevatorLearner (phone, near the elevator) | 52.9 s | 10.0 s | 46.7% |
| PullLearner (pull-to-refresh email) | 2.3 s | 1.6 s | 62.5% |
| WifiLearner (laptop joining Wi-Fi) | 6.8 s | 7.3 s | 17.5% |
| EmailLearner (after pressing Send) | 0.7 s | 2.7 s | 39.8% |
| WaitChatter (awaiting a chat reply) | 10.2 s | 2.9 s | 13.4% |

WaitChatter had the lowest engagement rate. It still produced the most exercises per day (6.6), because chat waits are so frequent. A standard workload questionnaire (NASA TLX) gave "no evidence that users perceived an additional workload". WifiLearner users rated the Wi-Fi wait as less irritating afterwards: 2.19 against 3.68 before, on a 7-point scale ([section 7.6](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).

**The eight-week study in Cai's thesis.** Chapter 6 of [Cai's 2017 PhD thesis](https://up.csail.mit.edu/other-pubs/cjcai-thesis.pdf) compared WaitChatter with a standalone flashcard website plus a daily email reminder. Twenty-one people switched between the two each week for eight weeks. With wait-learning they did about twice as many flashcards (415 against 217) and recalled more words (28 against 15). They practised on more days (35% against 16%), and 19 of the 21 preferred it. The finding that matters most for this ticket is about the main task. People replied to chats faster in wait-learning weeks (5.4 minutes against 9.8). Their Gmail tab was in focus more often when a message arrived (55% against 43%). The thesis explains that without the in-place exercise, people went to other tabs while waiting, which "often causes a chain of further diversions" ([thesis, section 6.5](https://up.csail.mit.edu/other-pubs/cjcai-thesis.pdf)).

**Limits the authors state themselves.** The conclusions came "not through a controlled study, but rather through an inductive analysis and iterative design process". Participants were mainly young adults in a university setting ([WaitSuite, section 8.8](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).

## 3. When wait-learning works: Cai's rules

**It works when the exercise is quicker to reach than the wait is long, the main task leaves the mind free, and the main task stays in view.**

Established, from [WaitSuite sections 3 and 8.1](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf) and the [WaitChatter design implications](https://sls.csail.mit.edu/publications/2015/Cai_CHI-2015.pdf):

1. **Low access-to-wait ratio.** If reaching the exercise takes about as long as the wait, people skip it. WifiLearner shows this: 7.3 seconds to reach the exercise during a 6.8-second wait gave 17.5% engagement. If the ratio is high, competing demands must be "even lower" for people to engage.
2. **Low competing demands.** The elevator worked partly because "the user simply needs to remember where they are headed next".
3. **The main task stays visible and does not resume abruptly.** The learning item should sit beside the main task, not on top of it. The authors left out waits whose task state is not shown, and waits that end with something needing immediate attention, such as a phone call connecting.
4. **Bite-sized, with little problem state.** Vocabulary was chosen because each word "can reasonably be learned in isolation".
5. **Soft, ignorable appearance.** Users said the exercises almost never felt disruptive "because they always had the option to ignore them". Ratings did not differ between well-timed and randomly timed days ([WaitChatter](https://sls.csail.mit.edu/publications/2015/Cai_CHI-2015.pdf)).
6. **Show it at the start of the wait** (section 6 below).
7. **Offer optional continuations.** A "fetch more" control let users keep going when a wait ran long.

**Frequency cuts both ways.** Established. Cai kept only waits that happen at least once a day, because spaced repetition needs repeated exposure ([WaitSuite, section 3.1.2](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). Very frequent triggers got ignored. One frequent chatter said "I knew it would show up very soon again". On EmailLearner, people who saw more triggers engaged less ([sections 7.5.5 and 7.5.6](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).

## 4. Cai left out the kind of wait that agent work is

**The research chose waits with a light mental load and removed work waits such as compiling code, so its good results do not carry over to agent runs as they stand.**

Established. WaitSuite says that waiting for code to compile "may consume high attentional capacity because a user must remember which bug they were trying to fix, which files and lines they are working on" ([section 3.1.3](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). The authors "filtered out tasks that were necessarily high-stakes or work-intensive, e.g., code compiling" ([section 4.1](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). Where the studies did touch work, engagement fell:

- WaitChatter users were "least likely to engage with exercises if they were having a particularly time-sensitive conversation or if the nature of the conversation was serious or work-related" ([WaitChatter](https://sls.csail.mit.edu/publications/2015/Cai_CHI-2015.pdf)).
- When chatting was secondary to "a primary work-related task", learning became a third task and "attentional capacity would be particularly low" ([WaitSuite, section 7.5.6](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).
- In the eight-week study, users "tended to ignore flashcards if they were working or doing homework". One participant who used Google Chat mainly for work preferred the reminder condition ([thesis, sections 6.4.3 and 6.6.3](https://up.csail.mit.edu/other-pubs/cjcai-thesis.pdf)).

Inference. Supervising an agent looks like compiling code, not like waiting for an elevator. Daniel holds a problem state during the run: what he asked for, what he will check, and what might go wrong. By Cai's own rules, unrelated learning content during that wait is a poor fit. Section 7 argues that content about the same change is a different case.

## 5. What agent waits look like

**Agent waits are frequent and mostly about a minute long. People mostly spend them watching the agent, and in parallel work they spend them on other agents.**

Established, from four sources:

- **Turn length.** Across Claude Code users, "the median turn lasts around 45 seconds", and it moved only between 40 and 55 seconds over several months. The longest turns grew. Between October 2025 and January 2026, the 99.9th-percentile turn "nearly doubled, from under 25 minutes to over 45 minutes" ([Anthropic 2026](https://www.anthropic.com/research/measuring-agent-autonomy)). The same post reports more full auto-approve with experience: about 20% of sessions for new users, over 40% by 750 sessions. Interruptions also rise with experience, from 5% to about 9% of turns. On complex tasks, Claude asks for clarification more than twice as often as on simple ones.
- **Watching live.** In a study of 19 developers using an in-editor agent, participants "followed the agent's execution as it happened in real time for 84% of their prompts". They did so for 95% of responses with code changes. "Usually, this live analysis was sufficient for participants to understand the agent's process." Some avoided working alongside the agent for fear of conflicts ([Kumar et al. 2025, ASE'25](https://arxiv.org/abs/2506.12347), [HTML](https://arxiv.org/html/2506.12347v3)).
- **Waiting and idle time.** In METR's randomized trial with experienced open-source developers, those allowed to use AI spent less time actively coding. They spent more time "reviewing AI outputs, prompting AI systems, and waiting for AI generations", and "a somewhat higher proportion of their time idle" ([METR 2025](https://arxiv.org/abs/2507.09089)).
- **Parallel agents.** In a 2026 preprint on running several agents at once, developers used parallel sessions to fill "waiting for builds or tests". Requests for clarification "were also the signals participants most reliably missed", and "turning away from the current task, even briefly, often led to missed agent updates" ([Long et al. 2026, ParallelPilot](https://arxiv.org/abs/2609.33113), [HTML](https://arxiv.org/html/2609.33113v1)). This is a recent preprint and not yet peer reviewed.

Inference. Combining these with Cai's rules gives four kinds of wait:

| Kind of wait | Typical length | What fits, by Cai's rules |
| --- | --- | --- |
| Gaps inside a turn (a tool call, the model thinking) | seconds | Nothing extra. Watching the live run is already the task. |
| One turn after Daniel sends a task | about 45 s median across all users | One small card about this change, or a one-line prediction prompt |
| A long autonomous run (auto mode, background agents) | minutes, with a tail near 45 minutes | Daniel will likely switch to other work anyway. Longer content is possible only if the agent's state stays visible and an alert pulls him back. |
| The agent waiting for Daniel (an approval or a question) | until Daniel acts | Not a wait for Daniel at all. Nothing may cover or delay it. |

The Anthropic figures cover all Claude Code users. Daniel runs auto mode and background agents, so his waits are probably longer than the median. That is an inference, and measuring it is a job for [#282](https://github.com/danielrispler/Falinks/issues/282).

## 6. Timing: the start of the wait

**Show the item right after Daniel hands work to the agent, not in the middle of the run, and not on every turn.**

Established. In WaitChatter, exercises shown 1.5 seconds after the user sent a message had the highest engagement (49.1%) and the fastest start (3.6 seconds). Exercises shown when the friend started typing did worse (41.2%). The authors' design rule: "Learning exercises should be delivered immediately at the start of or prior to waiting, rather than times that are well into the waiting period". By then, users "may be less likely to engage in learning once they have already shifted attention to other default waiting activities" ([WaitChatter](https://sls.csail.mit.edu/publications/2015/Cai_CHI-2015.pdf)). This fits a pupil-dilation study. Mental workload dips at subtask boundaries, and dips more at the end of larger chunks of work ([Bailey and Iqbal 2008](https://doi.org/10.1145/1314683.1314689)).

Inference. In agent work, the coarse boundary comes when Daniel has just finished his own subtask: he wrote and sent the task, or approved a plan. A tool call in the middle of a run is a fine boundary, and Daniel's attention is already on the live output there. Given the frequency findings in section 3, the item should appear at real hand-offs, not on every turn.

**A prediction fits this moment.** Established. Trying to answer a question before studying the material improves later learning, even when the first guess is wrong. This is the "pretesting effect" ([Richland, Kornell and Kao 2009](https://doi.org/10.1037/a0016496)). Inference: "What do you think the agent will change?" asked right after sending a task uses the best moment Cai found, and needs no score. The deeper research on this belongs to the "Learning that sticks" ticket.

## 7. Content about the change, or content about Daniel's interests

**Content about the change shares the mental context Daniel already holds; unrelated content opens a second one. The evidence leans toward the first.**

**Problem state.** Established. Interruptions are more disruptive when both the interrupted task and the interrupting task hold a problem state. The authors advise interfaces to "maintain the problem state for the user when interrupted" ([Borst, Taatgen and van Rijn 2015](https://doi.org/10.1145/2702123.2702156)). Inference: an explanation of the change Daniel is waiting on is not really a second task, because it adds to the context he already holds. A video on an unrelated topic carries its own problem state, so it pays the full switch cost twice: once going in and once coming back.

**Engaging with the code during AI work builds understanding.** Established. In Anthropic's randomized trial, 52 developers learned a new Python library. Those using AI scored 50% on the later quiz, against 67% for those coding by hand. Within the AI group, people who asked for explanations or conceptual questions scored higher (65% to 86%) than those who handed the work over (24% to 39%) ([Shen and Tamkin 2026](https://www.anthropic.com/research/AI-assistance-coding-skills)). Each pattern had only 2 to 7 people, and the study is not about waits. Inference: the kind of activity that builds understanding during AI work is engagement with the code being made. That argues for content tied to the change.

**Context-tied content: more relevant, but less of it.** Established. MicroMandarin compared location-tied Mandarin flashcards with a frequency-based word list over four weeks with 23 users. Location-tied sessions "lasted half as long but occurred in twice as many places". Users valued them for using words right away. The frequency version produced more cards "learned", and the authors warn this may reflect cramming. Preference split 12 to 10 for the location-tied version ([Edge et al. 2011](https://doi.org/10.1145/1978942.1979413), [PDF](https://cdn-prod.voxy.com/wp-content/uploads/2011/03/micromandarin.pdf)). WaitChatter users found words drawn from their own conversation "surprising", which may have made the system feel playful ([WaitSuite, section 6.4.3](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).

**Interesting but irrelevant material lowers learning.** Established. A meta-analysis of 58 studies with more than 7,500 students found that learners given "information that is interesting but irrelevant" scored lower on learning outcomes ([Sundararajan and Adesope 2020](https://doi.org/10.1007/s10648-020-09522-4), [WSU summary](https://news.wsu.edu/press-release/2020/03/19/seductive-details-inhibit-learning/)). Researchers call this the "seductive details" effect. Inference: a video chosen for Daniel's interests, placed next to the explanation of a change, is a seductive detail for the thing he most needs to understand.

**Interests help as a frame for relevant content.** Established. In a randomized study with 145 ninth-grade algebra students, problems rewritten around each student's interests (sports, music, films) were solved faster and more accurately, and the gains carried over to normal problems later ([Walkington 2013](https://doi.org/10.1037/a0031882)). Inference: Daniel's interests can make an explanation of the current change easier to grasp, for example an analogy from a field he likes. They should not replace that explanation. The study was in school maths, not adult software work.

## 8. Videos in particular

**Videos are a poor fit for most agent waits: they last longer than the wait, they cover the agent, and they are built to hold attention.**

**Length.** Established. Across 6.9 million viewing sessions of edX course videos, "median engagement time is at most 6 minutes, regardless of total video length". The authors recommend chunks "shorter than 6 minutes" ([Guo, Kim and Rubin 2014](https://doi.org/10.1145/2556325.2566239), [PDF](https://up.csail.mit.edu/other-pubs/las2014-pguo-engagement.pdf)). Inference: set against a median turn of about 45 seconds, even a well-made short video outlasts most agent waits.

**Covering the main task.** Established. Cai's rule is that the learning task "should appear in a multiplexed manner", so that people "can continue to view and monitor the main task" ([WaitSuite, section 3.1.4](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). A field study logged 2,267 hours of work by 27 people at Microsoft. After an email or chat alert, they spent about 8 to 10 minutes on the diversion before returning. They then spent another 10 to 16 minutes before getting back to their earlier state. After 27% of alerts, they stayed away from the windows they had been using for more than 2 hours. Windows less than 25% visible took longer to return to than mostly visible ones ([Iqbal and Horvitz 2007](https://doi.org/10.1145/1240624.1240730), [PDF](https://www.erichorvitz.com/CHI_2007_Iqbal_Horvitz.pdf)).

**What users said about videos.** Established. In the eight-week study, participants said their usual wait activities included "watching youtube". One said the flashcards "felt easier than looking at a comic or watching a video or reading a longform article. Those would get me into a different mindset and maybe higher cognitive load" ([thesis, section 6.4.3](https://up.csail.mit.edu/other-pubs/cjcai-thesis.pdf)).

**Recommendations work on bored people, and that is the risk.** Established. A phone-based boredom detector reached up to 82.9% accuracy. A second field study with 16 people suggested that "people are more likely to engage with recommended content when they are bored" ([Pielot et al. 2015](https://doi.org/10.1145/2750858.2804252)). Inference: a suggested video will often be clicked during a boring wait. The danger is that it succeeds in pulling Daniel away from the agent.

**Where a video might fit.** Inference, untested. During a long background run that Daniel would leave anyway, a video under 6 minutes about the change itself could fit. The agent's status would have to stay on screen, and an approval request would have to interrupt the video. No study has tested this.

## 9. How waits interact with interruption cost

**Wait-learning creates interruptions in both directions, and the agent's need for Daniel must always win.** [#270](https://github.com/danielrispler/Falinks/issues/270) covers interruptions in depth. This section covers only what wait-learning depends on.

**Learning can delay supervision.** Established. Claude stops to ask for clarification more often as tasks get harder ([Anthropic 2026](https://www.anthropic.com/research/measuring-agent-autonomy)). In parallel work, clarification requests were the signals developers most reliably missed ([ParallelPilot](https://arxiv.org/html/2609.33113v1)). After a diversion starts, the return can take a long time (section 8). Inference: this is the problem "I miss the moment an agent is waiting for my approval" in `problems.md`, and absorbing content makes it worse.

**The agent can interrupt learning.** Established. Cai left out tasks that "resume abruptly or demand the user's immediate attention when they resume". Users were frustrated when an exercise vanished before they could answer it. They "may be less tolerant of the premature disappearance of an exercise" ([WaitSuite, sections 3.1.4 and 6.4.6](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). Inference: a wait item should be finishable in seconds, or it should keep its place so that Daniel can drop it and return later.

**The alternative is not an empty wait.** Established. Information workers are interrupted about every 12 minutes, and half the time they interrupt themselves ([Jin and Dabbish 2009](https://doi.org/10.1145/1518701.1518979)). Cai's users described replacing Facebook, games and news with the exercises. The in-place exercise kept them closer to the chat than their usual habits did (section 2). Inference: a small item in the control room may cost less than whatever Daniel does now during a wait. That holds only if it stays small and stays beside the agent.

## 10. Part of the boredom may come from not knowing

**A wait feels longer when it is unexplained and of unknown length, so showing the phase and progress may reduce boredom before any learning is added.**

Established. Maister's classic essay on waiting lists these propositions, among others: "Occupied Time Feels Shorter Than Unoccupied Time", "Anxiety Makes Waits Seem Longer", "Uncertain Waits Are Longer than Known, Finite Waits" and "Unexplained Waits Are Longer than Explained Waits" ([Maister 1985](https://davidmaister.com/articles/5/52/index.html)). Wait-learning made the Wi-Fi wait less irritating (section 2).

Inference. Part of "Waiting is boring" may come from "Blind during a run": Daniel does not know the phase, whether the run is stuck, or how long it will take. A readable live view does three things. It explains the wait, it occupies the time, and it is relevant to the task. Kumar et al. found that watching the run live was usually enough to understand the agent's process (section 5). The first wait-learning feature may therefore be a clearer view of the run, not separate learning content.

## 11. Items that fade versus items that pile up

**In Cai's studies, items that faded on their own felt light, and items that stayed until handled felt like chores.**

Established. Email reminders "remained permanent until actively removed". Users felt "low priority messages introduced an unecessary extra to-do item", and some archived them like spam. Flashcards that disappeared on their own "gave the impression that it was optional, low-stakes, and transient" ([thesis, sections 6.4.3 and 6.6.2](https://up.csail.mit.edu/other-pubs/cjcai-thesis.pdf)). Users also wanted to start exercises themselves. They did so most when the panel sat in always-visible empty space on the screen ([WaitSuite, section 7.5.7](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)).

Inference. For "Non-blocking", this points to a side pile that Daniel opens when he chooses: no counts, no badges, and items that may expire or merge. Caveat: Cai's learners treated vocabulary as a casual hobby. Daniel treats understanding his project as important, so a pile might feel useful to him rather than nagging. Only trying it would tell.

## 12. A cost Cai names: losing rest

**Filling every wait can take away rest and reflection.**

Established. Cai notes that a "potential downside of wait-learning could be the inadvertent replacement of time spent reflecting or resting". She adds that it suits people who already fill waits with technology ([WaitSuite, section 8.7](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). Inference: `problems.md` says Daniel's brain is "already tired from the work". The control room should leave a wait empty when he wants it empty. That could be the default, with wait items as something he turns on.

## What this means for the hypotheses

| Hypothesis in `problems.md` | Lean from this research | Why |
| --- | --- | --- |
| Move learning into waiting time | Supported, with conditions | Works for small, optional, in-place items at the start of a wait (sections 2, 3, 6). Agent waits carry a heavy load (section 4), so the content should be about the current change (section 7). |
| Learning content while waiting (videos, interests) | Weakened for interest videos in short waits; untested for short videos about the change in long waits | Videos outlast most waits, cover the agent, pull bored users away, and add seductive details (sections 7, 8, 9). |
| Small and visual before text | The "small" part is supported; "visual" is not tested here | Cai's success depended on bite-sized items. Nothing here compares visual and text. |
| Guessing, not testing | Supported as a wait-start activity | The best moment is the start of the wait (section 6); the pretesting effect supports guessing first. |
| Non-blocking | Supported, but the "inbox" form is questioned | Ignorable items did not disrupt. Persistent piles felt like chores (section 11). |
| Serious control-room style; a game that rewards understanding; evidence, not promises | Not addressed | Out of scope. Cai's users likened the exercises to "low-stakes games", which is weak evidence at best ([WaitSuite, section 6.4.3](https://up.csail.mit.edu/other-pubs/tochi16-waitsuite.pdf)). |

## Open questions

- **How long and how frequent are Daniel's own waits?** The 45-second median covers all Claude Code users. Daniel's auto mode, background agents and parallel sessions may move his waits toward the long tail. Or they may leave no idle time at all, as in ParallelPilot. *For the map:* the data-sources ticket ([#282](https://github.com/danielrispler/Falinks/issues/282)) should check whether Claude Code transcripts carry turn start and stop times, approval events and phase changes. Those are the inputs a wait detector needs.
- **Does content about the change, shown during the wait, improve understanding of that change?** No study tests this. Cai measured vocabulary, and Anthropic's trial measured skill with a library, not with a reviewed change. *For the map:* this is the core claim behind "Move learning into waiting time", and it can only be held or weakened by a prototype or a self-experiment.
- **Does wait-learning slow Daniel's response to approvals?** Cai found people stayed closer to their chat. That was low-stakes chat with an exercise beside it, not an agent asking for a decision. *For the map:* any v1 wait feature needs a rule that approvals and questions override it. Its effect on approval delay should be measured, not assumed.
- **Do interest-based videos at least cure boredom without hurting supervision?** Untested. A cheap test would be a week of Daniel's own logs with and without a "watch something" option during long runs.
- **What does a tired brain tolerate?** Cai found no added workload, but her items took about two seconds and the waits had a light load. Agent supervision late in the day is neither. *For the map:* this links to the cognitive-load research and argues for wait items that are off or very light by default.
- **Where does learning sit in the control room's main job?** This research suggests the strongest wait-time "learning" may be a readable live view of the run itself (section 10), not a separate learning feature. *For the map:* the decision on the control room's main job should weigh whether wait-learning is a separate feature or a property of the run view.
- **Not settled here.** I found no high-quality review that says what fits in one to five minutes across subjects. The microlearning reviews I found were thematic and did not pool results. Google's work on build latency ([Jaspan and Green 2023](https://research.google/pubs/developer-productivity-for-humans-part-4-build-latency-predictability-and-developer-productivity/)) studies how developers react to waiting for builds. I could read only its abstract, so it is not used here. It is a good lead for the interruptions ticket.
- **What this means for v1 (inference).** The smallest version these findings support has three parts. First, a readable live view of the run. Second, one optional item at the start of a run, such as a prediction prompt or a two-line note on what the agent is about to touch. It would sit beside the agent and fade if ignored. Third, approvals that always come first. A video feed does not belong in v1.
