# Recovered PHPStan runtime foundation and ordinary call ownership

Status: accepted bounded recovered-foundation checkpoint. The original
simultaneous PHPStan instruction and time parity goal remains open. All final
source gates below pass; older scorecards retain their historical scopes.

## Why the reported count changed from 70G to 160G

Two incompatible comparisons had been mixed. The approximately 70G scorecard
counts the body of `AnalyseApplication::analyse()`. The later 160.185G count
covers the whole process, while its time field still covers analysis only.
That row cannot establish a same-region 70G-to-160G regression.

The isolated integration branch also started from main `41216484`, which lacked
the previously reviewed performance branch. An identical-input audit restores
acknowledged FIFO boundaries for both instruction and time measurements:

| Executable | Analysis instructions | Analysis seconds |
| --- | ---: | ---: |
| Reference PHP | 10.400G | 0.968 |
| Preserved old PGO | 69.940G | 6.934 |
| Preserved old ordinary release | 76.263G | 7.717 |
| Current main ordinary release | 134.799G | 13.626 |
| Preserved dirty measurement prototype | 74.691G | 7.014 |

These are two interleaved rounds per executable, with identical unpacked source,
five input files, fresh result-cache storage and exact PHP output. PGO uses a
different declared profile and is not an equal-profile source comparison.
The dirty prototype is retained evidence, not a production source import.
Older fast executables fail stronger lifecycle checks: 23/29 for old PGO and
25/29 for the dirty prototype. Restoring their speed requires repairing those
observable lifetime defects rather than accepting their binaries as correct.

## Integrated source and ownership boundary

Merge the committed performance history through `c2b7c134` into current main,
preserving its static/instance array RHS re-entry fixes. Restore main's unsafe
ceilings and coordination documents. The source is not a bulk import of the
dirty measurement checkout or a stack of earlier rejected micro candidates.

Ordinary heap arguments and receivers own their values across later argument
evaluation and PHP callbacks. The compiler may transfer a unique consumed TMP
only when its bytecode mentions and independent-entry boundaries prove that
the caller cannot read it again. Clear the caller before publishing the callee
owner. Scalar, reference, COW and native-argument GC behavior stay canonical.

Unentered pending activations and completed internal handlers retire actual
owners before catch selection or result observation. Wide pending frames have
initialized unwritten argument storage. Destructor replacements keep their
previous-exception chain; detached calls preserve any outer pending activation.
Throwing callees publish committed global writes before catch/finally reads.

Surplus user arguments live outside the compiler CV/TMP extent. Retire those
owners after locals on return, and retire local/tail aliases together when an
exception definitively leaves a function. Existing trace owners continue to
retain arguments. Native tail guards do not demand a boolean TMP eliminated by
a proven fused identity branch; both backend lowerings retain the same proof.

## Exact-source verification

Runtime source SHA-256:
`2857a188a70de26a8b7db3a843dfb09836c751898171d6269615462168d0de7f`.
Rust 1.98.1 / LLVM 22.1.8, ordinary release, default features, no PGO, existing
64-byte function alignment. Native performance evidence is x86-64 only; no
ARM64 speed or newly implemented lowering is claimed.

The focused library/adjacent packet passes 1,179 tests. Formatting and unsafe
policy pass at 1,746 blocks / 319 unsafe functions, within unchanged ceilings
1,747 / 321. The five feature configurations pass 36,280 test executions (default 7,264,
no-default 6,913, erased 7,335, reified 7,357, all-features 7,411); all-target
compilation also passes. Exact-source AddressSanitizer passes four consumed-owner
unit checks and 552 integration checks, with system allocation and leak checks
disabled. Thirty-five differential fixtures match PHP in default and actual
forced canonical release/vm-stats execution, seventy observations total.
The final executable is retained separately from test/build caches. Failed or
interrupted gates remain evidence and are never counted as passing observations.

The first complete default configuration retains five failing tests across
three targets. A real surplus-argument destructor omission and three native
tail-guard admission failures are repaired. The sensitive-wrapper test's old
expectation is corrected to PHP's measured order: destruction precedes the
following echo. No failed configuration counts as a passing acceptance gate.

