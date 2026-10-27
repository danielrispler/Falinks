# Situational awareness: showing what agents are doing without overwhelming

> **Answer:** Show meaning, not more data. Give each agent session one quiet row: a few words on what it is doing now, its phase, and a verdict (working, waiting on you, maybe stuck). Judge the verdict against the session's own plan and usual pace, and keep the evidence one click away. Normal stays grey and still. Colour and motion are kept for the off-nominal, and details open on demand.
> **Evidence:** Strong for the model and its core finding: 76% of awareness errors in aviation reports are failures of perception, and the largest single cause is not looking at data already on display (Endsley 1995; Jones and Endsley 1996). Moderate for specific display techniques: cockpit and process-control practice, plus a few small controlled studies (Matthews et al. 2006; Tharanathan et al. 2010). None is about coding agents.
> **Hypotheses:** "Serious control-room style": supported as calm, exception-based design (dark when normal, overview first); weakened as a telemetry look, because a new palette and gauges do not build awareness on their own. "Small and visual before text": supported for size, weakened for pictures alone. A few relevant words beat a bare icon or colour change; colour codes work only once learned, so pair each with a word. Addresses "Blind during a run".
> **Read yourself:** Endsley's slides ["The Role of Situation Awareness and Cognitive Engineering in Future Weapon Requirements, Design and Operation"](https://ndia.dtic.mil/wp-content/uploads/2012/armaments/WednesdayEndsley.pdf) (2012, slides 9–37, about 20 min); Matthews et al., ["Clipping Lists and Change Borders"](https://www.interruptions.net/literature/Matthews-CHI06-p989-matthews.pdf) (CHI 2006, 10 pages, about 30 min).

