# PHPStan release-graph identity storage checkpoint

Status: accepted by the integrating task; overall PHP parity remains open.
Baseline: clean `554eea08` on `codex/perf-phpstan-cold`.

## Outcome and evidence

Reduce allocating/hashing small transient identity tables during canonical
release inspection, preserving every encounter, owner-count proof, callback
boundary and deep-drop marker. Exact baseline source is
`6b40e5f07116b0db3d6293a2b27d3e0355995318911b22612bf7b51405ea6549` and
PGO executable is
`91f6a8424590da64f45347fa5858aeb20c29c367bcbfadd9f0cca50c68c31fc4`.
Analysis-only hardware instructions are 80.6218 billion against PHP's 10.3338
billion. The analysis-only Callgrind profile counts 82,091,935,324 instructions;
its distinct instruction protocol is not mixed with the hardware budget.

That exact profile records 882,396 calls to `value_tree_requires_vm_release`,
7,159,375 insertions from it into identity sets, and 1,268,570 count-map growth
calls from it. The complete analysis has 3,631,197 set growth calls (0.6374
billion inclusive instructions) and 9,645,223 set insertions. The inspection
walk's own cost is 1.0850 billion instructions. Its visited sets and encounter
maps represent transient membership/counts, not observable PHP iteration order.
Recursive or overlapping inclusive edges must not be added to these totals.

## Hypothesis, scope and semantic envelope

Use a small stack-resident set/count map for release-tree inspection, with a
finite inline capacity and the existing identity hasher/table after spilling.
An insertion must deduplicate or increment exactly as the canonical table does.
Spilling transfers all prior identities and counts before handling the new
entry; no graph edge, counter, final-owner proof or callback is skipped. Large
and cyclic graphs retain expected linear traversal through hash tables.

Keep source-held and opaque snapshot owners, queued reference counts, repeated
root inspection, generator/native visitors, deep-drop checkpoints and exception
unwind behavior intact. Restrict storage changes to the measured inspection
walk and its passed visited sets. Other release planners and candidate ordering
remain unchanged. This is safe Rust without a new unsafe operation or frame,
Value, opcode or JIT layout. No workload/source/class name controls storage.

The sole integrating task owns the release inspection helper, its local storage
module, focused tests and optional diagnostics in this isolated worktree. No
other agent edits it. Do not begin another implementation checkpoint concurrently.

## Verification and stop conditions

Verify exact small/spill boundary membership and counts against the canonical
hash tables, including repeated identities and removals from opaque snapshots.
Keep existing shared subtree and transient deep-native visitor tests. Compare
compact and wide alias, cycle, callback, resource, destructor and exception
specimens with PHP and the baseline; preserve known baseline compatibility gaps.
Run relevant default/no-default/all-feature gates, formatting, unsafe inventory
and all-target compilation under the aggregate memory boundary.

Build fresh independent PGO with the unchanged 18 inputs and exclude PHPStan
and all controls. Compare analysis-only hardware instructions, whole-command
counts, output, time, RSS, compile/code-size cost, established controls and an
independent deep/shared graph holdout. Retain all valid samples and failures.
Reject the storage representation if application instructions do not improve,
spill/count behavior differs, large graphs materially regress, or stack/memory
cost outweighs the target reduction. Native evidence is x86-64 only; unavailable
ARM64 measurements are explicit. This checkpoint cannot claim overall parity.

## Implementation and verified result

The inspection's visited sets and count maps store up to eight entries inline.
Duplicate insertions and zero counts remain distinguishable from missing entries.
The ninth distinct identity transfers every prior identity/count into the
existing hash table before inserting the new entry. Query-only storage has no
observable iteration order. Other release planners keep their current tables.
All owner-count, callback, native snapshot and deep-stack proofs are unchanged;
the implementation adds no unsafe block or frame/Value/opcode/JIT layout.

Two phase-only hardware windows show **80.6539 to 79.8489 billion instructions
(-1.00%)** and **80.6781 to 79.8590 billion (-1.02%)**. Reference PHP uses 10.3365
billion in the second window. Whole-command confirmation medians are 99.2590
versus 98.4550 billion instructions (-0.81%). Confirmed analysis time is
**7.3703 versus 7.2594 seconds (-1.50%)**, against PHP's 0.9113 seconds; RSS rises
122 KiB (+0.02%). All valid runs remain retained and each analysis has fresh
cache storage, identical five files/twenty findings, stderr and status.

The initial plain-node graph control and the final callback-containing local
graph holdout are distinct programs. The first returns a row and releases it
in the caller; it is not evidence of focused inspection coverage. The final
specimen relinquishes local owners while returning a scalar and exposes leaf
destructor counts. Both original programs/results survive. The final holdout
uses only **0.12% fewer instructions** and takes 0.81% less time; do not claim a
large graph-lifecycle win. The shared temporary-read control reduces instructions
0.05%. Remaining existing control instruction budgets are essentially unchanged.

The shared-frame time control regresses **2.79%** in confirmation after 2.43%
initially, despite unchanged instructions. Code placement is a possible cause,
not a proven attribution. The integrating task accepts the reproducible timing
tradeoff under the user's instruction priority for the repeated phase reduction;
no all-program speedup is claimed.

Validation records **368 focused executions** across default/no-default/all
features and resource callback contracts. Membership/count behavior matches the
canonical tables across inline/spill boundaries, duplicates and zero counts.
The existing shared subtree and 900-deep opaque transient visitor tests retain
their exact observations. Eight supported compact/wide PHP differentials pass on
the exact PGO executable, and the known callback-last-owner gap stays byte-equal
to baseline without counting it as a PHP pass. Formatting, unchanged unsafe
inventory and all-target/all-feature compilation pass.

Source SHA-256:
`f24637cce89489cd2a8c323d6dd2b1db4ce2af85fb7cdf0af5dc43d8cee260d7`.
Executable SHA-256:
`26410682002e68c683bd673d477d5a79c93920d2ea3b5ef883dff692889b2f6e`.
Fresh PGO uses the unchanged 18 independent inputs and excludes all controls.
Instrumented/profile-use builds take 254.25/193.04 seconds. Executable size rises
25,912 bytes and executor size 201 bytes; inspection code rises from 4476 to
6202 bytes. Preparation peaks at 5,044,129,792 bytes under the 6 GiB boundary,
without OOM or timeout. Native evidence remains x86-64 only. Exact distributions,
checks and limitations are in
[the identity-storage packet](performance-phpstan-release-identity-storage-samples.json).

Remaining analysis instructions are **7.73x PHP**. The next cost selection must
address the executor's larger budget; this narrow table improvement does not
establish instruction or time parity.

Review confirms that spill transfers all entries, zero counts remain present,
and these scratch tables have no observable iteration order. Staged public-data
hygiene, focused gates and exact-artifact checks pass. Both local cleanup hooks
completed and the superseded profile-use build target was removed after
preserving the exact baseline/candidate sources, profiles and executables.
No private benchmark host is configured.
