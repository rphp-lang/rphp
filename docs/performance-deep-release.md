# Bounded release and PHPStan repair

Status: paused by user request for a hardware change. This local checkpoint is
not accepted or merged. The current Rust candidate combines bounded storage
release, correct VM page restoration and disabled-GC storage sweeps. The default
suite passed 7,089 tests (15 ignored); the no-default suite was interrupted,
and the remaining feature/all-target and exact-current-source sanitizer gates
have not completed. Two regressions remain in the independent 23-case timing
confirmation; this is not an all-workloads speedup claim.

The user explicitly requested a checkpoint commit before powering off. All
expensive jobs were stopped as whole process groups. The final corpus rerun is
complete, but its independent confirmation remains pending. After the hardware
change, repeat timing for both immutable baseline and candidate together; do not
compare one side measured before the change with the other measured afterward.

## Current candidate

The executable SHA-256 is
`e922e13dfbe4c441e6f4d2282e9b5878d6e1ac804aadaf3a5afd11f39dd033ca`;
its source/test/Cargo fingerprint is
`b7965c1c1dfad4299f3b03e074e03490640d41af1a6969ecf6c7a2736f78f940`.
It uses default features and `max-perf`, with the same toolchain and flags as
the integrated-heap baseline. The cold `VmStack::extend` boundary restores the
dispatcher's original native stack reservation without changing ExecuteData.

Seven interleaved, output-validated rounds use a fresh PHPStan result cache for
each cold request and reuse it for the warm request. Filesystem caches are warm.
Self-restart and process workers are disabled and checked in all three runtimes.
The previous RPHP executable contains the other repairs but no disabled-GC
storage sweep; the original integrated baseline does not complete this input.

| Five-file PHPStan input | RPHP before GC sweep | Current RPHP | PHP |
| --- | ---: | ---: | ---: |
| Boot `--version` | 0.916 s | 0.918 s | 0.223 s |
| Cold analysis | 16.702 s | 16.719 s | 1.486 s |
| Warm analysis | 1.230 s | 1.238 s | 0.264 s |
| Cold peak RSS | 1,527.3 MiB | 1,045.2 MiB | 173.1 MiB |

This five-file input has 1,161 source lines. The earlier roughly three-second
allocator comparison used one 234-byte source file, so comparing those raw
times would confuse different workloads with a runtime regression.

The GC storage repair reduces peak RSS by 31.6%; the 0.1% cold-time change does
not establish a CPU improvement. Current cold user/system CPU medians are
16.13/0.53 seconds versus PHP's 1.36/0.12 seconds. This remaining roughly 11.3x
gap is computation, not a child process waiting to terminate. Both runtimes
return the same expected findings and exit 1. A separate small control improves
from 2.905 to 2.801 seconds cold against the integrated baseline, with RSS
falling from 723.6 to 668.4 MiB.

Removing repeated graph prepasses changes an 8,192-object release from a
0.980-second median to 0.0145 seconds, about 67.7x, with matching callback count
and order. Three retained samples per size follow one validated warm-up.
This is a deep-release result, not an overall application speedup.

Executable text grows from 19,417,449 to 19,424,709 bytes (+0.037%). The main
dispatcher shrinks from 336,687 to 336,589 bytes; it still moves within the
executable. Neither its size nor its location proves a timing explanation.

The 11-round confirmation retains all samples from 18 prior corpus suspects
plus five fixed controls. Scalar recurrence is 4.93% slower and strict scalar
function calls are 1.91% slower, with non-overlapping observed p10/p90 ranges.
These ranges describe dispersion, not confidence intervals. Reduced arithmetic
and strict-call instruction diagnostics are effectively unchanged: respectively
170,182,024 versus 170,181,740 and 39,818,705 versus 39,819,313 instructions.
Timing alone does not identify an added per-call operation or prove a particular
cache or branch-prediction cause. The fresh five-round corpus validates all 103
inputs against PHP and retains 1,030 timing samples. Its geometric mean of
per-case median ratios is 1.0082; fourteen cases meet the predeclared suspicion
rule. A separate confirmation includes all fourteen, the two previously
confirmed cases and five fixed controls. The integration decision remains
pending; a pooled average cannot waive individual regressions.

## Checkpoint contract

- Outcome: remove quadratic final-owner release work and make the affected
  PHPStan input terminate with PHP-equivalent output, preserving destructors,
  exceptions, references, resurrection, weak references and Fiber behavior.
