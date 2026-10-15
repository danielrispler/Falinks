# 4. Go analyses share a per-engine cache; checks keep a fresh one

Status: accepted

## Context

Every analysis used to start from an empty `GOCACHE` and a cold gopls cache in its disposable copy. Each Go analysis recompiled the standard library for `go vet` and re-indexed from scratch in gopls. This was the long pole of the engine suite (#82). With one shared cache, the six analyses of the arm test `milestones_checks_publication_and_readiness_decide_the_outcome` took about 10s in total instead of 17–35s ([measurement](https://github.com/danielrispler/Falinks/issues/82#issuecomment-6190864269)).

## Decision

Each engine keeps one Go build cache and one gopls cache per toolchain identity, at `<state>/analysis-cache/<toolchain key>/go-build` and `.../gopls`. Go analyses point `GOCACHE` and `GOPLSCACHE` at them. Everything else in an analysis (source copy, `HOME`, `TMPDIR`, `GOPATH`) stays in its disposable copy.

- Checks and oracles keep a fresh output directory per check, with their own empty `GOCACHE`.
- Rust analysis stays fresh, because `cargo check` runs `build.rs`, which is candidate code.

Sharing across analyses is safe because Go analysis runs no candidate code. Cgo is off and `GOPROXY=off`. `go list`, `go vet` and gopls only compile and type-check. Go keys cache entries by content hashes of their inputs, and both caches are safe for concurrent use. Go and gopls trim their own caches, so the directory needs no engine cleanup.

The split keeps one risk closed. During a check, a candidate's `_test.go` runs arbitrary code. If that code could write to a cache that an oracle later reads, it could plant an entry that turns a failing oracle into a false pass. The analysis cache lies under the state root, which containment denies to every check for reads and writes, under Seatbelt and Landlock alike.

## Rejected options

- **Keep fresh caches.** Safe, but the slowest option.
- **Also seed checks and oracles from a std-only cache.** The bigger win, but it touches check integrity and the fairness between evaluation arms, and it requires an evaluation re-freeze.
