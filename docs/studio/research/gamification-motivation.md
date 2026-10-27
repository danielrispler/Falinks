# Gamification and motivation: when a game helps learning and when it ruins it

> **Answer:** A game helps learning when its feedback tells you how well you understood and you stay free to play or not. It ruins learning when it pays you for doing more: points, levels, leaderboards and streaks raise the number of actions, not their quality, and a promised reward can drain interest in something you already like doing.
> **Evidence:** Moderate. The reward effects come from 128 experiments (Deci, Koestner and Ryan 1999), but their size is disputed. In a 273-person experiment, game elements raised quantity but not quality (Mekler et al. 2017). A meta-analysis finds real learning gains from gamification (g = .49), but its motivation gains disappear in the rigorous studies (Sailer and Homner 2020). None of it is about developers or coding agents.
> **Hypotheses:** Supports "A game that rewards understanding, not volume" if the reward is the answer to Daniel's guess, not a score. Weakens its chess-style rating as a visible number, because a falling rating is a loss. Supports the "subtle layer of personal progression" in "Serious control-room style" only as progress against Daniel's own past, seen at the debrief. Weakens XP, levels, badges and streaks.
> **Read yourself:** Ryan, Rigby and Przybylski 2006, ["The motivational pull of video games"](https://www.rochester.edu/warner/lida/wp-content/uploads/2022/11/02bfe513dd59366750000000.pdf) (theory section and discussion, about 30 min); Mekler et al. 2017, ["Towards understanding the effects of individual gamification elements"](https://glassmanlab.seas.harvard.edu/annotated_works/mekler17.pdf) (10 pages, about 30 min).

Research report for [#275](https://github.com/danielrispler/Falinks/issues/275), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-10-27.

It builds on the [measuring-improvement report](https://github.com/danielrispler/Falinks/blob/8550db7cc6c9eed83bccc7d0f3aa45518d1b61f3/docs/studio/research/measuring-improvement.md) (#280). That report already covers Goodhart's and Campbell's laws, gamed counters and the Brier score, so this one does not repeat them. Its angle is motivation: why some game parts feed the wish to understand and others replace it.

## The idea in plain words

There are two reasons to do something. **Intrinsic motivation** means you do it because the activity itself interests you. **Extrinsic motivation** means you do it for something outside it: points, a badge, a streak, a rank. Daniel already wants to understand the agents' work; Falinks is an educational project first. So the risk is not that he lacks motivation. The risk is that a game swaps his reason ("I want to understand") for the game's reason ("I want the number to go up").

The research gives a fairly clear rule. Game feedback that *informs* ("your guess was right", "you now predict this module better than last month") tends to support motivation. Game rewards that *pay for activity* ("+10 points per card", "7-day streak") tend to raise activity and leave understanding where it was.

## Words used here

- **Overjustification effect:** a promised reward for an activity you already enjoy lowers your interest in it once the reward stops.
- **Self-Determination Theory (SDT):** Deci and Ryan's theory that people have three basic needs: **competence** (feeling effective), **autonomy** (acting by choice) and **relatedness** (connection to others). Settings that meet these needs raise intrinsic motivation.
- **Informational vs controlling:** within SDT, the same feedback can feel like information about how you are doing, or like pressure to perform. The first supports motivation; the second undermines it.
- **Free-choice measure:** in these lab studies, how long people keep doing the task on their own, after the reward is gone and nobody is asking.
- **Effect size (d or g):** the size of a difference in units of the spread of scores. About 0.2 is small, 0.5 medium, 0.8 large. A minus sign means the reward made things worse.

Labels: **Lesson** is what a group of sources says. **Inference** is my reading for the studio, not something a study tested.

## What the evidence says

### 1. A promised reward can drain interest you already had

- **Lepper, Greene and Nisbett (1973)** watched nursery-school children who already liked drawing. One group was promised a "Good Player" certificate for drawing; one got it as a surprise; one got nothing. Afterwards, in free play, the children who had been *promised* the award showed "less subsequent intrinsic interest" than both other groups (from the abstract). Secondary summaries put it at about half as much free time spent drawing. The drop appeared only in the promised group, not in the surprise group.
- **Deci, Koestner and Ryan (1999)** pooled 128 experiments. On the free-choice measure, rewards for *doing* the task cut later interest (d = −0.40), as did rewards for *finishing* it (d = −0.36) and rewards tied to *how well* you did it (d = −0.28). All expected rewards and all tangible rewards also lowered it. Positive feedback did the opposite: it raised free-choice behaviour (d = +0.33) and self-reported interest (d = +0.31). Tangible rewards tended to hurt children more than college students.
- **The size is disputed.** Cameron and Pierce (1994) pooled 96 studies and concluded that "overall, reward does not decrease intrinsic motivation". In their analysis, the only harm came from expected tangible rewards given simply for doing a task. Cameron, Banko and Pierce (2001) added that rewards for *low-interest* tasks raise motivation. On high-interest tasks, they found harm when rewards are tangible, expected and loosely tied to performance.

Lesson: the two camps disagree on how often rewards hurt, but agree on the dangerous case. That case is an expected reward, for an activity people already find interesting, paid for doing it rather than for doing it well. Verbal, informative feedback helps in both camps' data. Understanding his agents' work is a high-interest activity for Daniel, so it sits in the dangerous case.

### 2. Self-Determination Theory: what makes a game feel good

- **Ryan and Deci (2000)** sum up the theory: three innate needs, competence, autonomy and relatedness. When satisfied, they "yield enhanced self-motivation"; when thwarted, they lead to "diminished motivation and well-being".
- **Ryan, Rigby and Przybylski (2006)** applied SDT to video games in four studies. Players' sense of autonomy and competence inside the game went with enjoyment, preference for the game and well-being after play. In an online multiplayer community, autonomy, competence and relatedness each predicted enjoyment and future play. Their summary of the theory names what helps autonomy: "provisions for choice, use of rewards as informational feedback (rather than to control behavior), and non-controlling instructions". For competence, it names "opportunities to acquire new skills or abilities, to be optimally challenged, or to receive positive feedback".
- **Reeve and Deci (1996)** had people solve puzzles against a confederate. Winning raised intrinsic motivation by raising felt competence. A pressured setting lowered it by lowering the sense of choice.

Lesson: games pull people in because they satisfy needs, not because they hand out points. Clear feedback at the right difficulty builds competence; pressure and lack of choice break autonomy. A loss lowers felt competence.

### 3. Points, levels, leaderboards and streaks buy volume

- **Mekler, Brühlmann, Tuch and Opwis (2017)** ran an online experiment with 273 people tagging images. People saw points, a leaderboard, levels or nothing. All three game elements raised the *number* of tags. None changed tag *quality*, felt competence or intrinsic motivation, and more tags went with lower-quality tags. The authors conclude that in this setting points, levels and leaderboards "functioned as extrinsic incentives, effective only for promoting performance quantity".
- **Cerasoli, Nicklin and Ford (2014)** meta-analysed 183 samples with 212,468 people from school, work and sport. Intrinsic motivation was a medium-to-strong predictor of performance (ρ = .21 to .45). Modelled together, intrinsic motivation predicted more of the *quality* of performance, and incentives better predicted its *quantity*.
- **Silverman and Barasch (2023)**, seven studies: people were more likely to repeat a behaviour when their log showed it extending an unbroken streak than after a broken one. This held even when their actual past behaviour was the same and only the log display differed. The authors argue people treat keeping a logged streak as a goal in itself. The effect was stronger when people blamed themselves for the break, and weaker when they could repair it.
- Goodhart's law states the general case: "Any observed statistical regularity will tend to collapse once pressure is placed upon it for control purposes" (Goodhart 1975). The measuring-improvement report (#280) covers how such numbers get gamed.

Lesson: the common game elements are activity counters. They reliably produce more activity, and a streak turns "don't break the chain" into the goal. For the studio, "more" would mean more cards opened and more guesses typed, which is exactly the volume the hypothesis wants to avoid.

### 4. What the gamification reviews find

- **Hamari, Koivisto and Sarsa (2014)** reviewed 24 empirical studies from 2008 to 2013. Gamification "provides positive effects", but the effects depend heavily on the context and on the users.
- **Sailer and Homner (2020)** meta-analysed 38 publications reporting 40 experiments on gamified learning. Pooled effects were small and positive: cognitive learning g = .49 (k = 19, N = 1,686), motivation g = .36 (k = 16, N = 2,246), behaviour g = .25 (k = 9, N = 951). Only the cognitive effect held in the subset of rigorous studies. For motivation, that subset showed g = .22, not significant (p = .20, k = 7, N = 1,063); the authors call the motivational and behavioural effects "not robust". For behaviour, and possibly for motivation, combining competition with collaboration worked better than competition alone. Motivation effects were not significant for interventions of one day or less, and medium for those of one to six months; few studies sat in each group.
- **Sailer, Hense, Mayr and Mandl (2017)**, 419 people in an order-picking simulation: one condition combining badges, a leaderboard and a performance graph raised felt competence and task meaningfulness. Avatars, a story and teammates raised felt relatedness. No element raised felt freedom of choice. They read this as support for their hypothesis "that gamification is not effective per se, but that specific game design elements have specific psychological effects". Because the three competence elements were tested together, the study cannot say which one did the work. Mekler et al. found no competence effect for similar elements, so the two studies disagree.
- **Hanus and Fox (2015)**: two sections of one university course, followed at four points over a 16-week semester; one section had a leaderboard and badges. Students in the gamified section ended with lower intrinsic motivation, satisfaction and empowerment. Their final exam scores were lower, and the drop in motivation statistically accounted for the gap. These findings come from summaries of the abstract, not the paper. A corrigendum was published in 2018; I could not read what it changed.

Lesson: gamification can help people learn, but its effect on *motivation* is weak and unreliable. What matters is which elements, for whom and how they are used. Competitive elements had a sustained negative case in a real course.

### 5. Seeing your own progress

- **Amabile and Kramer** collected about 12,000 daily diary entries from 238 knowledge workers in seven companies (*The Progress Principle*, 2011; summary in Harvard Business Review, May 2011). Of everything that marked people's best days at work, "by far the most prominent was making progress" in that work. Setbacks had an effect two to three times stronger than progress.
- **Sailer et al. (2017)** contrast a performance graph with a leaderboard. The graph does not compare players with each other; it shows "the player's own performance over time", on "an individual reference standard". Citing motivation theory, they argue this "fosters mastery orientation", a focus on getting better.

Lesson: the progress that motivates is progress in the work itself, and people feel setbacks more than gains. This is diary data, not an experiment, and it comes from teams at work, not from learning.

## What this could mean for the studio (to test, not decided)

All of this is **inference**.

1. **The reveal is the reward.** After Daniel guesses, the card shows what the agent actually did. That moment gives informational feedback about the task. Attach no points to guessing or reading. This matches the guess-and-card loop of v0 (#315) and the [learning-that-sticks report](https://github.com/danielrispler/Falinks/blob/87487b968385073ec17c928091f5d2cb881ad14a/docs/studio/research/learning-that-sticks.md) (#272).
2. **Count nothing that rises with activity.** No cards-read counter, no XP and no levels earned by volume. Mekler and Cerasoli show these raise quantity, not quality.
3. **No streaks.** If any continuity is shown, a missed day costs nothing and can be repaired. Silverman and Barasch show a broken streak lowers what comes next.
4. **Calibration as information, not a target.** Show prediction accuracy at the weekly debrief (#280), framed against Daniel's own past: "in the engine's regrouping code your guesses matched 7 of 10 this week, 4 of 10 a month ago". No live score to push up.
5. **No visible chess-style rating.** A rating that drops after a wrong guess is a loss, and losing lowers felt competence (Reeve and Deci). Elo can still work out of sight: Pelánek (2016) uses it in adaptive learning systems to estimate a learner's skill and each question's difficulty. The studio could use it to pick guess prompts at a fitting difficulty, which is SDT's "optimal challenge".
6. **Personal progression is a map of understanding.** For example, the debrief shows which parts of the project Daniel can now predict, compared with his own past. Nothing to unlock, no ranks and no comparison with anyone else. Since setbacks weigh more than gains, show a dip as plain information, never as a lost level.
7. **Informational wording.** Write "your guess matched" or "the agent did X instead", not "you should review this" or "keep it up". Keep every game part optional; pressure undermines the sense of choice.
8. **Watch for the swap.** If guessing rises but guesses get shorter or vaguer, something has turned into a counter. Remove it.

## Mapping to the hypotheses

| Hypothesis | Verdict | Why |
| --- | --- | --- |
| A game that rewards understanding, not volume | Supported, narrowed | The reward should be feedback on a guess. Common game elements reward volume (Mekler; Cerasoli). A visible chess-style rating is weakened; a hidden one that picks difficulty is untested. |
| Serious control-room style: "subtle layer of personal progression" | Supported only in one form | Progress against Daniel's own past, in meaningful work, at the debrief (Amabile and Kramer; Sailer 2017). XP, levels, badges and streaks are weakened (Hanus and Fox; Silverman and Barasch). |
| Guessing, not testing | Supported | No score keeps the guess informational, not controlling (Deci et al.; Ryan et al.). |
| Non-blocking | Indirectly supported | Pressure and lack of choice lower intrinsic motivation (Reeve and Deci). |

## Limits

- The reward experiments use children drawing, students solving puzzles, and short lab sessions. Daniel is an adult working on his own project over months.
- How much rewards hurt is still disputed (Deci et al. vs Cameron et al.). The two camps agree on the case that matters here, not on how common harm is.
- Gamification studies vary widely in design and quality; motivation effects did not survive a filter for rigour.
- Most positive gamification results involve other people (teams, competition with collaboration). Daniel uses the studio alone, so the best-performing setting in Sailer and Homner does not apply.
- No study covers developers, agent supervision or comprehension of code changes.

## Sources checked

Fetched and checked against the source on 2026-10-27. Every DOI below was resolved through Crossref, and its title, authors, journal, volume and pages matched.

- Lepper MR, Greene D, Nisbett RE, "Undermining children's intrinsic interest with extrinsic reward: A test of the 'overjustification' hypothesis", *Journal of Personality and Social Psychology* 28(1): 129–137, 1973. https://doi.org/10.1037/h0035519. Checked from the abstract (OpenAlex record): the three conditions, and less later interest in the expected-award group than in the other two.
- Deci EL, Koestner R, Ryan RM, "A meta-analytic review of experiments examining the effects of extrinsic rewards on intrinsic motivation", *Psychological Bulletin* 125(6): 627–668, 1999. https://doi.org/10.1037/0033-2909.125.6.627. Checked from the PubMed abstract: 128 studies; d = −0.40, −0.36, −0.28; self-report d = −0.15, −0.17; positive feedback d = +0.33 and +0.31; children vs college students.
- Cameron J, Pierce WD, "Reinforcement, reward, and intrinsic motivation: A meta-analysis", *Review of Educational Research* 64(3): 363–423, 1994. https://doi.org/10.3102/00346543064003363. Checked from the abstract: 96 studies; "overall, reward does not decrease intrinsic motivation"; expected tangible rewards for doing a task as the only negative effect.
- Cameron J, Banko KM, Pierce WD, "Pervasive negative effects of rewards on intrinsic motivation: The myth continues", *The Behavior Analyst* 24(1): 1–44, 2001. https://doi.org/10.1007/BF03392017. Checked from the abstract: low-interest vs high-interest tasks, and the conditions for harm.
- Ryan RM, Deci EL, "Self-determination theory and the facilitation of intrinsic motivation, social development, and well-being", *American Psychologist* 55(1): 68–78, 2000. https://doi.org/10.1037/0003-066X.55.1.68. Checked from the abstract: the three needs and the two quoted phrases. The author-hosted PDF (https://selfdeterminationtheory.org/SDT/documents/2000_RyanDeci_SDT.pdf) loads and has 11 pages, but its text could not be extracted, so nothing else is quoted from it.
- Ryan RM, Rigby CS, Przybylski A, "The motivational pull of video games: A self-determination theory approach", *Motivation and Emotion* 30(4), 2006. https://doi.org/10.1007/s11031-006-9051-8. Free copy: https://www.rochester.edu/warner/lida/wp-content/uploads/2022/11/02bfe513dd59366750000000.pdf. Read the abstract and theory section; both quotes are from the theory section. The PDF prints pages 347–363; Crossref lists 344–360.
- Reeve J, Deci EL, "Elements of the competitive situation that affect intrinsic motivation", *Personality and Social Psychology Bulletin* 22(1): 24–33, 1996. https://doi.org/10.1177/0146167296221003. Checked from the abstract: winning raised intrinsic motivation through competence; pressure lowered it through self-determination.
- Mekler ED, Brühlmann F, Tuch AN, Opwis K, "Towards understanding the effects of individual gamification elements on intrinsic motivation and performance", *Computers in Human Behavior* 71: 525–534, 2017. https://doi.org/10.1016/j.chb.2015.08.048. Free copy: https://glassmanlab.seas.harvard.edu/annotated_works/mekler17.pdf. Read the full text: N = 273; 4 × 2 design; game elements raised tag count (F(3, 265) = 10.09, p < .001, partial η² = .10); no effect on quality, competence or intrinsic motivation; quantity correlated negatively with quality; the quoted conclusion.
- Cerasoli CP, Nicklin JM, Ford MT, "Intrinsic motivation and extrinsic incentives jointly predict performance: A 40-year meta-analysis", *Psychological Bulletin* 140(4): 980–1008, 2014. https://doi.org/10.1037/a0035661. Checked from the PubMed abstract: k = 183, N = 212,468, ρ = .21 to .45, quality vs quantity.
- Silverman J, Barasch A, "On or off track: How (broken) streaks affect consumer decisions", *Journal of Consumer Research* 49(6): 1095–1117, 2023. https://doi.org/10.1093/jcr/ucac029. Checked from the University of Delaware repository record (https://udspace.udel.edu/items/42ce576b-8e1f-429a-8541-e29e48dbcfbb/full): citation, seven studies, the abstract's opening.
- Hamari J, Koivisto J, Sarsa H, "Does gamification work? A literature review of empirical studies on gamification", HICSS 2014, pp. 3025–3034. https://doi.org/10.1109/HICSS.2014.377. Checked the abstract (OpenAlex record); the count of 24 studies from 2008 to 2013 comes from Sailer and Homner's text.
- Sailer M, Homner L, "The gamification of learning: a meta-analysis", *Educational Psychology Review* 32: 77–112, 2020. https://doi.org/10.1007/s10648-019-09498-w. Open-access copy: https://opus.bibliothek.uni-augsburg.de/opus4/files/109056/109056.pdf. Read the full text: the three pooled effects; 38 publications with 40 experiments; the rigorous-subset motivation result (g = .22, p = .20, k = 7, N = 1,063); "not robust"; competition with collaboration; the duration result.
- Sailer M, Hense JU, Mayr SK, Mandl H, "How gamification motivates: An experimental study of the effects of specific game design elements on psychological need satisfaction", *Computers in Human Behavior* 69: 371–380, 2017. https://doi.org/10.1016/j.chb.2016.12.033. Open-access copy: https://opus.bibliothek.uni-augsburg.de/opus4/files/109059/109059.pdf. Read the full text: N = 419 of 699 who logged in; condition 1 combined badges, leaderboard and performance graph; the quoted conclusion; the individual reference standard.
- Hanus MD, Fox J, "Assessing the effects of gamification in the classroom: A longitudinal study on intrinsic motivation, social comparison, satisfaction, effort, and academic performance", *Computers & Education* 80: 152–161, 2015. https://doi.org/10.1016/j.compedu.2014.08.019. Checked: citation, and abstract fragments (two courses, four points in a 16-week semester, leaderboard and badges) quoted on a third-party page. The 2018 corrigendum (https://doi.org/10.1016/j.compedu.2018.09.019) exists per Crossref; its content was not read.
- Pelánek R, "Applications of the Elo rating system in adaptive educational systems", *Computers & Education* 98: 169–179, 2016. https://doi.org/10.1016/j.compedu.2016.03.017. Checked from the abstract on the Masaryk University record (https://www.muni.cz/en/research/publications/1376317): an answer treated as a match between student and item, estimating skill and difficulty.
- Amabile TM, Kramer SJ, "The power of small wins", *Harvard Business Review*, May 2011. https://hbr.org/2011/05/the-power-of-small-wins. Checked: authors and date only. The figures (about 12,000 diaries, 238 employees, seven companies, "by far the most prominent was making progress", setbacks two to three times stronger) come from Harvard Business School's interview page, Nobel C, "How small wins unleash creativity", 2011, https://www.library.hbs.edu/working-knowledge/how-small-wins-unleash-creativity, not from the article or the book.

Rests on search summaries or memory, not a fetched primary source:

- Lepper et al. (1973): "about half as much free time drawing" (about 8.6% vs about 17–18%) and the sample size come from secondary summaries that disagree on details. I did not read the paper.
- Hanus and Fox (2015): the findings (lower motivation, satisfaction and empowerment; motivation explaining the exam gap) come from search summaries of the abstract, not the paper.
- Goodhart (1975), "Problems of monetary management: the U.K. experience": the quoted sentence comes from search summaries of Central Banking's account, not the paper.
