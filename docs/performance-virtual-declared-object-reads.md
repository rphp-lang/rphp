# Virtual declared-object read checkpoint

Status: verified on ARM64 and x86-64, 2026-08-14

This report records an M1/M3/M5 checkpoint for a structurally proven object
lifetime. After canonical caches warm, RPHP can project immutable declared
`Long` defaults from `new LiteralClass() -> dead local -> immediate property
read(s)` without allocating the otherwise unobservable identity-bearing owner.

## Contract and baseline

- **Outcome:** reduce one million declared-object owner allocations to a
  bounded warm-up count and improve the supported shape on ARM64 and x86-64,
  while retaining canonical behavior at every identity, reference, escape,
  constructor, destructor, magic and dynamic-property boundary.
- **Profile-selection baseline:** clean commit
  `f964bdbb0fab5f2d7db742140546141433b2b5f9`.
- **Measured post-compatibility baseline:** clean commit
  `765dcd659e7c7a0110bf502cc4c75ee137737cdd`.
- **Measured candidate:** clean implementation commit
  `5f9d455445515b78bd94173dab35b9f3679e0a59`; the documentation commit does
  not change production code.
- **Scope:** literal zero-argument `NewObj`, a dead assigned CV, one to eight
  contiguous constant-name reads, the general typed loop IR and its shared
  ARM64/x86-64 lowering.
- **Acceptance:** exact baseline/candidate/reference-PHP output, focused guard
  and exit coverage, the complete feature matrix, owner allocations bounded by
  warm-up, target wins on both architectures, and no corpus or holdout median
  regression above one percent.
- **Stop rule:** reject the candidate if any identity-bearing use is admitted,
  a completed scalar read is lost or repeated on exit, a constructor,
  destructor or magic access can be skipped, either backend lacks the same IR
  operation, or a control remains above the regression ceiling.

The exact 50,000,000-object ARM64 profile at the selection baseline collected
2,866 main-thread samples. `op_new_obj` contained 710 samples, including 335 in
the resolved construction continuation. The slot-release path contained 775
samples, including 323 in `Rc::drop_slow` and 188 in `PhpObject` drop. Collapsed
leaf counts included 325 in resolved construction, 269 in allocator free,
203 in `PhpObject` drop and 151 in allocator malloc. These inclusive and leaf
categories overlap, but together with the exact one-owner-per-object counter
identified identity materialization and release as the remaining lifecycle
cost after declared-property storage reuse.

Compatibility work landed while this checkpoint was in progress and changed a
shared executor translation unit. The performance branch was therefore rebased
onto that clean checkpoint and the full correctness and dual-architecture A/B
evidence was regenerated against its exact parent.

## Design and semantic envelope

The compiler marks only this complete structural shape:

1. a literal, non-dynamic, zero-argument `NewObj` producing a temporary;
2. its canonical `DoFcall` and an unused-result assignment to a CV;
3. one to eight immediately contiguous ordinary `FetchObjR` operations with
   literal property names; and
4. a whole-OpArray use proof that the object temporary, constructor result and
   assigned CV have no other use.

That final use-set proof excludes returns, arguments, calls, comparisons,
identity observation, references, dynamic-property access, later reads and
post-loop observation. Admission is independent of source, function, class and
benchmark names.

The first 33 iterations remain canonical and warm the existing class,
constructor and property caches. Runtime resolution then requires an exact
class ID and spelling, a stable negative constructor cache, a concrete
non-interface/non-enum class, no direct or inherited `__construct` or
`__destruct`, and an exact read-safe declared-property cache for every read.
Every projected default must be a non-reference `Long`. Private/protected or
missing properties therefore preserve `__get` and error behavior. Erased and
reified generic builds deliberately keep the canonical object path.

The typed operation writes the proven scalar values into their ordinary result
slots and marks them dirty immediately. If later arithmetic overflows or any
other guard exits, completed reads are published and baseline resumes at the
first uncompleted operation. The object CV is not materialized because the
global proof makes it dead and the runtime guard forbids destructor behavior.
Value-producing prefix increment remains canonical; the general loop planner
accepts prefix increment only when its result is unused.

ARM64 and x86-64 lower the same resolved operation to constant moves into the
same result slots. Explicit `jit-prototype,vm-stats` executions on both
architectures report one native `typed_ops_loop` execution and zero native side
exits. The typed interpreter remains the semantic implementation used by the
ordinary default build.

No object, property buffer, persistent cache entry, dependency or unsafe
function was added. `QuickLongOp` remains 176 bytes on baseline and candidate;
the bounded read array therefore does not enlarge every typed operation.

