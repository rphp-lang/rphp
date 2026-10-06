# Owned operand-storage execution screen: rejected

This checkpoint fills the missing executed instruction evidence for a private
whole-body storage-constant expansion. It accepts no production optimization.
The [earlier architecture rejection](performance-phpstan-storage-expansion-rejected.md)
remains valid: that earlier executable was never run. This screen instead
regenerates all 24 canonical body groups from the frozen correct
[owned-frame repair](performance-phpstan-call-frame-ownership.md), then permits
bounded diagnostic execution after native prologue review. It does not promote
the enlarged architecture or mix its ordinary build with the accepted PGO
scorecard. Exact identities and every observation are in the
[data packet](performance-phpstan-owned-storage-screen-data.json).

The hypothesis is that exposing immutable CV/TMP/CONST/result storage as Rust
constants removes repeated operand selection inside canonical operations.
Classification uses only those metadata fields and retains a dynamic fallback;
Send families remain excluded because their storage can be rewritten. Original
instruction identities, Values, frames, caches, owners, COW/references,
callbacks, exceptions, GC and interrupt boundaries remain intact. Every fresh
canonical body passes a token roundtrip through the generator. Only the private
baseline-dispatch source changes; unsafe remains 1,745 blocks / 319 functions.

Twenty-five ownership/storage/reference/wide-frame/callback controls give **75
equal PHP/repair/candidate observations**. This is focused screening evidence,
not all-feature or full PHP compatibility. The actual analysis also matches
PHP's exit 1, five files, twenty findings, stdout and ordinary stderr.

Two alternating pairs, with a fresh cache for each request and 100% running
hardware counters, reject the predeclared two-percent instruction selector:

| Ordinary native analysis | Correct repair | Expanded candidate | Delta |
| --- | ---: | ---: | ---: |
| Instructions | 75.509426947G | 77.108010049G | +2.117064% |
| Paired analysis median | 6.803998s | 6.977906s | +2.555961% |

The single same-window PHP reference uses 10.338121945G / 0.954094s. It is a
reference observation, not an independently repeated timing result. All four
Rust samples are retained; timing direction alone is not an independently
confirmed corpus regression. Ordinary diagnostic medians do not update the
accepted PGO result of **66.8068G / 5.8162s** versus **10.3380G / 0.9114s**.

| Ordinary main architecture | Correct repair | Expanded candidate |
| --- | ---: | ---: |
| Main text | 336,537 bytes | 1,221,530 bytes |
| Stack locals | 3,048 bytes | 8,536 bytes |
| Prologue including saved registers | 3,096 bytes | 8,584 bytes |
| Stack probe pages | 0 | 2 |

Text grows 262.970% and prologue stack grows 5,488 bytes. The variant additionally
selects a storage combination while executing each listed opcode body; handler
selection was not moved to compilation. The measurements reject this exact
representation. They do not separately attribute the regression to runtime
selection, code sharing, register pressure or text layout, and do not reject
every possible predecoded operand representation. No compensating shape rule,
PGO run, full matrix or new baseline borrowing follows this rejection.

Build and focused controls use verified separate 6 GiB/no-swap/group-OOM
services and the exclusive lock. The instruction driver verifies the same
boundary and owns its own lock; it is deliberately not nested inside the
already-locking wrapper. Build peaks at 4,865,945,600 bytes and native measurement
at 631,255,040 bytes, with zero OOM or memory-limit events. Exact snapshots,
executable, source patch, failed hypothesis and raw observations remain private.
Cleanup completes in both local checkouts, no private benchmark host is
configured, and the superseded candidate build target is removed.

The [body-work control](performance-phpstan-body-work.md) already establishes
matching source iterations at dominant parser sites. This screen narrows the
execution hypothesis: retaining canonical owner/publication work while adding
whole-body storage selection is more expensive here. A next general design
must remove repeated work without adding a second classification/dispatch layer;
it still needs an actual coverage and native budget. Simultaneous PHPStan
instruction/time parity remains open.