- Baseline: clean `ceddd97b`, including the accepted Rust-only heap. Its exact
  default-feature `max-perf` executable has SHA-256
  `9360810eb8da9151d28befa8c7c054e08d69c57a90ac554be365ed407033fa4a`.
- Evidence: doubling a destructor chain from 512 to 1,024 quadrupled graph
  marking work. The five-file PHPStan input exceeded a 120-second validation
  limit while actively executing runtime code.
- Scope: final-owner storage release, bounded disabled-GC weak-root bookkeeping,
  nested-reference iterator cursor repair,
  canonical interface lookup, VM stack-page bounds/reuse and request-final
  destructor trace origins. One isolated checkout owns this checkpoint.
- Gates: reference-PHP output and lifecycle regressions, all five Cargo feature
  configurations, all-target compilation, formatting, unchanged unsafe policy,
  native A/B distributions, RSS and sanitizer validation.
- Stop conditions: reject semantic or stack-safety regressions; investigate
  repeatable performance regressions before acceptance.

All expensive jobs now verify a separate aggregate 6 GiB memory boundary, zero
swap and whole-group OOM/timeout termination. Builds, tests and timings run
sequentially under the exclusive benchmark lock and use the cleanup hook.
Private paths, raw diagnostics and third-party sources are not published.
No ARM64 or remote-host result is claimed.

## Causes and implementation

The final object-release paths and closure destructor repeatedly traversed the
entire retained ownership graph before retiring storage. Descendants repeated
that work. Those prepasses are removed. The existing `stacker` dependency now
protects actual recursive object/array/closure storage retirement, requiring
64 KiB headroom and entering a 1 MiB segment only when necessary. The array
check lives in `PhpArray::drop`, preserving the ordinary inline `Value`/reference
count decrement path. PHP callback phases, weak notifications, handle retirement
order and exceptional JSON checkpoints remain intact.

The larger project independently exposed a nested-foreach bug: comparing two
absent reference identities treated unrelated iterators as aliases. An inner
unset then rewound its outer iterator indefinitely. Cursor repair now requires
a present matching reference or array identity. A bounded regression covers
ordinary/generator frames and three array widths.

Reflection's interface predicate used a raw class-table lookup. It now uses the
canonical lookup, matching qualified names, case and aliases. A regression
covers these forms and inherited interfaces.

AddressSanitizer exposed a pre-existing VM-stack bug: popping across a page
boundary restored `top` but retained a different allocation's `end`. The next
push could subtract unrelated pointers and write past the page. Pop now restores
the owning page and bounds. Bidirectional links retain later pages for reuse;
oversized pushes skip inadequate retained pages. Accounting and destruction
visit the entire chain. Alignment assertions, native full/deferred stack tests
and a PHP cross-page exception/unwind regression cover the geometry and reuse.
The current implementation removes a duplicate page-size field: the allocation charge
already records the immutable size, even before request-budget adoption. This
restores the original 32-byte page header on the measured 64-bit host; a focused
test verifies retained-page accounting, adoption, reuse and final release.

Request-final cyclic destructors now publish the still-live root through the
existing internal-callback boundary. This restores PHP's live and stored trace
entries after normal completion, direct/callback exit and shutdown callbacks.
This trace-origin repair is distinct from the diagnostic recursion below.

## Diagnostic failure and memory containment

An ASan diagnostic deliberately restored the old interface predicate to trigger
the earlier error path. After repairing the VM-stack bounds, that old semantic
bug caused unbounded PHP recursion: `Hoa\\Event\\Event::register` rejected an
exception object as an event source, constructed another
`Hoa\\Exception\\Exception`, and its constructor called `register` again.
Repeated Throwable trace snapshots amplified the growing live call chain.
A bounded diagnostic captured 64 distinct frames with alternating calls; this
was recursion through newly allocated frames, not a cycle of frame pointers.

The first such diagnostic had no aggregate memory cap. It reached 25.57 GiB
anonymous resident memory and the global OOM handler killed it. The user-service
journal also records the enclosing desktop application scope failing with
`oom-kill`; it does not identify a direct kernel kill of the desktop process.
Subsequent diagnostic limits prevented
another host-wide exhaustion. OOM and timeout runs are recorded as failures.

The deliberately regressed diagnostic cannot serve as a passing oracle: its
interface predicate recreates the recursion. Final ASan validation instead
checks byte-for-byte runtime sources from the actual candidate against reference
PHP output. The independently reproduced stack-page bounds bug also has a small
sanitizer regression. No production recursion/trace limit was added to conceal
the underlying behavior, and no global security or memory setting was changed.

