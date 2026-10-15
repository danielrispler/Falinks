# Coding standards

This file holds judgement calls for authors and reviewers. Mechanical rules belong in tests, not here. For example, `tests/language_policy.rs` enforces the language policy and `tests/check_drift.rs` keeps the local check list in step with CI.

Add a rule when a review finding is a judgement call that will come up again. Keep each rule short and give the reason.

## Docs and tests agree

- A scope rule stated in docs must match what the tests or tools enforce. If a doc says "only X is allowed", a test must reject the rest, or the doc must say the rule is not enforced.
- A doc that describes a tool's behaviour must match the tool. When you change the tool, change the doc in the same change.
- Do not claim more than you checked. "CI runs these commands" must be true word for word.

## One source for configuration

- Keep toolchain and dependency settings in one place. The toolchain lives in `rust-toolchain.toml`; CI setup and docs should read it rather than repeat the version. Repeat it only where a tool cannot read the file, and say why next to the copy.
- When two lists must agree, generate one from the other or add a drift test. Do not keep two hand-edited copies.

## Domain names

- Name code, tests and issues with the terms in `CONTEXT.md`. Prefer a domain name over a generic one such as `data`, `item` or `manager`.
- If a needed term is missing, note the gap for domain modelling instead of inventing a local synonym.

## Locks

- Hold no `RwLock` guard while calling host code (observers, callbacks); only the operation's own mutexes, such as the writer, stay held. A host may read the engine on the same thread, and re-locking a std `RwLock` there may panic or deadlock. `host_observers_run_with_every_rwlock_released` checks the existing observers; new callbacks need the same care.
- Take the writer `Mutex` before the layout `RwLock`. Drop a layout read guard before `layout.write()`.

## Frozen and pinned material

- Do not edit hash-frozen evaluation material as a side effect of another change. Plan a versioned re-freeze, as described in `evaluation/README.md`.

## Platform-bound tests

- A test that needs the evaluation sandbox or `/private/tmp` carries `#[cfg_attr(not(target_os = "macos"), ignore = "the evaluation sandbox is macOS-only")]`. Linux CI runs the root suite, so an ungated test fails there (#62). Reviewers check this; no test enforces it.
