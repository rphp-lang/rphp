# Bounded packed-array reserve checkpoint

Status: verified on ARM64 and x86-64, 2026-08-13

This report records a baseline-runtime checkpoint for the existing typed
packed-array append loop. A structurally proven large unit-stride loop now
issues one bounded capacity request before appending. The change does not add a
new recognizer, JIT operation or backend-specific semantic path.

## Contract and baseline

- **Outcome:** reduce geometric reallocations in large dense array builds on
  both architectures without changing PHP output, COW behavior, interruption
  semantics or unrelated corpus performance.
- **Integrated baseline:** clean `main` commit
  `a7a26890b3b7fefab47a110edfbbc4c7216c00f5`.
- **Measured candidate:** clean rebased implementation commit
  `1705f3099e90b389d4183c36dfdedee13656e4ae`.
- **Scope:** the already-admitted `QuickArrayPushLoopKernel`, a packed-only
  `PhpArray` capacity API, bounded telemetry and exact semantic tests.
- **Acceptance:** identical candidate/baseline/reference-PHP output, the full
  feature matrix, one successful reserve on the target, a credible target win
  on ARM64 and x86-64, and no control median above the one-percent regression
  ceiling.
- **Stop rule:** do not reserve for small, non-unit, unstable-bound, shared,
  referenced or non-packed destinations. Allocation failure retains normal
  geometric growth.

The checkpoint was originally selected from clean ARM64 and x86-64 execution
scorecards at `a8a7ac29a45415ff09a96e6cf26c0744b9348057`. Array build/read was
already admitted by the typed executor but had no native lowering; JIT did not
improve it on either host. A scaled packed-build profile then put 2,342 of
3,792 ARM64 samples in the quick push loop, including 262 grow/reallocation
samples and 247 moves. The x86-64 profile attributed 82.44% of samples to the
same quick push loop. This identified capacity growth inside an existing
general kernel, not a missing benchmark-specific execution tier.

## Design and semantic envelope

The reserve hint is derived only when the loop header and post operation prove
the same Long induction slot, its bound cannot alias the induction, condition
or post-result slot, and the current induction is below the bound. Requests
below 65,536 remaining iterations are skipped. The additional-entry request is
capped at 524,288; the allocator may choose a larger geometric capacity.

The existing unique-COW array guard still runs first. `PhpArray` accepts the
request only for packed storage whose length matches `next_int_key`, so the
operation cannot change storage tier or key semantics. `try_reserve` failure is
non-observable and the loop continues through its existing append path. Every
append remains committed before interrupt handling or precise overflow
fallback.

The one-time allocation path is outlined. On Linux it lives in the dedicated
`.rphp_packed_array_reserve` section so it does not enlarge the established
`.rphp_cold` section or perturb the shared String/array hot kernels. ARM64 keeps
the monitored quick dispatcher, String append and array push symbol addresses
identical to the baseline.

This layout boundary was required by measurement. The first correct
`try_reserve_exact` candidate improved the x86-64 target but regressed the order
corpus by +2.68%; an independent 200-pair confirmation measured +2.33%.
Outlining into `.rphp_cold` reduced that regression but moved String append by
+5.63%. Switching to geometric `try_reserve` restored the shared
`RawVecInner::grow_exact` symbol, and placing the helper in its own section
removed the remaining control regressions. These rejected results are retained
as evidence that source-level coldness alone was insufficient for this hot
whole-program build.

## Correctness and coverage

The focused tests cover:

- a new empty packed array;
- a 70,000-entry pre-existing dense prefix;
- shared COW and referenced destinations, which retain canonical fallback;
- rejection of hash storage;
- the minimum request, maximum request and mutable-bound alias rules.

Candidate, baseline and `php -n` produced identical payloads before timing for
the target, both application corpora, the independent routing holdout and the
String/scalar controls. The array target payload is `124999750000`.

A separate `vm-stats` build reports the same values on both architectures:

| Counter | Value |
| --- | ---: |
| Quick loop entries / completions | 2 / 2 |
| Quick loop deoptimizations / guard failures | 0 / 0 |
| Quick loop iterations | 999,934 |
| Packed reserve attempts / successes | 1 / 1 |
| Requested reserve entries | 499,967 |

The two quick loops are the packed build and indexed read in
`benches/bench_array.php`. Instrumented binaries were not timed.

## Benchmark method

Both sides were built from clean source snapshots with Rust 1.93.1,
`max-perf` (fat LTO, one codegen unit), default `quick-loops`, and
`RUSTFLAGS=-C target-cpu=native` in separate task-scoped targets. The timed
value is the workload's internal `microtime(true)` interval, excluding parsing
and process startup while retaining quick-region admission. Each workload used
eight alternating warm-up pairs and 100 measured alternating pairs. Array,
ledger and String cells aggregate 8, 2 and 20 executions per side per pair;
other cells use one. Every valid pair was retained and no outlier was removed.

