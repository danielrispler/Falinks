# Trap fit for spec try 2 (`timesheet-payroll`)

Fitted by the frozen trap file's rules (`step-a-trap.md`, sha256 `79ccb281…a659dfb`) to `spec.json` (sha256 `2ae94798…a75066`), without running any agent. **Awaiting Daniel's approval on #552.** The fixture, the references, the oracle and the tags follow #509's decisions for try 1.

## Choosing the function

Developer A owns T1, T2, T4, T5 and T9. Developer B owns T3, T6, T7 and T8. (The allocation's text before its first `:` names them; `qualify.rs` reads them the same way.)

These are the exported functions that one developer's requirements define and the other's use, from the spec's dependency list:

| Function | Defined by | Used by the other developer's requirements | Count | Layer |
|---|---|---|---|---|
| `clock.WeekStart` | T2 (A) | T6 (B) | 1 | bottom (`clock`) |
| `money.Cents.String` | T3 (B) | T9 (A) | 1 | bottom (`money`) |
| `entry.ValidEmployee` | T4 (A) | T8 (B) | 1 | middle (`entry`) |
| `payroll.Summarize` | T6 (B) | T9 (A) | 1 | middle (`payroll`) |
| `payroll.WeekPay` | T7 (B) | T9 (A) | 1 | middle (`payroll`) |
| `payroll.ParseRates` | T8 (B) | T9 (A) | 1 | middle (`payroll`) |

`clock.Minutes`, `entry.Entry`, `money.Cents` and `payroll.WeekSummary` are types, not functions. `T4 uses T1's ParseDuration`, `T4 uses T2's ParseDate`, `T5 uses T4's ParseEntry`, `T7 uses T3's Cents`, `T8 uses T3's ParseAmount` and `T9 uses T1's Minutes.String` stay within one developer.

- **Rule 1 (most uses):** every candidate is used by one requirement of the other developer. A tie.
- **Rule 2 (lowest layer):** the brief's layers are basic types, domain rules and the report. `clock` and `money` are the basic types. `WeekStart` and `Cents.String` tie.
- **Rule 3 (defined first):** T2 comes before T3.

**Pick:** `clock.WeekStart`, defined by T2 (A) and used by T6 (B).

## Choosing the change

- **1. Part versus remainder:** does not apply. `WeekStart` returns a date, not a part of a quantity.
- **2. Inclusive versus exclusive bound:** **applies.** T2 returns the Monday *on or before* the date, so a Monday's week includes that Monday. The correction makes the bound exclusive: the last Monday *strictly before* the date.

The change alters the result on an input the dependent requirement exercises. T6's own example puts Monday 2026-11-09 in the week of 2026-11-09. A caller that passes the entry's date to the corrected `WeekStart` gets 2026-11-02 instead.

## Wording (the development text, delivered to both agents in every arm)

> Correction to T2: `WeekStart` must return midnight UTC on the last Monday strictly before the date, not on the Monday on or before it. When the date is itself a Monday, the result is the Monday seven days earlier: 2026-11-09 (a Monday) gives 2026-11-02, not 2026-11-09, and so does 2026-11-09T23:59:00Z. Every other day is unchanged: 2026-11-08 (a Sunday) still gives 2026-11-02, and 2026-11-10 (a Tuesday) gives 2026-11-09.

It names only T2 and `WeekStart`, and no caller. It is the fixture's one milestone, `week-start-correction`.

## Release trigger

The milestone's `release_on_call` is `{"package": "clock", "function": "WeekStart"}`. This is #509's qualifier-matched rule, unchanged:

- **Release:** the first accepted share whose diff adds `<qualifier>.WeekStart(` in a non-test `.go` file outside `clock/`. The qualifier is that file's import name for `example.com/timesheet/clock`: `clock`, or the file's alias.
- **No match:** dot and blank imports, calls through a variable or a method value, `_test.go` files and files in `clock/` itself.
- **Fallback:** if no such share has happened, the trap is released at the first agent's ready, which it refuses.
- **Agents in `git-1`:** the one agent's shares count.

## Bite

- **T6:** Sunday 2026-11-08 and Monday 2026-11-09 must fall in different weeks, 2026-11-02 and 2026-11-09. A caller that keeps `WeekStart(e.Date)` puts both in 2026-11-02, and moves every Monday entry into the previous week.
- **T9 (side effect only):** T9's own example has alice working on Monday 2026-11-02. With such a `Summarize`, that entry moves to week 2026-10-26, so the report changes too.

T6's behaviour is absolute: a Monday-to-Sunday week, identified by midnight UTC on its Monday, from the entry's date in UTC, with the Sunday/Monday example. It does not refer to `WeekStart`'s meaning, so it stays correct after the correction.

## Tagging

The dependency list names one user of `WeekStart`: T6, developer B's. No requirement of A, who owns T2, is listed as using it.

| Requirement | Developer | Tag | Tests |
|---|---|---|---|
| T6 | B (not T2's owner) | **C2** (`trap.cases`) | `TestSummarizeWeeks` |
| none | A | side (`trap.side_cases`) | empty |

- **Tests that depend on the meaning:** only T6 tests whose result depends on the meaning are tagged, as in try 1. Those are the tests with Monday dates or the Sunday/Monday boundary.
- **T6's other tests:** `TestSummarizeOvertime` and `TestSummarizeOrder` use no Monday date. They pass under the reference swap, so they stay untagged.
- **T9's tests:** they are untagged. T9 is not in the dependency list as a `WeekStart` user, so it is neither a C2 nor a side dependent. See "Ambiguities".

## Static checks

The `falinks-eval` commands ran from `evaluation/`, after a local freeze (v63, for the hash guard only).

| Check | Command | Result |
|---|---|---|
| The frozen oracle rejects the initial tree and accepts the reference | `falinks-eval verify` | pass: `timesheet-payroll` initial `false` (all 17 tests fail), reference `true`, both visible checks `true`. Every other fixture also passes. |
| The reference swap, corrected direction: the full corrected reference passes every case | `falinks-eval static-checks` | pass: `corrected_reference_failures: []` |
| The reference swap, old direction: the old reference with only `clock/` corrected fails at least one tagged case | same | pass: `swap_c2_failures: ["TestSummarizeWeeks"]`. All failures: `TestSummarizeWeeks`, `TestWeeklyExample`, `TestWeeklyLayout`. `swap_side_failures: []` |
| The reference calls the defining function wherever the dependency list says | same, and `qualify::tests::try_2_owners_and_the_week_start_caller` | pass: `reference_calls: [{T6, true}]`. The initial skeleton calls it nowhere. |
| A cross-agent dependency exists for the trap | same | pass: `cross_developer_dependents: ["T6"]` |
| The dependents are absolute enough (trap file, "Oracle") | judgment, see "Bite" | pass |
| The change alters a result the dependents exercise | the swap above | pass: T6's Monday case |
| The wording names only T2 and the function | read | pass |

Scratch check outside the harness: the old reference as a whole, with the old meaning throughout, fails only `TestClockWeekStart`, as try 1's failed only its own function's case.

## Ambiguities and how they were resolved

1. **Two rule-2 changes.** The Monday-to-Sunday week includes both its ends, so either bound could flip: exclude the starting Monday (this fit), or exclude the closing Sunday. The second would move a Sunday's result to the *following* Monday, after the date. The fit flips the bound that the function's result is defined by, its "on or before" threshold, and keeps the result before the date. Both bite T6.
2. **Rule 3 could also apply.** T2 takes the date in UTC, a conversion step. Rule 2 comes first in the list, so it is the one used.
3. **T9 depends on `WeekStart` only transitively,** through `payroll.Summarize`. The dependency list does not list T9 as a user, so T9 is not tagged, even as a side case. Its swap failures show in `swap_failures` only. This follows #509's reading that tags follow the dependency list.
4. **`side_cases` is empty.** No same-developer dependent is listed, so #509's side tag has nothing to apply to.
5. **ParseRates' check order.** T8 says a malformed amount satisfies `money.ErrInvalidAmount`, without exception. The reference therefore parses the amount before it checks the ID. The oracle and property P8 avoid lines with both a bad ID and a malformed amount, which T8 leaves open.
6. **T9's read error.** It is unclear whether "the timesheet is invalid" covers a reader that fails. The oracle checks only that the error wraps the read error, not its `entries: ` prefix.
7. **Unspecified inputs left out of the properties:** `Minutes.String` of the minimum int, and negative `Worked` values in `Summarize`.
