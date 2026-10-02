# PHPStan release protocol: bounded diagnostic budget

Status: read-only checkpoint. No new runtime candidate or performance gain.

The broad global-negative guard and a parity-scale rewrite limited to unique
plain objects at committed frame return are not admitted. The existing planner
already excludes shared objects before traversing their children. Keep looking
for common operand, ownership and publication work across the VM; these
measurements do not establish a single removable cause of the remaining gap.

Baseline is `4c8fd4d932dbf7244e823ed4627bcdd88160b4ac`, with runtime source SHA-256
`49305541ed9639c786d7d7852095b52da3db8de4e02a6916daf78206ec0d40ac`.
The [accepted source-unpack scorecard](performance-phpstan-source-unpack-entry.md)
stays at **66.8068G / 5.8162s**, versus PHP **10.3380G / 0.9114s**.
The **6.46x instruction gap** keeps simultaneous time/instruction parity open.

## Contract and scope

Three private, frozen diagnostic builds measure the existing release protocol,
then the live reason its global negative proof seldom applies, then actual
committed owners and borrowed tree-inspection visits. No callback, reference,
COW, exception, GC admission or execution policy is replaced. Instrumented
instruction counts and timings are excluded from the accepted scorecard.

All counters cover the whole identical request, including startup and rendering.
The phase PHAR, serial runtime flags and five-file/twenty-finding request match
the accepted comparison. Each diagnostic is compared with both PHP and the
unchanged accepted PGO executable. All **nine observations** have exit 1 and
identical stdout and ordinary stderr. This is one request per runtime per probe,
not an independent timing gate or a complete compatibility certificate.

The [complete public packet](performance-phpstan-release-protocol-samples.json)
contains exact source/binary/PHAR hashes, all observations, counter partitions
and memory events. The [diagnostic source patch](performance-phpstan-release-protocol-probe.patch)
reproduces the final frozen probe on the named baseline; it is evidence, not a
production change. Apply it with `git apply --unidiff-zero` only in a disposable
checkout at the exact baseline, build locked release with `vm-stats`, and run the same request with `RPHP_VM_STATS=1` inside the
repository's verified aggregate memory boundary and exclusive benchmark lock.
The instrumented runtime retains **1,749 unsafe blocks / 321 unsafe functions**.

## The global negative proof rarely applies

| Existing boundary | Global false | Global true | False share |
| --- | ---: | ---: | ---: |
| Committed frame retirement | 357,102 | 7,055,552 | 4.817% |
| Direct-value release preparation | 466,871 | 9,464,041 | 4.701% |
| Array-tree release preparation | 530,936 | 4,395,226 | 10.778% |
| Statement temporary release | 1,126,847 | 15,712,353 | 6.692% |

Array preparation already uses this negative proof. Extending it broadly is
not justified by these coverage counts, and must also preserve native deep-drop
stack protection when PHP callbacks are absent. No such guard is added.

There are 29 periodic class snapshots, at the first frame boundary and every
262,144 subsequent boundaries. Later snapshots retain one `Generator`, one
`Hoa\Compiler\Llk\TreeNode` population of 16, 27 or 140 objects, and one
`Hoa\File\Read`. These are observed PHPStan dependency class names, never
admission keys. Class totals match the tracked-live count in every snapshot.
One early snapshot has global true with tracked count zero: the other global
weak/lazy/fiber causes cannot be attributed from the class map alone. The
periodic snapshots are not a census of every live object or analysis instant.

Long-lived callback-capable objects explain the poor coverage of the global
negative proof. They do **not** prove that unrelated shared trees are walked,
that callback work can be omitted, or that the entire runtime gap is a release
problem.

## Actual committed owners prune most trees already

The final probe records **13,326,465** actual committed owners. A scoped counter
then attributes every existing `value_tree_requires_vm_release` loop visit to
its current owner category. Nested callback retirement installs its own scope
and restores the previous scope on return. Outside these scopes remains an
explicit residual; rows are disjoint counters, not additive native cost claims.

| Actual root category | Roots | Inspections | Visits |
| --- | ---: | ---: | ---: |
| Other raw Value types | 3,359,149 | 145,683 | 379,551 |
| Shared objects | 5,838,862 | 0 | 0 |
| Unique plain objects with dynamic storage | 2,940 | 1 | 52 |
| Unique plain objects with reference properties | 12,654 | 9,807 | 397,288 |
| Unique plain objects without aggregate children | 62,667 | 0 | 0 |
| Unique plain objects with aggregate children | 145,936 | 140,002 | 1,217,013 |
| Unique nested arrays | 691,642 | 454,747 | 1,366,477 |
| Unique plain arrays | 967,458 | 0 | 0 |
| Shared arrays | 2,245,157 | 0 | 0 |
| Outside committed-owner scope | — | 138,147 | 1,678,282 |
| **Total** | **13,326,465** | **888,387** | **5,038,663** |

Only 5,712 inspections require callback work; a negative result still includes
alias/ownership and deep-drop protection. Visits count loop iterations, not
unique nodes, allocation counts or hardware instructions. Raw reference roots
stay in the first category; dynamic/native storage and declared reference
constraints are intentionally separate from the plain-object proposal. No
unique raw object with its own active callback or sidecar occurred at these
committed-root boundaries; that does not exclude nested callbacks or shutdown.

The original four release-site histograms and all general VM counters exactly
match the preceding class probe. Each category's raw type counts reconcile to
its root count. No production source changes.

## Decision and limits

An earlier unique-array actual-owner walker was already rejected: ordinary
native analysis instructions rose **0.02449%**, with source restored. That
variant really did bypass array preplanning; repeating it as if it retained the
old array prepass would be incorrect.

Adding only unique ordinary objects would target 140,002 inspections and
1,217,013 visits. This does not justify a parity-scale rewrite of committed
frame retirement. It does not rule out a narrow improvement, but no measured
native saving is claimed and no production prototype is admitted here.

The saved preceding native profile attributes about **3.789G inclusively** to
committed frame retirement. That includes legitimate retirement and nested
work, overlaps its child helpers, and is not a removable budget. The previous
callchain review also puts about **27.519G** in main self and separates actual
regex computation inside full-call costs. The original simultaneous parity
objective still needs substantially broader removal of repeated VM work.

Every build/request cycle used an independently verified **6 GiB**, no-swap
systemd aggregate boundary, whole-cgroup kill and the exclusive benchmark lock.
All three boundaries report zero OOM, kill and limit events, with build peaks
below 3.12 GB. Superseded diagnostic build directories are removed after their
exact binaries and source snapshots are retained. The cleanup hook runs in both
local checkouts; a private host is cleaned when configured. No new feature
matrix, PGO training or timing confirmation is claimed for this diagnostic-only
checkpoint.
