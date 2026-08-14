# Virtual call/return aggregate checkpoint

Status: verified on ARM64 and x86-64, 2026-08-14

This report records an M1/M5 checkpoint for a structurally proven call graph
whose method creates a small associative result and whose caller immediately
extracts scalar fields and discards both that result and a constructor-created
request DTO. The existing typed-region and native JIT path already kept those
values virtual. This checkpoint makes the same proof and guarded transactional
executor available to the baseline dispatcher, independent of quick-loop
selection.

## Contract and measured baseline

- **Outcome:** remove per-iteration request-object and result-array ownership
  from the supported order shape in baseline execution, improve that corpus on
  ARM64 and x86-64, and keep every independent control below the one-percent
  regression ceiling.
- **Baseline:** clean commit
  `2457cea20843034ebf9fe970d5df0c8a052d9049`.
- **Candidate:** clean implementation commit
  `7eb4384adcfb68599dc4e090abebbb1c81f5823d`; the later documentation commit
  does not change production code.
- **Scope:** a literal constructor-initialized declared object, one exact
  method call through a compiler-proven read-only Long call graph, a result of
  one to four literal-key Long entries, and immediate dead-result scalar
  consumers.
- **Acceptance:** exact baseline/candidate/reference-PHP output, the complete
  feature and JIT matrix, named ownership counters, a corpus win on both
  architectures, unchanged default typed/JIT behavior, and no control or peak
  memory regression above one percent.
- **Stop rule:** reject if an observable object/array identity is omitted, a
  guard can replay a completed effect, either architecture loses the existing
  typed operation, or any corpus/holdout control remains above the ceiling.

The exact ARM64 profile used the baseline configuration and a 20,000,000-row
order run. Of 3,850 main-thread samples, 891 were in the direct object-array
call path, including 454 in aggregate evaluation and 300 in nested Long-plan
evaluation. Materialization was visible in `PhpArray::with_hash_capacity`,
`PhpArray::set_str_value`, allocator calls and array drops. Another 431 samples
were at object construction, including 398 below `op_new_obj`; object and array
release accounted for a further 496 samples in the executor cleanup site.
Together with one request owner and one result owner per hot iteration, this
identified a baseline allocation/lifecycle cost rather than a missing native
lowering.

## Design and semantic envelope

The optimization reuses the existing general structural proof. The compiler
marks only a complete shape with all of these properties:

1. a literal `NewObj` with one to eight positional scalar arguments, its
   constructor call and an otherwise-dead assigned CV;
2. a constructor described by the exact declared-property initialization plan;
3. one following method call that receives the virtual object exactly once;
4. a fixed-arity, non-variadic, non-reference method whose
   `ObjectArrayFunctionPlan` contains only guarded property reads, checked Long
   arithmetic, `intdiv`, and nested methods already proven by
   `ObjectLongFunctionPlan`;
5. an associative result with one to four literal string keys and Long values;
   and
6. an assigned result CV used only by the immediate proven fetch/add consumers,
   with a whole-OpArray proof that neither intermediate has any other use.

Admission is independent of source, benchmark, class, method and literal
values. Object/result returns, later reads, reference binding, identity tests,
dynamic uses and every other escape prevent the marker.

Runtime resolution still requires exact warmed class, constructor, method,
property and nested-call caches; matching fixed arity and type hints; public
declared property slots; a read-only supported method body; and no destructor.
All nested calls, key lookups and checked arithmetic finish before accumulator
or trailing-result slots are changed. A failed guard therefore resumes at the
original `NewObj` before any skipped effect and canonical PHP materializes both
owners. The materialization policy is deliberately conservative: when a
reference, identity, magic/dynamic property, constructor/destructor or escape
boundary exists, the complete region is rejected and canonical allocation
happens before that boundary. An admitted region has no live heap value that
requires late exit materialization.

Previously these caller-side markers were created inside
`prepare_quick_loops` only after its feature and environment gates. The
baseline dispatcher already had the guarded executor but a baseline-only build
or `RPHP_DISABLE_QUICK_LOOPS=1` could never select it. Marker construction now
precedes the loop gate. The gate still controls loop planning,
`VirtualDeclaredObjectReads`, callbacks and native compilation; it no longer
disables this independent direct call/return optimization.

No new unsafe code, heap representation, dependency, IR variant or backend
lowering was added. Default typed execution and ARM64/x86-64 JIT continue to use
the same existing `VirtualObjectArrayPipeline`; x86-64 `vm-stats` recorded one
native `typed_ops_loop` execution and zero side exits. Non-`vm-stats` builds
inline the new array-owner telemetry hook to nothing.

## Correctness and ownership evidence

Baseline, candidate and reference PHP produce identical complete payloads for
untyped/typed order, untyped/typed ledger and the routing holdout. Focused tests
cover structural selection in both default and no-default builds, constructor
fallback, escaping request/result values, polymorphic nested calls, return
overflow and consumer overflow. The complete default, no-default,
erased-generics, reified-generics and all-features test configurations pass,
as do formatting, all-feature/all-target checking, scorecard shell syntax and
the unsafe-policy ratchet.

