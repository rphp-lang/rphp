# Canonical operand-storage expansion: rejected native feasibility

Status: private prototype rejected before candidate execution. Production is
unchanged; no native candidate speedup or compatibility claim is made.

The prototype exposes immutable operand-storage metadata as constants across
24 groups of canonical opcode bodies. It keeps those bodies inside the original
VM loop, with the same instruction identity, slot writes, owners, callbacks,
references, COW, exceptions, GC and interrupt boundaries. Classification uses
only CV/TMP/CONST/result storage; unlisted combinations retain the dynamic body.
Send families are excluded because their storage metadata can be rewritten.

The generator verifies an exact token roundtrip to every original body, including
nested macros and the labelled dimension-read block. It adds no unsafe blocks
or functions and changes no bytecode, frame or Value layout. A first generator
transcriber omitted the emitted expression block and failed compilation; its
complete failure is retained privately. Correcting that generator error yields
a successful locked default release build, with **1,749 unsafe blocks / 321
unsafe functions** unchanged. Neither build changes production.

## Native architecture rejects whole-body expansion

The ordinary release comparison uses identical source dependencies, toolchain,
features and flags. It is separate from the accepted PGO scorecard.

| Native quantity | Baseline | Prototype |
| --- | ---: | ---: |
| Main executor text | 336,601 bytes | 1,226,540 bytes |
| Main stack locals | 3,048 bytes | 8,440 bytes |
| Saved native registers | 48 bytes | 48 bytes |
| Stack probe pages | 0 | 2 |
| Executable | 24,550,992 bytes | 25,579,896 bytes |

Main text grows **264.390%** and the native prologue stack grows **5,392 bytes**.
The latter violates the declared no-stack-growth stop condition. The prototype
is rejected at this gate, without executing focused controls, instruction
selection, fresh PGO training or a full feature matrix. It is not refined by
adding shape guards. A larger native body does not itself prove higher executed
instruction counts or slower analysis: those candidate measurements were not
run.

This rules out this whole-body expansion as an admitted implementation. It does
not establish that immutable metadata cannot be resolved efficiently in a
different representation, or that the original bodies contain no repeated work.

## Exact accepted runtime still reproduces the gap

A separate, output-checked single hardware observation of the current accepted
PGO records **66.802124G / 5.866304s**, versus PHP **10.337951G / 0.931718s**.
Both have the same five-file/twenty-finding analysis, exit 1 and exact stdout and
ordinary stderr hashes. This is a diagnostic observation, not a paired
acceptance gate or replacement scorecard. The accepted
[source-unpack scorecard](performance-phpstan-source-unpack-entry.md) remains
**66.8068G / 5.8162s** against **10.3380G / 0.9114s**.

A separate sample of the identical accepted PGO puts approximately **40.58%**
in main executor self, 2.73% in memmove, 2.14% in regex sequence matching, 2.00%
in committed return-owner retirement and 1.51% in release-tree inspection.
These are disjoint, rounded flat self shares; they are not removable budgets.
The packet retains every displayed self symbol at or above 0.5% and an explicit
unlisted residual. No out-of-line operand resolver appears in that table, so a
blanket inlining claim is not supported by it.

The profiler reports **43 lost samples / 7 lost chunks**. Its generated inclusive
table supplies no reliable additional call context and is excluded from causal
claims. Missing records, source-line motion and unknown/inlined work remain
limitations; neither a zero-loss profile nor exact operation attribution is
claimed. Prior [validated callchain attribution](performance-phpstan-callchain-validation.md)
remains separate evidence on its named preceding executable.

An additional workspace cleanup hook ran while this diagnostic service was
active. Retained source and executables were outside its cleanup targets. This
is another reason these single timing observations are not an acceptance gate;
no new speed comparison is inferred from them.

The release build and current application/profile use independently verified
**6 GiB/no-swap** aggregate boundaries and the exclusive benchmark lock. The
build peaks at 4,675,596,288 bytes; the profile packet records 1,748,262,912 bytes.
Both have zero OOM and memory-limit events. The superseded candidate build
target is removed after retaining its exact source and executable; local
cleanup hooks run, and no private benchmark host is configured.

The [public packet](performance-phpstan-storage-expansion-samples.json) records
source, executable and PHAR hashes, the native prologue/text audit, all current
request observations and the sampled flat self partition. Broader removal of
operand, ownership and publication work remains the next required direction;
simultaneous PHPStan instruction/time parity is not complete.
