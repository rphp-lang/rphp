# Direct cycle-root index: rejected

Baseline: accepted `724abb76`. Rejected final source:
`71d7e950a7004fc403708aee0de70c1b217f788b35e2515841419bda4d6e1978`.
The complete runtime diff is removed; the baseline remains authoritative.

Appending a live-only vector position to each cycle owner removes the ordinary
root-index hash map while preserving weak pinning, generation admission, root
ordering, callback roots and stale-index identity validation. The added owner
word costs eight bytes on x86-64; object accounting was corrected accordingly.
No new unsafe block or function was introduced.

The two-pair ordinary-release instruction selector improves 74.6431 to
74.1057 billion (-0.720%). Fresh independent PGO training improves the first
analysis window 68.6730 to 67.8814 billion (-1.153%). This is a first-window
instruction win, not accepted runtime performance. A separate timer-only
whole-application confirmation reports baseline/candidate analysis medians
6.8204/6.4304 seconds and RSS medians 620,352/609,172 KiB. Its 86.8313/85.8635
billion instruction totals include startup and rendering and must not be mixed
with the analysis-only selector.

All nine controls retain both the first three-pair and independent five-pair
windows. The independent trait-property median regresses 5.728% with nearly
unchanged instructions; shared-temporary reads regress 1.960%. The candidate
therefore fails the predeclared 1% time regression gate. Weak-object lifecycle
improves 3.053% in instructions and 8.858% in time, which does not excuse the
confirmed regressions. No selective rerun or pooling erased those observations.
A separate three-round 32 x 8,192-node deep-chain control retains all outputs,
construction/release times, counters and RSS.

All 306 focused root/GC/admission/weak/finalization/deep-release executions pass
across default, no-default and all-features builds. Thirty-two PHP/baseline/
candidate/forced-canonical CLI observations agree exactly. The same independent
18-program PGO recipe validates all 54 training observations. Formatting,
all-target/all-feature compilation and unchanged unsafe inventory pass
(1,749 blocks / 321 functions). An initial dead-owner test assertion and the
unsafe diff proof-comment failure remain recorded; both repairs preserved
runtime semantics, and the final comment-only rebuild has identical machine
text to the tested implementation.

Every expensive job used the verified 6 GiB/no-swap aggregate memory boundary
and exclusive benchmark lock, without OOM. Exact rejected binaries, source and
diff are retained privately. Local cleanup hooks ran in both checkouts and the
superseded PGO target was removed; the active baseline and ordinary feedback
cache remain. No private benchmark host is configured. Main is unchanged.

See [all retained observations](performance-phpstan-direct-root-index-rejected-samples.json)
and the [systemic VM comparison](performance-phpstan-vm-contrast.md).
