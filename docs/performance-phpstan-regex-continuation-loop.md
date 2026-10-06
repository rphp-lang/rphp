# Rejected deterministic regex continuation loop

Status: rejected exploratory checkpoint; runtime restored to `3d7aa6ae`.

The existing recursive matcher enters a large Rust function after each ordinary
atom through `match_rest`. Its preceding accepted PGO profile records 1.1581
billion self instructions and 15,277,605 grouped entries. Public Regex entry
functions total 4.2762 billion inclusive; recursive call edges overlap and do
not establish a 23-billion regex budget.

The candidate advances deterministic leaf continuations through a Rust loop,
retaining recursive rollback, capture, MARK/control, local flags and budget
boundaries. It adds no recognizer, allocation or unsafe invariant. The predefined
early selection requires at least a 0.5% whole-analysis reduction before broad
feature checks or fresh PGO.

Native analysis counters in identical default-feature `test-fast` builds instead
increase **115.2761 to 115.4138 billion (+0.119%)**. Analysis time is 9.7649 to
9.8340 seconds; exact PHPStan stdout/stderr/exit signatures match PHP. These
optimization-level-one results reject the implementation; they do not replace
the accepted PGO baseline of **73.4727 billion**. No fresh PGO or later feature
matrix is run. A source loop by itself does not guarantee a smaller executed
instruction budget; the precise added cost is not isolated after early rejection.

All 112 existing regex-core tests pass, with formatting and unchanged unsafe
inventory. Thirty supported global/nested differential lines match PHP, and all
31 lines are byte-equal between baseline and candidate. Nested line 15 retains
the previously documented PRUNE mismatch; it remains a failed PHP contract.
The initial all-lines PHP equality gate exits 1 and stays in the packet. A
separate explicit supported-line gate does not count PRUNE as a pass.

All services use the verified 6 GiB aggregate boundary, zero swap and exclusive
lock. Preparation peaks at 3,475,279,872 bytes without OOM or timeout; native
selection completes in 26.49 seconds. Exact source, executable and failed gate
are retained before restoring runtime and deleting the disposable Cargo target.
Automatic cleanup runs in both checkouts. Native evidence is x86-64 only.
See [the rejection packet](performance-phpstan-regex-continuation-loop-samples.json).
The original instruction/time parity goal continues on the accepted named-type
checkpoint; there is no production regex change.