## Correctness and telemetry

Focused tests cover structural selection, escape/post-loop observation,
property references, constructors, magic non-public reads, non-`Long` defaults
and overflow after a completed read. Existing class/object suites cover aliases,
inheritance, typed and uninitialized properties, readonly behavior, dynamic
properties, references, cloning, identity, Reflection, errors and destructor
lookup. The runtime guard uses method resolution rather than only scanning the
immediate class, so inherited constructors and destructors also fall back.

Candidate, baseline and reference PHP produce checksum `17000000` for the
million-object target. They also produce identical complete payloads for the
order corpus, ledger corpus and routing holdout before timing.

Separate, untimed `vm-stats` builds report:

| Target counter | Baseline | Candidate |
| --- | ---: | ---: |
| Declared object owner allocations | 1,000,000 | 33 |
| Declared property-storage allocations | 2 | 2 |
| Declared property-storage reuses | 999,998 | 31 |
| Declared property-storage returns | 999,999 | 32 |
| Canonical `NewObj` executions | 1,000,000 | 33 |
| Canonical `FetchObjR` executions | 1,000,000 | 33 |
| Quick-loop entries/completions | 0 / 0 | 1 / 1 |
| Quick-loop iterations | 0 | 999,967 |
| Quick-loop guard failures/deoptimizations | 0 / 0 | 0 / 0 |

The candidate owner and opcode counts are identical on ARM64 and x86-64. The
33 owners are the bounded admission warm-up, not a per-iteration residual
allocation. Instrumented binaries were never timed.

One destructor-loop reproducer exposed a pre-existing baseline compatibility
gap: repeated replacement currently observes only the final local destructor,
unlike reference PHP. This optimization does not encode that behavior and its
runtime resolver always rejects a destructor. The reproducer remains a
compatibility handoff rather than a performance expectation.

## Benchmark method

Both sides were built from clean source snapshots with Rust 1.93.1,
`max-perf` (fat LTO, one codegen unit), default `quick-loops`, and
`RUSTFLAGS=-C target-cpu=native` in separate task-scoped targets. The timed
value is each workload's internal `microtime(true)` interval, excluding parse
and process startup. Every workload used eight alternating warm-up pairs and
100 measured alternating pairs. The ledger workload aggregates two executions
per side per pair. All 100 valid pairs were retained; no outlier was removed.
Delta is the balanced mean of candidate/baseline median ratios for
candidate-first and baseline-first pairs. Spread is nearest-rank p10-p90.

ARM64 used an Apple M4 with 24 GiB memory, macOS 26.5.2, AC power and no CPU
affinity. X86-64 used an AMD Ryzen 9 7950X with 31,985,372 KiB memory,
Linux 7.0.0-28, the `performance` governor and CPU 2 affinity. The complete
x86-64 build, timing and JIT-counter cycle held the exclusive project lock.

Reference output used PHP 8.5.9 NTS/Homebrew on ARM64 and PHP 8.4.24
NTS/Ubuntu with Zend OPcache 8.4.24 on x86-64; CLI JIT was disabled on both.
The ARM64 CLI loaded `bcmath`, `bz2`, `calendar`, `Core`, `ctype`, `curl`, `date`,
`dba`, `dom`, `exif`, `FFI`, `fileinfo`, `filter`, `ftp`, `gd`, `gettext`,
`gmp`, `hash`, `iconv`, `intl`, `json`, `ldap`, `lexbor`, `libxml`,
`mbstring`, `mysqli`, `mysqlnd`, `odbc`, `openssl`, `pcntl`, `pcre`, `PDO`,
`pdo_dblib`, `pdo_mysql`, `PDO_ODBC`, `pdo_pgsql`, `pdo_sqlite`, `pgsql`,
`Phar`, `posix`, `random`, `readline`, `Reflection`, `session`, `shmop`,
`SimpleXML`, `snmp`, `soap`, `sockets`, `sodium`, `SPL`, `sqlite3`,
`standard`, `sysvmsg`, `sysvsem`, `sysvshm`, `tidy`, `tokenizer`, `uri`,
`xml`, `xmlreader`, `xmlwriter`, `xsl`, Zend OPcache, `zip` and `zlib` from
its single CLI ini. The x86-64 CLI loaded `calendar`, `Core`, `ctype`, `date`,
`exif`, `FFI`, `fileinfo`, `filter`, `ftp`, `gettext`, `hash`, `iconv`,
`json`, `libxml`, `openssl`, `pcntl`, `pcre`, `PDO`, `Phar`, `posix`,
`random`, `readline`, `Reflection`, `session`, `shmop`, `sockets`, `sodium`,
`SPL`, `standard`, `sysvmsg`, `sysvsem`, `sysvshm`, `tokenizer`, Zend OPcache
and `zlib` from the distribution CLI ini set.

