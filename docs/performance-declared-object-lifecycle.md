# Declared-object allocation and lifecycle checkpoint

Status: verified on ARM64 and x86-64, 2026-08-13

This report records an M1/M5 allocation checkpoint for ordinary declared PHP
objects. RPHP now reuses one bounded thread-local property buffer per common
declared-object width instead of allocating a replacement `Vec<Value>` for
every construction.

## Contract and baseline

- **Outcome:** reduce declared-property allocation and object lifecycle time on
  ARM64 and x86-64 without changing identity, properties, references,
  destructors, dynamic properties, cloning or Reflection.
- **Integrated baseline:** clean `main` commit
  `9eed14412eacb0a4088329bb9eacbf70733f657a`.
- **Measured candidate:** clean implementation commit
  `a1feda2f6682a383c92a0d97458c048a05f58946`; the final documentation and
  strengthened re-entrant-drop test do not change production code.
- **Scope:** object-local materialization and thread-local reuse of one to four
  declared property slots, disabled-by-default telemetry, and one general
  lifecycle workload.
- **Acceptance:** exact candidate/baseline/reference-PHP output, focused
  lifecycle and object semantic tests, the complete feature matrix, a target
  allocation and timing reduction on both architectures, and no corpus or
  independent holdout median above the one-percent regression ceiling.
- **Stop rule:** reject reuse if a buffer retains a live PHP value or request
  data, outlives its runtime thread, has unbounded capacity, or if either
  architecture or a control remains outside the regression budget.

The exact ARM64 baseline profile sampled a 50,000,000-object version of the
target for three seconds at 1 ms intervals. Of 2,313 main-thread samples,
`op_new_obj` contained 1,176 (50.8%); allocator `malloc`-family leaf frames
accounted for 400 (17.3%) and `free`-family leaf frames for 550 (23.8%).
`Rc::drop_slow` contained 358 samples (15.5%) and `PhpObject` drop 241 (10.4%).
Categories overlap because inclusive and leaf samples answer different parts
of the lifecycle. The baseline source constructed declared slots with
`property_defaults.to_vec()`, accounting for one property-buffer allocation per
non-empty object in addition to its identity-bearing owner allocation.

## Design and semantic envelope

Class defaults remain immutable and shared. Construction still clones each
default into object-owned slots, but widths one through four first take an
empty exact-capacity `Vec<Value>` from a thread-local pool. `PhpObject::drop`
clears the values before borrowing that pool, so nested heap-bearing property
drops may re-enter it safely, and returns only exact-capacity buffers. One
buffer is retained per width: at most ten `Value` slots, or 160 bytes of payload
capacity, per runtime thread. Empty and wider objects retain their prior paths.
The pool may survive multiple executions on the same thread, but every retained
buffer is empty, so no PHP value or request data crosses that boundary.

The owner remains `Rc<RefCell<PhpObject>>`; object identity, reference counts,
destructor state and address lifetime do not change. Declared and dynamic
properties remain separate, clones still materialize independent declared
slots, references retain their shared cells, and Reflection sees the canonical
layout and per-object values. PHP evaluates a replacement object before
releasing the previous CV, so the million-object target needs two initial
buffers and then reaches steady-state reuse.

`Value` remains 16 bytes and `PhpObject` remains 72 bytes under compile-time
layout assertions. No unsafe block, function or lifetime invariant was added.
Disabled diagnostics compile to inline no-ops.

Inline small storage was rejected for this slice because four inline values
would enlarge every identity-bearing object by 64 bytes, including empty and
dynamic objects, and would require wider clone/native-property changes. A
general object arena was rejected because owner reuse must additionally solve
stable identity, references, destructor re-entry and generator state. The
bounded property-only pool removes the measured allocation while preserving
those boundaries.

## Correctness and telemetry

Focused unit tests prove that returned buffers are cleared, nested pooled object
properties can drop re-entrantly, and an oversized internal capacity is not
retained. Existing suites cover shallow/deep clone independence, destructor
order and allocator-address reuse, declared and dynamic property references,
typed/default properties, magic methods, constructor failure and Reflection
reads/writes.

Candidate, baseline and reference PHP produce checksum `17000000` for one
million objects with three scalar declared defaults. On both architectures a
separate `vm-stats` build reports:

| Counter | Candidate value |
| --- | ---: |
| Declared object owner allocations | 1,000,000 |
| Declared property-storage allocations | 2 |
| Declared property-storage reuses | 999,998 |
| Declared property-storage returns | 999,999 |

The baseline property-storage count is source-accounted as 1,000,000 because
its exact path executes one non-empty `to_vec()` per instance; the old binary
does not contain the new counter. Owner allocation intentionally remains one
per object to preserve identity. Instrumented binaries were not timed.

## Benchmark method

