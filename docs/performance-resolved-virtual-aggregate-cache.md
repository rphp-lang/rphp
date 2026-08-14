# Resolved baseline virtual-aggregate cache checkpoint

Status: verified on ARM64 and x86-64, 2026-08-14

This report records an M1/M5 checkpoint for the existing baseline
call/return aggregate pipeline. The pipeline already proved that one
constructor-created request object and one small method-result array cannot
escape. The baseline dispatcher nevertheless repeated the complete structural
resolution on every execution. A bounded request-local sidecar now retains
that immutable resolution while preserving all dynamic guards and the exact
canonical fallback.

## Contract and measured baseline

- **Outcome:** remove repeated steady-state reconstruction of an already
  proven virtual object/array call plan, improve the affected order corpus on
  ARM64 and x86-64, and keep every corpus, holdout and peak-memory result below
  the one-percent regression ceiling.
- **Profile baseline:** clean `main` commit
  `730fc99652238bb6fc63632805e100155712e844`. Sampling the baseline-only order
  lane and auditing the selected call site placed the repeated work in
  virtual-pipeline resolution and nested-call validation. The old path rebuilt
  the same constructor, property, call and consumer descriptor once per hot
  iteration.
- **Integrated comparison baseline:** clean commit
  `19d1044fb6c571abb72f95cf3978c939c2093eb4`.
- **Measured candidate:** clean implementation commit
  `57786c6e73c9378349f1f349ea0ca2dfc643ad10`. It is the clean rebase of
  `574bd333a46bebda737d174d8621460f697a2d49`; both have stable patch ID
  `b8dc8ee834e81866bfcbbda3872fa31d1188f822`. Final integration commit
  `b49d41bacd976ca93f2598502a2d5ec7c42e8710` carries that identical patch
  above the later compatibility checkpoints.
- **Scope:** the existing compiler-proven constructor to read-only method to
  small associative-Long result pipeline, including its bounded nested scalar
  call graph and immediate dead-result consumers.
- **Acceptance:** exact baseline/candidate/reference-PHP output, named
  resolution/cache/invalidation counters, dynamic invalidation coverage, the
  complete feature and native-JIT matrices, a target win on both architectures,
  and no control or peak-memory regression above one percent.
- **Stop rule:** reject any retained pointer without an exact request-lifetime
  owner and a dynamic identity guard, any fallback after a visible write, any
  unbounded or heap-allocating cache, or any persistent result above the
  regression ceiling.

## Design and semantic envelope

`ExecutorGlobals` contains four direct-mapped cache entries. Each entry is 888
bytes on the measured 64-bit targets, so the complete inline cache is 3,552
bytes. A permanent test requires a power-of-two slot count and a total size no
larger than 4 KiB. The array allocates nothing, is reset with each executor,
and is mutated only by the VM thread through `RefCell`.

The key is the exact `NewObj` instruction address plus its owning `OpArray`.
The first successful execution resolves and records immutable bytecode facts,
class and function targets, constructor-property slots, method and nested-call
plans, result consumers and the baseline resume position. Publication occurs
only after that first execution has passed every guard and staged all values.
A direct-map collision simply resolves the new site and replaces the entry;
the uncached adapter remains available for shapes outside the narrower cache
contract.

A hit is not permission to trust mutable PHP state. It revalidates the current
constructor class/function cache, class definition, method receiver class and
identity, method target, every nested receiver identity, and every nested call
cache. Execution then rechecks argument hints and references, constructor
property class/slot/type contracts, nested operations and checked arithmetic.
Current receiver pointers are reconstructed from the live frame rather than
reusing an old slot address.

All accumulator and trailing-result writes remain staged until every check has
succeeded. A failed hit therefore returns to the canonical `NewObj` before an
observable effect. A changed dispatch identity invalidates the descriptor and
attempts a fresh resolution; a per-execution type, reference, property or
overflow guard failure uses canonical fallback without publishing a new
descriptor. No call, write, warning or exception can be replayed.

The checkpoint does not widen aggregate admission. References, identity,
magic/dynamic properties, destructors, escapes, unsupported values and wider
consumer graphs retain their existing materialization boundary. ARM64 and
x86-64 JIT continue to consume the same pre-existing virtual aggregate
operation; this change removes baseline-dispatch resolution work and adds no
backend-specific semantics.

## Telemetry and invalidation proof

Untimed `vm-stats` builds produced the same order-corpus counts on ARM64 and
x86-64:

| Counter | Candidate |
| --- | ---: |
| Declared-object owners | 4 |
| Array owners | 9 |
| Resolution attempts | 2 |
| Successful resolutions | 1 |
| Cache hits | 499,998 |
| Cache invalidations | 0 |
| Guard fallbacks | 0 |

The ownership counts are unchanged from the preceding virtual-aggregate
checkpoint: this slice removes plan reconstruction rather than another PHP
owner. The target's 500,000 hot iterations reduce steady-state structural
resolution from one reconstruction per iteration to one successful resolution
and 499,998 guarded hits.

A permanent nested-receiver replacement test changes a policy object to a
subclass during the loop and preserves exact output `300`. Its diagnostic run
records four attempts, two successful resolutions, 46 hits, two invalidations
and zero guard fallbacks. This proves that a cached nested target is discarded
when the receiver identity changes instead of calling through a stale pointer.

