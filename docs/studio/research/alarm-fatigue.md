# Alarm fatigue: how to alert without teaching people to ignore alerts

> **Answer:** People learn to ignore alerts in step with how often alerts are wrong or need no action. So the fix is fewer, truer alerts in a few strict priority levels, not louder ones. Only "an agent is blocked on me" earns an interruption.
> **Evidence:** Strong and consistent across hospital field data (Joint Commission 2013; Drew et al. 2014), aviation certification rules (FAA §25.1322) and lab studies (Bliss et al. 1995). None of it is about coding agents.
> **Hypotheses:** Supports "Non-blocking" (most events go to a quiet inbox). Reshapes the alert part of "Serious control-room style" (warning/caution/advisory levels, not a busy alarm panel). Directly addresses "I miss the moment an agent is waiting".
> **Read yourself:** Joint Commission Sentinel Event Alert 50 (3 pages, about 10 min); 14 CFR §25.1322 (1 page, about 5 min).

## The idea in plain words

An alarm is a promise: "this needs you now". Every alarm that breaks the promise teaches the listener to check less. People do not stop responding all at once. They respond about as often as the alarm has been right before. This is the "cry-wolf" effect.

So the danger for the studio is not one missed alert. It is a stream of alerts that are mostly "agent finished a step", "test passed", "FYI". After a week, Daniel would glance past the one that says "waiting for your approval".

## What the evidence says

### 1. Hospitals: most alarms need no action, and people tune them out

- The Joint Commission's Sentinel Event Alert 50 (April 8, 2013) says alarm signals per patient per day "can reach several hundred". It estimates that "between 85 and 99 percent of alarm signals do not require clinical intervention". Clinicians then turn alarms down, turn them off, or widen limits until they are unsafe.
- Its database held 98 alarm-related events from January 2009 to June 2012: 80 deaths, 13 permanent loss of function, 5 extra care or stay. Alarm fatigue is named "the most common contributing factor". "Alarm settings inappropriately turned off" was a factor in 36 events.
- Its recommendations target the alarm *system*, not the people: inventory alarms, set defaults per care area, identify "when alarm signals are not clinically necessary", tailor limits per patient, and keep reviewing the configuration.
- Drew et al. (PLOS ONE, 2014) logged every monitor alarm for 461 ICU adults over 31 days: 2,558,760 alarms, 187 audible alarms per bed per day. Of the annotated arrhythmia alarms, 88.8% were false positives. Their fixes: better detection, settings tailored to each patient, alarm only at the start and end of a lasting condition (atrial fibrillation), and configurable delays.

Lesson: the alarm count is the root problem. Tuning *what* fires beats training people to respond.

### 2. Aviation: few priority levels, strict meaning, false alerts designed out

The US airliner certification rule 14 CFR §25.1322, "Flightcrew alerting", sets three levels:

- **Warning:** "immediate flightcrew awareness and immediate flightcrew response". Red.
- **Caution:** "immediate flightcrew awareness and subsequent flightcrew response". Amber or yellow.
- **Advisory:** "flightcrew awareness and may require subsequent flightcrew response". Any colour except red or green.

The rule also requires the system to limit "false and nuisance alerts", including ones that are "inappropriate or unnecessary". Red, amber and yellow must be restricted for non-alert uses, so the colours keep their meaning. Suppressing an alert must not be easy to do by accident or habit, and a suppression must be announced to the crew.

Lesson: the level is defined by *what response is needed and when*, not by how important the event feels. Colour is reserved for alerts.

### 3. Lab studies: response rate matches alarm reliability

- Bliss, Gilson and Deaton (Ergonomics, 1995): 138 people did a demanding task while alarms fired that were true 25%, 50% or 75% of the time. About 90% matched their response rate to the alarm's true rate ("probability matching"). About 10% responded all-or-none.
- Dixon and Wickens (UAV simulations, 2006, as summarised in later papers): false alarms lower *compliance* (acting when the alarm fires); misses lower *reliance* (trusting silence). The two effects are fairly independent.
- Wickens et al. (Human Factors, 2009) tested for a cry-wolf effect in real en-route air-traffic conflict alerts. I could not read its findings; see "Sources checked".

