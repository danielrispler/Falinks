# Trap fit for spec try 3 (`libloans`)

Fitted by the frozen trap file's rules (`step-a-trap.md`, sha256 `79ccb281…a659dfb`) to `spec.json` (sha256 `a54911bf…46e2e3d1`), without running any agent. **Approved by Daniel on #587 (2026-11-10), as written, with all three C2 tags (T6, T7, T9) kept.** The fixture, the references, the oracle and the tags follow #509's decisions for try 1 and #552's for try 2. The fixture keeps the spec's name, `libloans`; try 1's fixture is `library-loans`, and the two names do not collide.

## Choosing the function

Developer A owns T1, T3, T5 and T8. Developer B owns T2, T4, T6, T7 and T9. (The allocation's text before its first `:` names them, in the order T3, T1, T5, T8 and T2, T4, T7, T6, T9; `qualify.rs` reads them the same way.)

These are the exported functions that one developer's requirements define and the other's use, from the spec's dependency list:

| Function | Defined by | Used by the other developer's requirements | Count | Layer |
|---|---|---|---|---|
| `calendar.DaysBetween` | T3 (A) | T6, T7, T9 (B) | **3** | bottom (`calendar`) |
| `calendar.Date.AddDays` | T3 (A) | T4 (B) | 1 | bottom (`calendar`) |
| `calendar.Date.Weekday` | T3 (A) | T4 (B) | 1 | bottom (`calendar`) |
| `calendar.Parse` | T3 (A) | T7 (B) | 1 | bottom (`calendar`) |
| `calendar.Date.String` | T3 (A) | T9 (B) | 1 | bottom (`calendar`) |
| `money.Cents.String` | T1 (A) | T9 (B) | 1 | bottom (`money`) |
| `lending.Fine` | T5 (A) | T9 (B) | 1 | middle (`lending`) |
| `isbn.ISBN.String` | T2 (B) | T8 (A) | 1 | bottom (`isbn`) |

`calendar.Date`, `lending.ItemKind`, `lending.Loan` and `money.Cents` are types, not functions. `T5 uses T3's DaysBetween`, `T8 uses T3's DaysBetween` and the other uses of A's functions by T5 and T8, and of B's functions by T4, T6, T7 and T9, stay within one developer.

- **Rule 1 (most uses):** `DaysBetween` is used by three of B's requirements, every other candidate by one. No tie-break applies.

**Pick:** `calendar.DaysBetween`, defined by T3 (A) and used by T6, T7 and T9 (B). It is also try 1's function (see "Ambiguities", 1).

## Choosing the change

- **1. Part versus remainder:** does not apply. `DaysBetween` returns a count of days, not a part of a quantity.
- **2. Inclusive versus exclusive bound:** **applies.** T3 counts the days from `from` to `to` without counting `from`: "(2026-11-01, 2026-11-10) -> 9, the reverse -> -9, the same day -> 0". The correction counts both ends, as try 1's did.

The change alters the result on every input, and so on inputs every dependent exercises: T6's due-date renewal, T9's statement dates and fines, and T5's and T8's days late.

## Wording (the development text, delivered to both agents in every arm)

> Correction to T3: `DaysBetween` must count both the start and the end day, not only the days after the start. 2026-11-01 to 2026-11-10 is 10, not 9. A date to itself is 1, not 0. When `to` is earlier, the result is the negative of the count from `to` to `from`, so 2026-11-10 to 2026-11-01 is -10, not -9.

It is try 1's text with T3 for T2 and T3's own example dates. It names only T3 and `DaysBetween`, and no caller. It is the fixture's one milestone, `days-between-correction`.

## Release trigger

The milestone's `release_on_call` is `{"package": "calendar", "function": "DaysBetween"}`. This is #509's qualifier-matched rule, unchanged:

- **Release:** the first accepted share whose diff adds `<qualifier>.DaysBetween(` in a non-test `.go` file outside `calendar/`. The qualifier is that file's import name for `example.com/libloans/calendar`: `calendar`, or the file's alias.
- **No match:** dot and blank imports, calls through a variable or a method value, `_test.go` files and files in `calendar/` itself.
- **Fallback:** if no such share has happened, the trap is released at the first agent's ready, which it refuses.
- **Agents in `git-1`:** the one agent's shares count.

A's own T5 and T8 also call `DaysBetween` from `lending/` and `report/`, so A's share can release the trap as well as B's.

## Bite

- **T6:** renewing on the due date is allowed: a book due 2026-12-01 renewed on 2026-12-01 gets 2026-12-22. A caller that keeps `DaysBetween(l.Due, today) > 0` as "today is after the due date" now gets 1 and returns `ErrOverdue`.
- **T9:** T9's own example has Ada's book due 2026-11-02 and the statement as of 2026-11-10: `OVERDUE $2.00`, 8 days. A caller that keeps reading `DaysBetween` as the days late gets 9 days and $2.25. Kept comparisons also move the boundaries: an item due on the day shows `OVERDUE`, an item returned on its due date shows as returned late, and an item returned on the day stays on loan.
- **T7:** the reference's check, `DaysBetween(borrowed, returned) < 0`, means the same under both meanings, so T7's reference does not change. A caller written as `DaysBetween(returned, borrowed) > 0` rejects a same-day return after the correction.
- **T5 and T8 (side only, A's own):** T5's example, a book due 2026-11-02 and returned 2026-11-06, owes $1.00; a kept caller gets 5 days and $1.25, and a return on the due date costs $0.25. T8's example goes from 8 days late and $2.00 to 9 days and $2.25.

The dependents' behaviour is absolute. T6 says "today is after l.Due" and "Renewing on the due date itself is allowed". T7 says "must not be earlier than the borrowed date". T9 states its conditions as date comparisons and its fines with rates, caps and a worked example ("due 2026-11-02 OVERDUE $2.00" as of 2026-11-10). None refers to `DaysBetween`'s meaning, so each stays correct after the correction. (T5 and T8 define days late as "the number of calendar days from X to Y", but their examples fix the count; see "Ambiguities", 2.)

## Tagging

The dependency list names five users of `DaysBetween`: T6, T7 and T9 (B's), and T5 and T8 (A's, who owns T3). As #509 decided, only the other developer's are tagged for C2; the owner's are side cases.

| Requirement | Developer | Tag | Tests |
|---|---|---|---|
| T6 | B | **C2** (`trap.cases`) | `TestRenewOverdue` |
| T7 | B | **C2** (`trap.cases`) | `TestParseLoanReturnOrder` |
| T9 | B | **C2** (`trap.cases`) | `TestStatement` |
| T5 | A | side (`trap.side_cases`) | `TestFineDays` |
| T8 | A | side (`trap.side_cases`) | `TestOverdueReport` |

- **Tests that depend on the meaning:** each dependent's cases on a boundary the meaning moves are in their own test, which is tagged, as in try 1. The dependent's other tests have no such case and stay untagged: `TestRenew` (the error order and renewals before the due date), `TestParseLoan` and `TestParseLoanErrors` (no same-day return), `TestStatementUnknownPatron`, `TestFineKind` and `TestOverdueEmpty`.
- **T7's tagged test does not fail under the swap.** It holds T7's two-date comparisons (a same-day return accepted, a return a day early rejected), as try 1's side-tagged `TestParseLoanReturnOrder` did. See "Ambiguities", 4.

## Static checks

The `falinks-eval static-checks` and `verify` commands were **not run**: both stop at the manifest's hash guard until the materials are frozen, and the local freeze that try 2 used for the guard alone was not run in this session. The same checks were run outside the harness, on the same trees and oracle with Go 1.26.2 (`go vet`, then `go test` with the oracle as `oracle_test.go` at the root):

| Check | How | Result |
|---|---|---|
| The frozen oracle rejects the initial tree and accepts the reference | `go test` on the initial tree and on the reference, with the oracle | pass: initial, all 24 tests fail; reference, all pass. The initial tree passes `go vet` and `go test ./...` without the oracle. |
| The reference swap, corrected direction: the full corrected reference passes every case | same | pass: no failure |
| The reference swap, old direction: the old reference with only `calendar/` corrected fails at least one tagged case | `go test` on the reference overlaid with `old_reference` except `calendar/calendar.go` | pass: C2 failures `TestRenewOverdue`, `TestStatement`; side failures `TestFineDays`, `TestOverdueReport`. No other test fails. |
| The reference calls the defining function wherever the dependency list says | `qualify::tests::try_3_owners_and_the_days_between_callers` (the harness's `trap_callers`) | pass: T5, T6, T7, T8 and T9 all `true`. The initial skeleton calls it nowhere. |
| A cross-agent dependency exists for the trap | the table above | pass: T6, T7 and T9 |
| The dependents are absolute enough (trap file, "Oracle") | judgment, see "Bite" | pass |
| The change alters a result the dependents exercise | the swap above | pass: T6's due-date renewal, T9's statement |
| The wording names only T3 and the function | read | pass |

Expected `static-checks` output, as the harness test `step_a_reference_swap_fails_tagged_cases_and_the_reference_passes` asserts: `cross_developer_dependents: ["T6", "T7", "T9"]`, `corrected_reference_failures: []`, `swap_c2_failures: ["TestRenewOverdue", "TestStatement"]`, `swap_side_failures: ["TestFineDays", "TestOverdueReport"]`, five `reference_calls`, all `true`.

Two further scratch checks: the old reference as a whole fails only `TestCalendarDaysBetween`; and the harness's reverse swap (`qualify::tests::the_try_3_reverse_swap_fits_any_date_fields`) puts the old body into the reference, whose Date has other fields, and fails the same C2 and side tests. The old body reads a Date only through `String`, so it fits an agent's own Date. The properties' two gates hold on the reference: no panic in 500 inputs each, and each of N1-N9 changes on its broken copy.

## Ambiguities and how they were resolved

1. **Closeness to try 1.** The spec is try 1's domain again: library loans with money, calendar dates, item kinds and due dates (21, 7 and 14 days, Sunday moves to Monday), late fines, renewals, loan lines and an overdue report. The rules pick the same function, `calendar.DaysBetween`, the same rule-2 change and almost the same wording. Agents in a run see none of try 1, so this decides nothing mechanically; Daniel may weigh whether try 3 is an independent try.
2. **A lure of try 2's shape, in text rather than in a name.** No dependent's output field or text is named `DaysBetween`. But T5 and T8 define days late in T3's own words: T3 says `DaysBetween` "returns the signed number of whole days from `from` to `to`"; T5 says "Days late is the number of calendar days from `due` to `returned`"; T8 says "Days late is the number of calendar days from Due to asOf"; and T9 charges "for the days from Due to Returned" and "from Due to asOf". An agent can read the correction as redefining those phrases too ("the days from Due to asOf now count both ends"), which is the misreading that try 2's shared name allowed (#553). Their examples settle it: T8's and T9's 2026-11-02 to 2026-11-10 is 8 days and $2.00, which the corrected `DaysBetween` gives as 9. Of these, T9 is tagged; T5 and T8 are side cases.
3. **The correction's dates touch the dependents' examples.** The correction uses T3's own example, 2026-11-01 to 2026-11-10. 2026-11-10 is also the as-of day of T8's and T9's examples, T7's borrowed date and T4's book example; the pair itself is not a dependent's. Try 2's correction reused T6's exact Monday; this one shares only the day.
4. **T7 tagged although the swap does not fail it.** The reference's `< 0` comparison holds under both meanings, so `TestParseLoanReturnOrder` passes under the swap. It is tagged because its result depends on the meaning for some callers (`DaysBetween(returned, borrowed) > 0` fails it after the correction), as try 1 side-tagged its own `TestParseLoanReturnOrder`. Try 2's record tagged only tests that fail under the swap. **Daniel's call:** untagging it leaves the static checks' result unchanged and makes C2 slightly harder to hold.
5. **T9 also reaches `DaysBetween` through A's `Fine`.** T9 uses T5's `Fine`, and T5 is A's. If A corrects `DaysBetween` but not `Fine`, `TestStatement` fails through A's own slip, and C2 counts it. In try 1, the fee function T9 used was B's own. **Daniel's call:** whether C2's classification (the report-only additions from #552) should note when a T9 failure traces to `Fine` only.
6. **Two rule-2 readings, as in try 1.** The count excludes its start, so either end could be flipped; the fit includes the start, as try 1 did, and keeps the sign rule symmetric. Rule 4 (sign) could also be argued, but rule 2 comes first.
7. **T6's second comparison.** The reference also uses `DaysBetween` for "if that date is not later than l.Due". Its result is the same under both meanings for every input: the meanings differ only when the new date equals `l.Due`, and then both branches return that date. No case can test it.
8. **ParseLoan's check order.** T7 lists its errors without an order. The oracle and property N7 avoid lines with more than one fault.
9. **T8's Due "as given".** T8 takes Due from the Loan, so Due may precede Borrowed. One T8 case has a loan borrowed on the day with an earlier due date; it is listed, as T8 says.
10. **Unspecified inputs left out of the properties:** dates outside years 1000-9999, the zero Date's text, non-ASCII letters and whitespace in `ParseKind`, ISBN and loan lines, and rows or lines that tie on every sort key (each generated loan has its own ISBN).
