# Current execution-tier and block coverage

Status: verified read-only discovery on `b2205e89`. Production runtime is
unchanged; simultaneous PHP instruction/time parity remains active. The
accepted [string-owner scorecard](performance-phpstan-string-owner-accounting.md)
is still 67.5210G / 6.0081 seconds versus PHP 10.3380G / 0.9445 seconds.

## Contract and measurement planes

Quantify current execution coverage before selecting a larger general
representation change. The hypothesis is that actual application work misses
existing typed/native shapes, making a series of narrow helper optimizations
an insufficient parity strategy. The sole integrating task owns this isolated
performance branch. This checkpoint admits diagnostic instrumentation only.
Stop before a runtime rewrite unless its actual coverage and reducible cost
support a material application gain with exact PHP effects and exits.

All runs pin production source
`76dd1a94701101cf8b425e4cd325ab6da07cb5bba313c71880562423bb2cbbe1`.
The current PGO executable is
`37138b687fd2430229c60a5c677047a281630ae1277a8d465226f135529a2bbd`.
Two ordinary release diagnostics use default features plus `vm-stats`.
The second uses a frozen source copy with safe dispatch-context counters;
its complete [patch](performance-phpstan-execution-coverage-probe.patch) and
source/binary fingerprints are retained in [the data](performance-phpstan-execution-coverage-data.json).
The embedded timezone data, Cargo files and configuration must accompany a
reproduced source copy. Each diagnostic runs in a fresh process and TMPDIR.

Diagnostic counters cover the **whole request**, including startup and
rendering. Their instrumented timing/instruction observations are not performance
results. Both three-runtime PHP/baseline/diagnostic comparisons have identical
exit 1, stdout and ordinary stderr signatures for all five files and twenty
findings. Stats and declared measurement markers alone are separated from
ordinary stderr. A separate current-PGO instruction sample is enabled only at
the existing analysis FIFO boundary; its output also agrees exactly.

## Current optimized execution

| Whole-request counter | Count |
| --- | ---: |
| Loop candidates / admissions / rejections | 5,995 / 4 / 5,991 |
| Straight candidates / admissions | 9,159 / 0 |
| Optimized entries / completions | 2 / 2 |
| Optimized iterations | 478 |
| Native executions / side exits | 2 / 0 |
| Frame pushes | 13,593,911 |
| Frame slot writes / heap-valued writes | 53,226,516 / 29,726,646 |

The generated mapping peak is only 4,096 bytes, with zero mapping-budget or
system failures. Array-shape rejection accounts for 3,549,180 backedge hits,
callback/indirect-call shapes for 635,628. These are a priority classifier's
counts, not disjoint semantic instruction budgets. Static admission totals do
not establish execution-weighted coverage. Actual optimized execution remains
negligible by these counters, but no hot-time coverage percentage is inferred.

Reference PHP has CLI OPcache off and JIT disabled in this protocol. Missing
RPHP JIT use therefore does not by itself explain Zend's advantage or prove that
enabling broader native execution will achieve parity.

## Execution occurs in short blocks inside larger functions

The private context probe records 269,824,196 main dispatches. Its function and
planner-block histograms reconcile exactly, with no unknown block entries.
The ordinary opcode counters total 269,903,711. The only per-opcode difference
is 79,515 AssignCv executions counted separately by the existing fused numeric
assignment; these synthetic entries bypass the probe's main dispatch site.

| Static size | Function share of recorded dispatches | Planner-block share |
| --- | ---: | ---: |
| Up to 4 instructions | 0.962% | 34.771% |
| 5–8 | 2.788% | 25.405% |
| 9–16 | 5.066% | 25.188% |
| 17–32 | 12.346% | 11.498% |
| 33–64 | 15.464% | 2.378% |
| 65–128 | 9.296% | 0.722% |
| 129–256 | 14.587% | 0.00048% |
| 257–512 | 4.458% | 0.00015% |
| 513–1,024 | 30.434% | 0.00067% |
| Over 1,024 | 4.598% | 0.037% |

Only 3.750% of recorded canonical dispatches belong to functions at most eight
instructions long, while 60.177% occur in planner blocks that size. Blocks at
most sixteen account for 85.364%. Functions above sixty-four account for
63.374%. Direct optimized paths do not dispatch at the probe site and are
outside that denominator. These are **dispatch shares, not instruction/time
shares**. The block partition is existing planner metadata: it deliberately
retains calls and is not a complete control-flow/effect eligibility proof.

This evidence favors investigating reusable partial regions and value/ownership
publication within larger functions. Adding more small-function or getter
recognizers alone would target a limited part of the recorded baseline work.
A new region representation needs complete CFG boundaries, guard-before-effect
ordering, reference/COW and live-owner proofs, and exact publication/resume at
calls, errors, destructors, GC, interrupts and suspension. No region implementation
or saving estimate is accepted from this size histogram.

## Current native cost and decision

The separate analysis-only sample has no lost samples. Main executor self
accounts for approximately **40.771%** of instruction-event periods. Listed
symbols whose names start with the VM namespace account for **64.292%**,
including that main share. These are flat self shares; nested inclusive costs
are not added. The 0.05% report threshold leaves **6.094%** unlisted, preserved
as a residual. Names are not a complete semantic module partition, and neither
namespace nor main self is a removable-work budget. Instruction IPs can skid;
optimized source/case attribution remains approximate.

The application cost is spread across shared interpreter mechanics and runtime
helpers. Existing scalar/native plans scarcely execute here. A larger general
block/value representation is a plausible direction to investigate; this
checkpoint does not claim one proven fix, a completion date, or that every
helper needs individual hand optimization. Further implementation admission
requires a prototype's native instruction reduction and exact canonical/PHP
outputs, followed by the relevant independent compatibility and performance
gates. The accepted runtime and parity scorecard remain unchanged.

## Verification and cleanup

All builds, diagnostics and sampling use the verified aggregate 6 GiB limit,
zero swap, OOM group handling and exclusive benchmark lock. Successful run
peaks are below 2.84 GB and have no OOM events. Wrapper/preflight failures remain
failures in the packet: missing FIFO control prevented analysis, a profile-root
path was wrong, and the frozen copy initially lacked embedded timezone data.
They are not accepted observations or production regressions.

The diagnostic patch passes a dry run against the exact baseline, both context
histograms and each opcode sum reconcile, and production source is unchanged.
No runtime correctness matrix is repeated for this read-only checkpoint. Build
cleanup runs before/after jobs and on both local checkouts at completion. The
superseded diagnostic build target is deleted; exact baseline/diagnostic
binaries, source snapshots and profiles are preserved. No private benchmark
host is configured. Measurements cover x86-64 only, with no ARM64 performance
or new runtime speedup claim. Public-data and complete diff review passed before
commit/push.