Lesson: both error types matter for the studio. False "needs you" alerts teach Daniel to ignore them. A missed "needs you" teaches him he must keep watching, which defeats the point of the panel.

## What this could mean for the studio (to test, not decided)

1. **One interrupting level only:** "an agent is blocked on you" (approval, decision, question). This is the aviation *warning*: immediate awareness, immediate response. It must be close to 100% true.
2. **Caution goes to a visible but quiet place:** run stuck, check failed. Seen soon, acted on later. No sound.
3. **Everything else is advisory:** progress, finished steps, cards to read. Into the non-blocking inbox. Never pops up.
4. **Reserve red, amber and sound** for levels 1 and 2. A control-room look full of blinking telemetry would spend those signals on nothing.
5. **Alert on the start and end of a state, not on every tick** (Drew's atrial-fibrillation fix). "Waiting since 14:02", not a repeat every minute.
6. **Clear the alert when the condition ends,** so stale alerts don't pile up.
7. **Measure it:** log how many interruptions fired and how many Daniel acted on. A falling act-on rate is the early sign of fatigue.

## Limits

- All sources are medicine, aviation or lab tasks with high stakes and many alarms. A solo developer with a few agents has lower stakes and one listener. The direction should transfer; the numbers will not.
- I found no study of alert fatigue with coding-agent notifications.
- The cost of interrupting focus is a separate ticket, #270.

## Sources checked

Fetched and checked against the source:

- The Joint Commission, *Sentinel Event Alert* Issue 50, "Medical device alarm safety in hospitals", April 8, 2013. Read the full 3-page PDF from a mirror, https://www.masimo.fr/siteassets/us/documents/pdf/sea_50_alarms.pdf (the official page is https://jointcommission.org/sea_issue_50; its PDF returned 403). Checked: date, "several hundred" per patient per day, 85 to 99 percent, 98 events / 80 deaths / 13 / 5, the contributing-factor counts, alarm fatigue as most common factor, the 11 recommendations.
- Drew BJ et al., "Insights into the problem of alarm fatigue with physiologic monitor devices: a comprehensive observational study of consecutive intensive care unit patients", *PLoS ONE* 9(10): e110274, 2014. https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0110274. Checked from the abstract: 461 patients, 31 days, 2,558,760 alarms, 187 audible alarms per bed per day, 88.8% false positives among annotated arrhythmia alarms, the recommended fixes.
- 14 CFR §25.1322, Flightcrew alerting. https://www.law.cornell.edu/cfr/text/14/25.1322. Checked: the three level definitions (quoted), colours, false and nuisance alerts, suppression rules.
- Bliss JP, Gilson RD, Deaton JE, "Human probability matching behaviour in response to alarms of varying reliability", *Ergonomics* 38(11): 2300–2312, 1995. https://stars.library.ucf.edu/scopus1990/1980. Checked from the abstract record: 138 participants, 25/50/75% reliability, about 90% probability matching, about 10% all-or-none.
- Wickens CD, Rice S, Keller D et al., "False alerts in air traffic control conflict alerting system: Is there a 'cry wolf' effect?", *Human Factors* 51(4): 446–462, 2009, doi:10.1177/0018720809344720. https://portfolio.erau.edu/en/publications/false-alerts-in-air-traffic-control-conflict-alerting-system-is-t/. Checked: bibliographic details only. Its findings were behind a paywall; this report makes no claim about them.

Rests on search summaries or memory, not a fetched primary source:

- Dixon and Wickens (2006): false alarms lower compliance, misses lower reliance. From search-result snippets of secondary pages.
- "Clear the alert when the condition ends" as an FAA requirement, and FAA AC 25.1322-1 (December 13, 2010) asking warnings and cautions to use at least two senses. From a search summary of FAA pages, not my own reading.