## Rejected variants and limits

The first complete candidate passed output checks on 103 corpus inputs, but an
independent 11-round run confirmed five 2–7% timing regressions. Its extra branch
in the common `Value::drop` array arm was rejected; protection moved to the actual
array destructor. A later intermediate candidate removed most regressions but
retained a roughly 9% arithmetic-loop regression. Baseline/candidate work on a
100,000-iteration diagnostic was almost identical (170,179,767 versus 170,181,075
instructions), while the dispatch function changed layout. This supports a code
generation/placement investigation, not an inference of added allocation work
from timing alone. Final source changes require fresh measurements.

The isolated single-unsigned-range pop change also retains nine confirmed
regressions in the same 23-case/11-round confirmation and is rejected as a
complete remedy. Its optimized dispatcher has exactly the same normalized
instruction sequence as the direct-bounds variant. Compared with the original
baseline, page growth now emits three outlined extension calls; two receiver
addresses are hoisted into dispatcher storage and native stack reservation
grows by 16 bytes. These static observations guide code-generation experiments,
but do not by themselves prove the timing cause. The page-header compaction is
measured separately, preserving both rejected executables and source snapshots.

A destructor that constructs a nested callback-bearing graph and then throws
already omits its nested leaf callback on the baseline, whereas PHP runs it.
That exception-release scheduling gap remains outside this storage repair. The
throwing-destructor regression uses an ordinary deep payload; successful,
resurrected, aliased and suspended cases independently verify callback order.

## Historical direct-bounds candidate validation

The measured default-feature `max-perf` executable has SHA-256
`3ee100c7507c8c682f04cf01701f161d3ea2e90470e11f3b42b0cf93f35b6e61`.
Its source/test/Cargo fingerprint is
`9a62367648b7cb0aa474c1eaca7edcba7307ad1283889f320cd2599689241305`,
identical to the full-matrix fingerprint. The package was explicitly rebuilt
to avoid stale artifacts from diagnostic snapshots. Executable size changes
from 21,444,264 to 21,458,440 bytes; text grows 13,025 bytes, about 0.067%.

The `test-fast` matrix, with assertions and overflow checks enabled, passes:

| Configuration | Passed | Ignored |
| --- | ---: | ---: |
| Default | 7,087 | 15 |
| No default features | 6,736 | 15 |
| Erased generics | 7,158 | 15 |
| Reified generics | 7,180 | 15 |
| All features | 7,234 | 18 |

That is 35,395 successful test executions. Ignored tests are not passing
evidence. The locked, offline all-feature/all-target check also passes.
Builds and test runners use four workers. Network integration tests ran on the
ordinary host inside the same aggregate resource boundary.

ASan with the candidate's byte-identical runtime sources completes the full
five-file PHPStan analysis, matching reference stdout, stderr and exit status.
Its sampled peak RSS is 3.33 GiB. The reference and candidate both exit 1 for
the same expected static-analysis findings; this is not a claim that those
input files have zero findings. ASan uses the system allocator fallback and
`detect_leaks=0:abort_on_error=1`: it supplies an invalid-access check, not a
leak-sanitizer result or a native timing. The independent cross-page stack
reproduction also passes.

Native deep-release medians below retain three samples per runtime/size after
one warm-up. Counts and callback order agree with PHP on every run.

| Objects | Baseline release | Candidate release | Speedup |
| ---: | ---: | ---: | ---: |
| 512 | 4.574 ms | 0.749 ms | 6.1x |
| 1,024 | 16.480 ms | 1.642 ms | 10.0x |
| 2,048 | 62.019 ms | 3.263 ms | 19.0x |
| 4,096 | 240.758 ms | 6.704 ms | 35.9x |
| 8,192 | 955.173 ms | 13.196 ms | 72.4x |

**Protocol correction:** the PHP results in this historical table allowed a
PHPStan self-restart. Its `TurboProcessRestarter` re-executes PHP with CLI
OPcache enabled and does not preserve the original disabled-function flags.
The startup configuration probe therefore did not describe the running analysis.
These samples remain recorded, but the earlier same-configuration/serial
comparison and 6.6x attribution are withdrawn pending the corrected benchmark.
The runner now disables `pcntl_exec` and `pcntl_fork` as well as `proc_open` and
`pcntl_signal`, validates that all four functions are unavailable, and records
the actual startup flags. No application binary or PHAR is patched.

Seven shuffled PHPStan rounds after one validated warm-up cycle produced these
process-wall medians. Every boot, cold and warm output/exit matches PHP, and the
warm result cache is checked before accepting a sample.