All expensive jobs use an exclusive lock, verified 6 GiB aggregate memory limit,
zero swap and whole-cgroup termination. Cleanup runs before/after builds and
between matrix configurations. No private benchmark host is configured.
Sanitizers use a new empty source-specific Cargo target: reusing artifacts
across copied source directories with old timestamps produced invalid evidence
in an earlier attempt. Those failures remain retained separately.

## Final performance decision

The identical five-file analysis is repeated six times per runtime, alternating
forward/reverse order with fresh equal-length executable copies and fresh cache
storage. Both counter boundaries are acknowledged; analysis instruction and
time scopes exclude startup and teardown. The inherited frozen phase hook
explicitly disables cyclic GC in both PHP and RPHP, as recorded by before/after
state markers. This configuration is identical to the historical 70G audit;
a default-GC application claim requires a separate control. Every exit/stdout/stderr signature
matches PHP. No outlier is excluded.

| Runtime | Median analysis instructions | Median analysis seconds | Analysis seconds range |
| --- | ---: | ---: | ---: |
| Reference PHP | 10.401169G | 0.949736 | 0.941108–0.998893 |
| Main ordinary release | 134.807162G | 12.917934 | 12.798558–13.004107 |
| Recovered ordinary release | 74.697080G | 6.769621 | 6.673581–6.844083 |

Candidate analysis instructions change -44.590%
and time changes -47.595% against main.
The remaining PHP gap is 7.182x instructions
and 7.128x time. This is ordinary release
without PGO; the historical 70G PGO score is a different build profile.

The three original phase windows also execute actual work and matching empty
instrumentation controls. The following single-round results subtract their
separately retained empty counts/time; they are attribution evidence rather
than repeated whole-analysis confidence claims. Counts and timing refer to the
same region on every runtime.

| Region | Main instructions / seconds | Candidate instructions / seconds | PHP instructions / seconds |
| --- | ---: | ---: | ---: |
| Native function reflection | 24.025556G / 2.148577 | 14.351797G / 1.140027 | 1.945126G / 0.175074 |
| Type intersections | 23.376219G / 2.222359 | 13.996737G / 1.319012 | 1.633056G / 0.128339 |
| Stub processing | 22.908849G / 1.907714 | 13.243893G / 0.935074 | 2.061920G / 0.163908 |

Whole-process peak RSS is recorded separately from phase-only instruction/time
measurements. Six analysis launches show median peak RSS about 515,928 KiB for
the candidate versus 1,021,084 KiB for main; these are process peaks, not an
allocation count or a phase-local memory claim. Production executable size is
24,560,960 bytes versus 24,069,152 bytes for main (+2.04%). The fresh production
build takes 82.684 seconds and diagnostic release/vm-stats 82.118 seconds.

The 13-program representative/holdout packet retains 14 rounds per runtime,
546 measured executions plus 39 validated warmups. Every output matches PHP.
Execution clocks measure each program body while their instruction counter
covers the entire short process; these scopes are explicitly different.

| Program | Body time change | Whole-process instruction change |
| --- | ---: | ---: |
| `bench_call_user_func` | -4.366% | -3.708% |
| `bench_mixed_trace_guard_loop` | -16.649% | -13.542% |
| `bench_modulo_branch_loop` | -20.631% | -15.249% |
| `bench_scoped_static_callback` | -9.268% | -2.292% |
| `bench_typed_float_composed_tree` | -5.378% | -3.109% |
| `corpus_order_pipeline` | -0.307% | +1.159% |
| `corpus_typed_order_pipeline` | -0.523% | +1.103% |
| `corpus_ledger_pipeline` | -0.029% | +1.794% |
| `corpus_typed_ledger_pipeline` | -2.359% | +1.799% |
| `bench_array` | +3.256% | +1.306% |
| `bench_declared_object_lifecycle` | +0.707% | +3.633% |
| `bench_regex_repeated_callback` | +10.382% | -3.075% |
| `bench_deep_object_release` | -21.039% | -9.476% |

