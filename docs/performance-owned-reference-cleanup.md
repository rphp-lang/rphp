# Wide-frame owned-reference cleanup

Status: verified branch checkpoint, with the integrating task accepting the
single documented correctness/performance tradeoff below. This is not a merge
to main or acceptance of the broader retirement prototypes. The 8,192-node
destructor control confirms a 1.306% regression outside its IQR. Baseline:
`b9c344712b954d882e381774ee258abe60f43c29`.

## Defect and correction

`cleanup_frame_slots` uses a bitmap to release owned values in frames of at most
64 slots. Larger frames instead scan their initialized values. The latter path
used two manually enumerated tag lists, depending on the resource-lifetime
feature, and both omitted owned PHP reference cells. Popping the activation
therefore abandoned a real reference-counted owner.

The direct regression test keeps one external object handle, then puts an owned
reference to that object and a borrowed reference to the external value into a
fresh frame. The expected object count after frame cleanup is one. Before the
correction, the two-slot case passes but the 80-slot case leaves count two. The
failing cleanup body exactly matches the baseline; this is not inferred from
RSS, exception-trace retention or destructor output alone.

The wide scan now uses `Value::needs_cleanup`, the ownership predicate already
used when publishing frame slots. Owned cells are dropped once; borrowed
references continue to own nothing and their external target survives. The
duplicated feature-specific lists disappear. The bitmap path is unchanged.

The PHP-level test returns a reference cell from compact and wide functions,
keeps a separate object owner in the caller, then releases both in order. Both
must print `live:7|copy|drop|after`, matching reference PHP. Before the fix, only
the wide baseline case fails this comparison.

This correction changes no frame layout, callback completion policy, return
planner, dispatch admission, allocator or JIT code. The new unsafe block exists
only in the ownership test, documents the lifetime of every initialized slot,
and stays within the existing unsafe budget.

## Relationship to the performance investigation

The [ordered-retirement experiment](performance-frame-retirement.md) found this
bug while investigating the ordinary-call instruction gap. That experiment
removes substantial planning work but fails independent native controls and a
shutdown ordering comparison. Its runtime changes were completely removed
before building this isolated correction. Its 44% instruction reduction and
8% PHPStan improvement must not be attributed to this patch.

`bench_scalar_frame_return.php` retains the matched scalar control that exposed
the experiment's regression. It uses the same setup and caller layout as the
shared-owner reproduction, with setup and final destruction outside the timer.

## Verification boundary

The bounded packet runs the raw frame ownership tests, returned-reference test,
existing shared-owner/release-plan suites and two wide-reference CLI cases on
default, no-default and all-feature configurations. Three existing resource
callback cases exercise the feature-dependent ownership rule. Formatting,
unsafe policy and all-target/all-feature compilation accompany those tests.
Two CLI fixtures compare exit status, stdout and stderr with PHP using the
same INI policy. This is not a full compatibility matrix or an ARM64 benchmark.

Performance uses the unchanged x86-64 protocol and exact baseline executable:
Rust 1.98.1/LLVM 22.1.8, `max-perf`, default features, fat LTO, one codegen unit,
Ryzen 9 7950X, performance governor and CPU 2. Startup-subtracted instruction
profiles use 50,000 iterations; native loops use one warmup and seven randomized
runs. All valid observations remain in the report. A signal above one percent
requires independent confirmation before acceptance.

Builds, tests and benchmarks run under a verified 6 GiB aggregate memory limit,
zero swap, whole-process-group termination and the exclusive benchmark lock.
Cleanup runs before and after each service.

## Results for the isolated correction

All 39 focused test executions pass: twelve per feature configuration plus
three resource callback cases. Formatting, the unsafe-policy gate and
all-target/all-feature compilation pass. Both PHP differential programs match
exit status, stdout and stderr. The wide reference case fails on the baseline
and passes on this candidate. This is independent of the rejected broader
retirement variants' test counts.

The shared-owner loop uses 5,377.105 instructions per iteration versus
5,380.079 baseline. Its planner remains exactly 2,676.000 instructions, with
four hash allocations and three vector growths per iteration. This correction
does not remove that structural cost.

The initial seven-run pilot suggests regressions in the target, mixed loop
and 2,048-node destructor case. An independent eleven-run confirmation does
not reproduce those signals. All observations, including the initial pilot,
are retained in the accompanying samples.