| Input/runtime | Boot | Cold analysis | Warm analysis | Cold RSS | Warm RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Small control, baseline | 0.852 s | 2.879 s | 1.061 s | 723.9 MiB | 347.8 MiB |
| Small control, candidate | 0.850 s | 2.785 s | 1.062 s | 723.6 MiB | 347.7 MiB |
| Small control, PHP | 0.493 s | 1.411 s | 0.537 s | 148.5 MiB | 132.9 MiB |
| Five-file input, candidate | 0.857 s | 15.205 s | 1.137 s | 1,528.4 MiB | 351.2 MiB |
| Five-file input, PHP | 0.489 s | 2.316 s | 0.552 s | 195.0 MiB | 132.7 MiB |

The small control improves 3.3% cold over the already integrated Rust heap;
warm time is effectively unchanged. The five-file input now terminates, but
remains 6.6x slower than PHP cold and 2.1x warm, with substantially higher RSS.
That input comprises 1,161 lines from five public PHPT helper files, analyzed
at level 5, not a full application. Its twenty expected findings are identical
across reference and candidate. Neither the earlier timeout nor results from
different build profiles are used as successful comparison samples.

The preselected eleven-round confirmation covers fourteen ordinary workloads.
Its arithmetic assignment loop changes from 0.6230 s to 0.6697 s (+7.5%), with
non-overlapping p10/p90 ranges. Other positive median changes in that set have
overlapping ranges, including shallow object lifecycle (+4.0%) and typed
construction (+1.5%). This is not a claim of universally improved performance.

The full 103-input corpus passes every PHP output check and completes five
interleaved rounds (1,030 retained samples). It identifies eighteen inputs
whose median is more than 1% slower with non-overlapping p10/p90 ranges,
including scalar recurrences, ordinary calls and property reads. These are
not covered by the earlier fourteen-case confirmation alone. An independent
eleven-round run of all eighteen suspects plus five controls confirmed nine
regressions with non-overlapping p10/p90 ranges: arithmetic assignment (+7.9%),
three property-read cases (+3.8–6.9%), untyped scalar calls (+5.9%), variadic
generic calls (+5.6%), strict calls (+5.4%), typed scalar calls (+5.3%) and
scalar recurrence (+5.2%). This candidate is rejected for integration. The
remaining fourteen cases have overlapping ranges or changes below 1%. All
validated timing samples, including unfavorable ones, are retained.

The final arithmetic diagnostic executes 170,180,368 baseline instructions
and 170,181,321 candidate instructions, a difference below 0.001%. Its ordinary
object-lifecycle control executes 31,207,588 versus 31,201,993 instructions.
The hot dispatch function changes from 336,687 to 336,734 bytes and moves
within the executable. This supports investigating code generation/layout;
it does not prove a specific cache or branch-prediction mechanism. Hardware
counters are unavailable under the host's existing perf policy, which was
not changed.

A complete-request sample points to VM dispatch, cycle-candidate registration,
hash-table work, temporary-value cleanup and string-memory accounting. The
collector reports that its interval timer changed and its displayed weights
are not calibrated to wall time, so those samples are qualitative leads only.
After removing the collector's own one-line banner, program stdout and stderr
match PHP exactly. An independent completed Callgrind run now counts 154,894,701,811 instructions
for the same cold request and matches reference output and exit status. The
instrumented 650-second duration is not a native timing. Self instruction
counts identify these costs without double-counting callees:

| Function / operation | Instructions | Share | Observed calls |
| --- | ---: | ---: | ---: |
| Largest VM dispatch context | 30.076 billion | 19.42% | — |
| Register cycle candidate | 7.332 billion | 4.73% | 83,911,984 |
| Find class | 5.711 billion | 3.69% | 50,375,557 |
| Release statement temporaries | 5.580 billion | 3.60% | 18,104,155 |
| Plan frame destructor release | 3.516 billion | 2.27% | 9,205,265 |
| Class inheritance predicate | 1.751 billion | 1.13% | 18,289,217 |

Counts refer to out-of-line calls in this optimized executable; inlined work
is included in its caller. The class inheritance predicate makes 36.6 million
of the class-lookup calls. Value destruction and temporary cleanup make most
cycle-registration calls. These counts identify repeated runtime bookkeeping;
they do not establish that each call can be removed without changing PHP
semantics. Recursive inclusive costs overlap and must not be summed. Likewise,
the small cost of explicitly named heap slow paths cannot quantify total
allocation cost because allocator fast paths are inlined.

