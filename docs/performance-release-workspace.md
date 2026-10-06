# Frame retirement: correct callbacks and reusable planning storage

Status: rejected experiment, verified only within its recorded test packet. Confirmed holdout
regressions prevent performance acceptance under the existing one-percent gate.
The main results below describe revision 3, preserved as a source snapshot.
Revision 4 also completed verification and was rejected after its independent
holdout confirmation. Both source snapshots remain available; neither runtime
variant is committed or accepted. Their runtime changes have been removed.

## Scope and baseline

This checkpoint continues the [ordinary value-flow profile](performance-ordinary-value-flow.md)
from `b9c344712b954d882e381774ee258abe60f43c29`. The measured shared-frame loop
spends about 2,676 instructions per iteration in destructor planning, including
four hash-table allocations and three vector growths. The user requested a
systemic reduction with focused verification and no new fast-path admission
conditions.

Two existing callback-retirement errors were resolved before reusing planner
storage. On a committed function return, one throwing destructor previously
stopped the planner and frame cleanup silently discarded its remaining PHP
destructors. Successful detached user callbacks, such as an `array_map()`
callback, previously skipped local destructor dispatch entirely.

Committed returns now finish the existing exception-unwind retirement before
reclaiming frame storage. Successful detached callbacks enter the local
destructor phase, preserving exception replacement chains. Terminal VM errors
from that phase, including `exit()`, still pass through callback-frame cleanup.
Request shutdown keeps its existing exception-handler policy.

The direct-owner audit also found a baseline ordering error: repeated local
aliases scheduled an object at its first slot. PHP calls its destructor when
the last alias retires. For four locals with late aliases of the first two,
PHP prints `2|3|0|1|`; the baseline printed `0|1|2|3|`. Direct planning now
orders identities by their last retiring slot. Compact and wide frame cases
are checked against PHP, rather than preserving the baseline's incorrect order.

The previously suspected shared-argument leak was a comparison-configuration
error: PHP and RPHP agree when `zend.exception_ignore_args` matches. With `0`,
the exception trace owns argument values; with `1`, it does not. The earlier
report's second-bug classification has been corrected.

## Reusable planner storage

Both existing frame-planning branches use a request-owned workspace for their
identity tables, candidate slots and traversal/deferred vectors. The existing
reference-count tests, object markers and callback admission rules remain the
same. No function, workload, type or small-frame shortcut was added.

The direct-owner branch counts references and records identities in the same
hash-table insertion, scanning opposite to retirement and then reversing the
unique identities to obtain last-alias order. This replaces a second scan with
`Vec::contains` for identity deduplication. Deduplication is now expected linear
time; the later representative search and fixed-point retirement are unchanged. This
does not claim that all frame retirement or PHPStan is linear. Each existing
branch clears and checks only the workspace containers it used. Other cached
containers were already empty and bounded before the invocation.

The active planner takes the workspace out of `ExecutorGlobals`. Reentrant
callbacks therefore obtain independent storage instead of aliasing an active
plan. All candidate `Value`s are dropped before recycling, including exception
and VM-error exits. Prepass snapshots are cleared under the existing cycle-root
suppression scope before candidate ownership checks. Cached storage contains
no live PHP value or frame pointer.

The idle cache retains at most one workspace, and each of its thirteen
containers must have capacity at most 1,024. Larger plans execute the same
algorithm and release their metadata after use. This is a cache-storage bound,
not a condition for executing PHP cleanup. The workspace option occupies one
existing reserved word in `FunctionArgumentState`; its compile-time size check
still passes and `ExecutorGlobals` does not grow. `Value`, `ExecuteData`, bytecode
and JIT interfaces are unchanged. No unsafe code was added.

## Correctness

Fourteen reduced programs match PHP with identical explicit exception settings.
They cover implicit and explicit return, `finally`, detached callbacks,
replacement exception chains, trace ownership, `exit()`, destructor re-entry,
alternating large/small graphs, and direct aliases in compact and wide frames.
Eight expose missing baseline callbacks, termination behavior or alias order;
the other six preserve their baseline results.

The final focused verification packet records 96 successful test executions:

- Default: 31 across release-workspace, release-plan, shared-release,
  deep-release, object-release-boundaries and weak-object tests.
- No default features: 17 across release-workspace, release-plan and
  object-release-boundaries.
- All features: 46 across the default packet plus frameless-call-cleanup and
  object-foreach-return-cleanup, followed by two generator force-close cases.

Formatting and the unsafe-policy gate pass. Build/test/benchmark jobs run under
an exclusive benchmark lock with verified aggregate limits of 6 GiB RAM, zero
swap, `OOMPolicy=kill`, and `KillMode=control-group`. Cleanup hooks bracket every
job. The final feature packet peaks at 3.08 GiB without OOM events.
No broad compatibility matrix or ARM64 performance claim is made.