ARM64 used an Apple M4 with 24 GiB memory, macOS 26.5.2 and PHP 8.5.9, without
CPU affinity. X86-64 used an AMD Ryzen 9 7950X with 31,985,372 KiB memory,
Linux 7.0.0-28, PHP 8.4.24, the `performance` governor and CPU 2 affinity. The
x86-64 cycle held the exclusive benchmark lock. Delta is the balanced mean of
the candidate/baseline median ratio for candidate-first and baseline-first
pairs. Spread is nearest-rank p10-p90 over the individual executable totals.

These results cover only the named supported workloads under these
configurations; they are not a general RPHP-versus-PHP claim.

## Final results

Times are median milliseconds with p10-p90. Negative delta favors the
candidate.

| ARM64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Packed array build/read | 23.760 (23.519-23.963) | 22.578 (22.410-22.877) | -4.96% |
| Order corpus | 62.261 (61.793-63.361) | 62.176 (61.737-63.202) | -0.06% |
| Ledger corpus | 42.278 (41.403-44.527) | 42.367 (41.610-44.928) | +0.28% |
| Routing holdout | 61.639 (61.198-62.253) | 61.668 (61.175-62.260) | -0.00% |
| String append control | 6.243 (6.157-6.366) | 6.226 (6.160-6.376) | -0.23% |
| Scalar loop control | 4.393 (4.339-4.475) | 4.392 (4.297-4.499) | -0.03% |

| X86-64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Packed array build/read | 44.038 (43.339-49.588) | 43.790 (43.129-47.801) | -0.57% |
| Order corpus | 70.458 (68.704-74.485) | 70.539 (68.746-74.928) | +0.05% |
| Ledger corpus | 54.693 (54.228-58.090) | 54.546 (54.112-57.979) | -0.30% |
| Routing holdout | 68.454 (67.758-69.876) | 68.538 (67.982-73.573) | +0.19% |
| String append control | 12.529 (12.320-13.902) | 12.639 (12.419-13.772) | +0.93% |
| Scalar loop control | 2.108 (2.082-2.146) | 2.101 (2.084-2.142) | -0.16% |

The target improvement is larger on ARM64, where the profile showed more
reallocation and move traffic, but remains reproducible on the physical AMD
host. Every control remains below the predeclared +1% ceiling.

## Footprint and gates

The ARM64 max-perf binary grows from 4,372,816 to 4,372,912 bytes (+96 B).
Cold builds from empty targets took 37.27 and 37.48 seconds. One diagnostic
array run observed 14,663,680 baseline versus 12,517,376 candidate bytes peak
RSS. The x86-64 binary grows from 5,124,384 to 5,125,120 bytes (+736 B); cold
builds took 41 and 42 seconds, and one diagnostic run observed 14,792 versus
14,468 KiB peak RSS. RSS cells are single observations, not distributional
claims.

Final validation after rebasing onto the integrated baseline completed with:

- `cargo fmt --all -- --check`;
- `cargo check --locked --all-features --all-targets`;
- complete default, no-default, erased-generics, reified-generics,
  `jit-prototype` and all-feature test matrices;
- focused packed-array reserve, prefix, COW and reference tests on ARM64 and
  physical x86-64;
- unsafe policy self-test and diff gate: 1,619 production unsafe blocks, 289
  unsafe functions and 121 `SAFETY` annotations against ceilings 1,623/289 and
  floor 58.

No dependency, Cargo feature, public API or backend-specific semantic
operation changed. Generated allocator counts and exact allocated bytes remain
unmeasured, not zero. Raw samples, symbol maps and diagnostics remain outside
the repository because they can contain local paths and addresses.

## Reproduction

Build clean baseline and candidate snapshots with the configuration above,
then capture one workload at a time with the runtime gate. For example:

```sh
RPHP_RUNTIME_GATE_PAIRS=100 \
RPHP_RUNTIME_GATE_WARMUPS=8 \
RPHP_RUNTIME_GATE_ONLY=bench_array.php \
RPHP_RUNTIME_GATE_CANDIDATE_BINARY=/tmp/rphp-candidate-array/max-perf/rphp \
RPHP_RUNTIME_GATE_BASELINE_BINARY=/tmp/rphp-baseline-array/max-perf/rphp \
benches/run_runtime_gate.sh CANDIDATE_ROOT BASELINE_ROOT
```

Use `holdout_routing_pipeline.php` as `RPHP_RUNTIME_GATE_ONLY` for the
independent holdout. On Linux, pass the isolated CPU number as the third
argument. Run `scripts/cleanup-builds.sh` before and after the complete cycle.
