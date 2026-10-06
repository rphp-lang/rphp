# String charge ownership

Status: verified x86-64 checkpoint on the performance branch, with explicit
instruction-priority timing tradeoffs. PHP instruction/time parity remains open.
Baseline documentation head: clean `d143348c`; accepted runtime `724abb76`, source
`f86729eb00a0b37058fdb228745baebf3933dae975aca2d44de071575c144146`.
Candidate was measured from a dirty working tree on `d143348c`; each build
and window pins its immutable source fingerprint. Candidate source:
`76dd1a94701101cf8b425e4cd325ab6da07cb5bba313c71880562423bb2cbbe1`.

## Change and proof

The older exact analysis attributes 0.569G own instructions to string
reservation and 0.177G to string-charge sweeps. These are descriptive self
costs, not a complete removable budget. The old sweeps were already amortized;
this checkpoint does not claim a new quadratic-loop repair.

Each reference-counted string now owns its charge record. This removes the
request-wide pointer/Weak hash table and its sweeps. `PhpString` contains the
ordinary String first, followed by charge metadata. Compile-time assertions
fix that field at offset zero with the original alignment. Every retention,
release and COW reconstruction uses the complete typed `Rc<PhpString>` owner;
no private Rc header layout is accessed. Value remains sixteen bytes, array
keys retain their shared pointer identity, and native String views still name
the same leading header.

A weak budget handle releases the charge when the last actual string owner
retires, without retaining an ended request or its emergency reserve. Additional
linked records preserve independent accounting if multiple live budgets adopt
one shared owner. Reservation and retirement traverse those records iteratively.
Logical PHP payload charges remain capacity plus String/Rc headers, matching
the preceding accounting; auxiliary metadata replaces auxiliary ledger storage.
The string owner payload grows from 24 to 56 bytes on x86-64, while the table and
its weak-pinned dead headers disappear. Physical RSS is measured below.

Reservation failure precedes publication or old-owner release. COW, binary
bytes, literal provenance, closures, shared array keys, dynamic callback caches,
source-file metadata and exact diagnostic behavior remain supported. There is
no string borrowing shortcut, opcode workload guard or new unsafe block/function.
The empty legacy sweep boundary is inline and has no execution cost; existing
accounting callers retain that boundary during this migration.

The sole integrating task owns this worktree and the minimum compiler/runtime
source-file owner projections. The contract requires at least one percent fewer
ordinary analysis instructions before independent PGO and focused feature
expansion. Native measurements cover x86-64 only; there is no ARM64 performance
claim and no change to typed IR or either native lowering.

## Application results

| Exact analysis window | Baseline instructions | Candidate instructions | Delta |
| --- | ---: | ---: | ---: |
| Ordinary release, two alternating pairs | 74.6429G | 73.7767G | -1.161% |
| Fresh PGO, first two pairs | 68.6870G | 67.5169G | -1.704% |
| Independent PGO confirmation, two pairs | 68.6833G | 67.5210G | -1.692% |

Confirmation analysis medians are 6.5048/6.0081 seconds (-7.637%). PHP uses
10.3380G and 0.9445 seconds in that same window. All five files, twenty findings,
exit status and ordinary output signatures match. The remaining instruction
ratio is about 6.53x; this partial checkpoint does not achieve parity.

A separate timer/whole-command counter window reports analysis medians
6.4828/6.0889 seconds and maximum RSS medians 620,298/566,680 KiB, about 52.36 MiB
less (-8.64%). Its 86.8183/85.1289G instruction medians include startup and
rendering; they must not replace analysis-only counts. All observations remain
separate, and every valid sample is retained.

PGO uses the unchanged eighteen independent public training inputs, with all
54 PHP/baseline/instrumented observations agreeing. PHPStan and acceptance
controls are excluded from training. The final PGO executable is 76,428,240
bytes, 116,760 bytes smaller than baseline. Instrumented/profile-use builds
complete in 260.20/202.81 seconds; 26 missing-profile warnings concern the build
script, with no runtime missing-profile warning.

## Controls and acceptance limits

All ten independent controls reduce instruction medians, by 0.396–2.044%.
The first three-pair and independent five-pair windows retain 160 measurements.
The strict one-percent time gate is **not met**: confirmed shared-temporary time
increases 12.265%, class-constant replay 1.909%, and trait-property scope 1.005%.
Other control time changes range from -4.361% to +0.740%. These regressions are
not discarded, pooled away, or attributed to a proven code-placement cause.

The checkpoint is retained under the user's explicit instruction-count priority:
all control instruction counts improve, and the actual PHPStan target improves
in instructions, time and RSS. This is a scoped target/representation tradeoff,
not an all-workloads speedup or satisfaction of the strict global timing gate.
Main integration and cross-architecture performance acceptance remain separate.

All 204 focused test executions pass across default, no-default, quick-loop-only
and all-features builds. Twelve PGO/reference/baseline/forced-canonical CLI
observations and nine ordinary CLI observations agree exactly. All-target/
all-feature compilation, formatting and unsafe policy pass, with unchanged
1,749 production blocks and 321 functions. The new public
[storage contract](../tests/fixtures/request_string_accounting.php) also covers
source-file reflection without publishing a filesystem path.

Initial type-projection, pointer-assertion and unsafe-test classification
failures remain recorded separately. Final verification preserves their fixes
and does not count those failures as test passes. All expensive work runs under
an exclusive benchmark lock and a verified 6 GiB/no-swap/OOM-group boundary;
there is no OOM. Local cleanup runs in both checkouts; no private benchmark host
is configured. Exact sources and executables remain preserved privately.
See [all measurements and gates](performance-phpstan-string-owner-accounting-samples.json).

## Why lifetime cleanup alone is insufficient

In the older reconciled profile, ReleaseTemps own bodies consume 3.676G out of
71.046G (5.17%). Selected named cycle-registry self functions consume 0.551G;
that excludes inlined admission, generic maps, polling and other helpers and is
not an upper bound for GC. No separate collection-function self row appears in
that window. None of this establishes a collector rewrite as a complete parity
solution.

The named VM self partition is about 45.45G, spread across operation bodies,
common operand/result work and call/frame protocols. Small helper removals alone
are not a sufficient parity strategy. The next implementation must first
quantify a shared execution/representation boundary with broad application
coverage, then test its ordinary full-analysis saving before expanding gates.
Do not claim that Rust or assembly selection by itself removes that work.