| Confirmed case | Baseline median (IQR), ms | Candidate median (IQR), ms | Change |
| --- | ---: | ---: | ---: |
| Shared-owner loop | 136.161 (134.258–137.716) | 135.978 (134.618–136.687) | -0.14% |
| Scalar control | 33.107 (33.050–33.304) | 32.900 (32.570–33.104) | -0.63% |
| Declared-property foreach | 278.509 (277.777–280.552) | 279.104 (278.459–282.300) | +0.21% |
| Mixed loop | 488.654 (481.597–489.874) | 482.295 (474.696–485.253) | -1.30% |
| Destructor release, 2,048 nodes | 3.098 (3.063–3.128) | 3.106 (3.086–3.144) | +0.25% |
| Destructor release, 8,192 nodes | 12.488 (12.438–12.519) | 12.651 (12.569–12.689) | +1.31% |

The last case remains outside the permitted one-percent/noise gate: the median
difference is 0.163 ms and the IQRs do not overlap. It is not rounded down or
discarded. Whole-request instruction counts for the 2,048-node case are
62,521,129 and 62,548,631 (+0.044%); these include construction and cannot be
interpreted as release-only instruction counts. The native regression's cause
is not established by these instruction counts.

Build time is 162.91 seconds. The executable shrinks from 21,456,264 to
21,455,496 bytes (-768). Runtime source fingerprint:
`2dd679e8e2b49262e434c2f2048d74563486e4b58bb9eba2e339c75c72333041`.
Candidate executable SHA-256:
`112980fc68b84ef80e4891b52d3bf7848ccc1a29bfcf82623ceed3b8bdedfaa5`.
Baseline executable SHA-256:
`b27c303799a1eb45170771d0a0a7ab95c0765ddd152c12f91a5e1c179a60498d`.

The verification/build/pilot service peaks at 4,787,998,720 bytes; independent
confirmation peaks at 149,118,976 bytes. Both have zero OOM, memory-limit and
swap events. The source is unchanged between these jobs.

The pilot stop remains a failed performance gate. A separate, same-binary
PHPStan diagnostic quantifies the application and memory benefit before a
concrete correctness/performance tradeoff is requested. This diagnostic does
not turn the rejected pilot into a pass.

That separate diagnostic is complete, using four measured requests per RPHP
binary in ABBA ABBA order after one warmup each. It analyses the same five
public tool files at level 5 with fresh PHPStan result-cache directories and a
warm OS cache. The twenty findings, five-file counter, stdout, ordinary stderr
and exit 1 match reference PHP in every run. Process-spawning functions are
disabled and `zend.exception_ignore_args=0` matches across runtimes.

| PHPStan measurement | Baseline median | Candidate median | Change |
| --- | ---: | ---: | ---: |
| Analysis phase | 11.785 s | 11.509 s | -2.34% |
| Whole request | 14.782 s | 13.822 s | -6.49% |
| Maximum RSS | 1,069,832 KiB | 593,442 KiB | -44.53% |

Analysis IQRs are 11.756–11.829 s and 11.505–11.536 s. PHP's single warmup
analysis is 0.926 s; it is not a repeated PHP median. The memory reduction is
approximately 1,045 to 580 MiB and follows the independently proven ownership
leak repair. The interpreter still has a large analysis-time gap versus PHP.
The diagnostic service peaks at 1,097,170,944 bytes with zero OOM, memory-limit
or swap events.

The integrating task explicitly accepts one bounded correctness/performance
tradeoff for this exact source: fix the leaked owners and reduce PHPStan memory
and time, while retaining the measured 0.163 ms / 1.306% penalty on the 8,192-node
release control. The decision follows review of the minimal ownership change,
the failing-before/passing-after proof, all 39 focused checks and every valid
performance sample. It uses the integration authority in the goal contract;
it does not claim that the user separately approved this measured regression.

The ordinary one-percent gate did not pass unconditionally. This accepted
exception is limited to the named control and current corrective patch, and
that control remains mandatory for subsequent performance work. No exception
is granted to a correctness failure or to any rejected retirement prototype.
The large ordinary-call planning cost remains a separate, unfinished
performance objective.

[All samples and verification evidence](performance-owned-reference-cleanup-samples.json)
include the failing pre-fix ownership proof, exact build identity and both
native measurement series. The runtime patch remains separate from the rejected
ordered-retirement experiments.
