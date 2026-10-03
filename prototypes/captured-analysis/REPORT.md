# Captured Rust/Go analysis probe

Decision evidence for [Probe captured Rust/Go analysis and owning-symbol correspondence](https://github.com/danielrispler/Falinks/issues/18), 2026-10-03. Human confirmed the bounded probe and scratch downloads; **findings await human review**. Throwaway branch: `prototype/captured-rust-go-analysis-20261003`. No production integration or real-agent trial.

**Provisional answer:** fresh, isolated analysis plus client-side capture tags preserves correspondence for these functions and exposes the known dependency. Neither interface supplies the tested results with a native whole-project revision identity or an exhaustive semantic graph. Keep independent-symbol application disabled and retain broader coordination.

## Exact executed bundle and inputs

| Component | Executed version |
| --- | --- |
| Host / target | macOS 26.6.2, arm64; Rust `aarch64-apple-darwin`, Go `darwin/arm64` |
| Python | 3.9.6 |
| rust-analyzer | `0.3.3065-standalone (03fcb77246 2026-09-27)`, release asset `2026-09-28` |
| Rust / Cargo | rustc `1.99.0 (b940084d7 2026-09-28)`, LLVM 23.1.1; Cargo `1.99.0 (5f94df478 2026-08-27)` |
| Rustup proxy | `1.29.1 (d95a37b6a 2026-08-13)`; scratch Rust 1.99.0 plus rust-src |
| Go / public APIs | local `go1.26.2`; `go/types` from that toolchain; `golang.org/x/tools v0.51.0` |
| Go loader dependencies | `x/mod v0.41.0`, `x/sync v0.23.0`; full selected module graph and sums retained |

[evidence.json](evidence.json) records executable/driver SHA-256 values, module identities, effective Go environment, requested LSP capabilities/options, positions, native replies and exact fixture bytes/file hashes. The compressed analyzer asset's verified SHA-256 is `54ec873d8996e2c127d758bf45d4eacb6d3371dae4f6f6d5d3f05cedbae5fd59`. Ambient Go configuration is disabled (`GOENV=off`, `GOTOOLCHAIN=local`, `GOWORK=off`, `GOPACKAGESDRIVER=off`); the earlier ambient `go version` selected 1.27.1 and is not this executed bundle.

Rust has `left` and `right` in `src/lib.rs`; `right` calls `dep::callee` in `src/dep.rs`. Two cfg-gated callee declarations select return value 7 or 9. Edition 2024; feature set empty or `alternate`; all-targets/test cfg, build scripts, proc macros and check-on-save disabled. Go has `Left`/`Right` in `siblings.go`; `Right` calls `Callee` in `callee.go` or tag-selected `callee_alt.go`; tests/cgo disabled, empty tags or `alternate`. No external fixture dependencies, generators or overlays.

Each capture gets a separate rust-analyzer process or Go load; Rust documents open at version 1. Source files are read-only. The primary scratch Cargo.lock is writable, but its bytes and complete fixture membership are checked unchanged after analysis/checks. Rust compilation uses a separate target directory per capture. Capture IDs hash canonical JSON of exact sources, explicit configuration and executed version/driver records. These are probe labels, not a selected production identity scheme. Full SHA-256 values are in the evidence; abbreviated below.

| Capture | Rust ID prefix | Go ID prefix | Input change |
| --- | --- | --- | --- |
| Baseline | `3a8e60a3050b` | `9efe8cb18b12` | Recorded original fixture |
| Shifted | `9088375c126c` | `5ea82c254311` | Expand first function with a comment/newlines |
| Unfinished | `6a6cf3e8b496` | `c5b83896518c` | Rust `1 + }`; Go `return Undefined()` |
| Configuration | `44c2b5a5f9f4` | `6d15cbcd12d9` | Same source bytes; enable feature/build tag `alternate` |

## Observed results

| Question | Rust | Go |
| --- | --- | --- |
| Capture association | Request IDs are mapped to the connection's capture by this client. Native locations have URI/range, no capture hash. Negotiated UTF-16, incremental sync, document symbols, definitions, references and call hierarchy. | The helper labels each `packages.Load` invocation; the returned package/type objects have no source revision. `LoadAllSyntax|NeedModule`, `./...`, no cross-load object-pointer reuse. |
| Owning-symbol correspondence | A definition/reference occurrence maps to the enclosing function's document-symbol range. Fixture key `captured_probe::right` persists; its range shifts from `(2,0)..(2,39)` to `(5,0)..(5,39)` in zero-based LSP coordinates. | AST enclosure plus package-qualified `objectpath` preserves `captured.test/probe.Right`; byte range `[43,79)` becomes `[61,97)`, line 3 becomes 6. |
| Forward / reverse evidence | Definition and outgoing call hierarchy show `right → dep::callee`. References to the callee map back to owner `right`. | `TypesInfo.Uses` resolves `Right → Callee`; inverting these observed uses gives `Callee ← Right`. |
| Unfinished edit | The same known edge remains available, alongside native `syntax-error: expected expression`; fresh `cargo check` exits 101. Server health still says `ok`, illustrating that health is not semantic validity. | `Load` returns no top-level error, yet package Errors reports `undefined: Undefined` and owners are `IllTyped`. The known edge remains; the invalid call has no resolved function-use edge. |
| Configuration change | Healthy quiescent analysis selects callee at `(3,7)..(3,13)` rather than `(1,7)..(1,13)`. Same fixture key, different active declaration/capture. | CompiledGoFiles switches `callee.go` to `callee_alt.go`. The package/objectpath key stays the same while file/range and capture differ. |
| Delayed / canceled result | After advancing the simulated consumer to the shifted capture, `$/cancelRequest` returned error -32800. A delayed successful old response was rejected against the configuration capture. | Context cancellation after the `loading` handshake returned `context canceled`. A successful baseline load delivered to the configuration consumer was rejected. |

The stale-result checks exercise **client-side result intake**, with immutable old inputs and a changed consumer capture. They do not test in-place LSP reload/didChangeConfiguration convergence or a concurrently changing disk workspace. Cancellation timing may differ on another run; even a successful late result must be discarded. No old range is used to apply an edit; there is no write engine here.

## Negative control and fallback

The same command with `--readonly-lock` reproduces a concrete degraded-analysis case. In [readonly-lock-evidence.json](readonly-lock-evidence.json), Cargo cannot update the copied lockfile in rust-analyzer's temporary metadata workspace. The analyzer reports `warning` even after `quiescent=true`, still returns owners/the known edge, and incorrectly selects the default callee for the requested alternate feature. This is an observed invocation failure, not a universal analyzer limitation. A writable **scratch** lockfile removes it without changing captured bytes. The documented [server-status extension](https://rust-analyzer.github.io/book/contributing/lsp-extensions.html#server-status) is a status hint, not a project revision or completeness certificate; [configuration options](https://rust-analyzer.github.io/book/configuration.html) define the requested feature settings.

| Condition | Conservative next step |
| --- | --- |
| Stale/canceled result or source/configuration mismatch | Discard for the current capture; recapture/reanalyze and derive new ranges. Never transfer old offsets as write authority. |
| Ambiguous enclosing owner | Widen to file. These probes cover top-level functions only; Rust's names/module paths are fixture correspondence, not global stable IDs. |
| Parse/type errors, degraded metadata, unresolved or missing relationships | Keep crate/package scope plus affected reverse dependents; remaining references do not establish completeness or independence. |
| Changed feature/tag, membership or resolution | Analyze the new configuration/capture; same symbol key or unchanged caller bytes do not make old evidence reusable. |
| Unknown external inputs or unbounded reverse analysis universe | Disable narrow acceptance pending recapture/a bounded input universe. Package-local widening alone is insufficient. |

The fixtures demonstrate direct edges only. No exhaustive type/constant/field/trait/interface/initialization graph, transitive reverse closure, rename/move identity, Unicode/nested-owner coverage, macro/generated provenance or configuration union was established. Installed sysroots, dynamic libraries and every external tool read were not snapshotted/traced. Pre/post hashes are not proof of atomic live capture or an enforced writer boundary. Sibling correspondence does not demonstrate preservation/application of both sibling edits. Native request IDs, status, objectpath and toy compilation cannot close those gaps.

## Reproduce and stop

From this directory, with the retained scratch tools/cache:

```sh
python3 probe.py --tools /private/tmp/falinks-analysis-tools-20261003
python3 probe.py --tools /private/tmp/falinks-analysis-tools-20261003 --readonly-lock
```

The first command is the single check; the second selects its negative-control mode. Both passed on the recorded bundle, verifying unchanged fixture bytes/membership, shifted correspondence, known forward/reverse evidence, broken-source outcomes, configured selection (or detected control failure), and rejection of old/canceled capture evidence. Runs overwrite their respective JSON artifacts with exact inputs/results.

For a fresh scratch tool directory: install Rust 1.99.0 plus rust-src through Rustup 1.29.1 with scratch `RUSTUP_HOME`/`CARGO_HOME` and `--no-modify-path`; place the release `2026-09-28` aarch64-apple-darwin analyzer at `<tools>/rust-analyzer`; use local Go 1.26.2. Populate modules from this directory using `GOMODCACHE=<tools>/go-mod GOCACHE=<tools>/go-cache GOENV=off GOTOOLCHAIN=local GOWORK=off go mod download all`. The check then runs with network access disabled. Different host/tools/settings produce new capture IDs; the recorded result is not a compatibility claim for them.

Only this prototype ticket may be resolved after human review. [The coordination decision](https://github.com/danielrispler/Falinks/issues/11#issuecomment-5954282492) remains authoritative; production interface selection, remaining capture/coordination guarantees and [notification handling and revalidation](https://github.com/danielrispler/Falinks/issues/17) stay open. The latest map must be fetched before adding a confirmed decision pointer; parallel files and tracker edits must be preserved.