## Measurement protocol

Native measurements use x86-64 Ryzen 9 7950X, 32 GiB RAM, Linux
7.0.0-34-generic, performance governor/EPP, Rust 1.98.1, LLVM 22.1.8, default
features, `max-perf`, fat LTO, one codegen unit and repository function alignment.
Baseline and candidate use the same source directory and build flags. Runtime
source fingerprint for this candidate is
`0e789a342be40d0f4e182e6f1e19ad7f5005bafb030d0d88495e311f5cc95ae8`.
The measured candidate is the uncommitted runtime source preserved in this
checkpoint; the baseline is the named clean commit above. PHP is 8.5.11 NTS,
with CLI OPcache disabled and JIT disabled. The full extension inventory and
all valid samples accompany this report.

Callgrind subtracts the zero-iteration program from 50,000 iterations. Native
microbenchmarks run 500,000 iterations, validate the checksum, warm once, and
retain seven randomized measurements per runtime. All timing runs are pinned
to CPU 2. PHPStan uses the same five public tool files, fresh result-cache
directories with a warm OS cache, identical disabled-process functions and
`zend.exception_ignore_args=0`. Its analysis timer excludes startup. After one
warmup per runtime, RPHP variants alternate in ABBA ABBA order, four measured
requests each. Exit status and all 20 expected findings must match reference
PHP. Three independent holdouts retain seven randomized pairs each. No valid
measurement is discarded.

## Rejected intermediate revision

The first workspace revision removed the repeated microbenchmark allocations
and reduced its instruction count from 5,380 to 4,234 per iteration. It did
not pass the holdout gate. Destructor-chain retirement regressed by 3.96% in
the initial seven pairs, then by 3.69% at 2,048 nodes and 4.86% at 8,192 nodes
in eleven independent randomized pairs each. All samples are retained.

Its profile showed two effects: the corrected detached callbacks now perform
their required local cleanup, while direct planning still repeated identity
classification and reset containers used only by nested graph planning. The
final revision changes general plan construction and recycling as described
above; it does not bypass the corrected cleanup.

The ordered-plan revision also failed its independent holdout gate: declared
property foreach was 2.35% slower, the mixed loop 1.41% slower, and destructor
release 3.81% slower at 2,048 nodes and 2.32% at 8,192. Its microbenchmark still
improved by 19.43% in instructions and 22.50% in native time. PHPStan changed
from 11.666 to 11.631 seconds, which does not establish an application win.

Reduced instruction profiles then showed effectively identical foreach counts
and only one additional instruction per mixed-loop iteration. These two native
regressions therefore are not explained by extra frame-planning work in those
loops. The next revision avoids growing the hot executor state and permits
inlining of the return-policy and container-prepass wrappers. The destructor
profile separately exposes the added wrapper and cleanup work. This is a
measured abstraction/layout hypothesis, not an instruction-count claim about
the two unchanged control loops.

A cache/branch simulation of revision 3 on 2,000 foreach rounds records
747,800,345 baseline instructions and 747,802,873 candidate instructions.
Simulated conditional mispredictions rise from 1,199,510 to 1,717,986; 513,769
of the 518,476 added mispredictions are attributed to `execute_ex_inner`.
L1 instruction misses change from 162,065 to 168,091, and L1 data-read misses
from 1,620,906 to 1,602,971. This supports investigating dispatch-code layout;
it does not establish the real CPU's misprediction counts. Hardware counters
are unavailable under the host's current perf access policy.

## Current candidate results

All valid runs, including the two rejected workspace revisions and the
correctness-only intermediate build, are in
[`performance-release-workspace-samples.json`](performance-release-workspace-samples.json).
IQR below uses linear interpolation at the 25th and 75th percentiles.

| Shared-frame loop metric | Baseline | Current candidate |
| --- | ---: | ---: |
| Instructions per iteration | 5,380.074 | 4,199.994 |
| Main VM, exclusive instructions/iteration | 2,121.440 | 2,123.440 |
| Frame planner, inclusive instructions/iteration | 2,676.000 | 1,494.015 |
| Remaining instructions/iteration | 582.633 | 582.539 |
| Hash allocation-helper calls across 50,000 iterations | 200,000 | 3 |
| Vector growth-helper calls across 50,000 iterations | 150,001 | 4 |
| Native median, 500,000 iterations | 134.940 ms | 95.485 ms |
| Native IQR | 132.559–136.598 ms | 94.955–96.299 ms |

The three instruction rows are disjoint and sum to the incremental total.
The candidate removes seven recurring metadata-allocation/growth operations;
the remaining growth occurs while warming the workspace, not every iteration.
It reduces this reproduction's instructions by 21.93% and native median by
29.24%. PHP's current seven-run native median is 12.232 ms (IQR
12.197–12.251 ms), so the candidate still takes 7.81 times as long.

