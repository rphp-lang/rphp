# Shared closure ownership checkpoint

Status: verified on ARM64 and x86-64, 2026-08-13

This report records an M1/M5 ownership checkpoint for ordinary PHP closure
copies. RPHP now shares the immutable closure payload and captured environment
behind one reference-counted owner instead of allocating and cloning the whole
payload for every copied `Value`.

## Contract and baseline

- **Outcome:** reduce closure-copy allocation, clone/drop traffic and retained
  memory on ARM64 and x86-64 without changing closure identity, captures,
  binding, references or invocation behavior.
- **Integrated baseline:** clean `main` commit
  `c9f197701a393513af4b3165e9738b43b27d449e`.
- **Measured candidate:** clean implementation commit
  `a93f386b27377083a4a3cacab7a11105743b9644`.
- **Scope:** `Value::Closure` ownership, its construction-only capture
  transition, bounded diagnostics and two general closure-copy workloads.
- **Acceptance:** exact candidate/baseline/reference-PHP output, lifecycle and
  reference-capture tests, the complete feature matrix, a credible target win
  on both architectures, and no representative corpus or independent holdout
  median above the one-percent regression ceiling after confirmation.
- **Stop rule:** reject sharing if published closure payloads need mutation, if
  binding no longer creates a distinct identity, or if either architecture or
  an independent control remains outside the regression budget.

The baseline clone path called `PhpClosure::clone`, copied the capture vector
and constructed a new boxed payload for every closure-valued `Value` copy. The
250,000-copy target therefore traversed 250,001 closure clones and, by the
baseline source path, allocated 250,001 replacement payloads and capture
vectors in addition to the original closure. Retaining those copies produced
39,108,608 bytes peak RSS on ARM64 and 41,752 KiB on x86-64. This isolated a
general ownership cost that native execution cannot remove.

The old diagnostic kind table ended before the closure discriminant, so it
could not publish a trustworthy baseline closure row. The candidate fixes that
coverage defect and reports the exact 250,001 clone operations. The allocation
comparison above is source-accounted rather than retroactively inferred from a
missing counter; the candidate-specific allocation counters below are measured.

## Design and semantic envelope

`Value::Closure` stores the raw owner produced by `Rc<PhpClosure>`. Copying a
closure increments that owner once; dropping it decrements once. The captures,
bound object and function metadata remain immutable after bytecode construction,
so a copy preserves the same PHP closure identity without cloning captured heap
values. Removing the former separate identity token makes payload address the
single canonical identity.

`ClosureUseVar` is the only mutating transition. It appends captures immediately
after `CreateClosure`, while the construction temporary is uniquely owned;
`Rc::get_mut` rejects an accidentally published payload. `Closure::bind()` and
Reflection still clone the semantic payload and pass it through the ordinary
constructor, producing a fresh owner and therefore a distinct closure identity.
By-value captures retain their existing values, by-reference captures retain
their shared cells, and a bound object retains its existing ownership.

The public `Value` representation remains exactly 16 bytes under a compile-time
layout assertion. No `unsafe` invariant was broadened: raw-owner conversion is
paired at construction, clone and drop, and the construction mutation requires
unique ownership. Disabled diagnostics remain inline no-ops and do not add an
ordinary-path counter update.

A mutable `Rc<RefCell<PhpClosure>>` alternative was rejected because it would
add borrow state and checks to every read even though published payloads are
immutable. A general cycle collector is a separate, broader roadmap decision;
it is not needed to remove this measured acyclic copy cost.

## Correctness and telemetry

Focused lifecycle tests prove that two copied values point at one payload,
retain one closure identity and add no ownership to a captured object. A second
test proves that capture construction panics after publication. The complete
closure suite covers invocation, static and bound closures, callback identity,
Reflection, capture ordering and PHP 8.2 by-reference capture behavior.

Candidate, baseline and reference PHP produced these exact payloads before
timing:

| Workload | Expected payload |
| --- | --- |
| 250,000 copied invocations | `34616936` |
| 250,000 retained copies | `250000,25` |
| Reference-capture differential | `2:3:3` |
| Object-capture differential | `11:12:12` |

A separate `vm-stats` build reports the same target values on both
architectures:

| Counter | Candidate value |
| --- | ---: |
| Closure `Value` clones | 250,001 |
| Closure `Value` drops | 250,002 |
| Closure payload allocations | 1 |
| Closure capture-storage allocations | 1 |

Instrumented binaries were not timed.

## Benchmark method

