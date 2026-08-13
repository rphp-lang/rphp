# Execution coverage scorecard

Status: measured at `0659191621748143ae258e36a4e20890b83451cf`, 2026-08-13

This report freezes the current baseline, typed-region and native-JIT evidence
for the representative corpus and its independent routing holdout. It is a
selection checkpoint: it identifies the next general optimization boundary but
does not implement that optimization.

## Checkpoint contract

- **Outcome:** one reproducible scorecard, one capability matrix and one
  evidence-ranked next execution checkpoint.
- **Baseline:** clean detached worktree at `0659191` before report work. The
  measured canonical lane uses the default `max-perf` binary with
  `RPHP_DISABLE_QUICK_LOOPS=1`, keeping toolchain, code layout and unrelated
  runtime fast paths equal to the typed lane.
- **Proof:** exact output comparison across three RPHP lanes and PHP with and
  without tracing JIT; 15 retained, order-rotated internal-time samples per
  lane; `vm-stats` coverage; and two-second CPU samples after scaling only the
  streamed iteration count.
- **Acceptance:** every output agrees; coverage counters reconcile; the chosen
  gap is general, structurally selected and reproduced outside the design
  corpus; no runtime or semantic source is changed.
- **Stop rule:** do not select a new recognizer or optimize an anonymous native
  address without an operation-level coverage counter and an independent
  workload that reproduces the boundary.

## Environment and method

Measurements used an Apple M4 MacBook Air (`Mac16,13`, 24 GB), macOS 26.5.2,
ARM64, Rust/Cargo 1.93.1, and Homebrew PHP 8.5.9. RPHP builds used the
`max-perf` profile, fat LTO, one codegen unit and
`RUSTFLAGS=-C target-cpu=native`. The canonical and typed binaries used the
default `quick-loops` feature; canonical execution disabled its planner at
runtime. The native binary added `jit-prototype`. Diagnostic counters came
from separate `vm-stats` builds and were never used for timing.

PHP no-JIT used `php -n`. PHP JIT loaded
`/opt/homebrew/etc/php/8.5/php.ini` with no additional ini files, then set
`opcache.enable_cli=1`, a 100 MB JIT buffer and tracing mode.
`opcache_get_status(false)` confirmed that PHP JIT was on. Both PHP lanes
loaded the same compiled module set: bcmath, bz2, calendar, Core, ctype, curl,
date, dba, dom, exif, FFI, fileinfo, filter, ftp, gd, gettext, gmp, hash,
iconv, intl, json, ldap, lexbor, libxml, mbstring, mysqli, mysqlnd, odbc,
openssl, pcntl, pcre, PDO, pdo_dblib, pdo_mysql, PDO_ODBC, pdo_pgsql,
pdo_sqlite, pgsql, Phar, posix, random, readline, Reflection, session, shmop,
SimpleXML, snmp, soap, sockets, sodium, SPL, sqlite3, standard, sysvmsg,
sysvsem, sysvshm, tidy, tokenizer, uri, xml, xmlreader, xmlwriter, XSL,
Zend OPcache, zip and zlib.

The timed value comes from `microtime(true)` inside each PHP workload, so it
excludes parsing and process startup. It does include each fresh process's
in-workload region admission and native/PHP JIT compilation. The warm-up policy
is one untimed output-validation execution per mode and workload; no warmed
process is reused for a measured sample. Each row retains all 15 samples with
no outlier removal. The five execution modes rotate their order each round.
The spread is nearest-rank p10–p90. Before timing, all five modes were required
to produce the same result. Workloads use only the fixed inputs declared in
their named repository source files.

The repository workload ABI currently exposes elapsed seconds through
`microtime(true)`, which is precise wall time but not monotonic. The narrow
distributions and large coverage deltas make it adequate for this selection
checkpoint, but it does not close roadmap gate M0; a release gate must migrate
the common timer to `hrtime(true)` or an equivalent monotonic clock and rerun
both architectures.

The diagnostic `vm-stats` build is not timed. CPU sampling uses the same
workload source streamed to RPHP with only its final iteration argument scaled
long enough to collect samples; no workload file or runtime source is changed.
Runs were sequential, with no CPU affinity, timeout or explicit power-mode
override. Low Power Mode was off during the completion audit. Peak RSS is one
fresh-process diagnostic observation per lane, not a timing distribution.
An audit smoke that timed immediately after four release builds was thermally
distorted and is excluded from every timing table. The reproduction runner now
keeps instrumented builds after all timing and declares a 60-second post-build
cooldown.

The private x86-64 benchmark host was not configured in this task. Current
x86-64 source and focused tests are inventoried below, but x86-64 runtime and
performance cells remain explicitly pending a fresh host run.

Exact validated result payloads (the elapsed suffix is excluded):

