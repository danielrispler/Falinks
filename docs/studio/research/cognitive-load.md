# Cognitive load when explaining a change

Research report for [#271](https://github.com/danielrispler/Falinks/issues/271), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-10-26.

## Card

- **Answer:** Explain a change in small pieces that Daniel steps through at his own pace: one idea per card, with no more than about four new things on it. Put each label directly on the code or diagram it describes. Name the goal of each step first ("this step saves the total"), then show the solved example. Cut decorative extras. Give less help in parts of the code Daniel already knows.
- **Evidence:** Strong for the general principles: decades of replicated randomized lab studies. Moderate for code: subgoal labels and change ordering, from small studies. Weak for tired learners: fatigue clearly hurts, but how to design for it is barely tested.
- **Supports:** "Small and visual before text" (segmenting, words with pictures, coherence). Partly supports "Move learning into waiting time" (small, spread-out pieces help; no study tests waiting on an agent) and "Guessing, not testing" (asking Daniel to fill in a step works once he knows the basics). **Complicates:** a visual helps only when the words sit on it, not beside it; and help that suits a novice slows down someone who already knows the code (the "expertise reversal effect").
- **For Daniel:** Mayer and Moreno, ["Nine ways to reduce cognitive load in multimedia learning"](https://doi.org/10.1207/S15326985EP3801_6) (2003, 10 pages, 30 min). Then Sweller, van Merriënboer and Paas, ["Cognitive architecture and instructional design: 20 years later"](https://doi.org/10.1007/s10648-019-09465-5) (2019, sections 1–3, 40 min).

## Words used here

- **Working memory:** the small "desk" where you hold what you are thinking about right now. It holds about four chunks at once ([Cowan, 2001](https://doi.org/10.1017/S0140525X01003922)).
- **Chunk:** a group of things memory treats as one unit. An expert sees `for i in 0..n` as one chunk; a novice sees several.
- **Cognitive load:** how full that desk is. Sweller splits it into *intrinsic* load (how hard the material is, given what you already know) and *extraneous* load (load added by poor presentation) ([Sweller et al., 2019](https://doi.org/10.1007/s10648-019-09465-5)).
- **Worked example:** a solved problem shown step by step, studied instead of solved.
- **Randomized experiment:** people are assigned to versions by chance, so a difference can be blamed on the version.

Labels: **Established** means a primary source reports it. **Inference** means my application to explanation cards, which no study tested.

## 1. The core idea: the desk is small

Established:

- Working memory holds about four chunks. Long-term memory is practically unlimited, and what is stored there becomes chunks that free up the desk ([Cowan, 2001](https://doi.org/10.1017/S0140525X01003922); [Sweller et al., 1998](https://doi.org/10.1023/A:1022193728205)).
- Learning fails when the total load overflows the desk. Design can rarely shrink intrinsic load without changing what is learned, but it can remove extraneous load ([Sweller, 1988](https://doi.org/10.1207/s15516709cog1202_4); [Sweller et al., 2019](https://doi.org/10.1007/s10648-019-09465-5)).

Inference: a long end-of-run page brings many new chunks at once (files, names, reasons, test results). That is the overflow case.

## 2. Effects that apply to explaining a change

Each effect below comes from randomized experiments, mostly with students learning maths, science or technical procedures.

| Effect | What was found | Card rule (inference) |
| --- | --- | --- |
| **Worked example** | Novices learn more from studying solved problems than from solving the same problems unaided ([Sweller and Cooper, 1985](https://doi.org/10.1207/s1532690xci0201_3); [Sweller et al., 1998](https://doi.org/10.1023/A:1022193728205)). | Show the change as a solved example: problem, step, result. |
| **Split attention** | When a diagram and its text are apart, learners must hold one in mind while searching the other. Putting the text on the diagram helps ([Chandler and Sweller, 1991](https://doi.org/10.1207/s1532690xci0804_2)). | Annotate the diff line itself, not a separate panel. |
| **Redundancy** | Showing the same content twice (a diagram plus text saying the same thing) *hurts* when either alone is understandable ([Chandler and Sweller, 1991](https://doi.org/10.1207/s1532690xci0804_2)). | If a visual and a sentence say the same thing, keep one. |
| **Segmenting** | Learners who stepped through short segments at their own pace learned more than those given one continuous presentation ([Mayer and Chandler, 2001](https://doi.org/10.1037/0022-0663.93.2.390)). | One idea per card; Daniel moves on when ready. |
| **Pre-training** | Learning the names and roles of the parts first makes the main explanation easier ([Mayer and Moreno, 2003](https://doi.org/10.1207/S15326985EP3801_6)). | Before the change, name the two or three pieces it touches. |
| **Signalling** | Cues that show structure (highlights, headings, "first, second") help ([Mayer and Moreno, 2003](https://doi.org/10.1207/S15326985EP3801_6)). | Highlight the changed lines; number the steps. |
| **Coherence** | Removing interesting but unneeded material improves learning ([Mayer and Moreno, 2003](https://doi.org/10.1207/S15326985EP3801_6)). | No decorative trivia on the card. |
| **Multimedia** | Words with pictures beat words alone, when the picture carries the structure ([Mayer and Moreno, 2003](https://doi.org/10.1207/S15326985EP3801_6)). | A small labelled diagram of what calls what. |
| **Expertise reversal** | Help that suits novices (worked examples, built-in notes) becomes extra load for people who already know the material; they do better with less ([Kalyuga et al., 2003](https://doi.org/10.1207/S15326985EP3801_4)). | Fade the help for code Daniel already knows. |
| **Completion and fading** | Moving from full worked examples to partial ones, where the learner fills in a step, works better than staying with full examples ([Sweller et al., 1998](https://doi.org/10.1023/A:1022193728205); [Kalyuga et al., 2003](https://doi.org/10.1207/S15326985EP3801_4)). | Later cards hide one step and ask Daniel to predict it. |

Caveat: each effect has limits. Most hold for novices with complex material and can shrink or reverse otherwise ([Sweller et al., 2019](https://doi.org/10.1007/s10648-019-09465-5)).

## 3. Evidence closer to code

Established:

- **Subgoal labels.** Worked examples whose steps were grouped under short goal labels improved novice programmers' problem solving, compared with the same examples without labels ([Morrison, Margulieux and Guzdial, 2015](https://doi.org/10.1145/2787622.2787733); the method comes from [Catrambone, 1998](https://doi.org/10.1037/0096-3445.127.4.355)). Small studies, students, one course.
- **Change ordering in review.** In a code review experiment, the order in which a change's parts were shown, and the reviewer's working memory capacity, were linked to review results on larger changes ([Baum, Schneider and Bacchelli, 2019](https://doi.org/10.1007/s10664-018-9676-8)).
- **Understanding is the review bottleneck.** Developers at Microsoft named understanding the change as the main challenge of code review ([Bacchelli and Bird, 2013](https://doi.org/10.1109/ICSE.2013.6606617)).

Inference: label each card with the goal it serves, and order the cards so each builds on the last (for example, the data type before the function that uses it).

## 4. The tired learner

Established:

- Mental fatigue weakens attention and goal-directed control, and raises reliance on habit ([Boksem and Tops, 2008](https://doi.org/10.1016/j.brainresrev.2008.07.001)).
- Sweller's group proposes a "working memory resource depletion" effect: after sustained effortful work the desk is effectively smaller, which is one reason spaced learning works ([Chen, Castro-Alonso, Paas and Sweller, 2018](https://doi.org/10.1007/s10648-017-9426-2)). This is a theoretical proposal backed by a few studies, not a settled result.
- The broader "ego depletion" idea (willpower as a fuel tank) failed a large multi-lab replication ([Hagger et al., 2016](https://doi.org/10.1177/1745691616652873)). Treat fatigue claims with care.

Inference: for a tired Daniel the same rules apply, more strictly: fewer new chunks per card, no forced reading, and pieces spread over the run rather than piled at the end. No study tests explanation design for tired developers.

## 5. What this means for the hypotheses

- **Small and visual before text:** supported, with a condition. Small and segmented is strongly supported. A visual helps when the words sit on it and it carries structure; a visual beside a paragraph that says the same thing adds load.
- **Move learning into waiting time:** partly supported. Segmenting and spacing help, and the depletion proposal favours small pieces over one long page. Whether waiting on an agent is a good moment to learn is not tested here (see #279).
- **Guessing, not testing:** partly supported. Fill-in-a-step works, but only after the basics; asking a novice to guess cold adds load.
- **Non-blocking:** neutral. Self-pacing helps (segmenting); the theory says nothing about an inbox.
- Not covered here: evidence and trust, games, control-room style, learning content.

## 6. Gaps

- No study tests explanation cards for AI-written changes.
- Most results come from students in short lab sessions, not working developers.
- "About four chunks" is an average; what counts as a chunk depends on the person.
- Source check: every DOI above (16) was resolved through Crossref on 2026-10-26, and each title, year and venue matched the citation. The findings rely on the sources' abstracts and well-known main results; full texts were not re-read for this report.
