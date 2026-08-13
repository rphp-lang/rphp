# Native dynamic-String hash-read checkpoint

Status: verified on ARM64 and x86-64, 2026-08-13

This report records the vertical execution checkpoint that admits a read-only
dynamic String-keyed `FetchArrayLong(ValueSlot)` into the existing common typed
IR and lowers its existing `HashLoad` operation on both native backends. The
result applies to structurally proven finite String inputs and immutable array
entries; it does not recognize benchmark names, variables, classes or key
contents.

## Contract and baseline

- **Outcome:** both changing-key controls execute as one native typed region on
  ARM64 and x86-64, preserve exact fallback behavior and improve credibly over
  the previous typed-only execution.
- **Baseline:** commit `bf5da553add6554d09fe91c09caedcd807bcfe1e`.
  Baseline binaries were built from a clean archive. The candidate was the
  complete dirty worktree diff on `codex/perf-native-string-hash-read`, built
  independently from the same archive plus that diff.
- **Root cause:** `native_mixed_kernel` previously required every dynamic
  String `FetchArrayLong` to have an immediately following arithmetic/store
  fusion. A standalone read therefore returned `None` even though the common
  `HashLoad` operation and both native lowerings already existed.
- **Scope:** retain one bounded finite literal proof in the typed loop plan,
  admit standalone `HashLoad`, and distinguish an immutable read context from
  the existing unique-COW mutable entry context.
- **Acceptance:** exact PHP/baseline/candidate output, transactional exit tests,
  one native entry and zero side exits for both controls on both architectures,
  target improvement, and no corpus/holdout median regression above one
  percent.
- **Stop rule:** reject native admission when the finite proof overflows, an
  entry is missing, non-Long or referenced, or the source does not satisfy the
  immutable read contract. Do not broaden the bound or weaken canonical
  fallback to increase coverage.

## Design and semantic envelope

`QuickLongOpsLoop` now retains the complete finite set of String literals
proved for assignments to a dynamic key. The proof includes immutable
preheader CV sources as well as direct literal assignments. It is bounded by
the existing four-entry String fetch-cache limit; a fifth distinct literal
sets an overflow bit and prevents native admission.

The target-neutral native builder consumes that proof for both standalone and
fused `FetchArrayLong(ValueSlot)`. The fused read/modify/write path keeps its
existing mutable `Entry` context. A standalone load uses a new
`ReadOnlyEntry`: runtime borrows the array immutably, applies canonical numeric
String-key normalization, verifies that every possible entry exists and is an
unreferenced Long, and only then supplies payload pointers to native code. The
native operation can load but cannot write through this context.

An invalid runtime String token exits at the exact `HashLoad` operation before
publishing its result or destination. Missing, non-Long and referenced finite
entries reject the context before native entry and execute through the
canonical path. Shared COW arrays remain admissible because the context is
read-only. The existing mutable hash update and taken cold-edge replay retain
their prior effect ordering.

## Coverage and correctness

| Capability | Baseline tests | Typed IR/executor | ARM64 JIT | x86-64 JIT | Exact fallback/exit | Corpus evidence |
| --- | --- | --- | --- | --- | --- | --- |
| Read-only dynamic-String `FetchArrayLong(ValueSlot)` | fresh | fresh | fresh: 1 entry, 0 exits | fresh: 1 entry, 0 exits | invalid token; missing/non-Long/reference; shared COW | isolated controls; five corpus/holdout controls |

Focused tests cover literal and immutable-CV sources, shared read-only arrays,
finite-proof overflow, invalid-token transactional exits, canonical
missing/non-Long/reference fallback, the pre-existing mutable hash update and
its cold-edge replay. The ARM64 and x86-64 execution tests consume the same
typed operation and assert the same runtime contract.

Validation completed with:

- `cargo fmt --check`;
- `scripts/test-unsafe-policy.sh` and `scripts/check-unsafe-policy.sh`:
  1,623 production unsafe blocks, 289 unsafe functions and 106 `SAFETY`
  annotations, against ceilings 1,623/289 and floor 58;
- `cargo check --locked --all-features --all-targets`;
- `cargo test --locked`;
- `cargo test --locked --features jit-prototype`;
- `cargo test --locked --all-features`;
- `cargo test --locked --no-default-features --lib` (239 tests);
- physical x86-64 `cargo test --locked --features jit-prototype --lib
  jit::x86_64` (74 tests) and `cargo test --locked --features jit-prototype
  --test jit_x86_64_prototype` (34 tests).

The complete `cargo test --locked --no-default-features` command still hits the
baseline configuration defect documented in `docs/execution-scorecard.md`:
`IntIndexValue::clear_cached_long` is compiled at a canonical mutation call
site while its method definition is excluded. The affected
`src/value/mod.rs` is unchanged from the exact baseline; fixing this unrelated
shared-runtime feature boundary is outside this checkpoint.

## Benchmark method

Both architectures used Rust 1.93.1, `max-perf` (fat LTO, one codegen unit),
`RUSTFLAGS=-C target-cpu=native`, `jit-prototype`, distinct task-scoped target
directories and one untimed output-validation run per executable/workload.
The measured value is each workload's internal `microtime(true)` interval, so
process startup and parsing are excluded while region admission and native
compilation remain included. Baseline and candidate processes alternated
order. Every valid sample was retained; no outliers were removed.