Both sides were built from clean source snapshots with Rust 1.93.1,
`max-perf` (fat LTO, one codegen unit), default `quick-loops`, and
`RUSTFLAGS=-C target-cpu=native` in separate task-scoped targets. The timed
value is each workload's internal `microtime(true)` interval, excluding parsing
and process startup. Every workload used eight alternating warm-up pairs and
100 measured alternating pairs. All valid pairs were retained with no outlier
removal. Delta is the balanced mean of candidate/baseline median ratios for
candidate-first and baseline-first pairs; spread is nearest-rank p10-p90.

ARM64 used an Apple M4 with 24 GiB memory, macOS 26.5.2 and PHP 8.5.9, without
CPU affinity. X86-64 used an AMD Ryzen 9 7950X with 31,985,372 KiB memory,
Linux 7.0.0-28, PHP 8.4.24, the `performance` governor and CPU 2 affinity. The
x86-64 cycles held the exclusive benchmark lock. Corpus and holdout results
come from the initial final-candidate cycle; the target was then independently
remeasured after removing constructor work so that it isolates declared-default
materialization and release.

These results cover only the named supported workloads under these
configurations; they are not a general RPHP-versus-PHP claim.

## Final results

Times are median milliseconds with p10-p90. Negative delta favors the
candidate.

| ARM64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Declared-object lifecycle | 108.557 (106.213-110.700) | 91.211 (89.141-94.077) | -15.868% |
| Order corpus | 61.803 (61.163-62.840) | 61.680 (61.212-62.462) | -0.174% |
| Ledger corpus | 44.950 (44.397-45.360) | 44.750 (44.024-45.312) | -0.127% |
| Routing holdout | 61.490 (60.989-62.353) | 61.675 (61.260-62.446) | +0.340% |

| X86-64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Declared-object lifecycle | 95.846 (94.912-100.695) | 93.214 (92.042-97.057) | -2.687% |
| Order corpus | 71.262 (68.891-74.108) | 71.486 (69.919-74.153) | +0.635% |
| Ledger corpus | 55.321 (54.674-57.917) | 55.122 (54.346-57.364) | -0.462% |
| Routing holdout | 69.123 (68.367-70.455) | 68.929 (68.222-70.023) | -0.243% |

Ten interleaved fresh-process observations on ARM64 put both baseline and
candidate median peak RSS at 4,620,288 bytes; the bounded pool therefore has no
measurable footprint cost. One x86-64 observation measured 6,476 and 6,708 KiB,
a 232 KiB process-level difference that is too coarse for the 160-byte bound
and is not treated as a regression or a memory improvement claim.

The ARM64 binary grew from 4,337,040 to 4,337,712 bytes (+672 B); clean builds
took 35.83 and 35.86 seconds. The x86-64 binary grew from 5,070,872 to 5,071,832
bytes (+960 B); clean builds took 41.96 and 41.62 seconds. Build times and size
changes are single observations, not performance claims.

## Gates and limitations

Final validation completed with:

- exact baseline/candidate/reference-PHP checksum validation;
- declared-storage unit tests and the complete clone, class, constructor,
  magic-method and Reflection integration suites;
- complete default, no-default, erased-generics, reified-generics,
  `jit-prototype` and all-feature test matrices;
- `cargo fmt --all -- --check`, all-feature/all-target checks and shell syntax;
  and
- unsafe-policy enforcement: 1,623 production unsafe blocks, 289 unsafe
  functions and 126 `SAFETY` annotations against ceilings 1,623/289 and floor
  58.

The optimization is deliberately narrow: it does not remove the object owner
allocation, pool widths above four, retain simultaneous live-object storage, or
claim that object-heavy applications see the isolated target speedup. Raw
samples and diagnostics remain outside the repository because they can contain
local paths and addresses.

## Reproduction

Build clean baseline and candidate snapshots with the configuration above,
then run the target through the runtime gate:

```sh
RPHP_RUNTIME_GATE_PAIRS=100 \
RPHP_RUNTIME_GATE_WARMUPS=8 \
RPHP_RUNTIME_GATE_ONLY=bench_declared_object_lifecycle.php \
RPHP_RUNTIME_GATE_CANDIDATE_BINARY=/tmp/rphp-candidate-object/max-perf/rphp \
RPHP_RUNTIME_GATE_BASELINE_BINARY=/tmp/rphp-baseline-object/max-perf/rphp \
benches/run_runtime_gate.sh CANDIDATE_ROOT BASELINE_ROOT
```

Use `corpus_order_pipeline.php` and `corpus_ledger_pipeline.php` for the
application controls and `holdout_routing_pipeline.php` for the independent
holdout. On Linux, pass the isolated CPU number as the third argument. Run
`scripts/cleanup-builds.sh` before and after the complete cycle.