These results cover only the named supported workloads under these
configurations; they are not a general RPHP-versus-PHP claim.

## Final results

Times are median milliseconds with p10-p90. Negative delta favors the
candidate.

| ARM64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Declared-object lifecycle | 65.466 (63.512-67.349) | 5.588 (5.418-5.847) | -91.440% |
| Order corpus | 61.875 (61.260-62.802) | 60.581 (59.945-61.484) | -2.140% |
| Ledger corpus | 45.072 (44.386-45.517) | 44.018 (43.353-44.547) | -2.333% |
| Routing holdout | 61.536 (61.131-62.006) | 61.335 (61.045-61.616) | -0.336% |

| X86-64 workload | Baseline | Candidate | Balanced delta |
| --- | ---: | ---: | ---: |
| Declared-object lifecycle | 71.699 (70.527-78.066) | 9.470 (9.249-10.215) | -86.786% |
| Order corpus | 72.428 (70.722-75.756) | 71.582 (70.101-74.790) | -1.022% |
| Ledger corpus | 54.296 (53.529-58.221) | 54.220 (53.431-56.329) | -0.145% |
| Routing holdout | 68.241 (67.535-72.617) | 68.553 (67.838-70.904) | +0.581% |

Ten alternating ARM64 observations put median maximum RSS at 4,644,864 bytes
for baseline and 4,734,976 bytes for candidate (+88 KiB), while the OS peak
memory-footprint metric moved from 2,105,680 to 2,081,104 bytes (-24 KiB).
Those opposite process/page-level signals are too coarse for an ownership
count claim; steady-state object elimination is therefore supported by exact
allocation telemetry, not presented as a peak-RSS improvement. The operation
and read arrays are bounded and add no per-iteration retained state.

The ARM64 binary grew from 4,397,424 to 4,397,520 bytes (+96 B), while
`__text` grew by 5,200 B; clean builds took 35.99 and 35.34 seconds. The
x86-64 binary grew from 5,145,600 to 5,152,496 bytes (+6,896 B), with `.text`
up 6,352 B; clean builds took 42.35 and 42.10 seconds. Build times, code size
and process memory are single-checkpoint observations, not broader performance
claims.

## Gates and limitations

Final validation completed with:

- exact baseline/candidate/reference-PHP payload validation for the target and
  all three controls;
- focused admission, escape, reference, magic, constructor, type and exact-exit
  tests;
- complete default, no-default, erased-generics, reified-generics and
  all-feature test matrices after the compatibility rebase;
- explicit non-generic `jit-prototype` execution on ARM64 and x86-64;
- `cargo fmt --all -- --check`, all-feature/all-target checks, shell syntax,
  diff checks and unsafe-policy enforcement; and
- local and private-host cleanup hooks, with the x86-64 lock released and its
  task-scoped source, build and timing artifacts removed.

The optimization is deliberately narrow. It accepts only zero-argument literal
construction, a dead local, at most eight immediate reads and exact non-reference
`Long` defaults. It does not virtualize general object identity, object writes,
constructors, destructors, magic access, dynamic properties, generic classes,
escaping objects or simultaneous live-object graphs. Raw samples and timing
logs remain outside the repository because diagnostic artifacts can contain
local paths and addresses.

## Reproduction

Build clean baseline and candidate snapshots with the configuration above,
then run the target and each control through the runtime gate:

```sh
RPHP_RUNTIME_GATE_PAIRS=100 \
RPHP_RUNTIME_GATE_WARMUPS=8 \
RPHP_RUNTIME_GATE_ONLY=bench_declared_object_lifecycle.php \
RPHP_RUNTIME_GATE_CANDIDATE_BINARY=/tmp/rphp-candidate-virtual-current/max-perf/rphp \
RPHP_RUNTIME_GATE_BASELINE_BINARY=/tmp/rphp-candidate-virtual-baseline/max-perf/rphp \
benches/run_runtime_gate.sh CANDIDATE_ROOT BASELINE_ROOT
```

Repeat with `corpus_order_pipeline.php`, `corpus_ledger_pipeline.php` and
`holdout_routing_pipeline.php`. On Linux, pass the isolated CPU number as the
third argument. Enable `vm-stats` only for counter validation, never for
timing, and run `scripts/cleanup-builds.sh` before and after the cycle.