## Benchmark method

Baseline and candidate were built from clean source with Rust 1.93.1,
`max-perf` (fat LTO, one codegen unit), default features and
`RUSTFLAGS=-Ctarget-cpu=native` in separate disposable targets. The measured
lane sets `RPHP_DISABLE_QUICK_LOOPS=1` so the result isolates the baseline
dispatcher. Workload-internal monotonic time excludes parse and process
startup. Runs alternate candidate-first and baseline-first order; all valid
runs are retained with no outlier removal. Delta is the mean of the two
order-specific median ratios and spread is nearest-rank p10-p90.

ARM64 used an Apple M4 with 24 GiB memory, Darwin 25.5/macOS 26.5.2 and PHP
8.5.9, without affinity. X86-64 used an AMD Ryzen 9 7950X with 31,985,372 KiB
memory, Linux 7.0.0-28, PHP 8.4.24, the `performance` governor and CPU 2
affinity. Eight alternating warm-up pairs preceded each final timing series.
Outputs were checked against the baseline and reference PHP before timing.

The order target and routing holdout use 30 measured pairs on each host. The
x86-64 ledger also uses 30 pairs. A short ARM64 ledger result crossed the
ceiling by 0.046 percentage point, so the predeclared investigation rule used
an independent 200-pair decision run. Peak RSS uses 200 order-balanced pairs on
ARM64 and 400 on x86-64. These results cover only the named supported workloads
and configurations; they are not a general RPHP-versus-PHP claim.

## Final results

Times are median milliseconds with p10-p90. The ledger gate executes the
fixture twice per observation; its displayed times below are normalized to one
execution. Negative delta favors the candidate.

| ARM64 workload | Pairs | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: | ---: |
| Order corpus | 30 | 128.156 (127.193-130.040) | 99.939 (99.261-100.994) | -22.172% |
| Ledger corpus | 200 | 48.315 (47.291-52.468) | 48.683 (47.752-51.374) | +0.778% |
| Routing holdout | 30 | 182.780 (180.757-185.306) | 183.129 (181.231-185.391) | +0.398% |

| X86-64 workload | Pairs | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: | ---: |
| Order corpus | 30 | 165.971 (164.929-171.052) | 133.298 (128.840-136.761) | -19.652% |
| Ledger corpus | 30 | 65.253 (64.190-66.262) | 64.058 (63.339-65.110) | -1.763% |
| Routing holdout | 30 | 207.089 (204.910-226.670) | 206.129 (203.225-211.947) | -0.435% |

The 200-pair ARM64 peak-RSS medians are 5,046,272 and 5,095,424 bytes
(+0.974%). The baseline-first and candidate-first halves are +0.649% and
+0.974%, respectively. The 400-pair x86-64 medians are 6,528 and 6,484 KiB
(-0.674%); both order halves are favorable. The process-level granularity is
much larger than the cache's 3,552-byte payload, so memory is treated only as a
regression gate, not as a footprint improvement claim.

## Gates, rejected variants and limitations

The final implementation passed the complete default, no-default,
erased-generics, reified-generics and all-features test configurations,
all-feature/all-target checking, formatting, scorecard shell syntax, focused
default/no-default virtual-pipeline tests, 103 ARM64 native-JIT tests and 34
x86-64 native-JIT tests. Unsafe-policy enforcement remains at 1,623 production
unsafe blocks and 289 unsafe functions; the helper consolidation in this patch
keeps those exact ceilings while retaining the documented safety contracts.

Several measured variants were rejected and fully reverted:

- splitting fallback frames and scratch state regressed the x86-64 ledger by
  2.51% in a 100-pair confirmation;
- one and two inline slots regressed the ARM64 ledger by 6.84% and 3.98%;
- a boxed four-slot cache exceeded the x86-64 peak-memory ceiling at +1.60%;
  and
- thread-local pointer variants moved the ARM64 ledger by +1.90% and +4.91%,
  depending on runtime-field placement.

The accepted four-slot inline form is therefore the smallest measured design
that preserves target capacity, steady-state timing, allocation freedom and
the cross-architecture memory gate. Raw timing, RSS and diagnostic logs remain
outside the public repository because they can contain local paths and runtime
addresses.

## Reproduction

Build clean baseline and candidate snapshots with the configuration above,
then run the baseline lane through the runtime gate:

```sh
RPHP_DISABLE_QUICK_LOOPS=1 \
RPHP_RUNTIME_GATE_PAIRS=30 \
RPHP_RUNTIME_GATE_WARMUPS=8 \
RPHP_RUNTIME_GATE_ONLY=corpus_order_pipeline.php \
RPHP_RUNTIME_GATE_CANDIDATE_BINARY=CANDIDATE_BINARY \
RPHP_RUNTIME_GATE_BASELINE_BINARY=BASELINE_BINARY \
benches/run_runtime_gate.sh CANDIDATE_ROOT BASELINE_ROOT
```

Use `corpus_ledger_pipeline.php` for the control and
`holdout_routing_pipeline.php` for the independent holdout. On Linux, pass the
isolated CPU number as the third argument. Run `scripts/cleanup-builds.sh`
before and after the complete cycle.
