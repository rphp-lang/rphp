# Main executor inline caller budget

This checkpoint reprocesses the exact retained zero-loss accepted and correct-
repair PGO profiles. It runs no new PHPStan request and accepts no runtime
optimization. Production and the native scorecard remain unchanged; PHPStan
instruction/time parity is open.

The [fully reconciled profiles](performance-phpstan-release-boundary-control.md)
previously grouped source positions by file. A generic `Value` or core-pointer
position does not identify its caller. This reader exports one physical record
per sample and decodes the complete DWARF inline chain at each sampled main PC.
The static ELF address comes from the exact main symbol plus its recorded
offset; all offsets stay inside the function and the load slide is consistent.
Every physical period reconciles to its retained full total, and all main
periods reconcile to the retained main self total. No inline frame is added to
an inclusive total.

Accepted main has 3,425 unique sampled PCs and 27,429 samples; correct repair has
3,306 PCs and 27,761 samples. Both profiles use period 1,000,003 and have zero
lost samples. Each available outer main source line is mapped to the exact
source opcode arm using a lexical reader that excludes comments and literals.
The following values are **billions of sampled self instruction periods**,
not exact per-opcode instruction counts or safely removable work:

| Main caller source correlation | Accepted | Correct private repair |
| --- | ---: | ---: |
| Return | 2.495 | 3.194 |
| AssignCv / BindCvRef | 3.107 | 2.947 |
| ReleaseTemps | 2.853 | 2.770 |
| FetchObjR | 2.632 | 2.396 |
| FetchDimR | 2.444 | 2.364 |
| DoFcall | 1.327 | 1.204 |
| main_before_opcode_match | 1.949 | 1.891 |
| main_after_opcode_match | 1.355 | 1.481 |
| unresolved_main_location | 1.651 | 2.084 |

Source positions outside the match include shared macros and other locations;
they do not establish an independent per-dispatch tax. Optimized or skidded
source positions can also be shared. Unknown locations remain explicit rather
than being redistributed. The correct repair's unresolved main period is
2.084 billion (7.51% of main), so this is not complete semantic attribution.

Within main, recognized inline ancestry in the actual `Value::clone` and
`Value::drop` implementation ranges accounts for **0.590 and 0.510 billion**
periods respectively on the correct repair. This is only recognized main
inline code: it excludes outlined drop functions, retirement planning, other
Value predicates and unknown/shared DWARF locations. It cannot prove total RC
cost or an achievable saving. Thus the [67 million whole-request heap copy events](performance-phpstan-owned-argument-census.md)
are not by themselves an instruction budget for the next change.

Deduplicated presence of named slot-writer/operand-getter helpers in the inline
chains gives a union of **5.556 billion** main periods on the correct repair.
Separate helper ancestry groups overlap source-arm/Value groups and must not be
added. The full source-arm cross-table and all unresolved totals are in the
[data packet](performance-phpstan-main-inline-budget-data.json); full paths,
ASLR addresses and raw inline chains remain private.

Four focused reader checks pass, followed by exact reconciliation of both real
profiles and frozen source/executable identities. The initial reader failed on
GNU addr2line's valid `compiler-generated unit:?` notation; its failure remains
visible. The repaired reader preserves it as an unresolved line and accepts
unqualified inline function names. No failed diagnostic is counted as a pass.
The final reader takes 3.55 seconds under the verified 6 GiB/zero-swap/group-OOM
boundary, peaks at 726,302,720 bytes and records zero OOM events. Existing intact
decoder output is reused rather than rerunning application profiling.

The next protocol investigation spans operand/result publication and retirement
across returns, assignments and reads. Neither a narrow argument transfer nor
another cold/inline helper variant is admitted by these correlations alone.
