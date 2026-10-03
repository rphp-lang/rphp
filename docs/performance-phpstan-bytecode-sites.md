# Canonical per-PC census and general block design screen

This checkpoint captures bytecode positions for faster offline iteration. It
accepts no runtime optimization and leaves simultaneous instruction/time parity
open. The correct private owned-frame repair remains the implementation baseline;
the production PGO scorecard does not change. Exact identities, every gate and
scope limits are in the [data packet](performance-phpstan-bytecode-sites-data.json).

The previous [body census](performance-phpstan-body-work.md) showed matching
dominant-parser source loops but only per-body opcode frequencies. Its private
default+vm-stats extension adds one Cell counter per canonical instruction and
scalar copies of instruction fields, literal kinds/Long payloads and existing
block ranges. It retains no PHP Value, frame, cache or OpArray owner and performs
no additional operand reads. Feature-off production is unchanged; unsafe remains
1,745 blocks / 319 functions. Native/typed direct paths and synthetic fused
observations stay outside the canonical decoded denominator.

The actual request captures **408,600 static positions**, **284,844 executed
positions**, **10,686 bodies** and **269,826,212 canonical steps**. Every PC sum
reconciles with its body, opcode and global totals. All prior body counts, slot
sizes, transitions and sequence counts remain exact. Only fresh-cache generated
container names/source identities differ; a rejected raw-name comparison remains
visible and is repaired offline with a narrowly scoped identity normalization.
The completed request is never repeated.

Six counter tests pass. Sixteen fresh diagnostic controls match 32 hash-verified
retained PHP/correct-repair observations; these are 16 new executions, not 48.
One fresh-cache diagnostic request matches the retained original five-file,
twenty-finding exit/stdout/ordinary stderr. Instrumented instruction counts and
times are not native acceptance results. Build peaks at 4,413,100,032 bytes and
the request/initial reader at 983,449,600 bytes, under verified separate 6 GiB,
no-swap, group-OOM boundaries with no OOM or memory-limit event. The reader's
exit 1 remains a failure despite intact completed request output.

## General structural ranges and ownership boundaries

The offline classifier considers contiguous ranges of 4–32 bytecode positions
inside existing planner blocks, with at least two non-release operations and at
most 64 locally touched physical slots. Large enclosing frames may use sparse
local slots. It classifies scalar operations, then adds ordinary array reads,
then ordinary literal-name property reads. Calls, external writes, unsupported
contexts and instructions stay boundaries. It selects by instruction structure,
never function or workload names.

These are nested structural envelopes with unknown live Value kinds and guard
outcomes. A stricter local metadata subset also requires no pre-region unknown
temporary in release intervals, no external heap TMP read root, stable root CVs
and in-range retirement of intermediate read views. This still is not a complete
alias/lifetime/CFG/GC proof or permission to execute/elide those operations.

| Envelope | Structural steps | Local metadata subset | Share of captured steps, subset |
| --- | ---: | ---: | ---: |
| Scalars | 17,840,372 | 1,105,333 | 0.410% |
| Scalars and array reads | 34,663,279 | 1,262,919 | 0.468% |
| Scalars, property and array reads | 58,701,150 | 49,082,008 | 18.190% |

Rows overlap and must not be added. The strict scalar/array subsets reject large
ranges that begin by retiring owners produced before entry. The property-array
envelope includes their production and consumption: 40,293,579 subset steps
belong to the dominant parser, but this is operation frequency, not its native
cost or successful typed coverage. The last subset contains 1,442 static ranges
in 789 bodies, with 7,873,581 potential entry hits; short entries can themselves
be expensive. A scalar-only native migration or blanket frame-limit widening is
therefore not admitted from this evidence. The next prototype must cover the
connected read/value/retirement protocol without adding a second interpreter.

Dynamic types/definedness, references/COW, source/result aliases, property
visibility/lazy/magic behavior, array-key effects, temporary overwrite vacancy,
side-exit publication, GC-root/poll and interrupt boundaries all remain required
proofs. A native prototype must demonstrate a material instruction saving and
pass code/stack/compile/memory and both architecture gates. No percentage above
is a promised speedup or parity forecast.

## Reproduce the offline iteration

[The full diagnostic probe](performance-phpstan-bytecode-sites-probe.patch)
applies after the separately preserved
[owned-frame repair](performance-phpstan-call-frame-ownership.md), at its exact
source identity in the packet. Apply with `patch -p1`; the patch has zero
context lines. Build with default features plus vm-stats in the
required memory boundary and capture its stats from the same controlled request.
Raw packets retain source-origin identities and stay private.

The [standalone reader](../scripts/analyze-bytecode-sites.py) consumes the
captured private JSON and emits both structural and local ownership metadata
screens; it never starts PHP or changes bytecode. Example:

```sh
python3 scripts/analyze-bytecode-sites.py --input "$RPHP_SITE_CENSUS" --out-dir "$RPHP_SITE_AUDIT"
```

Counts/metadata are validated before classification. Counter, malformed-reader
and structural/ownership checks pass; standalone replay exactly reproduces all
four saved audit/range files without another application run. The complete
source, executable, raw request, rejected reader and private PC packet remain
available. Cleanup completes in both checkouts, no private benchmark host is
configured, and only the superseded diagnostic build target is removed.
