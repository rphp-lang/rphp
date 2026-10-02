# Existing typed-IR feasibility on PHPStan

Status: two rejected private prototypes; production runtime unchanged.
The current accepted scorecard remains **67.5210 billion analysis instructions /
6.0081 seconds**, versus PHP **10.3380 billion / 0.9445 seconds**.
Simultaneous instruction/time parity is still active.

The [preceding coverage checkpoint](performance-phpstan-execution-coverage.md)
found only two optimized entries and 478 loop iterations over the whole request.
Before redesigning its executor, these probes ask whether two existing admission
filters account for the missing application execution. Each starts from the same
clean runtime and diagnostic overlay; the changes are never stacked.

| Private experiment | Static straight admissions | Actual optimized entries | Loop iterations | Result |
| --- | ---: | ---: | ---: | --- |
| Unrelated wide frame allowed, region confined to first 64 physical slots | 0 to 1 | 2 to 2 | 478 to 478 | No additional application execution |
| Original frame cap retained, generic straight regions allowed | 0 to 282 | 2 to 10,171 | 478 to 478 | Only 24,718 canonical dispatches displaced |

The wide-frame controls activate 4,749 optimized iterations and agree exactly
with PHP and forced canonical execution. This establishes that the first change
was exercised, but does not establish general wide-frame safety. The actual
application still executes only the original two optimized loops.

The second probe removes only the dense-kernel filter. In the actual application,
10,228 straight-region attempts include 59 entry guard failures; successful
entries include 10,110 straight completions and 59 side exits. Together with the
two original loops, total entries/completions are 10,171/10,112. The canonical
opcode total changes from **269,905,727 to 269,881,009**: 24,718 fewer dispatches,
**0.009158%** of that diagnostic baseline. A straight region has no backedge,
so unchanged loop iterations do not mean it was inactive.

Focused straight-region controls execute 907 entries, 904 completions and three
side exits, with exact PHP and forced-canonical outputs. They exercise integer
reads, branches, numeric strings, float/overflow fallback, referenced elements,
a missing-key warning and a type error before a later observable write. These
are focused exercised cases, not a completed feature matrix.

Both actual analyses retain the expected five files, twenty findings, exit one
and byte-exact stdout/ordinary stderr against PHP. Builds and descendants run
inside independently verified 6 GiB aggregate limits with no swap and no OOM.
The full counters, source/binary hashes and failure boundaries are in
[the data](performance-phpstan-existing-ir-feasibility-data.json).

## Decision

Neither individual filter admits enough existing application execution to
justify optimizing the old generic typed executor as the next large parity
change. No native instruction/time saving is inferred from dispatch counts,
and no PGO build or full feature matrix was run. The original source is intact.
The generic executor's large working state may still matter in other workloads;
this packet does not measure that cost.

The next region investigation needs general value and ownership support for
ordinary object, array and predicate work, together with exact exits around
calls, mutations and lifetime effects. It must first demonstrate actual coverage
and then a native instruction budget. The current scalar/Long-centered graph
and source pattern kernels do not establish that capability. This evidence
provides no promised speedup or completion date.

## Reconstruction

Start from the stated baseline, apply the existing
[dispatch-context diagnostic overlay](performance-phpstan-execution-coverage-probe.patch)
with `git apply --unidiff-zero`, then separately apply either the
[prefix probe](performance-phpstan-existing-ir-prefix-probe.patch) or the
[generic-admission probe](performance-phpstan-existing-ir-generic-probe.patch).
The resulting source fingerprints are recorded in the data. Build pinned release
with default features plus `vm-stats`, enable stats at runtime, and run the same
phase-marked input and the [wide-frame](performance-phpstan-existing-ir-prefix-control.php) or
[straight-region](performance-phpstan-existing-ir-generic-control.php) controls
inside the documented resource boundary.
These patches reconstruct rejected diagnostics, not accepted production fixes.
