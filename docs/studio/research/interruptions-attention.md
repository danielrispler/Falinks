# Interruptions, attention and window switching

Research report for [#270](https://github.com/danielrispler/Falinks/issues/270), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Researched 2026-10-26.

## Card

- **Answer:** A notification helps when it is relevant and arrives at a break in Daniel's work, such as finishing a thought or a subtask. It breaks focus when it arrives mid-thought, or when it is off-topic. Every switch to another window costs more than the switch itself: people drift to other apps and often take 10 minutes or more to get back. A visible, calm signal on the edge of the screen, plus a note of "where I was", cuts that cost.
- **Evidence:** Strong for the general effects, which several lab and field studies replicate. Weak for agent supervision: the studies used email, diagram editing and programming tasks with 16 to 48 people, and none tested an agent asking for approval.
- **Hypotheses:** Supports "Non-blocking" (hold non-urgent items until a breakpoint) and a calm "Serious control-room style". Supports fixing "Too many windows" with one visible panel. Complicates "Move learning into waiting time": a wait is a breakpoint only if Daniel is not deep in other work. "I miss the approval moment" needs a gentle but persistent signal, not a pop-up.
- **For Daniel:** Iqbal and Bailey, ["Effects of Intelligent Notification Management on Users and Their Tasks"](https://www.interruptions.net/literature/Iqbal-CHI08.pdf) (CHI 2008), about 30 minutes. Weiser and Brown, ["Designing Calm Technology"](https://calmtech.com/papers/designing-calm-technology.html) (1995), about 15 minutes.

## How to read this report

Each section opens with its point in bold. **Established** means the claim comes straight from a cited source. **Inference** means my own reasoning from those sources to agent work; no study has tested it.

## 1. Words used here

- **Interruption:** something that pulls attention off the current task. It can be external (a notification) or self-made (deciding to check another window).
- **Breakpoint:** a moment between units of work, such as finishing a subtask. Iqbal and Bailey use three sizes: **coarse** (between big subtasks), **medium** and **fine** (between small actions).
- **Resumption lag:** the time from coming back to a task until working on it again.
- **Attention residue:** thoughts about task A that stay in the head while doing task B, and make B worse (Leroy 2009).
- **Periphery and center:** in calm technology, the periphery is what you sense without attending to it; the center is what you focus on.

## 2. Interrupted work gets faster, but costs stress

**People speed up to make up for interruptions, and pay in stress, frustration and effort.**

Established. Mark, Gudith and Klocke, ["The Cost of Interrupted Work: More Speed and Stress"](https://www.interruptions.net/literature/Mark-CHI08.pdf) (CHI 2008). Forty-eight people, mostly German university students, answered emails in a lab, either uninterrupted or interrupted every two minutes by phone or instant messages. The interrupted groups finished faster (20.31 minutes with same-topic interruptions against 22.77 minutes uninterrupted), with no difference in errors. But on the NASA workload scale they reported significantly more stress, frustration, time pressure and effort. Interruptions about the same topic as the task cost the same as off-topic ones, even though people *feel* same-topic interruptions are helpful.

Inference: speed is a bad sign of whether a notification scheme works. Daniel may feel productive while the panel interrupts him, and still end the day drained. A felt measure, such as "how frazzled am I", belongs next to any speed measure.

## 3. Timing: deliver at breakpoints

**Notifications held until a breakpoint frustrate less and get answered faster than notifications shown at once. Relevant content frustrates less than off-topic content.**

Established. Iqbal and Bailey, ["Effects of Intelligent Notification Management on Users and Their Tasks"](https://www.interruptions.net/literature/Iqbal-CHI08.pdf) (CHI 2008). Their system used statistical models to spot breakpoints from what users did, and deferred notifications until then. In the second study, 16 people (8 doing diagram editing, 8 programming) got notifications either at once or at a coarse, medium or fine breakpoint.

- Off-topic notifications frustrated more than relevant ones (4.98 against 3.59 on a 7-point scale).
- In diagram editing, medium breakpoints frustrated least (2.6), then coarse (3.6), against 4.5 for immediate and 5.5 for fine.
- In programming, the trend was the same but not significant. The authors suggest programming's higher mental demand as the reason.
- Users answered faster at breakpoints. When a notification arrived at once, users first finished their current thought or action, or wrote it down, before responding.
- The models detected breakpoints "reasonably well", but struggled to tell the three sizes apart.

Inference for the studio:

- An agent asking for approval is relevant by definition, so it costs less than a random alert. A learning card or a video suggestion is closer to "general interest", which costs more.
- The studio cannot see Daniel's breakpoints inside other apps. It can see some: Daniel just sent a prompt, just approved something, just switched back to the panel. Those are natural moments to show held items.
- Programming results were not significant, so the breakpoint benefit is less sure exactly when Daniel is coding.

## 4. Switching windows costs much more than the switch

**After an alert, people wander through other apps and take 10 to 15 more minutes to get back. A visible window of the left task brings them back sooner.**

Established. Iqbal and Horvitz, ["Disruption and Recovery of Computing Tasks"](https://www.interruptions.net/literature/Iqbal-CHI07.pdf) (CHI 2007). They logged the computers of 27 people at Microsoft for two weeks.

- Users spent on average nearly 10 minutes on switches caused by email and instant-message alerts, then another 10 to 15 minutes before getting back to focused work on the task they left.
- After an alert, people often visited several apps besides the one that alerted them.
- 27% of task suspensions lasted more than two hours before the task was resumed.
- Windows of the suspended task that stayed more than 75% visible were returned to significantly faster than windows less than 25% visible. The authors read this as visual cues acting as reminders.

Developers have the same problem. Parnin and Rugaber, ["Resumption strategies for interrupted programming tasks"](https://doi.org/10.1007/s11219-010-9104-9) (Software Quality Journal, 2011), studied about 10,000 recorded sessions of 86 programmers. Only 10% of sessions resumed editing within one minute of an interruption. DeLine and Parnin, ["Evaluating Cues for Resuming Interrupted Programming Tasks"](https://www.microsoft.com/en-us/research/publication/evaluating-cues-for-resuming-interrupted-programming-tasks/) (CHI 2010), found that automatic cues showing recent activity doubled task success compared with notes alone. Developers preferred a time-ordered list of recent code.

Inference for "Too many windows": the cost is not the alt-tab. The cost is the drift after it, and the lost context. Two cheap remedies follow from the evidence: keep the agent's state visible in one small always-on panel, so Daniel never needs to hunt through windows; and when Daniel comes back to an agent, show what it did since he left, in time order.

## 5. Attention residue: unfinished tasks follow you

**Switching away from an unfinished task leaves part of the mind behind, and the next task suffers. Writing a short "where I was, what next" note helps.**

Established, with gaps. Sophie Leroy, "Why is it so hard to do my work? The challenge of attention residue when switching between work tasks", *Organizational Behavior and Human Decision Processes* 109(2), 168–181, 2009 ([DOI](https://doi.org/10.1016/j.obhdp.2009.04.002)). The paper named "attention residue". I could not open the paper; its basic finding (finishing a task makes it easier to focus on the next) is from secondary write-ups. The University of Washington Bothell [describes](https://www.uwb.edu/news/2024/05/17/an-ongoing-study-on-the-success-of-staying-focused) her later work with Glomb, "Tasks Interrupted" (*Organization Science*): people expecting little time to return to an interrupted task showed more residue and did worse on the interrupting task. A "ready-to-resume plan", noting where you were and what comes next, reduced residue; "less than a minute's work may suffice."

Inference: an agent's approval request is an interruption *of Daniel's other work*. The panel could offer a one-line "where was I" note before Daniel jumps, or capture it for him. In the other direction, when an agent finishes and Daniel is still thinking about it, the residue falls on whatever he does next. That argues for closing each agent turn cleanly: a short summary that lets Daniel let go.

## 6. Calm technology: the periphery

**Good signals live at the edge of attention and move to the center only when needed.**

Established. Weiser and Brown, ["Designing Calm Technology"](https://calmtech.com/papers/designing-calm-technology.html) (Xerox PARC, 1995). Calm technology "engages both the center and the periphery of our attention", moving things between them. Their example is the Dangling String, a plastic string that twitched with each bit of network traffic: visible and audible from nearby offices, without demanding attention. Peripheral things are not unimportant; a driver focused on the road still notices an odd engine noise at once.

This is a design essay, not an experiment. It gives a direction, not a measured effect.

Inference: the panel's normal state should be peripheral: a colour or a small motion per agent, such as working, waiting for me, stuck. A pop-up or sound should be kept for one thing only, an agent blocked on Daniel, and then it should stay until handled rather than vanish. The alarm-fatigue report ([#269](https://github.com/danielrispler/Falinks/issues/269)) should set how loud that one signal may be.

## 7. Background: attention spans on screens

**People now switch screens roughly every 40 to 50 seconds on average.**

Partly checked. In interviews about her book *Attention Span* (2023), Gloria Mark says her team measured about two and a half minutes per screen in 2004, 75 seconds in 2012, and 47 seconds on average (median 40) since about 2016, and that others replicated similar numbers ([Steelcase transcript](https://www.steelcase.com/research/articles/our-47-second-attention-span-with-gloria-mark-s5-ep3-transcript/)). These numbers measure time on one screen before switching, not a limit of the brain, and many switches are self-made, not caused by notifications.

Inference: Daniel's own switching may be the larger cost, more than the agents' alerts. A panel that answers "does any agent need me?" at a glance removes a reason to check windows.

## 8. What this means for the studio

Inferences, in order of confidence:

1. **One signal is loud: an agent blocked on Daniel.** It persists until handled. Everything else is peripheral.
2. **Hold non-urgent items for a breakpoint** the studio can see: after Daniel sends a prompt, approves, or returns to the panel. This is "Non-blocking", and it is supported.
3. **Keep the state visible** in one small panel, so checking agents needs no window hunt.
4. **On return, show what changed since Daniel left, in time order.**
5. **Offer a one-line "where was I" note** when Daniel jumps away from his own work.
6. **Treat learning content as general interest:** it costs the most as an interruption, so it should never push itself; it waits to be pulled.

## 9. Open questions

- No study tests approval requests from coding agents. How fast must Daniel answer before an agent's wait costs more than his interruption?
- Iqbal and Bailey's programming results were not significant. Is a breakpoint rule worth it while Daniel is coding?
- Can the studio detect Daniel's breakpoints outside its own window without reading other apps?

## Sources checked

Checked by fetching the source on 2026-10-26:

- Mark, Gudith and Klocke, CHI 2008: full PDF text. Checked 48 participants, two-minute interruptions, 22.77 minutes baseline, the stress, frustration, time-pressure and effort results, and the same-context finding.
- Iqbal and Bailey, CHI 2008: full PDF text. Checked abstract, 16 users (8 per domain), frustration means 4.98 and 3.59, the diagram-editing breakpoint means, and the non-significant programming trend.
- Iqbal and Horvitz, CHI 2007: full PDF text. Checked 27 users, two weeks, nearly 10 minutes plus 10 to 15 minutes, 27% over two hours, and the window-visibility result.
- Weiser and Brown, 1995: full web text. Checked the quote, the Dangling String and the periphery example.
- Leroy 2009: title, journal, volume, issue, pages and year from Crossref only.
- Leroy and Glomb, "Tasks Interrupted": University of Washington Bothell news article only.
- DeLine and Parnin, CHI 2010: Microsoft Research abstract page.

Rests on secondary sources or memory:

- Leroy 2009's findings (secondary write-ups; the paper is paywalled).
- Parnin and Rugaber 2011: the 10% and 86-programmer figures come from a search summary of the journal abstract; the paper is paywalled. A preprint lists 85 programmers.
- Gloria Mark's 47-second figures come from her interviews, not the underlying papers. *Attention Span* itself was not read.
- Mark's widely quoted "23 minutes to get back" figure is not used here because I did not verify it.
