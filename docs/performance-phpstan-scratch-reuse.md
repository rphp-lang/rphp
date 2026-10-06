# Completed statement scratch reuse

Status: rejected exploratory checkpoint; the complete implementation is removed.
Baseline is `b0a67522`, production source SHA-256
`9635606606194d209de413515955f5159194a1edbf5f456b3b96dff7a080c967`.
The accepted PGO result remains 74.2814 billion hardware analysis instructions,
against PHP's 10.3374 billion. The original instruction/time parity goal stays open.

A separate baseline diagnostic records 13,593,078 whole-command frames. Of these,
524,219 exceed 64 slots and initialize 78,276,887 TMP slots. The candidate recycles
a completed Assign/ExprStmt suffix only after its explicit ordinary ReleaseTemps
covers the entire interval. Frame allocation keeps the maximum watermark;
activation-entry trait/closure carriers reserve slots above it. Try/catch/finally
and entire loop regions retain distinct TMP definitions. No cleanup instruction,
exception poll, instruction address, frame ABI or unsafe invariant is removed.

Scalar facts forget released slots and unknown producers. Backward dimension
searches select the latest physical-slot producer before classifying it. Straight
typed regions retain their existing read-before-write input mask; closed loops
keep their distinct-definition proof. These changes occur during compilation or
planning, without a new executor condition or workload recognizer. The sole
integrator owns the compiler and affected region planner in the isolated branch.

The default optimization-level-one analysis-only Callgrind count falls from
**118,420,810,256 to 118,031,439,986 (-0.32880%)**. The retained exact-binary and
exact-input baseline is an exploratory selection policy, not a fresh paired
native release gate. All output agrees with PHP. This fails the declared
one-percent whole-analysis filter, so no feature expansion, canonical coverage,
fresh PGO or native acceptance measurement is performed. Profiler elapsed time
is not a native timing claim; hardware counters remain blocked after reboot.

The candidate counter diagnostic explains the small budget: initialized TMPs
fall to 69,000,004, but wide release checks fall only 15,364,947 to 15,214,451
(-0.980%). Actual predicate visits fall 25,169,792 to 24,878,135. Frame count and
all 38,175,996 cleanup markers remain identical. Capacities, tail scans and
predicate visits overlap; they are not added as instruction savings.

The final candidate passes **45 actual focused tests**, eight exact PHP CLI
contracts, the full PHPStan output contract and four independent control
checksums. Formatting and unchanged unsafe inventory pass. Two earlier scope
runs each retain 44 passes and one failure. The first exposed a real carrier
collision, repaired before selection. The original trait-closure __CLASS__
fixture still fails PHP on both baseline and the exact final candidate; that
complete failure remains evidence. The focused supported contract uses method
__CLASS__ and closure self::class with the original class-name expectation,
which all three runtimes satisfy. The preexisting gap is never counted as a pass.

Every build, diagnostic and profile uses the verified 6 GiB/no-swap aggregate
boundary and exclusive lock. There is no OOM or timeout. Production source is
restored exactly, including the temporary region-planner edits and new test.
Exact source snapshots, binaries, original failing fixtures and raw samples
remain private evidence. Both local cleanup hooks run and superseded disposable
targets are removed; no private benchmark host is configured. The next selection
profiles the accepted PGO executor
for a larger general instruction budget; no further slot-recycling implementation
is admitted by this result.

Exact identities and counts are in
[the rejection packet](performance-phpstan-scratch-reuse-samples.json).
