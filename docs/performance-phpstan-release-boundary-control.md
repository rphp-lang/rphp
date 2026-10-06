# Owned release boundaries: rejected code-generation control

Status: rejected private prototype; production runtime and accepted PHPStan
scorecard remain unchanged. Simultaneous instruction/time parity is open.

Two fresh flat profiles of the exact accepted PGO and preserved owned-call
repair lose zero samples. Every unfiltered function-self period reconciles:
66,789 samples / 66,789,200,367 periods and 68,458 / 68,458,205,374.
Main plus other explicitly named VM functions accounts for 66.436%/67.025%.
Generic drop/hash/allocation descendants remain a residual; inlined work stays
in main. These symbol partitions are not semantic or removable-cost budgets.

Fully qualified main source positions reconcile to each main self period.
Short filenames would merge separate `mod.rs` files, including Value and core
pointer operations; the initial short-name maps are preserved and excluded from
file interpretation. Optimized/inlined/skidded and line-zero locations remain
uncertain. The older retained callgraph loses 43 samples/seven chunks, so its
zero-loss gate fails and a fresh flat accepted profile replaces it here.
Neither diagnostic window replaces the accepted native scorecard.

## Hypothesis and exact scope

The correct private repair's committed frame retirement has 2,918/68,458 self
samples (4.262%), versus 1,333/66,789 (1.996%) in accepted production. The
candidate tests whether common ownership functions being marked `cold`, and
final-tree scratch storage sharing that boundary, contribute a material cost.
The descriptive self shares do not predict that the entire cost is removable.

One runtime file permits ordinary optimization/inlining of existing ownership
classification and retirement. A cold, non-inlined helper contains the exact
existing tree inspection and its four identity sets. Callback machinery, order,
reference counts, exception replacement/previous, Fiber/resource handling and
constructor completion remain the existing protocol. No borrowing, runtime
eligibility guard, workload recognizer, ASM, layout or unsafe invariant is added.
Unsafe inventory stays 1,745 blocks / 319 functions.

The baseline is the preserved ownership repair, source
`42414620c2fa1475945534d69b06f94c4b7d515f8d6d43c4d5c5cf0806d89ab3`;
candidate source is
`d1ba9e462337055d64afdf57699498a08cbd51f635191932461588a163173541`.
Neither is a new accepted production runtime. Candidate ordinary executable
SHA-256 is `4ae0f740d97da5fccd7eb7032ef70cdf746acf6f17fedfcc0f14cb00511be4e7`.

## Selector and decision

All sixteen existing ownership/diagnostic controls agree with PHP and the
correct repair in 48 observations. This is a small screening packet, not a full
feature or memory-sanitizer certificate. Ordinary main text grows only 45 bytes,
336,537 to 336,582; its 3,096-byte native stack prologue does not grow.

Pinned Rust 1.98.1, default features and ordinary release use the same actual
five-file/twenty-finding analysis, serial INI flags, CPU 2, fresh TMPDIR and
analysis FIFO. Two alternating pairs retain every output-checked observation;
all counters run at 100% and exit/stdout/ordinary stderr agree with PHP.

| Ordinary analysis | Correct repair | Boundary candidate |
| --- | ---: | ---: |
| Median instructions | 75.513335G | 75.464544G |
| Median analysis seconds | 6.759992 | 6.715912 |

Instructions fall only **0.064612%**, far below the predeclared two-percent
selector. The 0.652% two-sample time difference is not a confirmed speedup.
No PGO, broad feature, holdout or ARM64 performance expansion follows rejection.
Do not stack further cold/inline variants onto this rejected design or claim
that changing these annotations closes the instruction gap.

The correct ownership repair and production sources remain untouched. The next
protocol review must quantify actual redundant Value ownership/publication,
rather than infer a large saving from a cold attribute or a sampled function.
In particular, an older argument-transfer result predates removal of unsound
ordinary borrowing; any new transfer candidate needs fresh coverage and lifetime
proofs on the owned-frame baseline, not automatic reuse of that old conclusion.

The [incremental candidate patch](performance-phpstan-release-boundary-control.patch)
applies only after reconstructing the preserved
[ownership repair](performance-phpstan-call-frame-ownership.md), using
`git apply --unidiff-zero` in a disposable checkout. Reconstruction matches the
exact candidate file. It is rejected diagnostic evidence, not production code.
[All observations and aggregate profile partitions](performance-phpstan-release-boundary-control-data.json)
retain identities, control results, all four native samples and resource events.

All expensive work uses verified six-GiB/no-swap aggregate services, whole-group
OOM/termination and the exclusive benchmark lock. All final boundaries have zero
OOM; build peak is 3,801,038,848 bytes. Exact candidate source and executable stay
local, its superseded build cache is removed, and cleanup runs in both checkouts.
No private benchmark host is configured. Parity and the production lifetime
defects described in the ownership report remain open.