Native cold medians contain 14.57 seconds of user CPU and 0.61 seconds of system
CPU, compared with 1.94 and 0.27 seconds for PHP. Waiting for process termination
therefore does not explain this gap. The initial PHP configuration probe reported CLI OPcache disabled, but the
subsequent application restart enabled it. The corrected protocol must be used
for any controlled cross-runtime comparison. The 15.205-second cold result remains an open performance problem,
not an accepted endpoint.

The corrected reference PHP instruction profile completes with matching output
and counts 15,582,023,381 instructions. The previously profiled repaired RPHP
executable counted 154,894,701,811 on the same input, about 9.9 times as many.
Their executable identities and process settings are retained separately.
The first unrestricted PHP attempt re-executed outside the profiler and produced
an empty recording; no instruction count is claimed from that failed attempt.

The corrected seven-round serial protocol confirms all four process functions
are unavailable. With the cold-growth executable, the five-file input measures
16.648 seconds cold and 1.235 seconds warm, versus PHP's 1.470 and 0.265 seconds;
RSS is 1,527.5 versus 172.4 MiB cold. This controlled comparison is about 11.3x
cold, so the restart mistake concealed part of the runtime gap. These medians
come from a new interleaved series; a change relative to an older series is not
by itself evidence of a candidate regression. The small same-series RPHP
control remains about 3.2% faster than the integrated heap baseline cold.

PHPStan disables automatic GC at startup. The initialized root buffer previously
pruned dead weak entries at automatic admission or explicit status/memory
queries, so a disabled collector could retain the Rc allocations of already-dead
values. A 300,000-object alias/release reproduction uses 102.8 MiB and 0.2982
seconds; an explicit memory query every 4,096 iterations drops these to 48.6 MiB
and 0.2768 seconds. PHP's RSS stays around 19 MiB. The current implementation
prunes only dead records during disabled-GC admission, keeps live roots in
insertion order, and doubles the next scan threshold past the surviving roots.
It neither runs destructors nor collects live cycles. Native storage bounds and
explicit collection/destructor-order tests pass, and whole-request measurement
confirms the RSS reduction recorded above. Startup-disabled, uninitialized root
storage is a separate path.

The earlier direct-bounds native cycle peaks at 1.60 GiB aggregate resident/accounted memory
under the 6 GiB boundary, with zero memory-limit and OOM events. Adding memory
capacity is therefore not supported as a remedy for this measured runtime gap.

## Reproduction and interpretation

The [evidence directory](performance-deep-release-evidence/README.md) records
all retained samples, hashes and settings. Native comparisons use the same
default-feature `max-perf` profile, four build jobs, no incremental compilation
and the repository's function-alignment flags. Cold PHPStan runs use a fresh
result-cache directory with warm OS filesystem caches. Both runtimes disable
`proc_open`, `pcntl_signal`, `pcntl_exec` and `pcntl_fork`, so these are serial runs; no parallel-worker
performance claim follows from them.

Run each complete command sequence under the verified systemd boundary in
[`benchmarking.md`](benchmarking.md), retaining the exclusive benchmark lock.
With exact baseline/current executables and the recorded PHAR/project:

```sh
flock -x /tmp/rphp-benchmark.lock python3 scripts/bench-phpstan.py \
  --variant "baseline=$BASELINE_BINARY" --variant "candidate=$CANDIDATE_BINARY" \
  --phar "$PHPSTAN_PHAR" --project "$PHPSTAN_PROJECT" \
  --rounds 7 --seed 280941 --output "$FRESH_REPORT_JSON"

"$CANDIDATE_BINARY" benches/bench_deep_object_destructor_release.php 8192
```

For the larger input, omit the baseline variant: its known invalid result and
120-second timeout are retained in the prior checkpoint, not counted as a
timing sample or divided into a speedup. The public source identities of this
five-file input are recorded separately. Quantiles use linear interpolation;
p10/p90 ranges describe observed dispersion, not statistical confidence.

The matrix uses `cargo test --offline --locked --profile test-fast
--no-fail-fast` with default, no-default, erased, reified and all features.
Set `CARGO_PROFILE_TEST_FAST_DEBUG_ASSERTIONS=true`,
`CARGO_PROFILE_TEST_FAST_OVERFLOW_CHECKS=true`, `CARGO_BUILD_JOBS=4` and
`RUST_TEST_THREADS=4`. All-target compilation uses `cargo check --offline
--locked --profile test-fast --all-features --all-targets`.