| Workload | Expected payload |
| --- | --- |
| Order pipeline and typed variant | `9895778000,1327440292,11223218292,210000` |
| Ledger pipeline and typed variant | `500000,7981250000,280500000,182500` |
| Routing holdout | `290394364,154183816,54660174,384960,192495,64134,108411` |

## Current performance scorecard

Times are median milliseconds with p10–p90 in parentheses.

| Workload | Canonical/quick-off | Typed region | ARM64 JIT | PHP no JIT | PHP tracing JIT | Typed gain | JIT gain over typed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Order pipeline | 226.954 (224.345–229.658) | 65.160 (64.710–65.860) | 4.164 (4.102–4.281) | 93.556 (92.200–94.364) | 55.696 (55.156–56.991) | 3.48x | 15.65x |
| Typed order pipeline | 264.372 (260.637–270.863) | 85.341 (84.641–86.903) | 4.188 (4.146–4.487) | 97.628 (96.446–98.957) | 57.597 (57.259–59.532) | 3.10x | 20.38x |
| Ledger pipeline | 51.355 (50.187–52.498) | 23.836 (23.167–24.135) | 2.396 (2.281–2.561) | 29.619 (29.080–30.131) | 9.717 (9.440–9.958) | 2.15x | 9.95x |
| Typed ledger pipeline | 51.768 (50.463–52.244) | 23.843 (23.543–24.452) | 2.359 (2.255–2.611) | 30.686 (29.931–31.630) | 10.245 (9.816–10.503) | 2.17x | 10.11x |
| Routing holdout | 193.593 (191.417–199.267) | 64.995 (63.584–65.160) | 6.477 (6.183–7.104) | 68.703 (67.344–70.508) | 29.283 (28.678–31.278) | 2.98x | 10.03x |

These numbers cover only the named supported workloads under the stated
configuration. They are not a claim about PHP applications generally.

## Runtime traffic, footprint and build envelope

The following diagnostic counters are from separate `vm-stats` candidates.
Each cell is `frame-slot writes / Value clones / Value drops` for one complete
workload. They count runtime operations, not allocated bytes.

| Workload | Canonical/quick-off | Typed region | ARM64 JIT |
| --- | ---: | ---: | ---: |
| Order pipeline | 3,710,022 / 8,710,191 / 9,500,052 | 93 / 273 / 204 | 93 / 273 / 204 |
| Typed order pipeline | 3,710,022 / 8,710,191 / 9,500,052 | 93 / 273 / 204 | 93 / 273 / 204 |
| Ledger pipeline | 2,182,520 / 2,182,687 / 500,046 | 152 / 319 / 79 | 152 / 319 / 79 |
| Typed ledger pipeline | 2,182,520 / 2,182,687 / 500,046 | 152 / 319 / 79 | 152 / 319 / 79 |
| Routing holdout | 4,608,425 / 7,391,779 / 4,283,244 | 218 / 515 / 253 | 218 / 515 / 253 |

Frame creation and cleanup remain cold and identical across lanes: 9 calls for
the order pair, 7 for the ledger pair and 6 for routing. The large canonical
traffic reduction therefore reconciles with typed-region completion rather
than hiding a changed call count.

One uninstrumented fresh-process peak-RSS observation, in MiB:

| Workload | Canonical/quick-off | Typed region | ARM64 JIT |
| --- | ---: | ---: | ---: |
| Order pipeline | 4.672 | 4.891 | 5.266 |
| Typed order pipeline | 4.719 | 4.875 | 5.250 |
| Ledger pipeline | 4.578 | 4.766 | 5.047 |
| Typed ledger pipeline | 4.531 | 4.766 | 5.016 |
| Routing holdout | 4.625 | 4.828 | 5.234 |

From empty task-scoped targets, the completion-audit smoke build took 53 s for
typed and 61 s for JIT. Their stripped-on-disk `rphp` binaries were respectively
4,287,312 and 4,517,344 bytes; enabling JIT adds 230,032 bytes (5.37%). These
single cold-build observations include dependencies and are an envelope, not a
compile-time performance claim. Generated-region code/cache bytes, allocator
calls and COW detachments are not exposed by current telemetry; those cells are
**unmeasured, not zero**, and must be instrumented before a checkpoint makes a
claim about them.

## Execution-weighted coverage

Every corpus/holdout workload has one hot backward loop. All five are admitted
as the common `typed_ops_loop`, enter it once after 33 baseline backedges,
complete without a guard failure or deoptimization, and execute it natively on
ARM64 without a side exit.

