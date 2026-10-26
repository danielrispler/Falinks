# Learning that sticks without an exam

Research report for [#272](https://github.com/danielrispler/Falinks/issues/272), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-10-26.

## Card

- **Answer:** Learning sticks when you pull the answer out of your own head (a guess, a recall, a prediction), see the correct answer shortly after, and do it again days later. Rereading or reading a summary feels like learning but fades fast. None of it needs a score: ungraded, low-stakes questions work, and in one classroom study they lowered anxiety.
- **Evidence:** Strong for retrieval practice and spacing (decades of lab work, large meta-analyses, classroom replications). Moderate for guessing first (pretesting). Weak for anything specific to developers: no study tests these on someone watching an agent change code.
- **Supports:** "Guessing, not testing" (strongly, if the true answer follows soon). "Move learning into waiting time" only if the wait is used for a guess, not for reading. "Non-blocking" with a twist: the inbox should come back days later as recall prompts. **Weakens:** "Learning content while waiting" (passive watching and reading are the weakest forms).
- **For Daniel:** Dunlosky et al., ["Strengthening the student toolbox"](https://www.aft.org/ae/fall2013/dunlosky) (American Educator, 2013), the plain-language version of the big 2013 review (20 min). Then Roediger and Karpicke, ["Test-enhanced learning"](https://doi.org/10.1111/j.1467-9280.2006.01693.x) (2006), a short paper with one clear experiment (20 min).

## Words used here

- **Retrieval practice (testing effect):** recalling something from memory instead of reading it again. The act of recalling strengthens the memory.
- **Spacing (distributed practice):** spreading repeated encounters over days instead of cramming them into one sitting.
- **Pretesting effect:** trying to answer before being taught. Even a wrong guess makes the later answer stick better.
- **Generation effect:** information you produce yourself (complete, predict, explain) is remembered better than information you only read.
- **Desirable difficulty:** Robert Bjork's term for effort that slows learning in the moment but improves long-term memory. It is "desirable" only if the learner can eventually succeed.
- **Feedback:** being shown the correct answer after an attempt.
- **Effect size (g or d):** how big a difference is, in units of the spread of scores. About 0.2 is small, 0.5 medium, 0.8 large.

Labels: **Established** means a primary source reports it. **Inference** means it is my reading, not something a study tested.

## 1. Recall beats rereading, and people believe the opposite

Established:

- Roediger and Karpicke (2006) had students read a prose passage, then either reread it or recall it. After five minutes, rereading looked slightly better. After one week the picture flipped: in experiment 2, students who studied once and recalled three times recalled about 61% of the passage, roughly 20 percentage points more than those who studied four times. Yet the rereaders predicted they would remember more ([Roediger and Karpicke, 2006](https://doi.org/10.1111/j.1467-9280.2006.01693.x)).
- Karpicke and Roediger (2008): once an item was learned, dropping it from further *study* changed little, but dropping it from further *recall* cut one-week memory sharply (about 80% vs about 35%) ([Science](https://doi.org/10.1126/science.1152408)).
- Karpicke and Blunt (2011): retrieval practice beat building concept maps, even on questions that asked students to draw inferences, not just repeat facts ([Science](https://doi.org/10.1126/science.1199327)).
- Dunlosky and colleagues rated ten common techniques. Only two got "high utility": practice testing and distributed practice. Rereading, highlighting and summarizing got "low utility" ([Dunlosky et al., 2013](https://doi.org/10.1177/1529100612453266)).
- Meta-analyses put the testing effect around g = 0.5 overall, larger when the correct answer is shown afterwards ([Rowland, 2014](https://doi.org/10.1037/a0037559); [Adesope, Trevisan and Sundararajan, 2017](https://doi.org/10.3102/0034654316689306)).

Inference: an agent's end-of-run summary is rereading. It will feel understood and then fade. "I forget what happened in previous runs" is what this research predicts.

## 2. Guessing first helps, even when the guess is wrong

Established:

- Kornell, Hays and Bjork (2009) had people try to guess an answer before seeing it. Failed guesses still led to better later memory than studying the pair for the same total time ([JEP: Learning, Memory, and Cognition](https://doi.org/10.1037/a0015729)).
- Richland, Kornell and Kao (2009) found the same with a science text: questions asked before reading, answered wrongly, improved later test scores ([JEP: Applied](https://doi.org/10.1037/a0016496)).
- The generation effect: words people produce themselves are remembered better than words they read ([Slamecka and Graf, 1978](https://doi.org/10.1037/0278-7393.4.6.592)).
- Confident wrong answers, once corrected, are fixed especially well (the "hypercorrection" effect) ([Butterfield and Metcalfe, 2001](https://doi.org/10.1037/0278-7393.27.6.1491)).

Conditions that matter (established):

- The correct answer must follow. Guessing helps because feedback arrives; without it, errors can stick ([Dunlosky et al., 2013](https://doi.org/10.1177/1529100612453266), practice testing section).
- The learner must be able to make a real attempt. Bjork stresses that a difficulty is desirable only if the learner can respond to it ([Bjork and Bjork, 2011](https://bjorklab.psych.ucla.edu/wp-content/uploads/sites/13/2016/04/EBjork_RBjork_2011.pdf)).

Inference: "What do you think the agent will change?" before the diff, then the diff, is a close fit to the pretesting design. The question must be answerable in a few seconds from what Daniel already knows.

## 3. Spacing: come back later, not more now

Established:

- Across hundreds of studies, the same practice spread over time beats massed practice ([Cepeda et al., 2006](https://doi.org/10.1037/0033-2909.132.3.354)).
- The best gap grows with how long you want to remember. For memory meant to last months, gaps of days to weeks beat gaps of hours ([Cepeda et al., 2008](https://doi.org/10.1111/j.1467-9280.2008.02209.x)).

Inference: a recall prompt about yesterday's run ("what did run 41 refuse, and why?") is worth more than another explanation of today's. The "Non-blocking" inbox could resurface items days later as quick recall questions, not only hold them.

## 4. Does it have to feel like an exam?

Established:

- The benefit comes from the act of retrieval, not from grades. Most lab studies use ungraded tests ([Roediger and Karpicke, 2006](https://doi.org/10.1111/j.1467-9280.2006.01693.x)).
- Agarwal, D'Antonio, Roediger, McDermott and McDaniel (2014), "Classroom-based programs of retrieval practice reduce middle school and high school students' test anxiety", *Journal of Applied Research in Memory and Cognition* 3(3): in a survey of 1,408 middle and high school students, 72% said retrieval practice made them less nervous for unit tests and exams; 6% said more nervous. This is self-report, not a controlled experiment.
- Free recall and short-answer prompts tend to help more than recognition (multiple choice), because they demand more retrieval effort ([Dunlosky et al., 2013](https://doi.org/10.1177/1529100612453266)).

Not established: no study I found shows that framing a question as a guess ("what do you think?") instead of a question ("what is?") keeps the benefit while feeling better. That is a design bet.

## 5. The catch: it feels worse while it works

Established:

- People judge learning by how fluent it feels, so they prefer rereading and cramming and rate them as more effective ([Bjork, Dunlosky and Kornell, 2013](https://doi.org/10.1146/annurev-psych-113011-143823); [Kornell and Bjork, 2008](https://doi.org/10.1111/j.1467-9280.2008.02127.x)).
- Learners left to choose often stop practicing too early ([Karpicke and Roediger, 2008](https://doi.org/10.1126/science.1152408)).

Inference: if prompts are optional, expect them to be skipped, and a feeling of "I got it" is not evidence. The prompt must be cheap enough to answer by reflex. This matches the comprehension-debt report's finding that useful measures cost effort and get skipped.

## 6. What the research does not cover

- The strong evidence is about facts, word pairs and textbook passages, mostly with students. Transfer to understanding a codebase is plausible (Karpicke and Blunt's inference questions) but untested.
- No study tests retrieval practice for a developer supervising a coding agent.
- Little is known about tired, interrupted adults learning in the gaps of other work.

## Mapping to the hypotheses

| Hypothesis | Verdict | Why |
| --- | --- | --- |
| Guessing, not testing | Supported | Pretesting and generation effects; feedback must follow quickly. No score is needed for the effect; the anxiety benefit is weakly shown. |
| Move learning into waiting time | Partly | Good for a prediction prompt while the agent works; reading an explanation there is weak. |
| Non-blocking | Partly, with a change | Use the inbox to space recall over days; optional prompts will be skipped. |
| Small and visual before text | Not covered | Retrieval research says little about format. |
| Learning content while waiting | Weakened | Passive videos and reading are the low-utility forms. |
| A game that rewards understanding | Indirect | Scoring predictions fits, but stakes may add pressure; see the calibration and gamification tickets. |

## Sources checked

On 2026-10-26, every DOI above was resolved through Crossref, and each title and year matched the citation. The AFT and Bjork lab links loaded. Key figures were cross-checked against abstracts and summaries: Agarwal et al. 2014 (72% less nervous, 6% more, n = 1,408) against the abstract; Rowland 2014 (g = 0.50) against several reviews; Karpicke and Roediger 2008 (about 80% vs about 35%) against summaries. Roediger and Karpicke 2006 (about 61% after repeated recall, roughly 20 points above repeated study) was checked only against summaries, because the PDF text could not be extracted. Read its experiment 2 before quoting exact numbers.