ARM64 measurements used an Apple M4 with 24 GiB memory, macOS 26.5.2, and PHP
8.5.9. After a 60-second post-build cooldown, the independent confirmation
used 60 pairs per target and per corpus/holdout control.

x86-64 measurements used an AMD Ryzen 9 7950X with 31,985,372 KiB memory,
Ubuntu 24.04, Linux 7.0.0-28, PHP 8.4.24, the `performance` governor and CPU 2
affinity. Builds and all measurements held the project benchmark lock. The
primary cycle used 30 pairs for every workload; independent confirmation used
60 target pairs and 60 corpus/holdout pairs. A second 60-pair corpus check
scaled only each final iteration argument by 10x to resolve the original
short-timer bimodality. Outputs were revalidated against `php -n` after
scaling.

These results cover only the named supported workloads under these
configurations; they are not a general RPHP-versus-PHP claim.

## Target results

Times are median milliseconds with interpolated p10-p90. Delta is the median
of paired candidate/baseline changes; speedup is the ratio of separate
medians.

| Architecture and workload | Baseline | Candidate | Speedup | Paired median delta |
| --- | ---: | ---: | ---: | ---: |
| ARM64 literal-selected key | 11.868 (11.668-12.298) | 1.064 (1.037-1.314) | 11.16x | -91.04% |
| ARM64 immutable-CV-selected key | 11.782 (11.577-12.129) | 1.060 (1.032-1.368) | 11.12x | -90.99% |
| ARM64 adjacent mutable update | 2.262 (2.205-2.409) | 2.254 (2.206-2.416) | 1.004x | +0.79% |
| x86-64 literal-selected key | 27.279 (27.076-27.583) | 1.717 (1.693-1.747) | 15.89x | -93.71% |
| x86-64 immutable-CV-selected key | 26.452 (26.039-26.758) | 1.367 (1.325-1.402) | 19.35x | -94.83% |
| x86-64 adjacent mutable update | 2.518 (2.479-2.584) | 2.512 (2.477-2.579) | 1.002x | +0.09% |

Separate `vm-stats` builds report `typed_ops_loop=1,side_exits=0` for both
candidate controls on both architectures. Their baselines report no native
execution. On x86-64, each side retains identical complete-run traffic:
3 frame pushes/cleanups, 35 `write_val` calls and 246/111 Value clones/drops for
the literal control; the CV control has 3/3, 35 and 248/113. ARM64 recorded the
same corresponding counts.

## Corpus, holdout and footprint

The independent 60-pair x86-64 10x control gives these stable distributions.
Both the ratio of separate medians and the paired median are shown because the
unscaled sub-10ms runs were sensitive to scheduler/timer modes.

| x86-64 control | Baseline ms (p10-p90) | Candidate ms (p10-p90) | Ratio-of-medians delta | Paired median delta |
| --- | ---: | ---: | ---: | ---: |
| Order corpus | 41.141 (40.935-41.764) | 41.466 (41.243-42.118) | +0.79% | +0.81% |
| Typed order corpus | 41.242 (41.048-41.715) | 41.647 (41.445-45.384) | +0.98% | +1.01% |
| Ledger corpus | 20.291 (20.180-20.448) | 20.294 (20.134-20.425) | +0.02% | -0.12% |
| Typed ledger corpus | 20.366 (20.245-20.527) | 20.361 (20.234-21.099) | -0.02% | -0.04% |
| Routing holdout | 73.522 (73.060-74.113) | 73.961 (73.598-74.429) | +0.60% | +0.58% |

The typed-order paired statistic rounds just above one percent while the
separate medians remain below the global one-percent gate and the p10-p90
ranges overlap. It is retained and reported rather than selectively rerun.
The initial unscaled 60-pair paired medians were +0.94%, +0.73%, -0.07%,
-0.59% and +0.25%, respectively.

The ARM64 independent 60-run corpus/holdout median ratios changed by -0.04%,
+0.78%, +0.07%, +0.28% and -0.01% for order, typed order, ledger, typed ledger
and routing. Their retained p10-p90 ranges overlap.

On x86-64 the max-perf binary grows from 5,287,944 to 5,288,592 bytes (+648 B,
+0.012%). Cold builds from empty targets took 42.84 and 43.04 seconds with
peak compiler RSS 974,572 and 987,280 KiB. Fifteen fresh-process RSS pairs put
the literal control at 6,872 versus 6,976 KiB (+1.51%) and the CV control at
6,836 versus 6,848 KiB (+0.18%), with overlapping observed ranges. ARM64
binary growth was 80 bytes; its corresponding fresh-process RSS medians moved
by 64 and 80 KiB.

Generated-region cache bytes, allocator calls and COW detachments are not
exposed by current telemetry. They remain unmeasured, not zero. The unchanged
frame and Value traffic, successful shared-array test and bounded context
table rule out an unbounded per-iteration cost.

## Handoff notes

The implementation changes the typed-loop plan and native mixed builder/runtime
plus mirrored ARM64/x86-64 tests. These compiler/runtime files can conflict with
another execution-region checkpoint; the integrating agent should merge this
coherent commit before later edits to `QuickLongOpsLoop` or
`native_mixed_kernel`.

No backend-specific PHP semantic operation was added. No dependency, Cargo
feature, lockfile or public API changed. Private connectivity, local paths,
raw diagnostics and benchmark-host identifiers are absent from tracked files.
Raw samples remain outside the repository for the integration audit.