Research report for [#267](https://github.com/danielrispler/Falinks/issues/267), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Researched 2026-10-27.

**How to read this.** **Established** means a cited source reports it. **Inference** means my own reading for the studio; no study tested it. The problem and hypothesis names are the ones in [`problems.md`](https://github.com/danielrispler/Falinks/blob/660c11a/docs/studio/problems.md).

## The idea in plain words

Situation awareness is knowing what is going on well enough to decide what to do next. Mica Endsley splits it into three levels:

1. **Perception:** seeing the facts. "Agent B ran the tests."
2. **Comprehension:** knowing what they mean. "Agent B has failed the same test three times; it is going in circles."
3. **Projection:** knowing what comes next. "Agent B will not finish without help; it will need me in a few minutes."

A display can fail at any level. Most displays only serve level 1: they list facts and leave the meaning and the forecast to the person. With several agents running, that work grows with every agent added. So "without overwhelming" does not mean "show less". It means the display does more of the level 2 and level 3 work, and shows the rest only when asked.

## Words used here

- **SA:** situation awareness, the three levels above.
- **Mode or phase:** the state an automated system is in, which changes how it behaves. In a cockpit, "climbing to the set altitude". For an agent, "planning", "editing", "running tests".
- **Off-nominal:** not normal. It includes failed and also "running, but not the way it should".
- **Neglect time:** how long a robot or agent can be left alone before its work goes off course.
- **Fan-out:** how many robots or agents one person can supervise well at once.
- **Preattentive:** seen at a glance, all over the screen at once, before you aim your attention.
- **Glanceable:** read in a quick look while you do something else.

## What the evidence says

### 1. The core model, and where awareness breaks

**Most awareness errors are failures to perceive. The largest single cause is not missing data: the data is on display, and the person does not look at it.**

Established:

- Endsley defines SA as "the Perception of elements in the environment within a volume of time and space, the Comprehension of their meaning, and the Projection of their status in the near future" ([slides](https://ndia.dtic.mil/wp-content/uploads/2012/armaments/WednesdayEndsley.pdf), slide 9, citing Endsley 1988). Her theory paper names attention and working memory as the limits, and goals and mental models as the ways people get around them ([Endsley 1995, *Human Factors* 37(1)](https://doi.org/10.1518/001872095779049543)).
- Jones and Endsley studied aviation incident reports tagged "situational awareness" in NASA's Aviation Safety Reporting System. "Of the errors identified, 76.3% were Level 1 SA errors, 20.3% were Level 2, and 3.4% were Level 3", "with failure to monitor or observe available information forming the largest single category" ([Jones and Endsley 1996, PubMed abstract](https://pubmed.ncbi.nlm.nih.gov/8827130/)).
- Her slide on the causes of SA errors names "task distraction" as the biggest single cause (slide 16). She lists the usual enemies of SA, her "SA demons": attentional tunneling, the requisite memory trap, workload, fatigue and other stressors, misplaced salience, complexity creep, errant mental models and the out-of-the-loop syndrome (slide 17).
- Her design advice starts from goals, not from data. The first six of her 50 principles are (slide 32): organize information around goals; "Present Level 2 SA Directly"; support level 3 projections, which are "cognitively taxing & difficult for novices"; support global SA, "awareness of status across goals"; support trade-offs between goal-driven and data-driven processing, to "avoid attentional narrowing"; and make critical cues salient. Under "Common pitfalls" (slide 31): good SA "requires more than putting data on the same display"; data "must be transformed into true meaning"; and "Cool is not necessarily functional".
- Her method for finding what a display must show is the goal-directed task analysis: list the person's goals, the decisions under each goal, and the projection, comprehension and data needed for each decision (slides 22–27). She and Rodgers applied it to en-route air traffic control by interviewing eight experienced controllers ([Endsley and Rodgers 1994, FAA report](https://rosap.ntl.bts.gov/view/dot/21425)).

Inference for the studio: Daniel's problem is the textbook case. While he works in another window, the agent's state is "available information" he does not look at, the largest category above. Two consequences follow. The panel must sit where he can see it with a glance (see the interruptions report, [#270](https://github.com/danielrispler/Falinks/issues/270)). And it must say what the state *means*, so a glance is enough.

### 2. Phase changes are easy to miss, even for trained pilots

**People watch the main numbers and miss the automation's mode, even when it is shown on screen.**

Established:

- Sarter and Woods describe "mode errors": when automation has many modes and nothing helps people track them, "new mode-related error forms and failure paths can result" ([Sarter and Woods 1995, *Human Factors* 37(1)](https://doi.org/10.1518/001872095779049516)).
- Sarter, Mumaw and Wickens tracked the eyes of 20 experienced Boeing 747-400 pilots in a one-hour simulator flight. The "pilots monitor basic flight parameters to a much greater extent than visual indications of the automation configuration". They "frequently fail to verify manual mode selections or notice automatic mode changes", and do not process mode annunciations deeply enough to see what they mean for the aircraft's behaviour ([Sarter, Mumaw and Wickens 2007, *Human Factors* 49(3)](https://doi.org/10.1518/001872007X196685), abstract).

Inference: an agent's phase is its mode. A phase label in a corner will be missed for the same reason. The phase should sit inside the same row as the agent's main status, where the eye already goes. A phase change that matters (for example "editing" back to "planning" late in a session) deserves a short, salient cue, then quiet.

### 3. Several agents at once: the cost is catching up

**With many automated workers, the limit is the time a person needs to catch up on one before acting. Losing awareness is the largest part of that time.**

Established:

- Olsen and Goodrich define **neglect time** and **fan-out**: fan-out = neglect time ÷ interaction effort + 1 ([Olsen and Goodrich 2003](https://faculty.cs.byu.edu/~mike/mikeg/papers/OlsenGoodrichPERMIS2003.pdf)). On switching to another robot, "the user must take a moment to understand the situation of the new robot". They call this context acquisition, and judge it "probably the largest contributor to an upper bound on fan-out regardless of robot capability". "A key approach is the externalization of memory": put on screen what the person would otherwise have to remember.
- The same paper names the "progressing or stuck" trap directly: a robot "might be getting closer to the target very rapidly and yet be wandering into a cul-de-sac … It currently appears to be effective but on the overall goal it is making negative progress."
- Cummings and Mitchell modelled one operator supervising several drones. Counting the waits that human interaction causes, predicted capacity "dropped by up to 67%", with "loss of situation awareness as the primary source of wait time delays". Even with high automation, SA wait time caused "a 36% decrease" in capacity ([Cummings and Mitchell 2008, *IEEE SMC Part A* 38(2)](https://doi.org/10.1109/TSMCA.2007.914757); abstract from [MIT DSpace](https://dspace.mit.edu/handle/1721.1/90280)).
- Cummings and Guerlain had operators retarget 8, 12 or 16 missiles in flight. With 16, performance on three objective measures got significantly worse, and SA dropped. They propose "a 70% utilization (percentage busy time) score" as a threshold for "significant performance decay" ([Cummings and Guerlain 2007, *Human Factors* 49(1)](https://doi.org/10.1518/001872007779598109), abstract).

Inference: the studio should not try to make Daniel watch more closely. It should make catching up on any one agent fast. That means an overview of all agents at once (Endsley's "global SA"). It also means a short "what happened since you last looked" when he opens one, as #270 also found for window switching. The utilization number is a reminder that a person who is busy all the time cannot supervise anything well. It comes from military tasks, so it does not transfer as a number.

### 4. Show the agent's plan and current step, not only its activity

**Making an automated agent's current action, plan and expected outcome visible improves awareness and performance, mostly without extra workload.**

Established:

- A systematic review of 17 experiments on agent transparency found "an overall trend in the data pointing towards a beneficial effect of transparency" on SA and performance, with no added workload where the human supervises or responds to the agent's proposals ([van de Merwe, Mallam and Nazir 2024, *Human Factors* 66(1)](https://doi.org/10.1177/00187208221077804); [full text](https://pmc.ncbi.nlm.nih.gov/articles/PMC10756021/)). The review describes three layers: the agent's current action and plans, its reasoning, and its projected outcomes and uncertainty. More detail did not always help. In one study, adding projected outcomes raised workload without changing SA.
- Mercado and colleagues tested three levels of transparency in an agent that recommended plans for several unmanned vehicles and was sometimes wrong on purpose. Performance, trust and perceived usability rose with transparency; workload and response time did not ([Mercado et al. 2016, *Human Factors* 58(3)](https://doi.org/10.1177/0018720815621206), abstract).

Inference: for a coding agent, the useful layer-1 facts are its plan (its task list), the step it is on, and what it is doing in that step. Its reasoning can wait behind a click. This fits the supervising-automation report ([#268](https://github.com/danielrispler/Falinks/issues/268)): show how the system works and when it fails, not just its claims.

### 5. Control rooms: dark when normal, overview first, deviation against a norm

**Cockpits and plant control rooms keep the normal state quiet, show the whole plant on one overview, and show each value against its normal range, so that the off-nominal stands out.**

Established:

- **The dark cockpit.** In the Douglas MD-11, when the cockpit is configured for flight, "all annunciators on the overhead panel will be extinguished". "This Dark Cockpit immediately confirms to the crew that the panels are correctly configured and that no abnormalities are present." Blue lights mean a normal system in use; amber means a fault or a switch out of position; red means immediate action. "Manual input is required for irreversible actions such as engine shutdown, fuel dump" ([Oldale 1990, NASA conference paper](https://ntrs.nasa.gov/citations/19910001626), record summary).
- **Process control.** The Abnormal Situation Management (ASM) Consortium, a Honeywell-led group that includes oil, gas and refining companies, publishes display guidelines. Reising and Bullemer (2015) write that supporting operator SA means supporting all three levels, "not just … perception of alarms through gray-scale color-coding of all non-alarm information". An ASM-compliant display "is not simply applying a new gray-scale color palette" and not just swapping old diagrams for a "dashboard display of high-performance shapes". It needs a display hierarchy, trends at every level, a small set of display shapes, and an analysis of what the operator's tasks require. It should let operators see "the overall big-picture situation (i.e., the 'forest')" and the details (the "trees") at the same time, and monitor "proactively … rather than reacting as alarms occur" ([Reising and Bullemer 2015, AFPM AM-15-07](https://process.honeywell.com/content/dam/process/en/documents/document-lists/doc_asm-consortium/white-papers/March%2031%202015%20-%20Operator%20Situation%20Awareness%20and%20Delivering%20ASM%20compliant%20High%20Performance%20HMIs%20Failure%20Modes%20and%20Success%20Factors.pdf)).
- **One controlled test.** Tharanathan and colleagues showed 18 professional refinery operators recorded plant upsets on two overview displays, while they did a separate matching task. The traditional schematic showed raw numbers; operators had to judge each value against ranges they remembered. The new display used shapes that show each value against its normal and alarm limits, and how fast it is moving, arranged by plant function. Operators detected 28.8% of process changes with the new display against 11.9% with the schematic, and answered 62.8% of comprehension probes correctly against 56.4%. The matching task did not suffer. The gain was larger in simple scenarios, and the authors warn the new display "may become visually overwhelming for high complexity scenarios". Shapes and arrangement were changed together, so their effects cannot be separated. The study was funded by the Honeywell-led ASM Consortium ([Tharanathan et al. 2010, HFES slides](https://process.honeywell.com/content/dam/process/en/documents/document-lists/doc_asm-consortium/presentations/September%2030%202010%20-%20Functional%20Versus%20Schematic%20Overview%20Displays%20Impact%20on%20Operator%20Situation%20Awareness%20in%20Process%20Monitoring.pdf); record [doi:10.1037/e578652012-011](https://doi.org/10.1037/e578652012-011)).
- **Air traffic control.** Europe's monitoring-aids specification exists "to assist the controller in the routine monitoring of the traffic situation". It does so by "warning the controller when aircraft deviate from their planned route or clearances" and "reminding the controller of actions that need to be performed" ([EUROCONTROL MONA specification, Edition 2.0, 2017](https://www.eurocontrol.int/publication/eurocontrol-specification-monitoring-aids-mona)). Here off-nominal means deviating from what the aircraft was cleared to do. Paper flight strips, one card per flight, proved hard to replace: Mackay's four-month study of Paris controllers concluded "we do not know enough to simply get rid of paper strips" ([Mackay 1999, *ACM TOCHI* 6(4)](https://doi.org/10.1145/331490.331491), abstract).
- **Overview, then detail.** "Overview first, zoom and filter, then details-on-demand" is Shneiderman's "Visual Information-Seeking Mantra" ([Shneiderman 1996](https://drum.lib.umd.edu/handle/1903/5784)). The ASM hierarchy runs from console overview to area summary, equipment summary and details (Tharanathan et al., slide 3).

Inference: these are the parts of a control room worth borrowing. Dark when normal. One overview. Each state judged against a norm: the agent's own plan (like a clearance) and its usual pace (like an operating range). And "manual input for irreversible actions" is the cockpit version of an agent asking for approval. The look of a control room, dark panels and many live gauges, is not the active part. The ASM authors warn against exactly that shortcut.

### 6. Glanceable means fewer, stronger cues, and a few relevant words

**At a glance, the eye finds one distinct feature (a colour, a size, motion) in parallel. Combinations need a slow search. A few words of the most relevant content beat a bare "something changed" signal.**

Established:

- **Preattentive features.** Tasks that can be done "in 200 milliseconds or less are considered preattentive", because eye movements take at least that long to start. Hue, intensity, orientation, size and motion can be detected this way. A target defined by a combination of two features, each shared with other items, "cannot be preattentively detected, forcing subjects to search serially" ([Healey, Booth and Enns 1996, *ACM TOCHI* 3(2)](https://doi.org/10.1145/230562.230563)).
- **Glanceable visuals.** Matthews and colleagues tested icons, pictures and text on a second-monitor display while people did another task. When people had not learned the icons, text and highly symbolic pictures worked best. Once learned, simple renditions such as coloured squares were fastest and were often read with peripheral vision alone. But people disliked them: "It is hard to remember what color represents what." The time differences were small, under one second per glance ([Matthews et al. 2006, UC Berkeley tech report EECS-2006-173](https://www2.eecs.berkeley.edu/Pubs/TechRpts/2006/EECS-2006-173.html)).
- **Relevant content beats a change signal.** Matthews, Czerwinski, Robertson and Tan gave 26 people two high-priority tasks that involved waiting 30 to 120 seconds for uploads and quiz modules to load, plus a puzzle to do meanwhile. A side panel showing a small clipped piece of each task's most relevant content (such as its progress bar) cut task time and task switches. A coloured "Change Border" that only signalled *that* a window changed did not improve efficiency. With distracting emails, it made people more likely to switch away from their current task; the authors call this result tentative, because of lost log data. The authors conclude that change detection "did not provide enough information to help users recognize windows, reacquire tasks, or to determine if an update was worthy of interruption" ([Matthews et al., CHI 2006, pp. 989–998](https://doi.org/10.1145/1124772.1124920)).

Inference: this is the most direct test of "Small and visual before text" I found, and it splits the hypothesis. *Small* holds. *Visual instead of words* does not: the winning panel showed a few words of real content, not an abstract signal. Use one strong visual cue per meaning (for example amber for "maybe stuck"), always paired with a word. Never encode a state as a combination, such as "amber and blinking and a triangle". Motion is one of these glance-level cues, so an animation that runs while all is normal spends it on nothing. Endsley's list of SA demons has a name for this: "misplaced salience".

## What this could mean for the studio (to test, not decided)

All inferences. A sketch of the panel's normal view, one row per agent session:

```
  api-refactor   Testing 3/5    "running cargo test"           12 min, last step 40 s ago
▲ docs-update    WAITING ON YOU "approve: edit README.md"      since 14:02
◆ parser-fix     Editing 2/4    "editing lexer.rs"   no new step for 9 min (usual < 2), 3 test runs failed
```

1. **One row per agent session, all visible at once.** This is the overview and Endsley's "global SA". Nothing to open just to learn that all is well.
2. **Words first, a few of them, extracted from the agent:** its current task-list item and what it is doing in it. That is Matthews' clipping, not a bare "changed" dot.
3. **Phase in the same row,** with a short cue when it changes in a way that matters (Sarter). The data-sources report ([#282](https://github.com/danielrispler/Falinks/issues/282)) found no phase field; phase has to come from the task list, the mix of tools in use and plan mode.
4. **A verdict, judged against a norm.** "Progressing" means new steps on the plan at about the session's usual pace. "Maybe stuck" means no new step for well beyond its usual pace, or the same failure repeating, or work drifting away from its plan (MONA's "deviate from … clearances"). Show the evidence behind the verdict, as #282 recommends, so a false alarm is visible as such.
5. **Dark when normal.** Grey text and no motion for working sessions. One symbol and one colour per off-nominal state. Red, amber and sound follow the levels in the alarm-fatigue report ([#269](https://github.com/danielrispler/Falinks/issues/269)).
6. **Details on demand.** One click opens the session's plan, its recent steps in time order, and "since you last looked". The agent's reasoning sits one level deeper.
7. **No telemetry wall.** Token counts, cost and raw tool logs are level 1 data with little meaning for Daniel's decisions. They belong in the detail view, if anywhere.
8. **Before designing further, run a tiny goal-directed task analysis with Daniel.** Ask: what does he decide while agents run ("step in now?", "is it on track?", "what will need me next?"), and what must he know for each decision?

## What this means for the hypotheses

- **"Serious control-room style": supported in substance, weakened as a look.** The parts that work are calm and exception-based: dark when normal, an overview, states against a norm, approval for irreversible actions. The ASM authors warn that a new palette or a "dashboard display of high-performance shapes" does not deliver awareness, and Endsley that "cool is not necessarily functional". Mission, briefing, comms and debrief can name the views, but "telemetry" as a wall of live numbers works against the goal. This matches #268 (control room as staying in the loop, not decoration) and #269 (warning, caution and advisory, not a busy alarm panel).
- **"Small and visual before text": supported for size, weakened for pictures without words.** Small, glanceable and in the periphery is supported (Healey; Matthews 2006; #270). But a few words of the most relevant content beat a bare visual change signal (Matthews, CHI 2006), and colour codes are fast only once learned, and people find them hard to remember. A visual that shows a value against its normal range does help (Tharanathan). So: a visual for *state against norm*, words for *what it is doing*. The cognitive-load report ([#271](https://github.com/danielrispler/Falinks/issues/271)) reached a similar condition for explanation cards: a visual helps when the words sit on it.
- **"Evidence, not promises": supported.** A "maybe stuck" verdict must show its evidence (section 5, and #282).
- **"Non-blocking": consistent.** The overview never blocks; detail is pulled, not pushed.
- **Problem "Blind during a run":** "what each agent is doing now" and "which phase" are level 1, cheap to show from hooks. "Progressing or stuck" is level 2 and 3, and it is the hard part. The existing-tools report ([#277](https://github.com/danielrispler/Falinks/issues/277)) found no tool that does it. The map leaves stuck detection out of v1. This report suggests the smallest honest form: pace against the session's own norm, with the evidence shown.

## Limits

- No study here involves coding agents. The evidence comes from aviation, air traffic control, refineries, military drones and office multitasking, mostly with trained operators and high stakes.
- The display studies are small: 26 office workers (Matthews, CHI 2006), 18 operators watching videos (Tharanathan), one simulated flight with 20 pilots (Sarter). The ASM study was funded by a Honeywell-led consortium and changed two things at once.
- An agent's "usual pace" is not like a pressure range. Agents vary a lot between tasks, so a pace-based "maybe stuck" will raise false alarms until it is tuned on Daniel's real sessions.
- Fan-out and utilization numbers come from military tasks and do not transfer as numbers.

## Sources checked

Fetched and checked against the source on 2026-10-27:

- Endsley, "The Role of Situation Awareness and Cognitive Engineering in Future Weapon Requirements, Design and Operation", slides, NDIA Armaments conference 2012, https://ndia.dtic.mil/wp-content/uploads/2012/armaments/WednesdayEndsley.pdf. Full text of all 46 slides. Checked: the definition (slide 9), error causes and "task distraction" (slide 16), the SA demons (slide 17), the 50 principles and principles 1–6 wording (slides 22, 30, 32), the pitfalls quotes (slide 31), goal-directed task analysis (slides 22–27).
- Jones DG, Endsley MR, "Sources of situation awareness errors in aviation", *Aviation, Space, and Environmental Medicine* 67(6): 507–512, 1996, PMID 8827130. Abstract: 76.3%, 20.3%, 3.4%, and the largest category. The abstract does not give the number of reports.
- Sarter NB, Mumaw RJ, Wickens CD, "Pilots' monitoring strategies and performance on automated flight decks", *Human Factors* 49(3): 347–357, 2007, doi:10.1518/001872007X196685. Abstract from PubMed and Europe PMC: 20 pilots, one hour, eye tracking, both quotes.
- Olsen DR, Goodrich MA, "Metrics for Evaluating Human-Robot Interactions", 2003, https://faculty.cs.byu.edu/~mike/mikeg/papers/OlsenGoodrichPERMIS2003.pdf. Full text: the fan-out equation, context acquisition quotes, externalization of memory, the cul-de-sac quote. The venue (the PerMIS workshop) is from the file name only.
- Cummings ML, Mitchell PJ, "Predicting Controller Capacity in Supervisory Control of Multiple UAVs", *IEEE Trans. SMC Part A* 38(2): 451–460, 2008, doi:10.1109/TSMCA.2007.914757 (Crossref). Abstract from MIT DSpace: 67%, 36%, loss of SA as the primary source. The PDF was rate-limited and not read.
- Cummings ML, Guerlain S, "Developing operator capacity estimates for supervisory control of autonomous vehicles", *Human Factors* 49(1): 1–15, 2007, doi:10.1518/001872007779598109. Abstract from Europe PMC: 8, 12 or 16 missiles, degradation at 16, the 70% quote.
- van de Merwe K, Mallam S, Nazir S, "Agent Transparency, Situation Awareness, Mental Workload, and Operator Performance: A Systematic Literature Review", *Human Factors* 66(1): 180–208, 2024, doi:10.1177/00187208221077804. PMC full text, first part: 17 studies, the trend quote, the workload findings, the three-layer description.
- Mercado JE et al., "Intelligent Agent Transparency in Human–Agent Teaming for Multi-UxV Management", *Human Factors* 58(3): 401–415, 2016, doi:10.1177/0018720815621206. Crossref record and Europe PMC abstract summary: three levels, performance, trust and usability up, workload and response time not.
- Reising DV, Bullemer P, "Operator Situation Awareness & Delivering ASM-compliant High Performance HMIs: Failure Modes and Success Factors", AFPM AM-15-07, 2015. Full text of pages 1–4 (the PDF link above). All quotes in section 5.
- Tharanathan A, Laberge J, Bullemer P, Reising DV, McLain R, "Functional versus schematic overview displays: Impact on operator situation awareness in process monitoring", HFES 54th Annual Meeting, 2010. Full text of the presentation slides (link above): 18 operators, the method, 11.9% and 28.8%, 56.4% and 62.8%, the "visually overwhelming" caution, the confound, the funding. The proceedings paper itself was not read; its PsycEXTRA record is doi:10.1037/e578652012-011.
- Oldale P, "Douglas flight deck design philosophy", NASA Aviation Safety/Automation Program Conference, 1990, https://ntrs.nasa.gov/citations/19910001626. The record summary, quoted sentence by sentence.
- EUROCONTROL Specification for Monitoring Aids (MONA), Edition 2.0, 3 March 2017, https://www.eurocontrol.int/publication/eurocontrol-specification-monitoring-aids-mona. The publication page's purpose statement. The specification number EUROCONTROL-SPEC-0142 is from the file name. The specification itself was not read.
- Shneiderman B, "The Eyes Have It: A Task by Data Type Taxonomy for Information Visualizations", University of Maryland ISR TR 96-66, 1996, https://drum.lib.umd.edu/handle/1903/5784. Abstract: the mantra.
- Healey CG, Booth KS, Enns JT, "High-speed visual estimation using preattentive processing", *ACM TOCHI* 3(2): 107–135, 1996, doi:10.1145/230562.230563. Full text, section 2: 200 ms, the feature list, the conjunction quote.
- Matthews T et al., "Evaluating Glanceable Visuals for Multitasking", UC Berkeley tech report UCB/EECS-2006-173, December 2006. Full text: findings on learned and unlearned renditions, the colour quote, the time differences, peripheral vision use.
- Matthews T, Czerwinski M, Robertson G, Tan D, "Clipping Lists and Change Borders", CHI 2006, pp. 989–998, doi:10.1145/1124772.1124920. Full text: 26 users, the tasks and waits, the task-time, task-switch and distracter results, the change-detection quote.
- Endsley MR, "Toward a Theory of Situation Awareness in Dynamic Systems", *Human Factors* 37(1): 32–64, 1995, doi:10.1518/001872095779049543. Crossref record and abstract summary only; the body is paywalled and was not read.
- Sarter NB, Woods DD, "How in the World Did We Ever Get into That Mode? Mode Error and Awareness in Supervisory Control", *Human Factors* 37(1): 5–19, 1995, doi:10.1518/001872095779049516. Crossref record and abstract only.
- Mackay WE, "Is paper safer? The role of paper flight strips in air traffic control", *ACM TOCHI* 6(4): 311–340, 1999, doi:10.1145/331490.331491. Crossref abstract only; the PDF did not load.
- Endsley MR, Rodgers MD, "Situation Awareness Information Requirements for En Route Air Traffic Control", DOT/FAA/AM-94/27, 1994, https://rosap.ntl.bts.gov/view/dot/21425. Abstract only; the PDF is a scan without text.

Rests on search summaries or memory, not a fetched primary source:

- Endsley MR, Jones DG, *Designing for Situation Awareness*, 3rd edition, CRC Press, 2025, with "60 detailed design principles". From retailer listings; the book was not read. The 2012 slides above give the earlier count of 50.
- Endsley MR, "Supporting Human-AI Teams: Transparency, explainability, and situation awareness", *Computers in Human Behavior* 140: 107574, 2023, doi:10.1016/j.chb.2022.107574 (Crossref). Not cited above. It may be the closest source to agents: by a webinar description in search results, it argues that real-time, forward-looking transparency serves SA better than after-the-fact explanation. Worth reading next; paywalled.
- Woods, Patterson and Roth, "Can We Ever Escape from Data Overload?", *Cognition, Technology & Work* 4(1): 22–36, 2002, doi:10.1007/s101110200002. Not cited above. By search summaries, it argues that filtering alone does not cure overload and that "organisation precedes selectivity". The PDF mirrors did not load.
- Not used, on purpose: trade-press figures for an ASM study at Nova Chemicals ("48% detected before alarms", "41% faster"). I found no primary report. Also not used: the Airbus "dark cockpit" slides (the server was unreachable) and the claim that flight mode annunciators box a new mode for 10 seconds (memory only).
