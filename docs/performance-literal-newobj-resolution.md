# Literal `NewObj` class-resolution checkpoint

Status: verified on ARM64 and x86-64, 2026-08-14

This report records an M1/M5 checkpoint for ordinary literal object creation.
After the first successful resolution at a monomorphic `new ClassName` site,
RPHP now reuses the inline cache's stable numeric class identity instead of
allocating an owned class-name `String` and hashing it for every object.

## Contract and baseline

- **Outcome:** make warmed literal `NewObj` class resolution allocation-free
  and hash-free on ARM64 and x86-64 without changing PHP-visible construction.
- **Integrated baseline:** clean `main` commit
  `90f77858169974a803350e2a8f0a5bf9f3c0595a`.
- **Measured candidate:** clean implementation commit
  `18b841bf8afe21ea52e89590a4881ac7c447b24c`.
- **Scope:** literal-class admission, the existing per-opline inline cache,
  disabled-by-default telemetry, focused re-entry/alias/anonymous-class tests,
  and the existing million-object lifecycle workload.
- **Acceptance:** exact candidate/baseline/reference-PHP output, focused object,
  constructor and autoload semantics, the complete feature matrix, target wins
  on both architectures, and no corpus or independent-holdout median regression
  above one percent.
- **Stop rule:** reject the cache if class identity can change in place, if it
  observes a runtime/static class expression, if autoload re-entry can borrow a
  mutable value, or if either architecture or a control fails its gate.

The exact ARM64 baseline profile sampled a 50,000,000-object version of the
target for three seconds at 1 ms intervals. `op_new_obj` contained 1,176 of
2,313 main-thread samples (50.8%). Source and instrumented counts then isolated
one owned class-name materialization and repeated class-table lookup per object,
in addition to the separately optimized property-storage lifecycle.

## Design and semantic envelope

The existing `InlineCache` already stores the constructor pointer and the
resolved class ID. A literal site is admitted only when its operand is a
compiler-owned constant, neither dynamic-class nor late-static flags are set,
and that class ID is nonzero. The first execution retains canonical anonymous
class registration, dependency loading, autoload and name lookup. It then
publishes the class ID with the constructor cache. Subsequent executions borrow
the immutable constant spelling for diagnostics and use `class_by_id` for O(1)
metadata access.

Dynamic `new $expr` and `new static` remain on the canonical path and own the
evaluated name before autoload can re-enter the VM. Anonymous classes, aliases,
missing classes, interfaces, abstract classes, enums, the reserved `Generator`
case, constructor hit/miss caching, property-init plans and erased/reified
generic checks keep their prior order and error behavior. Class definitions are
boxed before publication, redeclaration is rejected, and numeric identities are
not reused within an executor, so the existing class-ID cache invariant is
unchanged.

The handler is split after resolution into a non-inlined continuation. This
keeps class-resolution growth from shifting neighboring property hot code while
leaving one general object-construction implementation. It is not keyed to a
class, function, source file or benchmark. `InlineCache` remains 16 bytes and no
unsafe block, unsafe function, allocation, dependency or persistent cache entry
was added. Disabled telemetry compiles to inline no-ops.

## Correctness and telemetry

Focused tests cover repeated literal autoload (one loader invocation), aliases,
anonymous-class identity and a dynamic class-name value mutated during autoload
re-entry. Existing class and constructor suites cover variable class cache
rekeying, `new static`, promoted/default/typed properties, constructor misses,
exceptions, references, destructors, Reflection and generic construction.

Candidate, baseline and reference PHP produce checksum `17000000` for one
million objects with three scalar declared defaults. A separate candidate
`vm-stats` build reports:

| Counter | Candidate value |
| --- | ---: |
| Literal cache hits | 999,999 |
| Literal cache misses | 1 |
| Class-name materializations | 1 |
| Class hash lookups | 3 |
| Declared object owner allocations | 1,000,000 |
| Declared property-storage allocations | 2 |

The exact baseline reports 1,000,000 class-name materializations and 1,000,002
class hash lookups for the same output. Instrumented binaries were not timed.

## Benchmark method

