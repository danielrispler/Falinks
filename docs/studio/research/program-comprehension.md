# Program comprehension and code review limits

Research report for [#273](https://github.com/danielrispler/Falinks/issues/273), part of the studio map [#266](https://github.com/danielrispler/Falinks/issues/266). Written 2026-10-26.

## Card

- **Answer:** Understanding someone else's change is the hard part of review, not finding bugs. People understand code by mixing a top-down guess ("this probably does X") with line-by-line reading, and they need context that review tools rarely show. Review quality drops as changes grow: big reviews get fewer useful comments. The often-quoted "200–400 lines, under 500 lines an hour, 60–90 minutes" limits come from one vendor study at Cisco and are rules of thumb, not laws.
- **Evidence:** Strong that understanding is the core difficulty and that bigger changes get worse review (large field studies at Microsoft and Google). Weak for exact numeric limits (one vendor study). Thin and early for reviewing agent-written code.
- **Supports:** "Small and visual before text" (small, focused pieces review better; *visual* is untested here). "Evidence, not promises" (reviewers need the why and the context, and much agent code now gets no human review at all). Partly supports "Move learning into waiting time" (comprehension takes most of a developer's time, so it has to happen somewhere).
- **Weakens:** Nothing directly. It warns that a tidy summary is no substitute for the reader building their own picture.
- **For Daniel:** Bacchelli and Bird, ["Expectations, Outcomes, and Challenges of Modern Code Review"](https://www.microsoft.com/en-us/research/publication/expectations-outcomes-and-challenges-of-modern-code-review/) (ICSE 2013), sections on outcomes and "understanding" (40 min). Then Google's [small changes guide](https://google.github.io/eng-practices/review/developer/small-cls.html) (10 min).

## Words used here

- **Program comprehension:** building a mental picture of what code does and why.
- **Mental model:** that picture, held in your head.
- **Top-down comprehension:** start from a guess about the purpose, then look for code that confirms it.
- **Bottom-up comprehension:** read statements, group them into chunks, and build up the purpose from the chunks.
- **Modern code review:** lightweight, tool-based review of each change (a pull request), as opposed to formal meeting-based inspection.
- **Change (CL, PR):** one reviewable unit of code change.

Labels: **Established** means a primary source reports it. **Inference** means it is my reading, not something a study tested.

## 1. How people understand code they didn't write

**Point: people switch between guessing the purpose and reading the details, and they build the picture themselves.**

Established:

- Von Mayrhauser and Vans's "integrated model" ([Computer, 1995](https://doi.org/10.1109/2.402076)) watched professional maintainers. They switched often between a top-down model (domain knowledge, hypotheses), a program model (control flow, bottom-up) and a situation model (what the code means in the real world). Which one they used depended on how well they knew the code and the domain.
- Storey's review ([Software Quality Journal, 2006](https://doi.org/10.1007/s11219-006-9216-4)) summarises the classic theories (Brooks, Soloway, Pennington, Letovsky, von Mayrhauser and Vans). Their common thread: an expert forms hypotheses and checks them; a novice or an unfamiliar reader falls back to slow line-by-line reading. Tools should support both and let the reader switch.
- Comprehension takes most of the working day. Xia et al. tracked 78 professionals over 3,148 working hours and found about 58% of their time went on comprehension ([TSE, 2018](https://doi.org/10.1109/TSE.2017.2734091)).
- Brain imaging: Siegmund et al. scanned 17 people reading short snippets and reported activity in language-related areas ([ICSE 2014](https://doi.org/10.1145/2568225.2568252)). A later study found code comprehension relies mainly on the "multiple demand" network used for logic and problem solving, not the language network ([Ivanova et al., eLife 2020, arXiv version](https://arxiv.org/abs/2304.12373)). The brain question is unsettled and does not change the design advice.

Inference: an explanation that hands over a finished summary skips the hypothesis-and-check loop that these models say builds understanding. A prompt like "what do you think this change does?" before the summary matches the models better.

## 2. Code review: what it is for and what it actually does

**Point: people expect review to find bugs; what it mostly delivers is shared understanding, and understanding the change is the main obstacle.**

Established:

- Bacchelli and Bird observed, interviewed and surveyed developers at Microsoft ([ICSE 2013](https://www.microsoft.com/en-us/research/publication/expectations-outcomes-and-challenges-of-modern-code-review/); [DOI](https://doi.org/10.1109/ICSE.2013.6606617)). Finding defects is the top motivation, but reviews find fewer defects than expected. They bring knowledge transfer, team awareness and alternative solutions instead. The authors name **understanding the code and the change** as the central challenge, and most strategies reviewers use for it are not supported by tools.
- Google's study ([Sadowski et al., ICSE SEIP 2018](https://research.google/pubs/modern-code-review-a-case-study-at-google/)) combined 12 interviews, a 44-person survey and logs of 9 million reviewed changes. It describes review at Google as built around small changes and fast turnaround, with education and code understanding among its goals. I could not extract the paper's size and latency tables, so this report quotes no figures from it.

## 3. When review stops working: size and pace

**Point: bigger changes get worse review. The exact limits are soft.**

Established:

- Bosu, Greiler and Bird classified review comments at Microsoft as useful or not ([MSR 2015](https://www.microsoft.com/en-us/research/publication/characteristics-of-useful-code-reviews-an-empirical-study-at-microsoft/)). The more files a change touches, the lower the share of useful comments. Reviewers' usefulness rises sharply in their first year, then levels off.
- Google's public review guide says "100 lines is usually a reasonable size" and "1000 lines is usually too large", that files count as well as lines, and that small changes are reviewed faster and more thoroughly ([Google eng-practices](https://google.github.io/eng-practices/review/developer/small-cls.html)). This is practice guidance, not a study.
- The well-known numbers come from SmartBear's study of a Cisco team ([SmartBear best practices](https://smartbear.com/learn/code-review/best-practices-for-peer-code-review/)): review 200–400 lines at a time, under 500 lines an hour, no more than 60–90 minutes in one sitting. The underlying data appears in SmartBear's book *Best Kept Secrets of Peer Code Review* (Cohen, 2006). It is a vendor study of one company with a lightweight review tool, never peer reviewed. The 60-minute limit cites general attention research, not the Cisco data.

Inference: the shape (quality falls with size and speed, and attention runs out within about an hour) is consistent across sources. The exact numbers are not something to build hard rules on.

## 4. Reviewing agent-written code

**Point: early evidence says agent changes often get little or no real human review.**

Established (preprints, not yet well replicated):

- Duma et al. studied GitHub pull requests in the AIDev dataset ([arXiv 2605.02273](https://arxiv.org/abs/2605.02273), accepted at EASE 2026). Most agent-written PRs got no review. Where review happened, other agents did most of it, and human involvement often looked like "steering" the agent rather than evaluating the code. Review counts may not mean human oversight.
- Temkulkiat et al. measured 591 agent PRs (5,027 Python files) from the same dataset ([arXiv 2604.00299](https://arxiv.org/abs/2604.00299)). Over 90% of constructs were basic-level, similar to human PRs. So line-level difficulty is not the problem; volume and missing context are more likely to be.

Inference: an agent can produce more change than a person can review well in one sitting. If review quality falls with size, a tool that shows evidence (what was checked, what passed, what was refused) and splits the change into small pieces fits the research. Evidence alone cannot replace comprehension; the companion report on comprehension debt (#274) found explanations raise trust without raising understanding.

## What this means for the hypotheses

- **Small and visual before text:** supported on "small". No source here tests visuals.
- **Evidence, not promises:** supported. Reviewers lack context and rationale; showing checks helps decide trust, but not understanding.
- **Move learning into waiting time:** partly supported. Comprehension is most of the work; nothing here tests doing it while waiting.
- **Guessing, not testing:** fits the hypothesis-and-check comprehension models, but no review study tests it.

## Sources checked

Checked against the publisher page, DOI record (Crossref) or official page: von Mayrhauser and Vans 1995; Storey 2006; Bacchelli and Bird 2013; Siegmund et al. 2014; Xia et al. 2018; Sadowski et al. 2018 (abstract only); Bosu et al. 2015 (abstract only); Ivanova et al. (arXiv abstract); Duma et al. and Temkulkiat et al. (arXiv abstracts); Google eng-practices; SmartBear page. Not checked directly: Cohen's 2006 book (Cisco raw data) and the full text of the Google paper. Figures I could not confirm, such as Google's median change size, were left out.