| Workload | Hot loop candidates | Typed admissions/executions | Optimized iterations | ARM64 native executions | Native side exits | Rejected hot backedge executions | Straight candidates rejected at `no_typed_span` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Order pipeline | 1 | 1 / 1 | 499,967 | 1 | 0 | 0 | 4 |
| Typed order pipeline | 1 | 1 / 1 | 499,967 | 1 | 0 | 0 | 4 |
| Ledger pipeline | 1 | 1 / 1 | 499,967 | 1 | 0 | 0 | 4 |
| Typed ledger pipeline | 1 | 1 / 1 | 499,967 | 1 | 0 | 0 | 4 |
| Routing holdout | 1 | 1 / 1 | 749,967 | 1 | 0 | 0 | 13 |

The straight candidates are static sites outside the admitted closed loop; no
runtime-weighted rejected backedge is attributed to them. They are not the
next hot coverage gap.

## Baseline / typed-region / JIT capability matrix

`fresh` means exercised by this scorecard on ARM64. `tree` means the current IR,
lowering and focused exact-exit tests exist in the tree but this checkpoint did
not execute that cell. `pending` means a required current host run is missing.

| General capability | Baseline tests | Typed IR | Typed executor | ARM64 JIT | x86-64 JIT | Exact-exit tests | Corpus/holdout evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Long arithmetic, loop branches and checked recurrence | tree | fresh | fresh | fresh | tree; host pending | tree | all five |
| Scalar calls, monomorphic methods and declared properties | tree | fresh | fresh | fresh | tree; host pending | tree | all five |
| Virtual constructor → object/array consumer pipeline | tree | fresh | fresh | fresh | tree; host pending | tree | order pair |
| Mixed dynamic-String hash read/modify/write | tree | fresh | fresh | fresh | tree; host pending | tree | routing holdout; isolated control |
| Read-only dynamic-String hash lookup | tree | fresh | fresh | **missing** | **missing** | typed tree | isolated controls only |
| Integer-index array read and structural write | tree | tree | tree | tree | tree; host pending | tree | benchmark matrix only |
| Packed/hash `foreach` Long/property accumulation | tree | tree | tree | missing | missing | typed tree | benchmark matrix only |
| Array append and String append | tree | tree | tree | missing | missing | typed tree | benchmark matrix only |
| Exact Double calls, methods, branches and loops | tree | tree | tree | tree | tree; host pending | tree | benchmark matrix only |
| Invariant JSON projections and callback pipelines | tree | tree | tree | tree | tree; host pending | tree | benchmark matrix only |

The baseline configuration itself has one current build defect:
`cargo build --profile max-perf --no-default-features` fails because
`IntIndexValue::clear_cached_long` is conditionally absent while a canonical
mutation caller remains compiled. The quick-off lane above is therefore the
same-binary canonical comparison, not proof that the standalone no-default
feature gate is green. Fixing that feature-boundary error belongs in a separate
owned correctness checkpoint because `src/value/mod.rs` is shared runtime
state.

## Profiles

The canonical lane is dispatcher-bound: `execute_ex` accounts for 24–30% of
self samples in the order pair, 60–62% in the ledger pair and 45% in the
routing holdout. The typed lane moves that cost into the common region
executor: `run_quick_long_ops_loop` accounts for 25–34% in the order pair,
98% in the ledger pair and 79% in routing. Order additionally spends 14–34% in
the object Long/array plan evaluators; the typed order variant exposes another
12% in class-contract checks.

Profiles used macOS `/usr/bin/sample` for two seconds. Source was sent through
standard input after a `perl -0pe` substitution changed only the final
iteration argument: 10x for canonical and typed profiles, 20x for the canonical
ledger profile, and 1000x for native profiles. This made each process live long
enough to sample without editing a workload. The raw files are intentionally
not retained because they contain local paths and executable addresses.

In the ARM64 JIT lane, 92.2–95.7% of all samples land inside anonymous
executable-memory addresses belonging to the generated region. That agrees
with the native-entry counters and the 10–20x typed-to-JIT improvement. It also
means that a later instruction-scheduling optimization needs native operation
mapping or perf-map-style symbolization; Rust helper names cannot identify it.

## Gap ranking

The corpus and holdout themselves contain no rejected hot backedge: all five
measured loops are already native. The next checkpoint therefore comes from
the highest-confidence general empty cell, not from raw static candidate count.

- Read-only dynamic-String lookup has two fresh 15-sample controls. Both enter
  and complete typed execution, both fail native admission, and the JIT build
  is slightly slower than the typed build. The adjacent update form proves a
  10x-plus native opportunity through the same hash substrate.
- Value-only `foreach` is also not native, but the accepted repository record
  already reduced packed iteration from 11.51 ms to 0.759 ms and hash
  iteration from 14.83 ms to 4.075 ms in the typed executor. Its remaining hash
  delta tracked 40-byte RPHP entries versus 32-byte Zend buckets, so another
  dispatch lowering does not address the measured dominant cost.
- String append and packed array append already use dedicated chunked typed
  kernels. Their remaining work mutates capacity and COW state; selecting them
  without allocator/COW counters would violate the measurement stop rule. A
  documented 200-million-append String control also did not reproduce a
  steady-state kernel regression.
