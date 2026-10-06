# Rejected eager general-value graph executor

A private implementation over the existing `MacroStep`/`MacroPlan` executor
preserves the six focused controls and actual PHPStan output, but ordinary
release analysis instructions rise **73.7787 to 76.5876 billion (+3.807%)**.
Two alternating pairs retain all valid observations. Analysis time medians are
**6.7140/6.9669 seconds (+3.768%)**; the same-window PHP reference is
10.3379 billion / 0.9489 seconds. These are ordinary release binaries, not the
accepted PGO pair. The production **67.5210-billion / 6.0081-second PGO
scorecard remains unchanged**, and simultaneous parity stays active.

The candidate is rejected and was never integrated. Production source is
byte-identical to `5e00e504`. There is no PGO rebuild or full feature/architecture
matrix for a candidate already failing the native instruction selector.

## Implementation and actual coverage

The bounded graph supports cached property and present-array reads, transfers
to primitive CV destinations, checked Long arithmetic/comparisons, identity,
truthiness, and owner-free lifetime markers. Original canonical instruction
identities retain cache and scope metadata. The existing slot helpers eagerly
publish every successful node. A guard resumes at the first unexecuted
instruction; owned cleanup stays in canonical execution before callbacks or
GC. A shared interrupt budget prevents crossing a canonical poll boundary.
Finite budgets and pending exceptions retain canonical execution. Wide frames
use validated physical slots and the established initialized-tail invariant.

The full instrumented request admits **15,780** programs and executes
**12,928,785** entries / **35,317,006** nodes: 3,505,458 complete and 9,423,327
partial entries, plus 883 zero-operation guard failures. Average successful
coverage is only **2.732 nodes per entry**. Canonical dispatch observations fall
to 247,515,490 from the earlier 269,903,711 count, but each selected entry is
still counted by main before the graph executes it. Therefore dispatch counts
and graph nodes must not be added as equivalent operations or treated as native
instruction savings. Owned lifetime markers explain many exact prefix exits.

Six controls exercise wide frames, eight value kinds, scope/traits, references,
COW, dynamic property names, missing/invalid keys, magic getters, uninitialized
typed properties, overflow, alternate arithmetic types, and destructor cleanup.
Every control matches PHP, ordinary baseline, diagnostic candidate and forced
canonical in exit/output/stderr. The initial reference fixture accidentally
redeclared built-in `copy()` and failed before execution; that failure is
retained privately and never counted as a pass. The renamed fixture passes.
These controls are focused evidence, not full compatibility certification.

## Why the extra interpreter loses

Separate instruction-sampling runs over the exact ordinary binaries have zero
lost samples. Sampled main self falls approximately **24.6420 to 22.8712
billion** instructions. The new macro executor adds about **3.3997 billion**
self instructions and its entry wrapper **0.9801 billion**. These approximate
self costs explain the direction of the measured whole-analysis regression;
they are not exact per-operation or removable-work budgets. Source-PC skid and
separate-run sampling limit precise subtraction.

This graph changes where operations execute while retaining the original
operand resolution, owner copies, frame publication and cleanup. A second
runtime dispatch plus entry/guard/result transport consumes more than the
main work it displaces. Broad dynamic execution coverage alone does not admit
a faster executor. Reducing just dispatcher counts is insufficient.

The private prototype also contains one additional explicit unsafe block:
1,750 against the production ceiling of 1,749, with 321 unsafe functions
unchanged. It was not policy eligible. Neither that issue nor performance was
hidden by changing ceilings or weakening gates; no runtime source is committed.

## Reproduction and next admission

[The numeric packet](performance-phpstan-value-graph-rejected-samples.json)
contains exact source/binary/input identities, every paired native observation,
focused output hashes, diagnostic counters and sampled executor attribution.
[The zero-context patch](performance-phpstan-value-graph-rejected.patch)
reconstructs the exact rejected source from `5e00e504` using
`git apply --unidiff-zero`; it is evidence, not an accepted runtime patch.

Default + `vm-stats` and default ordinary release use the pinned toolchain,
offline/locked Cargo and identical release flags. Each build/profile/request
runs in a verified 6 GiB / zero-swap OOM-group user service and under the
exclusive benchmark lock. Native instruction counters use the analysis FIFO,
CPU 2, fresh per-run TMPDIR and 100% running counters. Diagnostic coverage is
whole-request evidence, not the analysis instruction budget. No OOM or timeout
is counted as a pass. Exact binaries/source/profiles remain private; disposable
build targets are removed and the cleanup hook runs on both local checkouts.

Do not compensate this rejected eager executor with isolated hot-path rules.
A next general representation must remove repeated operand/owner/publication
work and demonstrate an actual native benefit before broader gates or migration.
No speedup, parity date, ARM64 result or main integration follows from this probe.
