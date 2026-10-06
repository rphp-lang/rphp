# Combined release-tree observation

Status: rejected exploratory checkpoint; production source restored exactly.

The goal was to remove repeated identity bookkeeping during canonical tree
inspection without changing final ownership, callbacks, references, cycles or
sparse deep-drop markers. Baseline is `c33783f6`, source SHA-256
`9635606606194d209de413515955f5159194a1edbf5f456b3b96dff7a080c967`.
The accepted PGO executable remains at 74.2814 billion hardware analysis
instructions, against PHP's 10.3374 billion; parity remains open.

## Evidence and selected representation

A separate exact-output diagnostic records 888,363 whole-command walks and
5,038,606 observed edges. It finds 3,247,830 incomplete shared-owner observations,
876,956 unique descendants, 23,711 complete shared-owner observations and only
493 opaque snapshots. Four unique repeats occur across roots, so persistent
first-observation membership cannot replace per-root descent membership.

The candidate combines first-observation membership, encounter counts, descent
membership and opaque-snapshot counts in one entry per identity. A generation
preserves membership across roots while resetting each root's encounter/descent
state. Reference edges keep the canonical separate alias set. Type-specific
membership also survives transient allocation-address reuse. Root, unique-owner
and all-in-tree-owner admission remain exact; no guard, callback, new unsafe
invariant, workload recognition or ownership transition is added.

The sole integrating performance task owns the observation implementation in
`call_frames.rs`, `release_identities.rs` and its module import. No concurrent
agent edits the checkout. The declared filter requires at least one percent
analysis instruction improvement before feature expansion or fresh PGO.

## Rejected measurement

The identical default-feature optimization-level-one Callgrind candidate counts
**117,974,161,239 instructions**, against the retained exact-binary, exact-input
baseline's **118,420,810,256 (-0.37717%)**. This baseline reuse is an exploratory
selection policy, not a fresh paired native release gate. Both complete outputs
match PHP. Hardware counters remain unavailable with `perf_event_paranoid=4`.
Profiler elapsed time is not a native timing claim.

The retained profiles explain the limit: tree inspection falls from
3,681,126,380 to 3,128,912,706 inclusive instructions, while the main executor's
self cost is exactly unchanged at 32,587,203,690 instructions. Both profiles
contain 882,396 analysis-phase tree entries. The net application saving is
446,649,017 instructions, smaller than the inclusive tree saving; other
allocation and caller costs remain part of the result. Inclusive costs overlap
and are never added to the application total.

All **35 actual focused executions** pass: six observer/identity unit tests and
29 frame, shared-owner, callable and finally tests. Eight compact/wide CLI
contracts agree with reference PHP and baseline. Formatting and unchanged
unsafe inventory pass. No all-feature acceptance, forced-canonical coverage,
PGO or native release result is claimed for the rejected candidate. Prepared
later-stage scripts remain unexecuted.

Diagnostic, preflight and Callgrind jobs use the verified 6 GiB/no-swap aggregate
boundary, process-group cleanup and exclusive benchmark lock. There is no OOM
or timeout. The first mechanical reverse-patch construction misread Git's
reversed path header and failed verification before editing any file; the
corrected reverse patch restores production source exactly. Cleanup runs in
both local checkouts; no private benchmark host is configured.

The entire implementation is removed. Smaller inspection code and a roughly
15 percent reduction inside this helper do not satisfy the whole-application
filter. Selection returns to repeated work in ordinary opcode bodies, whose
unchanged executor cost is substantially larger.

Exact identities and valid samples are in
[the rejection packet](performance-phpstan-release-tree-observation-samples.json).
