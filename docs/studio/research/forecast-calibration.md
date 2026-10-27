# Forecast calibration: scoring personal guesses

Research report for [#276](https://github.com/danielrispler/Falinks/issues/276), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-10-27. It builds on the [measuring-improvement report](https://github.com/danielrispler/Falinks/blob/8550db7cc6c9eed83bccc7d0f3aa45518d1b61f3/docs/studio/research/measuring-improvement.md) (#280). That report already chose the Brier score on Daniel's own predictions as the one game number, reviewed weekly, and explains why. This report covers what it left open: what a scorable guess looks like, which part of the score shows understanding, how many guesses a score needs, and what chess ratings need.

> **Answer:** Free-text guesses cannot be scored: a scorable guess is a yes/no claim the system checks itself, with a confidence level chosen before the result. The Brier score splits into calibration (knowing how sure to be, learned fast) and resolution (telling apart what will happen from what won't: the understanding signal). A score means little before about 40 guesses, a calibration chart before about 200, and a monthly trend needs a few hundred guesses a month. Elo and Glicko need opponents, and Daniel has none.
> **Evidence:** Strong for the maths (Brier 1950; Murphy 1973; Ferro and Fricker 2012) and for calibration training in lab tasks (Lichtenstein and Fischhoff 1980; Stone and Opel 2000). The sample sizes are my own calculation. Weak for developers: their "90% sure" effort ranges hold 60–70% of the time (Jørgensen), and no study covers guesses about agent work.
> **Hypotheses:** Supports "Guessing, not testing" if the card shows the outcome and never a per-guess score. Supports the calibration rating in "A game that rewards understanding, not volume", reported as calibration plus resolution with an error band. Weakens the chess-style rating.
> **Read yourself:** Tetlock and Gardner, *Superforecasting* (2015), chapter 3, "Keeping Score" (about 40 min); Glickman, ["The Glicko system"](http://www.glicko.net/glicko/glicko.pdf) (first 2 pages, about 10 min).

## The idea in plain words

A guess becomes measurable when it has three parts: a claim that turns out true or false, a number for how sure you are, and a check afterwards that needs no argument.

The Brier score is the average squared gap between how sure you were and what happened (1 if it happened, 0 if not). 0 is perfect. Always saying 50% scores 0.25. Lower is better.

One number hides two skills:

- **Calibration:** when you say 80%, does it happen about 80% of the time? This is knowing how much you know.
- **Resolution:** are your numbers different for things that happen and things that don't? This is knowing something about the code.

Example: tests pass 70% of the time, and Daniel always says "70% they pass". He is perfectly calibrated and has zero resolution. He learned the base rate, not the code. For the studio's goal, understanding, resolution is the part that matters.

And small numbers lie. With 20 guesses, luck moves the score more than skill does.

## Words used here

- **Base rate:** how often an outcome happens overall, for example how often the tests pass.
- **Confidence level:** the number Daniel picks for "how sure", from a few fixed choices.
- **Reliability (REL), resolution (RES), uncertainty (UNC):** the three parts of the Brier score in Murphy's split. REL is the calibration error, so lower is better.
- **Error band:** the range the true value probably lies in (here, 95%).
- **Rating deviation (RD):** Glicko's error band on a chess rating.

Labels: a cited claim comes from the source named. **Inference** marks my reading, which no study tested.

## 1. What a guess must look like to be scorable

The v0 prototype ([#315](https://github.com/danielrispler/Falinks/issues/315#issuecomment-6418665855)) asks free-text outcome guesses with a little context, such as "will the tests pass?" or "what will change in auth.rs?". The card shows the guess back, with no score.

- **The score needs a probability and a clean outcome.** Brier defined the score for forecasts that give a probability to each possible outcome, checked against the one that happened ([Brier, 1950, *Monthly Weather Review* 78(1): 1–3](https://doi.org/10.1175/1520-0493(1950)078%3C0001:VOFEIT%3E2.0.CO;2)). Free text has neither.
- **Words are not numbers.** A 1951 US intelligence estimate called a Soviet attack on Yugoslavia a "serious possibility". Sherman Kent then asked the board members who had agreed that wording what odds they meant. The answers ranged from 20 to 80 to 80 to 20 ([Kent, "Words of Estimative Probability", *Studies in Intelligence* 8(4), 1964](https://www.cia.gov/resources/csi/static/Words-of-Estimative-Probability.pdf)). So "probably" cannot be scored; a number can.
- **Precision pays, for skilled forecasters.** Across 888,328 geopolitical forecasts, rounding numbers into word-like categories lost accuracy every time ([Friedman, Baker, Mellers, Tetlock and Zeckhauser, *International Studies Quarterly*, 2018](https://doi.org/10.1093/isq/sqx078)).
- **Inference.** A tired user needs few choices, and few levels make the score's parts exact (section 2). Four or five fixed levels are the trade-off.

What this means for v0's two examples (**inference**):

- **"Will the tests pass?"** is already a yes/no claim. It needs a confidence level and a check. The check is the command's exit status in the `PostToolUse` payload (its field names are still unchecked, per #315). Scorable.
- **"What will change in auth.rs?"** has no probability and no mechanical check. It is not scorable as it stands. Three options:
  1. Keep it unscored, as a thinking prompt. The [learning-that-sticks report](https://github.com/danielrispler/Falinks/blob/87487b968385073ec17c928091f5d2cb881ad14a/docs/studio/research/learning-that-sticks.md) (#272) found that the benefit comes from producing the guess, not from grading it.
  2. Turn it into a claim the diff can settle, such as "the change touches `verify_token`", with a confidence level.
  3. Let the card writer judge whether the free text matched, and pair that with a confidence level. This is scorable, but the judge's errors enter the score, and Daniel can argue with it.

  Options 1 and 2 together keep the free guess for learning and add one clean claim for the score.

The other rules for a scorable guess:

- **Lock the guess before the result shows.** v0 already does this: the card waits for the step's result.
- **The system picks which steps get a scored claim, not Daniel.** Otherwise the score rewards picking easy claims. #280 makes the same point.
- **Format:** pick a side (pass or fail), then how sure, from 50% ("no idea") up. This matches the two-choice items used in the calibration training studies in section 3.

## 2. How to score: Brier and its three parts

- **Two scales exist.** The common form is the average of (confidence − outcome)², from 0 to 1. Brier's original sums over both outcomes, so for yes/no it is twice as large, from 0 to 2. Good Judgment Open uses the 0–2 scale: "The best (lowest) possible Brier score is 0, and the worst (highest) possible Brier score is 2" ([GJ Open FAQ](https://www.gjopen.com/faq)). A 50/50 guess scores 0.25 on one scale and 0.5 on the other. **Inference:** use 0–1, and say so on screen, so numbers from *Superforecasting* don't confuse.
- **It rewards honesty.** A proper scoring rule gives its best expected score to the forecaster who reports what they believe ([Gneiting and Raftery, 2007, *JASA* 102(477): 359–378](https://doi.org/10.1198/016214506000001437)). #280 covers why this matters.
- **Inference: it is bounded.** The worst single guess costs 1. The log score, also proper, costs about 4.6 for a 99% miss, so one confident miss can dominate a small personal sample.

**Murphy's split.** Murphy partitioned the score into three parts ([Murphy, 1973, *Journal of Applied Meteorology* 12(4): 595–600](https://doi.org/10.1175/1520-0450(1973)012%3C0595:ANVPOT%3E2.0.CO;2)). Group the guesses by confidence level. Then:

`Brier = REL − RES + UNC`

- **REL (calibration error):** for each level, the gap between the stated confidence and how often it came true, weighted by how often the level was used. 0 is perfect.
- **RES (resolution):** how far each level's hit rate moves away from the overall base rate. Higher is better. This is the "knows something" part.
- **UNC (uncertainty):** base rate × (1 − base rate). It depends only on which questions came up, not on Daniel.

**Inference.** UNC is the trap for a personal score. If a project's tests start passing 95% of the time, UNC falls, and Daniel's Brier score improves with no change in him. So compare REL and RES over time, not the raw score. Or compare against a base-rate guesser on the same claims: skill = 1 − Brier ÷ (Brier of always guessing the base rate).

Two technical points decide how the split is computed:

- **Fixed levels make it exact.** When forecasts are grouped into bins, two extra within-bin parts appear, and the split depends on the bin width ([Stephenson, Coelho and Jolliffe, 2008, *Weather and Forecasting* 23(4): 752–757](https://doi.org/10.1175/2007WAF2006116.1)). With a few fixed levels, each level is its own group, so the split is exact.
- **Small samples bias it.** The standard split "yields biased estimates": reliability is overestimated, uncertainty is underestimated, and resolution may go either way. An unbiased split is impossible, but a corrected one has smaller biases ([Ferro and Fricker, 2012, *QJRMS* 138(668): 1954–1960](https://doi.org/10.1002/qj.1924)). The corrected estimates vary more from sample to sample ([Siegert, arXiv:1303.6182](https://arxiv.org/abs/1303.6182)).

## 3. Which part improves, and how

- **Calibration improves fast with feedback.** In Lichtenstein and Fischhoff's study, people answered sessions of 200 two-choice general-knowledge questions. After each session they got personal calibration charts and scores, discussed for 5 to 10 minutes. Calibration clearly improved, mostly after the first session. The authors concluded that one intensive feedback session is enough to become well calibrated. This is as described by [Stone and Opel](https://doi.org/10.1006/obhd.2000.2910); I did not read the original ([*OBHP* 26(2): 149–171, 1980](https://doi.org/10.1016/0030-5073(80)90052-5)).
- **Calibration and resolution are separate skills.** Feedback on your own performance reduced overconfidence. Feedback about the subject itself ("environmental feedback") improved discrimination, which is resolution. Neither improved the other, and environmental feedback raised overconfidence ([Stone and Opel, 2000, *OBHDP* 83(2): 282–309](https://doi.org/10.1006/obhd.2000.2910)).
- **Inference for the studio.** A calibration chart in the weekly debrief should teach Daniel how sure to be, within weeks. The cards about the code are the "environmental feedback" that should raise resolution. Expect a dip: learning about the code may make him more overconfident for a while.
- **People with fast, clear feedback get well calibrated.** US weather forecasters' probability forecasts for rain and temperature were reliable ([Murphy and Winkler, 1977, *JRSS C* 26(1): 41–47](https://doi.org/10.2307/2346866)). **Inference:** they make many forecasts and learn each outcome within a day, the conditions the studio can give Daniel.
- **Skill shows over hundreds of questions.** The Good Judgment Project's top forecasters "maintained high accuracy across hundreds of questions", two years in a row, against the expected regression to the mean ([Mellers et al., 2015, *Perspectives on Psychological Science* 10(3): 267–281](https://doi.org/10.1177/1745691615577794)).
- **Developers start overconfident.** Software effort ranges given with 90% confidence "typically" contain the actual effort 60–70% of the time ([Jørgensen and Gruschke, 2009, *IEEE TSE* 35(3): 368–383](https://doi.org/10.1109/TSE.2009.2), citing [Jørgensen, Teigen and Moløkken, 2004](https://doi.org/10.1016/S0164-1212(02)00160-7)). In general-knowledge tests, people's "90%" ranges matched a felt confidence of 50% or less, with hit rates lower still ([Teigen and Jørgensen, 2005, *Applied Cognitive Psychology* 19(4): 455–475](https://doi.org/10.1002/acp.1085)).
- **Reflection alone did not fix it.** Twenty professionals were split at random. Those who spent at least 30 minutes after each task on lessons learned were no more accurate, and no more realistic about uncertainty, than the control group. A follow-up with 83 professionals found that feedback on *other* people's estimates made uncertainty judgments more realistic than the same feedback on one's own (Jørgensen and Gruschke, 2009). **Inference:** the debrief needs the numbers, not just prompts. A comparison guesser on the same claims, such as a model or the base rate, might help in the same way; that is untested.

## 4. How many guesses before a score means anything

All figures in this section are my calculation. They assume yes/no claims, a guesser who uses five levels (50, 60, 70, 80 and 90%) equally often and is perfectly calibrated, and independent guesses. Such a guesser averages 0.19, and always saying 50% scores 0.25. One guess's score has a spread (standard deviation) of 0.19. The script and a 4,000-run simulation per sample size were run for this report; they are not in the repo.

| Guesses | 95% band on the average Brier score | 95% band on the hit rate, if 80% of guesses at the "80%" level came true |
| ---: | :---: | :---: |
| 10 | ± 0.12 | 49–94% |
| 20 | ± 0.08 | 58–92% |
| 50 | ± 0.05 | 67–89% |
| 100 | ± 0.04 | 71–87% |
| 200 | ± 0.03 | 74–85% |

The hit-rate column uses the Wilson interval. The textbook "± 1.96 × √(p(1−p)/n)" interval behaves erratically at small sizes; Wilson or Jeffreys is recommended there ([Brown, Cai and DasGupta, 2001, *Statistical Science* 16(2): 101–133](https://doi.org/10.1214/ss/1009213286); [Wilson, 1927](https://doi.org/10.1080/01621459.1927.10502953)).

What the numbers say:

- **About 40 guesses** before a calibrated guesser's average is clearly better than always saying 50%.
- **About 200 guesses** before the calibration part (REL) can be trusted. From luck alone, a perfectly calibrated guesser shows a calibration error of about 0.95 ÷ N: 0.019 at 50 guesses, 0.005 at 200. Compare a clearly overconfident guesser, who says 90% and is right 70%, says 70% and is right 60%, and so on. That guesser's true error is 0.015. At 50 guesses, the calibrated guesser looks worse than that in about half the runs. At 100 it is 16%, and at 200 it is 1%. This matches the direction Ferro and Fricker report: small samples overstate REL.
- **A few hundred guesses per period** before a trend shows. To detect a drop of 0.05 in the average between two periods (80% power, 5% test), each period needs about 220 guesses. A drop of 0.03 needs about 610.
- **Overconfidence can cancel real knowledge.** The overconfident guesser above knows something (right 70% when saying 90%), yet averages 0.25, the same as always saying 50%.

**Inference for the studio.** With one scored claim per wait and, say, 10 waits a day, 200 guesses take about four working weeks. A weekly number is mostly noise. Show a rolling window of the last 200 guesses with its error band, and read trends monthly. Hide the calibration chart until 200 guesses exist. Draw it with consistency bars, which show how far a perfectly calibrated guesser's dots could stray by chance ([Bröcker and Smith, 2007, *Weather and Forecasting* 22(3): 651–661](https://doi.org/10.1175/WAF993.1)). Real guesses in one session are not independent (same code, same mood), so the true bands are wider than these.

## 5. Elo and Glicko: what they measure and what they need

- **Elo measures strength relative to opponents.** Each game result is compared with the result expected from the rating difference. FIDE converts the difference to an expected score with a table. A new player's rating is published "when it is based on at least 5 games against rated opponents", and stays at the fast-moving K = 40 "until they have completed events with at least 30 games" ([FIDE Rating Regulations, effective 1 March 2024, articles 7.1.4 and 8.3.3](https://handbook.fide.com/chapter/B022024)).
- **Glicko adds an error band.** Glickman built it because Elo has no measure of how reliable a rating is. Each player gets a rating deviation (RD). An unrated player starts at 1500 with RD 350. The 95% interval is the rating ± 2 RD. Games shrink RD, and time without games grows it. The system "works best when the number of games in a rating period is moderate, say an average of 5-10 games per player". Elo is a special case of it ([Glickman, "The Glicko system"](http://www.glicko.net/glicko/glicko.pdf); [Glickman, 1999, *JRSS C* 48(3): 377–394](https://doi.org/10.1111/1467-9876.00159)).
- **In education, the opponent is the question.** Adaptive learning systems treat an answer as a match between student and item, and learn the student's skill and the item's difficulty together. They expect each item to be answered by many students, often orders of magnitude more answers per item than per student ([Pelánek, 2016, *Computers & Education* 98: 169–179](https://doi.org/10.1016/j.compedu.2016.03.017); [accepted manuscript](https://www.fi.muni.cz/~xpelanek/publications/CAE-elo.pdf), sections 2.3 and 5.3).

**Inference for Daniel:**

- **There are no opponents.** Each claim is new, answered once, by one person. A question's difficulty can never be learned, so the "question as opponent" trick fails.
- **One fixed opponent collapses a rating to a win rate.** That opponent could be a model guessing the same claims, or the base-rate guesser. A relative Brier score carries the same information with less machinery: Daniel's score minus the opponent's, on the same claims. Good Judgment Open does this against the crowd's median ([FAQ](https://www.gjopen.com/faq)).
- **What carries over from Glicko is the idea, not the formula.** Always show the error band with the number. Publish nothing before a minimum count. Even chess publishes no rating before 5 games and lets it move fast for its first 30.
- **#280 found** that ratings, levels and points risk becoming targets.

## What this could mean for the studio (to test, not decided)

1. Keep v0's free-text guess as a thinking prompt. Never score it.
2. Add one checkable claim per wait, chosen by the system from the step. Examples: "this command exits with success", or "the diff touches this function". The answer comes from the hook payload or Git, never from Daniel's own judgment.
3. Confidence is one tap: pick a side, then one of four or five fixed levels.
4. The card shows the outcome next to the guess ("you said 80% pass; it passed"). It never shows a per-guess score.
5. The weekly debrief (#280) shows, over the last 200 claims: the Brier score on the 0–1 scale with its error band, calibration as a chart with consistency bars, and resolution or skill over the base rate. The chart stays hidden until 200 claims exist.
6. Store each claim with its type, side, level, time, outcome and how it was checked, so every number can be recomputed later.
7. An existing tool could hold the record for v1. Fatebook keeps predictions private by default and shows a Brier score, a relative Brier score and a calibration chart ([introduction, July 2023](https://www.lesswrong.com/posts/yS3d46m23wRKDQobt/introducing-fatebook-the-fastest-way-to-make-and-track)). Its API documents creating and reading questions with an API key ([API setup](https://fatebook.io/api-setup)). I found no documented way to resolve a question by API. It would also send Daniel's data off his machine and need a stored key.

## Limits

- No study covers guesses about coding agents' steps. The training evidence comes from general-knowledge quizzes, weather, geopolitics and effort estimates.
- The sample sizes rest on my assumptions: five levels used equally, a perfectly calibrated guesser, independent guesses. Real guesses are correlated, so real bands are wider.
- Claims the system can check may be easy and repetitive (tests usually pass). That lowers UNC and leaves little room for resolution to show.
- The studio's number of waits per day is unknown. The "four working weeks" figure is an example, not a measurement.
- I did not read in full: Brier 1950, Murphy 1973, Lichtenstein and Fischhoff 1980, Elo's book, or *Superforecasting*.

## Sources checked

Fetched and checked against the source:

- Kent, "Words of Estimative Probability", *Studies in Intelligence* 8(4), 1964. Full text from the CIA PDF (link above). Checked: the 1951 "serious possibility" wording, Kent's own 65 to 35, and the board's range from 20 to 80 to 80 to 20.
- Stone ER, Opel RB, *OBHDP* 83(2): 282–309, 2000, doi:10.1006/obhd.2000.2910. Read the abstract and introduction from a full-text PDF on a course site (home.csulb.edu). Checked: the abstract's findings, and its description of Lichtenstein and Fischhoff (200 questions per session, charts discussed 5 to 10 minutes, gains mainly from the first session, one session "sufficient").
- Glickman, "The Glicko system" (glicko.net PDF), pages 1–3. Checked: the reason for RD, 1500 and 350, rating ± 2 RD, RD growing with time, "5-10 games", Elo as a special case.
- FIDE Rating Regulations, effective 1 March 2024, via the FIDE handbook page. Checked: article 7.1.4 (5 games) and article 8.3.3 (K = 40 until 30 games).
- Pelánek, *Computers & Education* 98: 169–179, 2016. Read the accepted manuscript: abstract, section 2.3 (items answered by many students) and section 5.3.
- Jørgensen M, Gruschke TM, *IEEE TSE* 35(3): 368–383, 2009. Abstract from OpenAlex; full text from Simula (cms.simula.no, Simula.SE.223.pdf) for the "60-70% interval hit rate" sentence.
- Teigen KH, Jørgensen M, *Applied Cognitive Psychology* 19(4): 455–475, 2005. Abstract from OpenAlex.
- Ferro CAT, Fricker TE, *QJRMS* 138(668): 1954–1960, 2012. Abstract from Crossref.
- Stephenson DB, Coelho CAS, Jolliffe IT, *Weather and Forecasting* 23(4): 752–757, 2008. Abstract from OpenAlex.
- Bröcker J, Smith LA, *Weather and Forecasting* 22(3): 651–661, 2007. Abstract from OpenAlex.
- Brown LD, Cai TT, DasGupta A, *Statistical Science* 16(2): 101–133, 2001. Abstract from OpenAlex (recommends Wilson or Jeffreys for small n).
- Gneiting T, Raftery AE, *JASA* 102(477): 359–378, 2007. Abstract from OpenAlex (definition of proper).
- Glickman ME, *JRSS C* 48(3): 377–394, 1999. Abstract from OpenAlex.
- Mellers B et al., *Perspectives on Psychological Science* 10(3): 267–281, 2015. Abstract from OpenAlex ("hundreds of questions", two years).
- Friedman JA et al., *International Studies Quarterly*, 2018, doi:10.1093/isq/sqx078. Abstract from OpenAlex (888,328 forecasts). Volume and pages were not in the metadata.
- Murphy AH, Winkler RL, *JRSS C* 26(1): 41–47, 1977. Abstract from RePEc ("weather forecasters can formulate such forecasts in a reliable manner").
- Siegert S, arXiv:1303.6182. Abstract only.
- Good Judgment Open FAQ: the 0–2 range quote and the relative Brier score against the crowd median.
- Fatebook: the API setup page (two endpoints, API key, made by Sage Future) and the LessWrong introduction post (11 July 2023; private by default; Brier, relative Brier, calibration chart).
- *Superforecasting* table of contents, from a library catalog record (gapines.org): chapter 3 is "Keeping Score".
- The figures in section 4: computed and simulated for this report (Python, 4,000 runs per size, seed 276). The simulated REL matched the 0.95 ÷ N formula at every size.

Checked only for title, authors, journal, volume and pages (Crossref or OpenAlex), not content:

- Brier 1950, *Monthly Weather Review* 78(1): 1–3. Its 0–2 range comes from Wikipedia's "Brier score" page and the GJ Open FAQ, not the paper.
- Murphy 1973, *Journal of Applied Meteorology* 12(4): 595–600. The formula `REL − RES + UNC` and the definitions come from Wikipedia's "Brier score" page, consistent with Stephenson et al.'s abstract.
- Lichtenstein S, Fischhoff B, "Training for calibration", *OBHP* 26(2): 149–171, 1980. Content only through Stone and Opel. Sources disagree on the number of sessions: Stone and Opel say 11; a search summary of a DTIC report said 2 or 10. The DTIC site was down for maintenance.
- Wilson 1927, *JASA* 22(158): 209–212.
- Jørgensen, Teigen and Moløkken 2004, *JSS* 70(1–2): 79–93. "Much too narrow" comes from a search summary of its abstract; the full-text PDF did not extract.

Rests on memory, not a fetched source:

- The content of *Superforecasting* chapter 3 (Brier scores, calibration and resolution, the weather forecaster example, Kent's story). The reading time is my estimate.