Both sides were built from clean source snapshots with Rust 1.93.1,
`max-perf` (fat LTO, one codegen unit), default `quick-loops`, and
`RUSTFLAGS=-C target-cpu=native` in separate task-scoped targets. The timed
value is each workload's internal `microtime(true)` interval, excluding parsing
and process startup. Every workload used eight alternating warm-up pairs and
100 measured alternating pairs. The ledger workload aggregates two executions
per side per pair. All valid pairs were retained with no outlier removal.
Delta is the balanced mean of candidate/baseline median ratios for
candidate-first and baseline-first pairs; spread is nearest-rank p10-p90.

ARM64 used an Apple M4 with 24 GB memory, macOS 26.5.2 and PHP 8.5.9, without
CPU affinity. X86-64 used an AMD Ryzen 9 7950X with 31,985,372 KiB memory,
Linux 7.0.0-28, PHP 8.4.24, the `performance` governor and CPU 2 affinity. The
x86-64 build and timing cycles held the exclusive benchmark lock.

These results cover only the named supported workloads under these
configurations; they are not a general RPHP-versus-PHP claim.

## Final results

Times are median milliseconds with p10-p90. Negative delta favors the
candidate.

| ARM64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Declared-object lifecycle | 91.802 (89.796-94.785) | 63.245 (61.788-65.216) | -31.084% |
| Order corpus | 61.383 (61.035-62.158) | 61.900 (61.504-62.608) | +0.828% |
| Ledger corpus | 44.978 (44.290-45.550) | 41.768 (41.278-42.257) | -7.191% |
| Routing holdout | 61.597 (61.248-62.128) | 61.506 (61.113-61.952) | -0.187% |

| X86-64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Declared-object lifecycle | 93.727 (92.313-99.079) | 70.313 (69.043-76.378) | -25.054% |
| Order corpus | 70.746 (68.936-73.235) | 70.764 (69.212-73.843) | -0.010% |
| Ledger corpus | 55.059 (54.450-61.145) | 54.279 (53.650-56.873) | -1.360% |
| Routing holdout | 68.943 (68.480-70.349) | 67.958 (67.314-69.488) | -1.534% |

One x86-64 observation measured 6,564 and 6,584 KiB peak RSS for baseline and
candidate. The 20 KiB process-level difference is too coarse for a change that
adds no persistent storage and is not treated as a memory claim. macOS denied
the resident-set query in the local sandbox; layout evidence instead confirms
that the existing 16-byte inline cache is unchanged.

The ARM64 binary grew from 4,337,712 to 4,337,840 bytes (+128 B), while its
`__text` section shrank by 160 bytes. Clean builds took 37.57 and 35.82 seconds.
The x86-64 binary grew from 5,071,832 to 5,072,064 bytes (+232 B); clean builds
took 41.65 and 41.74 seconds. Build times and size changes are single
observations, not performance claims.

## Gates and limitations

Final validation completed with:

- exact baseline/candidate/reference-PHP checksum validation;
- focused autoload, class, anonymous-class and constructor integration suites;
- complete default, no-default, erased-generics, reified-generics and
  all-feature test matrices;
- `cargo check --locked --all-features --all-targets`, formatting, shell syntax
  and diff checks; and
- unsafe-policy enforcement: 1,623 production unsafe blocks, 289 unsafe
  functions and 126 `SAFETY` annotations against ceilings 1,623/289 and floor
  58.

The optimization is deliberately narrow. It does not cache dynamic or
late-static class expressions, remove the identity-bearing object allocation,
change property storage, or claim that object-heavy applications see the
isolated target speedup. Raw samples and diagnostics remain outside the
repository because they can contain local paths and addresses.

## Reproduction

Build clean baseline and candidate snapshots with the configuration above,
then run the target and each control through the runtime gate:

```sh
RPHP_RUNTIME_GATE_PAIRS=100 \
RPHP_RUNTIME_GATE_WARMUPS=8 \
RPHP_RUNTIME_GATE_ONLY=bench_declared_object_lifecycle.php \
RPHP_RUNTIME_GATE_CANDIDATE_BINARY=/tmp/rphp-candidate-newobj/max-perf/rphp \
RPHP_RUNTIME_GATE_BASELINE_BINARY=/tmp/rphp-baseline-newobj/max-perf/rphp \
benches/run_runtime_gate.sh candidate-source baseline-source
```

Repeat with `corpus_order_pipeline.php`, `corpus_ledger_pipeline.php` and
`holdout_routing_pipeline.php`. On Linux, pass the isolated CPU number as the
third argument. Enable `vm-stats` only for counter validation, never for timing.