PHPStan analysis medians are 11.609 seconds for baseline (IQR 11.519–11.693)
and 11.697 seconds for candidate (IQR 11.583–11.802). The +0.76% difference
does not establish an application speedup. A PHP warmup measures 0.918 seconds;
it is a single control observation, not another four-run distribution. The
input is `scripts/phpt/{case,execution,expectation,process,report}.php`, 1,161
lines at level 5, using the analysis-phase instrumentation described in the
[PHPStan profile report](performance-phpstan-hot-profile.md).

The initial seven-pair holdout medians regress by 1.12% for property foreach,
1.70% for the mixed loop, and 2.95% for the 2,048-node destructor chain. The
independent confirmation retains eleven randomized pairs in each of four
cases, after a warmup per binary:

| Confirmed holdout | Baseline median (IQR), ms | Candidate median (IQR), ms | Change |
| --- | ---: | ---: | ---: |
| Declared-property foreach | 278.245 (277.927–278.540) | 284.476 (282.462–287.157) | +2.24% |
| Mixed loop | 477.871 (473.192–486.937) | 489.657 (488.930–491.696) | +2.47% |
| Destructor chain, 2,048 nodes | 3.179 (3.151–3.241) | 3.309 (3.298–3.319) | +4.09% |
| Destructor chain, 8,192 nodes | 12.866 (12.699–12.949) | 13.497 (13.379–13.581) | +4.90% |

All checksums and PHPStan findings match. These regressions remain visible;
the current implementation has not passed the general performance gate and
is not ready to merge under that gate. Preserving executor size and inlining
wrappers improved the target loop, but did not resolve the control regressions.
The layout explanation for the nearly instruction-identical control loops
remains a hypothesis, not a demonstrated root cause. The destructor chain also
executes previously omitted callback retirement, so its baseline does less work.

## Build and resource evidence

The candidate release build takes 163.415 seconds. Executable sizes are
21,456,264 bytes for baseline and 21,468,328 for candidate (+12,064 bytes).
PHPStan median maximum RSS changes from 1,069,756 to 1,070,018 KiB (+262 KiB).
The release build/measurement service peaks at 2.70 GiB, and confirmation at
approximately 136 MiB, without OOM or swap use.

Executable SHA-256:

- Baseline: `b27c303799a1eb45170771d0a0a7ab95c0765ddd152c12f91a5e1c179a60498d`.
- Candidate: `68727247f27292ae967174bdd2bb4e8874b35e9d4ef0ecab0fd192f0b1ab314f`.
- PHP: `2d059aa6d433b73285c68cef6fa37b66bf7f4834240b34b944ad68caade7aad6`.

The remaining measured microbenchmark cost is about 2,123 instructions in the
main VM and 1,494 in eager frame planning per iteration. This checkpoint has
not removed that planning model or closed the PHPStan execution gap. A wider
ownership-retirement change must preserve last-owner order, trace ownership,
exceptions and callback re-entry rather than add workload admission shortcuts.

## Revision 4: outlined return policy (rejected)

Moving exception completion inside the outlined retirement boundary restores
`execute_ex_inner` to the baseline size (336,589 bytes) and its exact exclusive
instruction count in the reproduction. The function address still differs.
This is not sufficient to remove the native regression.

The reproduction falls from 5,380.080 to 4,211.977 instructions per iteration
and from 136.760 to 92.960 ms (seven valid samples each). Independent eleven-pair
confirmation gives 277.676 versus 281.086 ms for declared-property foreach and
486.074 versus 487.477 ms for the mixed loop. The mixed-loop pilot regression
is not confirmed. Destructor-chain retirement regresses from 3.131 to 3.303 ms
at 2,048 nodes and from 12.667 to 13.393 ms at 8,192 nodes (+5.47% and +5.74%).
PHPStan was not rerun for this variant: the declared pilot stop rejected its
initial holdout medians before application measurement.

All 96 focused test executions and fourteen PHP differential programs pass.
The build takes 162.66 seconds; binary size is 21,465,776 bytes. Runtime source
fingerprint is `a23b236c0e6539b693f2286a23b4a9a98513eaeece8b8a74ebed5924c358b0d2`;
binary SHA-256 is `191ba9d5da104dcb795cadfe68623542472e852a19dbdee12c937a877e51eec9`.
All valid pilot and confirmation samples accompany this report.

The next experiment starts again from the accepted baseline. It replaces
committed function-frame planning with actual, ordered slot retirement and
uses the existing final-owner release boundary. This requires independent
proof for constructor completion, references, dynamic symbols, callback
re-entry, exception replacement and returned owners. It does not add a
workload- or type-shape admission shortcut.
