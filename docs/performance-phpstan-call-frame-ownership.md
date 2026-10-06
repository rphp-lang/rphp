# Ordinary call-frame ownership: correctness repair and measured cost

Historical candidate report: its performance rejection remains intact. The
separately baselined [recovered foundation](performance-phpstan-recovered-foundation.md)
merges reviewed performance history into current main and adds stronger
call-lifecycle repairs with fresh full gates; it does not retroactively accept
this older candidate's failed scorecard.

Status: preserved private repair; performance acceptance fails. Production
runtime remains unchanged. Simultaneous PHPStan instruction/time parity is open.

Ordinary by-value arguments and method receivers could be raw-copied into a
callee without contributing an owner. The compiler excluded callee rebinding,
but did not prove that the caller's PHP reference target survives later argument
evaluation or a callback. Replacing that target could destroy the object while
the callee still reads it. Borrowed arrays could also bypass required COW.

The ordinary and PGO baseline fail eight of fourteen focused PHP controls,
including two segmentation faults and a prematurely closed resource. A minimal
object/reference control additionally fails system-heap Memcheck with invalid
reads and writes: exit 99, ten errors in six contexts. These are correctness
defects, not evidence that cheaper ownership is a valid optimization.

## Repair and semantic boundary

The retained candidate removes ordinary heap-argument/receiver borrowing and
its compiler masks. Existing owned argument initialization and receiver cloning
retain PHP values across callbacks and subsequent argument expressions. Existing
frame-free, effect-restricted execution plans remain independent. No workload
recognizer, new borrowing guard, assembly or JIT lowering is introduced.

Pending calls retire their initialized owners before catch/finally selection;
completed internal handlers retire their argument owners before frame pop.
Destructor replacements retain the original exception as `previous`. Generic
method callback entry also initializes the existing hidden trait `__CLASS__`
slot, which an incomplete candidate exposed as missing.

The earlier incomplete candidate fails three controls: pending-call destructor
ordering, destructor exception replacement and trait callback scope. A later
review finds two additional failures at `Deprecated`/`NoDiscard` diagnostics:
the original exception was published after retiring arguments. Publishing it
first preserves the destructor's replacement and its previous chain. Failed
variants are retained separately and are not counted as passing gates.

The [candidate patch](performance-phpstan-call-frame-ownership-candidate.patch)
applies to documentation baseline `99b11cd5b7670f07c5e8eeae47197c400df8c2ad`.
Use `git apply --unidiff-zero` only in a disposable checkout at that baseline.
It is evidence, not an accepted production patch. Exact runtime source hashes:

- Baseline: `49305541ed9639c786d7d7852095b52da3db8de4e02a6916daf78206ec0d40ac`.
- Candidate: `42414620c2fa1475945534d69b06f94c4b7d515f8d6d43c4d5c5cf0806d89ab3`.

Value, frame, instruction and FunctionCommon/CallPlan representations remain
unchanged. The obsolete trailing UserFunction mask is removed. Ordinary main
text changes 336,601 to 336,537 bytes; its 3,096-byte stack prologue does not
grow. The inventory falls to 1,745 unsafe blocks / 319 unsafe functions without
raising the committed ceilings.

## Focused verification

All 178 focused test executions pass across default, no-default, quick-only and
all features, with all-target compilation, formatting and unsafe-policy checks.
This is a focused matrix, not the complete repository matrix. The first gate
runner fails its own summary-count assertion after two successful configurations;
the repaired runner verifies and reuses their exact logs. That service failure
remains visible and is separate from successful test execution.

All sixteen controls match reference PHP in ordinary release and PGO. Four
system-heap Memchecks report zero errors, excluding leak checks. The same sixteen
controls pass in a test-fast/all-feature binary with actual `vm-stats` support,
all six existing frame-free projection switches disabled, and quick loops
disabled. Ordinary baseline call strategies remain; this is not a claim that
every optimization is disabled.

Fresh PGO uses the unchanged eighteen public training programs, excluding
PHPStan and acceptance controls. All 54 training observations match. Builds
take 259.484/197.023 seconds. Twenty-seven build-script missing-profile warning
lines remain visible, with no runtime profile-hash mismatch warning. Candidate
PGO SHA-256 is `2a11d6083672cc69125c53d962b3c238295bbc3b645fb8352f9a912847cb578c`.

## Actual analysis and decision

Pinned Rust 1.98.1/LLVM 22.1.8, identical features/flags and inputs, CPU 2,
fresh TMPDIR, serial runtime flags and the existing analysis FIFO are retained.
Every valid sample is kept; all counters run at 100%. Exit, stdout, ordinary
stderr, five files and twenty findings agree with PHP.

| Window | Baseline instructions | Candidate instructions | Analysis seconds, baseline/candidate |
| --- | ---: | ---: | ---: |
| Ordinary first | 72.9955G | 75.5166G | see complete samples |
| Ordinary independent | 72.9940G | 75.5162G | see complete samples |
| PGO first, two alternating pairs | 66.7987G | 68.4737G | 5.8835 / 6.0212 |
| PGO independent, two pairs | 66.8050G | 68.4784G | 5.9718 / 5.9759 |

Independent PGO instructions regress **2.505%**. The independent time difference
is essentially flat; the first window regresses 2.339%. Reference PHP in the
independent window uses **10.3382G / 0.9283s**. The candidate's roughly **6.62x**
instruction gap does not meet the original goal.

Two independent two-pair sets retain all eighty observations for the same ten
untrained controls. Shared frame retirement adds 14.089% instructions; trait
scope adds 3.017%; inherited metadata adds 3.384% in confirmation; shared
temporaries add 7.121%; return scope adds 6.365%; regex continuation adds 1.690%.
These are explicit failures of the one-percent performance gate. No universal
speedup or production acceptance is claimed.

Separate ordinary flat instruction samples lose zero samples. Their rounded
self shares put frame retirement near 2.44G/3.62G and main near 24.56G/25.02G.
These estimates are disjoint self shares, not exact function counts, inclusive
costs or removable budgets. They locate much of the added work in ownership
retirement; they do not explain the whole instruction gap to PHP.

Preserve the necessary correctness repair as the baseline for further ownership
work. Do not restore raw borrowing as a newly accepted implementation or hide
the cost behind another hot-path eligibility guard. A subsequent shared release
protocol needs a measured budget and complete lifetime/exception proofs before
production migration. The existing accepted scorecard remains unchanged.

All expensive work runs in verified 6 GiB aggregate boundaries, zero swap,
whole-cgroup OOM/termination and the exclusive benchmark lock. Completed final
services have zero OOM events. PGO peak is 4,745,908,224 bytes. Source snapshots,
exact binaries and active comparison caches are retained; cleanup hooks run in
both local checkouts. No private benchmark host is configured. Evidence is
x86-64 only, with no ARM64 performance claim.

[All samples and identities](performance-phpstan-call-frame-ownership-data.json)
contain no private paths, machine identifiers or unredacted diagnostics.