Untimed `vm-stats` builds report the following order-corpus counts on both
architectures:

| Counter | Baseline | Candidate |
| --- | ---: | ---: |
| Declared object owner allocations | 500,003 | 4 |
| Array owner allocations | 500,008* | 9 |
| Declared property-storage allocations | 3 | 2 |
| Declared property-storage reuses | 499,998 | 0 |
| Array `Value` clones | 500,010 | 11 |
| Object `Value` clones | 500,013 | 14 |
| Canonical `FetchDimR` executions | 2,000,004 | 8 |
| Quick-loop guard failures/deoptimizations | 0 / 0 | 0 / 0 |

`*` The exact baseline commit predates the new array-owner counter. Its value
is source-accounted as one owner for each direct materialized result plus eight
other arrays and was reproduced by an instrumentation-only control build. The
exact candidate contains the counter. Instrumented binaries were not timed.

## Benchmark method

Both sides were built from the exact clean commits above with Rust 1.93.1,
`max-perf` (fat LTO, one codegen unit), default `quick-loops`, and
`RUSTFLAGS=-Ctarget-cpu=native` in separate task-scoped targets. The optimized
lane is the existing scorecard baseline mode,
`RPHP_DISABLE_QUICK_LOOPS=1`; only loop-region selection is disabled. The
timed value is each workload's internal `microtime(true)` interval, excluding
parsing and process startup. Every output was checked before timing. All valid
runs were retained and no outlier was removed.

ARM64 used an Apple M4 with 24 GiB memory, macOS 26.5.2, no affinity, one
pre-timing validation run and 25 alternating measured pairs for the baseline
lane. Default typed/JIT controls used 15 alternating pairs; a 50-pair
independent rerun resolved the only initial above-one-percent noise signal.

X86-64 used an AMD Ryzen 9 7950X with 31,985,372 KiB memory, Linux
7.0.0-28, the `performance` governor and CPU 2 affinity. Eight alternating
warm-up pairs preceded 50 measured baseline pairs; default typed/JIT controls
used four warm-up and 30 measured pairs. The complete build, timing, stats and
peak-memory cycles held the exclusive benchmark lock. Reference PHP was 8.5.9
on ARM64 and 8.4.24 on x86-64.

These results cover only the named supported workloads and configurations;
they are not a general RPHP-versus-PHP claim.

## Results

Times are median milliseconds with p10-p90. Negative delta favors the
candidate.

| ARM64 baseline-lane workload | Baseline | Candidate | Delta |
| --- | ---: | ---: | ---: |
| Order corpus | 186.054 (183.997-188.415) | 123.496 (121.475-124.095) | -33.624% |
| Typed order corpus | 224.040 (222.496-226.931) | 165.176 (163.411-165.772) | -26.274% |
| Ledger corpus | 47.699 (47.061-48.229) | 47.521 (46.936-47.813) | -0.373% |
| Typed ledger corpus | 47.788 (47.170-48.117) | 47.680 (47.063-48.353) | -0.227% |
| Routing holdout | 179.741 (178.194-182.487) | 178.587 (176.895-180.366) | -0.642% |

| X86-64 baseline-lane workload | Baseline | Candidate | Delta |
| --- | ---: | ---: | ---: |
| Order corpus | 236.886 (232.073-243.586) | 161.797 (159.667-165.523) | -31.699% |
| Typed order corpus | 282.889 (278.924-290.593) | 210.365 (207.980-217.953) | -25.637% |
| Ledger corpus | 58.993 (58.354-59.730) | 58.632 (56.918-59.409) | -0.612% |
| Typed ledger corpus | 58.552 (57.282-59.336) | 58.491 (57.203-59.509) | -0.104% |
| Routing holdout | 191.811 (188.854-193.647) | 191.358 (188.325-194.423) | -0.236% |

Default typed/JIT paths were controls because they already selected the virtual
aggregate before this checkpoint. The largest observed candidate regression
was +0.878% on ARM64 typed ledger and +0.292% on x86-64 typed order; every
control stayed below the one-percent ceiling. The independent ARM64 JIT ledger
rerun was favorable (2.063 ms to 2.040 ms, 50 pairs).

Median peak RSS for ten interleaved order runs was 4,964,352 versus 4,947,968
bytes on ARM64 (-0.330%) and 6,868,992 versus 6,918,144 bytes on x86-64
(+0.716%). ARM64 baseline/candidate binaries were both 4,397,520 bytes and JIT
binaries both 4,610,992 bytes. X86-64 candidate binaries were 176 bytes smaller
in the baseline build and 16 bytes smaller with JIT. Release build time was
38.25 versus 37.92 seconds on ARM64 and 43 versus 42 seconds on x86-64; JIT
build time was 39.94 versus 39.98 seconds and 45 versus 44 seconds,
respectively.

## Remaining boundary

This checkpoint converges the already-proven order-shaped aggregate path; it
does not claim general escape analysis. Standalone functions, arbitrary
consumer graphs, wider or non-Long arrays, a DTO returned from the callee, and
late materialization after partial escape remain canonical. A later goal
should extend the common virtual-value/materialization vocabulary only after a
profile finds one of those shapes in a representative corpus.
