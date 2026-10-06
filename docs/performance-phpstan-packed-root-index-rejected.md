# Packed cycle-root membership: rejected

Status: the prototype is completely removed. The accepted runtime remains
`724abb76`, source `f86729eb00a0b37058fdb228745baebf3933dae975aca2d44de071575c144146`,
at 68.6894 billion analysis instructions and 6.3610 seconds versus PHP's
10.3377 billion and 0.9294 seconds. Simultaneous instruction/time parity remains
active. The candidate's lower instruction count is not an accepted result.

## Contract and representation

The earlier [direct-index prototype](performance-phpstan-direct-root-index-rejected.md)
removed ordinary address hashing but enlarged each cycle allocation by eight
bytes and failed independent time controls. Its allocation growth was a
possible cause, not an established explanation. This follow-up tests membership
inside the existing eight-byte admission word, with no allocation growth.

While the complete request-local generation fits in 32 bits, the word stores
that generation in its low half and the candidate-vector position in its high
half. Position lookup is independent of the current admission generation and
validates both bounds and exact Weak identity. Admission preserves the position;
reindexing preserves the generation. This retains callback publication and
incoming-buffer restoration when vector positions become stale.

Unrepresentable positions and dead Weak registrations retain the original
indexed registry. A generation beyond 32 bits retains the complete monotonic
64-bit epoch, the indexed registry, and a separately accounted exact-epoch
admission map. The latter prevents an old packed word from accidentally matching
a future full epoch. Epoch advance clears that map; dead-root sweeps remove dead
records. The existing exhaustion behavior remains unchanged.

The payload offset, metadata offset, allocation size and alignment remain
identical for arrays, objects, closures and owned references. There is no Value,
frame, instruction, Rc-header or JIT ABI change. The existing live-Weak metadata
projection is reused; only integer Cell operations run within that borrow, with
no owner release or PHP callback. Unsafe inventory stays at 1,749 production
blocks and 321 functions.

Admission required at least 0.5% fewer ordinary full-analysis instructions, exact
PHP output, independent same-recipe PGO, and no independently confirmed control
time regression above one percent. A failed control removes the whole prototype
before further feature expansion.

## Actual application selection

The baseline documentation head is `5bc84c93`. The candidate source is
`2bdeca695139b8f2b4d2356706581204c063438eb9f1582ac793593df198dd9c`.
Rust 1.98.1/LLVM 22.1.8 and default features are unchanged. PGO independently
uses the same 18 public training inputs; PHPStan is not a training input.

Each analysis window retains two alternating pairs on CPU 2 with fresh temporary
directories, analysis FIFO boundaries and 100% counter-running time. All runs
match the known five-file/twenty-finding PHP output exactly, including exit 1.

| Analysis-only median | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| Ordinary instructions | 74.6445G | 74.0786G | -0.758% |
| Ordinary seconds | 7.2228 | 6.9112 | -4.313% |
| Independent PGO instructions | 68.6641G | 68.1064G | -0.812% |
| Independent PGO seconds | 6.4830 | 6.2816 | -3.106% |

The reference in the PGO window uses 10.3379G instructions and 0.9424 seconds.
A separate fresh-cache whole-command check processes the same five files and
retains all four A/B observations: analysis medians are 6.3838/6.2052 seconds,
wall medians 8.3420/8.1415 seconds and peak RSS medians 620,284/609,440 KiB.
Its 86.8289/86.2123G counters include startup and are not analysis-only counters.
The target improvement is real but bounded; it does not justify parity or a
general speed claim.

## Independent controls reject the candidate

All nine controls run first in three shuffled pairs and then independently in
five pairs with another fixed seed. Every valid sample is retained, and each
control's deterministic output matches PHP. The second window does not pool
away the first window's regressions.

| Independent control | Instruction change | Time change |
| --- | ---: | ---: |
| Shared frame release | -1.643% | +2.286% |
| Trait property scope | -0.312% | +9.240% |
| Nested regex continuation | +0.382% | +1.696% |
| Inherited method metadata | -0.458% | +2.007% |
| Shared temporary reads | -0.639% | -0.525% |
| Class constant replay | -0.704% | +0.238% |
| Return scope projection | -0.745% | -4.861% |
| Declared object lifecycle | +0.001% | +1.064% |
| Weak object lifecycle | -1.597% | -5.849% |

The decisive trait regression reproduces the first window's +8.737% as +9.240%.
Its cycles increase 9.317% while instructions decrease 0.312%; branch misses
increase from 1,160,842 to 1,876,911. These observations do not establish one
microarchitectural cause. Allocation growth cannot be the sole explanation,
because this representation does not grow allocations. The main executor's
machine code grows 391,888 to 396,044 bytes and total text 16,622,010 to
16,712,210 bytes; changed guards and generated-code layout remain hypotheses.
Fewer instructions alone do not guarantee less time.

The three-pair 8,192-object/32-loop deep-release check preserves all PHP outputs.
It has lower candidate whole-command instructions, broadly similar release time
and slightly higher RSS. These observations remain separate controls and do not
offset the confirmed trait/shared-frame/inherited-method/regex failures.

## Proof, failures and restoration

The default proof executes 17 root tests and five cleanup tests successfully,
including unchanged owner layout, stale positions after pruning and incoming
splits, full-generation collisions, callback readmission, dead Weak lifetime,
startup enablement and request isolation. Eight supported CLI scenarios produce
32 exact observations across PHP, baseline PGO, candidate PGO and forced
canonical candidate execution. All 54 training observations preserve outputs.

Two preceding failures remain in the data: the rewritten unsafe projection
initially lacked an added SAFETY proof, and a new internal assertion incorrectly
expected dead Weak metadata to expose a position. The proof was added and the
assertion now explicitly verifies that dead metadata is not read; live-root
order, count, sweep and collection assertions remain intact. Neither failed
attempt is counted as a successful gate.

All expensive jobs use verified 6 GiB, zero-swap, whole-group OOM boundaries and
exclusive benchmark locks. No OOM occurred; the largest observed peak is
4,788,555,776 bytes during PGO construction. Native measurements are x86-64;
there is no ARM64 performance or JIT-lowering claim. The full feature matrix is
not run after independent rejection, and this candidate is not represented as
compatibility-complete.

The exact rejected patch, source, binaries, logs and samples are preserved in
private evidence. Runtime source is restored byte-for-byte to the accepted
fingerprint; no prototype tests or metadata machinery remain. Local cleanup
hooks run and superseded disposable PGO build storage is removed. No private
benchmark host is configured. Continue with quantified general operation/storage
and ownership work; this bounded GC gain is insufficient as a parity strategy.

Exact identities, all observations, boundaries and failed attempts are in
[the normalized data](performance-phpstan-packed-root-index-rejected-data.json).