An independent 20-pair repeat reverses initial runtime order for the four
predeclared concerns, retaining another 240 measured executions and twelve
warmups. The identical-binary, identical-input null control is retained. Paired
log-ratio estimates balance both runtime-order strata and bootstrap 10,000
samples within each stratum, fixed seed 20261006. No outlier is excluded.

| Concern | Independent median time change | Balanced geometric change | 95% interval | Classification |
| --- | ---: | ---: | ---: | --- |
| `bench_typed_float_composed_tree` | -7.202% | -7.179% | [-8.528%, -5.892%] | improvement |
| `bench_array` | +2.000% | +1.431% | [-0.198%, +3.148%] | overlaps null noise |
| `bench_declared_object_lifecycle` | +3.724% | +3.330% | [+1.597%, +5.185%] | confirmed regression |
| `bench_regex_repeated_callback` | +9.975% | +6.660% | [+3.104%, +10.079%] | confirmed regression |

The sole integrating task explicitly accepts the two confirmed timing losses
and the recorded small instruction losses for this recovered foundation only. These are about 39 microseconds in the object-lifecycle program
and 71 microseconds in the regex program. Array timing overlaps its null noise,
but its roughly 1.3% instruction increase is real, as are the 1.1–1.8% corpus
instruction increases and 3.64% lifecycle instruction increase. No semantic,
unsafe, or exact-output exception is permitted. A recovered foundation is not
a universal speed claim or a completed simultaneous PHP parity goal.


A separate two-round acknowledged analysis control enables GC in both runtimes
without changing the frozen PHP source or binary. It also matches every PHP
signature: PHP **10.501946G / 1.026235s**, main **140.469968G / 13.068395s**,
candidate **76.918484G / 6.967444s**. The candidate instruction gap remains
7.324x and its time gap 6.789x. This control does not retroactively relabel the
historical disabled-GC observations or replace their six-round distribution.

The approval is evidence-backed and bounded: all previously measured speed is
recovered through reviewed general runtime changes with ordinary owners and
correct callback/exception lifetimes, and the whole analysis saves about six
seconds against current main. Both observed micro losses remain future
controls. Old faster binaries with lifetime failures are not production
baselines. This checkpoint does not authorize workload recognition, waived
PHP behavior, unsafe-limit increases or another stack of compensating patches.

## Reproduction and ownership

The attached data file contains every sample, input/binary hashes, matrix
fingerprint, resource boundary and comparison scope. Build each source with
Rust 1.98.1, locked/offline ordinary release and identical features/Rust flags;
retain the executable before cleaning Cargo artifacts. The repository
instruction driver accepts an already prepared unpacked PHPStan entry point:

```sh
python3 scripts/bench-instructions.py \
  --variant baseline=/tmp/rphp-candidate-baseline/rphp \
  --variant candidate=/tmp/rphp-candidate-current/rphp \
  --reference php --project /tmp/instruction-input \
  --output /tmp/instruction-results --input /tmp/phpstan/bin/phpstan \
  --phase --rounds 6 --cpu 2 -- \
  -d disable_functions=proc_open,pcntl_signal,pcntl_exec,pcntl_fork \
  -d zend.exception_ignore_args=0 /tmp/phpstan/bin/phpstan \
  analyse --no-progress --no-ansi
```

Use the declared memory-limited user-systemd boundary and the original five-file
project/source identities in the data. Instrument only the selected PHP phase
with acknowledged FIFO enable/disable and microtime; no exception wrapper,
workload-specific runtime condition or cached analysis replaces real work.
The private frozen source/diagnostic artifacts remain recoverable locally.

The sole integrating task owns compiler/runtime/VM changes in the isolated
`codex/perf-phpstan-runtime-costs` checkout. Main is unchanged. The compatibility
roadmap is unchanged, and the unsafe ceiling stays at main's 1,747/321. The
merge includes reviewed committed performance history through `c2b7c134`, with
recent main fixes and the documented call-lifecycle repairs in one verified
source. Public-data and staged-diff checks gate commit/push; the accompanying data is
sanitized and the source/test matrix fingerprint is unchanged after measurement.