- The 92–96% anonymous native samples in the five main workloads cannot rank a
  native instruction change until generated operations are symbolized.

This ranking rejects the other visible cells for this checkpoint; it does not
declare them permanently low priority.

## Selected next checkpoint

The largest actionable general coverage gap is **read-only dynamic-String hash
lookup in a native typed region**.

Two independent one-million-iteration controls are already admitted and
completed by `typed_ops_loop`, but record no native execution:

| Control | Typed median (p10–p90) | JIT-build median (p10–p90) | PHP no-JIT median (p10–p90) |
| --- | ---: | ---: | ---: |
| Literal-selected changing String key | 12.016 ms (11.505–12.465) | 12.638 ms (12.125–13.157) | 9.531 ms (9.105–10.062) |
| Immutable-CV-selected changing String key | 11.583 ms (11.247–11.753) | 12.083 ms (11.723–12.287) | 9.639 ms (9.422–9.942) |

The cause is structural and visible in `native_mixed_kernel`: a
`FetchArrayLong` with `QuickArrayIndex::ValueSlot` lowers to native `HashLoad`
only when `array_update_fusions[plan_index]` proves an immediately following
arithmetic plus `StoreArrayLong`. A read-only fetch hits that unconditional
fusion requirement and the native builder returns `None`. The nearby mixed
read/modify/write control does enter one native region with zero side exits and
measures 2.412 ms versus 26.743 ms in the typed executor, proving that the
shared String-token, hash context and both backends already have the required
native lookup substrate.

The next optimization checkpoint should therefore:

1. lower a standalone read-only `FetchArrayLong(ValueSlot)` through the same
   bounded String-token and `HashLoad` context without requiring a store;
2. prove it first through the common typed operation and exact missing/non-Long
   side exit, with no allocation or repeated effect;
3. add ARM64 and x86-64 backend/runtime tests from the same native operation;
4. demonstrate native entry and zero side exits on both changing-key controls;
5. expose bounded generated-code/cache size plus allocation or COW-detachment
   evidence for the new lookup context, treating absent counters as unknown;
6. retain the mixed update, corpus and routing-holdout outputs and keep their
   medians inside the one-percent regression gate; and
7. reject the slice if lookup-only native execution does not beat the typed
   executor credibly on both architectures or if context setup outweighs the
   loop benefit.

This is a coverage extension, not a benchmark recognizer: admission depends on
the existing typed `ValueSlot` key, immutable guarded array and exact fetch
contract, never on key text, variable names or workload identity.

Completion note (2026-08-13): this selected checkpoint is now implemented and
verified on ARM64 and physical x86-64. Both isolated controls enter one native
`typed_ops_loop` with zero side exits, exact fallback tests cover invalid
tokens plus missing/non-Long/referenced entries, and the target and regression
distributions pass the dual-architecture gate. The historical selection table
above remains frozen at its named baseline; the current capability row,
methodology and full evidence are recorded in
[`performance-native-string-hash-read.md`](performance-native-string-hash-read.md).

Follow-up completion note (2026-08-13): a fresh dual-architecture scorecard
and profile selected geometric capacity growth inside the already-admitted
packed-array append kernel as the next baseline structural cost. A bounded,
packed-only reserve now improves `bench_array.php` by 4.96% on ARM64 and 0.57%
on physical x86-64 while the application corpora, independent routing holdout
and String/scalar controls remain below the one-percent regression ceiling.
The exact integrated baseline, semantic envelope, rejected code-layout
variants and complete distributions are recorded in
[`performance-packed-array-reserve.md`](performance-packed-array-reserve.md).

Ownership completion note (2026-08-13): the next M1/M5 measurement found that
copying a closure-valued `Value` allocated and cloned its immutable payload and
capture vector. Sharing one owner improves the 250,000-copy target by 22.69%
on ARM64 and 8.84% on physical x86-64, reduces retained-copy peak RSS by 72.23%
and 74.29%, and keeps both application corpora plus the independent routing
holdout below the one-percent regression ceiling after confirmation. The exact
baseline, allocation counters, lifecycle envelope and distributions are in
[`performance-closure-ownership.md`](performance-closure-ownership.md).

## Reproduction

Run the complete timing and telemetry scorecard, including the structural
selection controls, with:

```sh
RPHP_SCORECARD_RUNS=15 benches/run_execution_scorecard.sh
```

The runner invokes the mandatory cleanup hook before and after the cycle,
builds separate task-scoped typed, JIT and stats candidates under the temporary
directory, waits 60 seconds after the two production builds, validates outputs,
prints every retained sample, records peak RSS, and only then builds and runs
the instrumented coverage lanes. Raw macOS `sample` files are deliberately
untracked because they contain local paths and addresses; only the redacted
aggregate evidence above belongs in the public repository.
