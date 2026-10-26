# Measuring personal improvement without gaming it

Research report for [#280](https://github.com/danielrispler/Falinks/issues/280), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-10-26.

## Card

- **Answer:** Any single number you watch and want to raise will drift away from what it was meant to show. Measure several things at once, prefer measures you cannot raise without actually improving (prediction accuracy scored against outcomes), keep the numbers for reflection rather than reward, and look at them rarely, in a short review, not live.
- **Evidence:** Moderate. Goodhart/Campbell effects and the costs of rewards are well established in organisations and lab studies; self-tracking studies are mostly qualitative and small; no study tests a developer tracking their own understanding of agent work.
- **Supports:** "A game that rewards understanding, not volume" only in its calibration form (Brier score is a proper scoring rule: honest guessing is the best strategy). Supports "Guessing, not testing". **Weakens** chess-style ratings, streaks, points and visible progression levels as rewards; "personal progression" should be a reflective debrief, not a score to chase.
- **For Daniel:** Li, Dey and Forlizzi, ["A Stage-Based Model of Personal Informatics Systems"](https://doi.org/10.1145/1753326.1753409) (CHI 2010, 30 min). Forsgren et al., ["The SPACE of Developer Productivity"](https://queue.acm.org/detail.cfm?id=3454124) (ACM Queue 2021, 25 min).

## Words used here

- **Goodhart's law:** "when a measure becomes a target, it ceases to be a good measure" (Marilyn Strathern's phrasing of Charles Goodhart, 1975).
- **Campbell's law:** the more a number is used for decisions, the more it is corrupted and the more it distorts the activity it measures (Donald Campbell, 1979).
- **Proper scoring rule:** a way of scoring forecasts where your best expected score comes from reporting what you really believe. The Brier score is one.
- **Overjustification effect:** an expected reward for an activity you already enjoy lowers your interest in it once the reward stops.
- **Reactivity:** the act of measuring yourself changes the behaviour being measured.

Labels: **Established** means a primary source reports it. **Inference** means my reading, not something a study tested.

## 1. Why the measure becomes the goal

- **Established.** Campbell observed that quantitative social indicators used for decisions get corrupted ([Campbell, 1979, "Assessing the impact of planned social change", *Evaluation and Program Planning* 2(1)](https://doi.org/10.1016/0149-7189(79)90048-X)). Goodhart made the same point for monetary targets (1975); Strathern gave the common wording ([Strathern, 1997, *European Review* 5(3)](https://doi.org/10.1002/(SICI)1234-981X(199707)5:3%3C305::AID-EURO184%3E3.0.CO;2-4)).
- **Established.** Manheim and Garrabrant split Goodhart into four mechanisms: regressional (the proxy is noisy), extremal (pushing a proxy far breaks its link to the goal), causal (you change the proxy without the cause) and adversarial ([arXiv:1803.04585](https://arxiv.org/abs/1803.04585)). For one person tracking themselves, the first three apply; there is no adversary except your own wish to see the number rise.
- **Inference.** For agent work the obvious proxies all fail this way: tickets closed, lines merged, time saved, explanations read, questions answered. Each can rise while understanding falls. Earlier wave-1 research (#274) found people misjudge how much they understand, so self-rated understanding is a weak proxy too.

## 2. What rewards do to motivation

- **Established.** A meta-analysis of 128 experiments found expected, tangible rewards tied to doing a task lower intrinsic motivation for interesting tasks; unexpected rewards and informational positive feedback do not, and verbal feedback can raise it ([Deci, Koestner and Ryan, 1999, *Psychological Bulletin* 125(6)](https://doi.org/10.1037/0033-2909.125.6.627)).
- **Established.** Self-Determination Theory says feedback helps when it feels *informational* (tells you how you are doing) and hurts when it feels *controlling* (pressures you) ([Ryan and Deci, 2000, *American Psychologist* 55(1)](https://doi.org/10.1037/0003-066X.55.1.68)).
- **Established, mixed.** Gamification reviews find mostly positive but context-dependent and often short-lived effects, from studies of uneven quality ([Hamari, Koivisto and Sarsa, 2014, HICSS](https://doi.org/10.1109/HICSS.2014.377)).
- **Inference.** Points, levels and streaks are expected rewards for doing things, the case shown to hurt. A number that only *informs* (your calibration this month) fits the safer side.

## 3. What self-tracking research says

- **Established.** Li, Dey and Forlizzi describe five stages: preparation, collection, integration, reflection, action. Problems in an early stage spoil later ones; many tools collect well but support reflection poorly (CHI 2010, link in the card; 68 interviews plus a survey).
- **Established.** People stop tracking for reasons such as the data not answering their questions, effort of collection, and guilt or discouragement; lapsing is normal and resumption should be easy ([Epstein et al., 2015, "A Lived Informatics Model of Personal Informatics", UbiComp](https://doi.org/10.1145/2750858.2804250); [Epstein et al., 2016, "Beyond Abandonment to Next Steps", CHI](https://doi.org/10.1145/2858036.2858045)).
- **Established.** Self-monitoring itself changes behaviour (reactivity), usually toward the goal in the short term ([Korotitsch and Nelson-Gray, 1999, *Psychological Assessment* 11(4)](https://doi.org/10.1037/1040-3590.11.4.415)). In a randomized trial, adding a step tracker to a weight-loss programme gave *less* weight loss over 24 months ([Jakicic et al., 2016, *JAMA* 316(11)](https://doi.org/10.1001/jama.2016.12858)): tracking is not guaranteed to help.
- **Inference.** Collect automatically (no effort), and spend design effort on the reflection stage: a weekly debrief that asks "what surprised you?" over the data, not a live dashboard.

## 4. What developer-productivity research says

- **Established.** SPACE says productivity cannot be one number. Use at least three of five dimensions (Satisfaction and well-being, Performance, Activity, Communication and collaboration, Efficiency and flow), mix self-report with system data, and keep metrics in tension so gaming one shows in another. It warns that activity counts alone are misleading (link in the card).
- **Established.** DORA's four delivery measures (lead time, deploy frequency, change failure rate, time to restore) are team and system measures, not individual ones; the DORA authors warn against using them to judge individuals ([Forsgren, Humble and Kim, *Accelerate*, 2018](https://itrevolution.com/product/accelerate/); [dora.dev](https://dora.dev/guides/dora-metrics-four-keys/)).
- **Inference.** For one developer and agents, a SPACE-style set could be: an outcome measure (did the agent's change hold up: reverts, rework), a calibration measure (were my predictions right), and a felt measure (how tired or confident I was). Activity (tickets, tokens) belongs only as context.

## 5. What makes feedback drive improvement

- **Established.** Ericsson's deliberate practice needs a well-defined task just beyond current skill, immediate informative feedback, and repetition with refinement ([Ericsson, Krampe and Tesch-Römer, 1993, *Psychological Review* 100(3)](https://doi.org/10.1037/0033-295X.100.3.363)). Later meta-analysis found practice explains less of performance in less structured domains (about 1% in professions) than in games or music ([Macnamara, Hambrick and Oswald, 2014, *Psychological Science*](https://doi.org/10.1177/0956797614535810)).
- **Established.** Across 607 effect sizes, feedback improved performance on average, but in over a third of cases it made performance *worse*, especially when it drew attention to the self ("you are good/bad") rather than the task ([Kluger and DeNisi, 1996, *Psychological Bulletin* 119(2)](https://doi.org/10.1037/0033-2909.119.2.254)).
- **Established.** Forecasters improve with feedback scored by Brier score, and calibration training measurably improves accuracy ([Mellers et al., 2014, *Psychological Science* 25(5)](https://doi.org/10.1177/0956797614524255)). Brier is proper: you cannot raise your expected score by shading your guesses ([Brier, 1950, *Monthly Weather Review*](https://doi.org/10.1175/1520-0493(1950)078%3C0001:VOFEIT%3E2.0.CO;2)).
- **Inference.** "Predict, then see" (will the tests pass? which file will change? will this need rework?) gives task-focused, immediate, hard-to-game feedback. The game can be gamed in one way: only predicting easy things. Counter it by mixing in prompts the studio picks.

## 6. Reflection prompts versus metrics

- **Established.** Writing a short reflection after a task improved later performance more than extra practice of the same length in a field study of call-centre trainees and lab studies ([Di Stefano et al., 2014/2023, "Learning by Thinking", HBS working paper 14-093](https://www.hbs.edu/faculty/Pages/item.aspx?num=47228)). Small sample sizes; treat as suggestive.
- **Inference.** Metrics tell you *whether* something changed; reflection prompts tell you *why*, and cannot be gamed because nobody scores them. Use both: numbers rarely, prompts at the debrief.

## Design implications (inference)

1. No volume counters as goals. Show activity only as context next to outcomes.
2. Use calibration (Brier score on your own predictions) as the one "game" number; it rewards honesty, not output.
3. Pair it with an outcome measure (rework, reverts) and a felt measure, SPACE-style, so gaming one shows in another.
4. Collect automatically; review weekly in a debrief with one or two reflection prompts.
5. Feedback about the task, never about you. No streaks; a missed week costs nothing.

## Limits

No study covers a single developer measuring their own understanding of agent work. The self-tracking literature is mostly qualitative. Sources were taken from memory of the primary literature with DOIs; I did not re-read every paper for this report.
