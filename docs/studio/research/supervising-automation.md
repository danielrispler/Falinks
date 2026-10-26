# Supervising automation and calibrated trust

Research report for [#268](https://github.com/danielrispler/Falinks/issues/268), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-10-26.

## Card

- **Answer:** Supervising automation is hard because the better it works, the less you practise, watch and understand, so you are weakest exactly when it fails. Trust ends up right-sized only when you can see how the system works and when it fails, not just what it claims. Showing evidence helps; showing explanations or confidence alone can raise trust in wrong output.
- **Evidence:** Strong for the general human-factors findings (40 years, aviation, process control, many lab studies and meta-analyses). Moderate-to-weak for coding agents specifically: a few small randomized studies, none of a control-room-style supervisor.
- **Supports:** "Evidence, not promises" (with a caveat: the evidence must expose failures and limits, not just green ticks). Supports "Serious control-room style" only in its active parts (staying in the loop), not as decoration. Weakens "Non-blocking" if it lets the supervisor ignore the run entirely: out-of-the-loop people are slow and wrong when they must step in.
- **For Daniel:** Bainbridge, ["Ironies of Automation"](https://doi.org/10.1016/0005-1098(83)90046-8) (1983, 5 pages, 30 min). Then Lee and See, ["Trust in Automation"](https://doi.org/10.1518/hfes.46.1.50_30392) (2004), sections on calibration and "Designing for appropriate trust" (45 min).

## Words used here

- **Supervisory control:** the human sets goals and watches; the machine does the moment-to-moment work.
- **Out of the loop:** the supervisor no longer follows what the system is doing, so they need time to catch up when they must act.
- **Use, misuse, disuse, abuse:** Parasuraman and Riley's four ways people relate to automation. Misuse is relying on it when you should not; disuse is ignoring it when you should not; abuse is designers automating without thinking about the human.
- **Complacency:** checking the automation less often than its reliability justifies.
- **Automation bias:** taking the automation's output as a shortcut instead of checking it. Two errors: missing a problem the automation did not flag (omission), and following a wrong suggestion (commission).
- **Calibrated trust:** trust that matches how reliable the system really is, case by case.

Labels: **Established** means a primary source reports it. **Inference** means my reading, not something a study tested.

## 1. Why supervising is hard: the ironies

Established:

- Bainbridge (1983, [Automatica 19(6)](https://doi.org/10.1016/0005-1098(83)90046-8)) points out that automation leaves humans two jobs: watching for failures and taking over when they happen. Both get harder because of the automation. Skills fade without practice. Knowledge of the process fades when you no longer act on it. And people cannot hold attention on a system that rarely does anything wrong; vigilance drops within about half an hour.
- Her main irony: the more advanced the system, the more crucial the human's contribution, and the less prepared the human is to make it.
- Her remedies: let operators practise by hand, show the system's state in a way that supports understanding (not just numbers), and design displays that make abnormal states easy to notice.
- Endsley and Kiris (1995, [Human Factors 37(2)](https://doi.org/10.1518/001872095779064555)) measured it: people supervising full automation had lower situation awareness and took longer to recover after failure than people at intermediate levels of automation, where they still made some decisions.

Inference for the studio: a coding agent running for 20 minutes is the textbook case. Daniel stops following, then is asked to review the whole diff at once. "Blind during a run" is the out-of-the-loop problem by another name.

## 2. How people misuse or ignore automation

Established:

- Parasuraman and Riley (1997, [Human Factors 39(2)](https://doi.org/10.1518/001872097778543886)) show reliance depends on trust, workload, self-confidence and the cost of being wrong, not only on actual reliability. Too many false alarms causes disuse; high reliability causes misuse.
- Parasuraman and Manzey (2010, [Human Factors 52(3)](https://doi.org/10.1177/0018720810376055)) review complacency and automation bias. Both appear in experts as well as novices, are not cured by simple training or instructions, and grow when the human has other tasks. Making people feel accountable for the outcome reduces bias.
- Mosier, Skitka and colleagues (1998, [Int. J. Aviation Psychology 8(1)](https://doi.org/10.1207/s15327108ijap0801_3)) found that experienced pilots missed events the automation did not flag and followed wrong automated directives, even when other cues contradicted them.

## 3. How trust becomes calibrated

Established:

- Lee and See (2004, [Human Factors 46(1)](https://doi.org/10.1518/hfes.46.1.50_30392)) say trust rests on three things a person learns about a system: **performance** (what it does and how reliably), **process** (how it works) and **purpose** (why it was built, whose goals it serves). Trust is calibrated when people can observe these directly.
- Their design advice: show the system's past performance; make its process understandable; show when it is in conditions where it is less reliable; train people on its limits, not just its features.
- Trust drops sharply after a visible failure and recovers slowly. Failures on easy tasks hurt trust more than failures on hard ones ([Hoff and Bashir, 2015, Human Factors 57(3)](https://doi.org/10.1177/0018720814547570), a review of 127 studies).

## 4. Evidence from AI and coding

Established:

- Bansal et al. (CHI 2021, [arXiv 2006.14779](https://arxiv.org/abs/2006.14779)): AI explanations made people accept the AI's answer more often, whether it was right or wrong. Team performance did not beat the best of human or AI alone.
- Buçinca et al. (2021, [arXiv 2102.09692](https://arxiv.org/abs/2102.09692)): making people commit to their own answer before seeing the AI's ("cognitive forcing") reduced overreliance, but people liked it less.
- Perry et al. (CCS 2023, [arXiv 2211.03622](https://arxiv.org/abs/2211.03622)): developers with an AI assistant wrote less secure code and were more likely to believe it was secure.
- METR (2025, [arXiv 2507.09089](https://arxiv.org/abs/2507.09089)): in a randomized trial, 16 experienced open-source developers were 19% slower with AI tools, yet believed they were about 20% faster. Self-judged trust was badly calibrated. Small sample, one setting.

Inference: these match the older findings. Fluent output and explanations raise trust; they do not show reliability. The supervisor's sense of how well it is going is not a good gauge.

## 5. What this means for the hypotheses

- **"Evidence, not promises": supported, with a condition.** Lee and See's "performance" and "process" bases map onto "what was checked, what passed, what was refused". The condition: evidence must expose limits and failures (what was not checked, where the agent was unsure, past failure rate), or it becomes another reassuring explanation (Bansal). Inference.
- **"Serious control-room style": partly supported.** Control rooms exist because of these findings, but the value is in keeping the supervisor in the loop (small decisions during the run, Endsley and Kiris), not in looks. Inference.
- **"Non-blocking": weakened if it means full hands-off.** Being out of the loop makes takeover and review slower and worse. Small, regular touchpoints during the run fit the evidence better than one big review at the end. Inference.
- **"Guessing, not testing": supported indirectly.** Asking for your own prediction before seeing the agent's result is a cognitive forcing function; it reduces overreliance (Buçinca). Inference for the agent setting.
- **Calibration rating ("A game that rewards understanding"):** Lee and See's calibration idea gives a concrete target: compare how much Daniel trusted a change with whether it held up. Inference; untested.

## Limits of this report

- Most evidence comes from aviation, process control and short lab tasks, not from supervising coding agents for hours.
- Full texts of Bainbridge, Lee and See and Parasuraman and Riley are paywalled; claims here follow their abstracts and widely reproduced content. Daniel should check the two card sources directly.