Both sides were built from clean source snapshots with Rust 1.93.1,
`max-perf` (fat LTO, one codegen unit), default `quick-loops`, and
`RUSTFLAGS=-C target-cpu=native` in separate task-scoped targets. The timed
value is each workload's internal `microtime(true)` interval, excluding parsing
and process startup. Every workload used eight alternating warm-up pairs and
100 measured alternating pairs. All valid pairs were retained with no outlier
removal. Delta is the balanced mean of the candidate/baseline median ratio for
candidate-first and baseline-first pairs; spread is nearest-rank p10-p90.

ARM64 used an Apple M4 with 24 GiB memory, macOS 26.5.2 and PHP 8.5.9, without
CPU affinity. X86-64 used an AMD Ryzen 9 7950X with 31,985,372 KiB memory,
Linux 7.0.0-28, PHP 8.4.24, the `performance` governor and CPU 2 affinity. The
x86-64 cycle held the exclusive benchmark lock. The ledger control's first
100-pair x86-64 run measured +1.208%, so the predeclared regression rule
triggered one independent 200-pair confirmation with 12 warm-up pairs; that
confirmation, not a selective favorable rerun, is the accepted cell.

These results cover only the named supported workloads under these
configurations; they are not a general RPHP-versus-PHP claim.

## Final results

Times are median milliseconds with p10-p90. Negative delta favors the
candidate.

| ARM64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Closure copy/invoke | 51.018 (49.580-52.759) | 39.454 (38.479-41.337) | -22.690% |
| Order corpus | 61.375 (60.503-62.026) | 60.647 (59.871-61.433) | -1.134% |
| Ledger corpus | 44.804 (44.064-45.621) | 44.896 (44.291-45.377) | +0.154% |
| Routing holdout | 61.071 (60.689-61.487) | 61.074 (60.647-61.471) | +0.028% |

| X86-64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Closure copy/invoke | 51.598 (50.922-54.697) | 47.075 (46.551-49.637) | -8.842% |
| Order corpus | 72.077 (70.275-75.737) | 71.233 (69.108-77.757) | -1.182% |
| Ledger corpus, 200-pair confirmation | 54.807 (54.164-57.364) | 55.315 (54.622-61.347) | +0.917% |
| Routing holdout | 68.535 (67.758-70.854) | 68.824 (68.019-70.831) | +0.338% |

The retained-copy footprint fell from 39,108,608 to 10,862,592 bytes on ARM64
(-72.23%) and from 41,752 to 10,736 KiB on x86-64 (-74.29%). These are one
fresh-process peak-RSS observations per binary, not distributions. The target
win and footprint reduction survive both architectures, while every accepted
control remains below the +1% ceiling.

The ARM64 binary shrank from 4,373,136 to 4,320,000 bytes (-53,136 B); clean
builds took 35.18 and 37.88 seconds. The x86-64 binary shrank from 5,130,040 to
5,051,416 bytes (-78,624 B); clean builds took 41.67 and 41.43 seconds. Build
times are single cold observations, not performance claims.

## Gates and limitations

Final validation after rebasing onto the compatibility checkpoint completed
with:

- exact reference-PHP closure and capture differentials;
- closure ownership, all 33 closure integration tests, all 75 callable tests,
  frame-cleanup and closure Reflection tests;
- complete default, no-default, erased-generics, reified-generics,
  `jit-prototype` and all-feature test matrices;
- `cargo fmt --all -- --check` and all-feature/all-target checks; and
- unsafe-policy diff enforcement: 1,622 production unsafe blocks, 289 unsafe
  functions and 125 `SAFETY` annotations against ceilings 1,623/289 and floor
  58.

This checkpoint does not introduce cycle collection, change array or object
ownership, or claim that all closure-heavy applications see the target speedup.
Allocator call stacks and exact allocated bytes remain unmeasured, not zero.
Raw samples and diagnostics remain outside the repository because they can
contain local paths and addresses.

## Reproduction

Build clean baseline and candidate snapshots with the configuration above,
then run one workload at a time through the runtime gate. For example:

```sh
RPHP_RUNTIME_GATE_PAIRS=100 \
RPHP_RUNTIME_GATE_WARMUPS=8 \
RPHP_RUNTIME_GATE_ONLY=bench_closure_copy.php \
RPHP_RUNTIME_GATE_CANDIDATE_BINARY=/tmp/rphp-candidate-memory/max-perf/rphp \
RPHP_RUNTIME_GATE_BASELINE_BINARY=/tmp/rphp-baseline-memory/max-perf/rphp \
benches/run_runtime_gate.sh CANDIDATE_ROOT BASELINE_ROOT
```

Use `bench_closure_storage.php` for the retained-footprint observation,
`corpus_order_pipeline.php` and `corpus_ledger_pipeline.php` for the application
controls, and `holdout_routing_pipeline.php` for the independent holdout. On
Linux, pass the isolated CPU number as the third argument. Run
`scripts/cleanup-builds.sh` before and after the complete cycle.
