# Layered execution optimization plan

Status: validated planning snapshot at `e78342bdb12e04e124c16a0e4ae2687b95007172`,
2026-08-14

This document orders execution work as one cumulative optimization stack. It
exists to prevent an empty coverage cell or an old benchmark from being
mistaken for a new high-value goal. The active
[execution roadmap](roadmap-execution-performance.md) owns milestone policy,
the [execution scorecard](execution-scorecard.md) owns reproducible measured
coverage, and the historical logs retain detailed experiments. This plan is
the capability registry and dependency order used to choose the next measured
slice.

The list is deliberately capability-level. A historical checkpoint may have
many commits and tests, but it appears once here so that later work builds on
it instead of recognizing the same PHP source shape again.

## Stacking contract

Every performance slice extends this chain:

```text
canonical baseline semantics
  -> shared typed operation and ownership facts
  -> guarded region with exact live-value publication and side exits
  -> equivalent ARM64 and x86-64 lowering where profitable
  -> measured corpus and independent-holdout benefit
  -> removal or narrowing of the superseded special path
```

An optimization is not admitted merely because a typed or native coverage cell
is empty. A specialized typed runner may already own the complete hot loop, in
which case another JIT lowering can duplicate code without removing the
measured bottleneck. Conversely, a native region can still hide an important
allocation, layout or ownership cost that machine code cannot repair.

The following rules apply after every accepted layer:

1. Regenerate execution-weighted coverage and profile the exact new baseline.
2. Preserve reference-PHP output and canonical execution independently of the
   optimization.
3. Require one structural proof and exit contract for both native backends.
4. Keep every representative corpus and independent holdout inside the global
   one-percent regression ceiling.
5. Delete or converge an older recognizer only after the common replacement
   matches its correctness, coverage and performance.
6. Reorder later layers when current measurements disprove their expected
   impact. The sequence is a decision dependency, not a promise to implement
   an unmeasured idea.

## Evidence grades

Future entries use these grades rather than false numeric precision:

| Grade | Required evidence |
| --- | --- |
| A | Current target, representative corpus and independent holdout evidence on ARM64 and physical x86-64, with counted structural cost. |
| B | Current evidence on one architecture plus accepted dual-architecture evidence for the same runtime boundary. A fresh second-host gate is required before implementation acceptance. |
| C | Isolated benchmark or static coverage gap only. It may justify a measurement checkpoint, not a production optimization. |
| D | Architectural possibility without a current measured cost. Deferred by default. |

No item below Grade B may become an implementation goal. Grade C may become a
bounded profiling/holdout goal whose valid result is rejection.

## Validated baseline snapshot

The source baseline is clean commit `e78342b`. The ordinary build includes the
bounded native JIT and retains explicit typed-only and runtime-disabled lanes.
A fresh 15-run ARM64 scorecard at this commit validated output across canonical,
typed, default-JIT, PHP no-JIT and PHP tracing-JIT modes. A fresh x86-64 cycle
was not started because the exclusive benchmark lock was occupied. Current
x86-64 planning evidence therefore comes from the accepted dual-host
checkpoints already contained in this exact commit; any new implementation
still needs a fresh locked x86-64 gate.

The current ARM64 selection rows are median in-workload milliseconds:

| Workload | Canonical | Typed | Default JIT | PHP no JIT | Interpretation |
| --- | ---: | ---: | ---: | ---: | --- |
| Order corpus | 102.179 | 63.907 | 4.224 | 94.450 | The hot loop is already one native `typed_ops_loop`; only nine physical frame lifecycles remain. |
| Typed order corpus | 121.100 | 84.401 | 4.239 | 98.142 | Same native coverage and aggregate ownership result as the untyped source. |
| Typed ledger corpus | 51.551 | 25.887 | 2.407 | 33.004 | The hot loop is native with zero side exits; only seven frame lifecycles remain. |
| Routing holdout | 184.708 | 64.446 | 6.616 | 68.972 | The hot loop is native with zero side exits; only six frame lifecycles remain. |
| Dynamic String-key read | 44.953 | 12.454 | 1.608 | 9.311 | The previously missing read-only hash lowering is complete and profitable. |
| Packed value `foreach` | 10.914 | 0.381 | 0.392 | 1.689 | The typed runner already removes the loop cost; missing native mapping is not a current target. |
| Hash value `foreach` | 11.112 | 0.462 | 0.525 | 1.643 | Same conclusion; historical residual cost was entry density, not dispatch. |
| String append | 1.968 | 0.327 | 0.324 | 1.312 | The chunked typed kernel is already effective. |
| Packed array build/read | 18.614 | 3.190 | 3.194 | 4.538 | The typed kernel and bounded reserve are already effective; JIT parity alone is not a goal. |
| Closure copy and invoke | 46.115 | 48.576 | 49.527 | 11.787 | Largest visible uncovered execution boundary, but currently only isolated evidence. |
| Declared-object lifecycle | 111.466 | 6.679 | 0.638 | 24.134 | Resolution, storage reuse and nonescaping read virtualization are already layered. |

The closure target supplies the clearest remaining counted gap. One execution
performs 500,004 frame pushes, 500,004 frame cleanups and 1,250,008 frame-slot
writes. Its only hot loop is rejected as `direct_call_shape`, accounting for
250,000 rejected backedge executions. The closure owner itself is no longer
the problem: the accepted ownership checkpoint reduced the target to one
payload and one capture-storage allocation. This makes direct call/capture
execution a plausible next layer, but the absence of a representative
closure-heavy holdout keeps it at Grade C until Layer 1 admission is complete.

## Completed foundation: do not reimplement

These capabilities are accepted inputs to future layers. They may be widened
only when a new profile identifies a different general shape or remaining
cost.

| Capability | Accepted state | Durable evidence |
| --- | --- | --- |
| Scalar and control-flow regions | Checked Long and exact Double arithmetic, recurrences, branches, scalar functions and guarded monomorphic methods execute through typed plans with exact exits. | [Dispatch analysis](performance-dispatch-analysis.md), [combined execution log](roadmap-performance-jit-compatibility.md) |
| Dual-architecture native execution | Shared typed regions lower to ARM64 and x86-64; default production admission is bounded, W^X, observable and exactly falls back to typed execution. | [Default native JIT](performance-default-native-jit.md) |
| Packed/hash array reads and writes | Integer and dynamic String-key reads, guarded structural writes, update fusion, materialized accumulation and bounded packed reserve are present. | [Native String hash read](performance-native-string-hash-read.md), [packed reserve](performance-packed-array-reserve.md) |
| Value-only `foreach` accumulation | Packed and ordered-hash Long iteration retains iterator and accumulator state in one guarded typed runner. | [Dispatch analysis, Phase 2p](performance-dispatch-analysis.md#phase-2p-result-guarded-value-only-foreach-accumulation) |
| String execution | Borrowed immutable String facts, dynamic-key retention and chunked append kernels avoid repeated general `Value` work in admitted shapes. | [Dispatch analysis](performance-dispatch-analysis.md), [combined execution log](roadmap-performance-jit-compatibility.md) |
| Closure ownership | Published closures share one immutable payload and capture environment while preserving identity, binding and reference-capture semantics. | [Closure ownership](performance-closure-ownership.md) |
| Declared-object lifecycle | Literal class resolution is cached, small declared-property storage is reused and proven nonescaping read-only objects can remain virtual. | [Declared storage reuse](performance-declared-object-lifecycle.md), [literal `new`](performance-literal-newobj-resolution.md), [virtual reads](performance-virtual-declared-object-reads.md) |
| Call/return aggregate virtualization | The proven order-shaped constructor/method/small-array graph can remain virtual in baseline, typed and native lanes, with a bounded resolution cache. | [Virtual call aggregate](performance-virtual-call-aggregate.md), [resolved aggregate cache](performance-resolved-virtual-aggregate-cache.md) |
| Collection, callback, JSON and regex programs | Several pure callback pipelines, invariant JSON projections and bounded regex consumers already have specialized typed/native programs. Treat them as migration inputs, not proof of general call-graph coverage. | [Combined execution log](roadmap-performance-jit-compatibility.md) |

The largest accepted gains also show why stacking matters. Virtual declared
reads reduced their isolated target by 91.44% on ARM64 and 86.79% on x86-64.
Making the pre-existing call aggregate available to the baseline dispatcher
improved the order corpus by 33.62% and 31.70%, then caching its resolved plan
added another 22.17% and 19.65% on the new baseline. These are cumulative
layers over one semantic contract, not competing fast paths.

## Ordered future stack

### Layer 0 — refresh measurement and operation visibility

Status: **mandatory before every selection; Grade B snapshot is current**

Keep the scorecard reproducible on both hosts and attach execution weights to
admission gaps, allocations, frame traffic, clones/drops, COW detachments and
code-cache growth. Before changing an already-native corpus, add generated-code
operation mapping or equivalent symbolization: the frozen scorecard placed
roughly 92–96% of native corpus samples in anonymous executable mappings, which
cannot rank instruction scheduling or register-pressure work.

This layer does not justify permanent telemetry cost. Instrumented builds stay
separate, and disabled counters must compile to no ordinary-path work.

Exit gate:

- current ARM64 and locked x86-64 scorecards agree on the top structural cost;
- the candidate has corpus or independent-holdout evidence, not only a static
  site count; and
- every missing metric needed by the claim is measured rather than recorded as
  zero.

### Layer 1 — general direct-call regions, starting with immutable closures

Status: **next admission checkpoint; Grade C until a holdout and fresh x86-64
profile exist**

Extend the existing typed call vocabulary so a stable direct user-function or
closure target can participate in a region without constructing a canonical
frame for every invocation. The first supported closure envelope should require
an immutable published payload, stable function identity and capture layout,
exact positional by-value arguments, a proven scalar return and a body already
expressible by the shared typed IR. It must publish captures, arguments and
completed effects exactly at every exit.

This layer builds on shared closure ownership and the existing scalar/method
call IR. It must not recognize `bench_closure_copy.php`, assume a copied local,
or introduce a closure-only second call engine. Named functions, compatible
closures and later callback consumers should share one direct-call contract.

Before implementation, add or identify an independent closure-heavy
application holdout and reproduce the `direct_call_shape` cost on physical
x86-64. Reject or defer the layer if the gap remains microbenchmark-only.

Semantic envelope and gates include references, by-reference captures,
`Closure::bind`, `$this` and lexical scope, exceptions, named/variadic
arguments, recursive/re-entrant calls and changed callable identity. Unsupported
cases must reject before mutation. Acceptance requires a target win on both
architectures, reduced frame/slot counts, exact forced exits and neutral
order/ledger/routing controls.

Expected leverage: high. It removes the measured boundary in the closure
target and becomes the call substrate for Layers 2 and 3. Implementation cost
and semantic risk are medium to high; the evidence checkpoint is intentionally
cheap and may reject it.

### Layer 2 — general virtual heap values and materialization across calls

Status: **second implementation layer after the direct-call/liveness contract;
Grade B architectural evidence**

Replace the current collection of narrow object/array proofs with one typed
virtual-value vocabulary describing small declared objects, packed/associative
arrays and immutable closure environments. Region liveness and escape analysis
should decide whether a value stays virtual, is borrowed or materializes at an
exact bytecode boundary. Calls consume and return the same virtual values
through the Layer 1 call graph.

The first new slice must come from a representative graph that is not the
already-completed order shape. Good evidence would be a small DTO or array
created in one function, consumed in another and discarded, or an escaping
branch that forces late materialization. Identity, writes, references, COW,
constructors, destructors, magic access and exception paths define explicit
materialization boundaries rather than ad-hoc rejection scattered among
executors.

Acceptance requires fewer named owners/allocations on the target, exact
materialization tests at every escape, common typed and dual-backend operations,
and removal or narrowing of a superseded order/object recognizer. If no new
corpus graph allocates materially, do not widen the existing proof.

Expected leverage: high because prior object and aggregate layers delivered
large dual-host wins. Cost and correctness risk are high, so the layer advances
in small vertical slices.

### Layer 3 — callback and collection composition over the common call graph

Status: **conditional on Layers 1–2; Grade C outside existing specialized
programs**

Normalize `foreach`, `array_map`, `array_filter`, `array_reduce`, `array_walk`
and JSON-adjacent callback flows onto a shared collection program. Pure stable
callbacks use the Layer 1 call ABI; intermediate arrays, tuples or DTOs use the
Layer 2 virtual-value/materialization contract. Preserve key order, packed/hash
transitions, callback arity, references, mutation, exceptions and partial
progress.

The purpose is not to add another fused benchmark shape. It is to make existing
special callback, JSON and `foreach` programs consumers of one vocabulary and
then cover a measured application pipeline that the current regions reject.
The already-fast value-only `foreach` remains in place until the common program
matches its 0.38–0.46 ms ARM64 result and exact exits.

Expected leverage: high for application pipelines once general callbacks and
virtual intermediates exist; premature cost is high. Admit only with an
execution-weighted corpus gap and an independent holdout.

### Layer 4 — region coverage and convergence

Status: **continuous after each preceding layer; current straight-region gaps
are Grade C/D**

Form partial loop, branch, straight and bounded-call regions from control flow,
effects and liveness. Migrate legacy quick plans onto the shared typed IR and
deoptimization protocol as their replacement becomes complete. Delete their
recognizer, cache and executor only after performance parity on both hosts.

Current order/ledger/routing programs expose 4/4/13 straight candidates rejected
as `no_typed_span`, but they are static post-loop sites with no execution-
weighted hot evidence. They are not a reason to build a broad trace compiler.
Straight-region work becomes eligible only when sampling and counters assign it
material corpus time.

Expected direct gain: workload-dependent. Expected maintainability and future
leverage: high, because every later operation otherwise multiplies planner,
executor and backend paths.

### Layer 5 — representation, allocation and locality after dispatch removal

Status: **profile-selected branches; Grade B for known boundaries, no blanket
rewrite**

Once a region removes dispatch and frames, profile its remaining memory costs.
Candidate families are compact hash entries/value views, object and closure
headers, small-container storage, bounded arenas or request-local reuse, and
clone/drop or COW reductions. Preserve the 16-byte `Value` unless a complete
dual-host layout and corpus proof justifies changing it.

The hash `foreach` record is the model for admission: its remaining difference
tracked 40-byte RPHP ordered-hash entries versus 32-byte Zend buckets, so data
density is a plausible boundary while more loop dispatch specialization is
not. The packed-array reserve is already complete and produced 4.96% on ARM64
but only 0.57% on x86-64; further capacity work requires allocation/COW
counters and new corpus evidence.

Expected leverage: medium to high when memory bandwidth or allocation is
sampled; risk is high because representations affect all tiers. Each change is
one measured layout checkpoint, never a combined memory-model rewrite.

### Layer 6 — target-neutral native optimization and runtime intrinsics

Status: **deferred until native operation visibility ranks a cost; Grade D for
current corpora**

Improve shared lowering, liveness, register allocation, branch layout and
helper ABI before adding architecture-specific tricks. Both backends consume
the same typed operation and exit metadata. After portable code is measured,
small target-neutral intrinsic contracts may gain NEON or AVX2 variants for
validated buffers: hashing, JSON/UTF-8 scans, string search, numeric conversion
or packed reductions.

Do not equate x86-64 with one vendor, handwrite assembly before normalized
compiler output is proven insufficient, or add an ISA check to ordinary PHP
paths. Short-input thresholds, CPU/OS feature guards, forced-portable tests and
end-to-end corpus evidence are mandatory.

Expected leverage: potentially high only after Layers 1–5 expose a stable hot
kernel. Current anonymous native samples make a specific code-generation goal
unvalidated.

### Layer 7 — long-lived program cache, then persistent artifacts

Status: **required for a long-lived-process throughput claim; Grade C until a
repeated-request corpus measures parse/compile cost**

This is two ordered cache layers with different lifetimes and correctness
contracts.

#### Layer 7a — process-resident compiled-program cache

Keep immutable compiled units alive across requests in one server or worker so
the same source is not lexed, parsed, compiled, planned and JIT-compiled for
every request. The cache should retain canonical bytecode `OpArray` graphs,
immutable function/class metadata, typed plans and eligible bounded native
code. Request execution still starts from fresh mutable state.

This is a real current boundary, not an assumed future optimization. The CLI
entry reads, lexes, parses and compiles its main source before constructing one
`ExecutorGlobals`. `execute_included_file` likewise reads, tokenizes, parses and
compiles each non-short-circuited include. The current `include_once` set avoids
a second include only inside that executor; it is not a process-wide compiled-
unit cache. A long-lived SAPI must therefore separate immutable program state
from request state before claiming that warmed requests avoid compilation.

The cache key and validation contract include canonical source identity,
content or equivalent freshness identity, compiler/runtime ABI, feature and
generic mode, relevant configuration, and every dependency that can affect
compilation. Includes whose compilation depends on request-defined constants,
autoload side effects or mutable declaration state remain uncached until those
dependencies have explicit generations. `include` may execute cached bytecode
more than once as PHP requires; `include_once` membership remains request-local.

Cached code must never retain request-owned globals, static values,
superglobals, included-file membership, handlers, exceptions, output buffers,
resources, coroutine state or mutable service objects. Request-local symbol
tables may point at immutable cached declarations, but inline caches and native
code that embed class/function identities must validate process-stable
generations or rebind before entry. Invalidation publishes a new generation;
an old unit stays alive while an in-flight request can still execute it.

The cache is memory-budgeted and observable. Telemetry records source lookups,
compile hits/misses, invalidations and their reasons, reused bytecode/typed/JIT
bytes, evictions, warm-up cost and resident memory. A disabled cache remains a
canonical comparison mode.

Acceptance requires a repeated-request application corpus that separates first
request, warm request and steady-state throughput; proves source and transitive
include invalidation while requests are active; resets every request-owned
state category; produces identical output with the cache disabled; and reaches
a bounded RSS plateau on ARM64 and physical x86-64. The gain must include cache
lookup and invalidation checks, not compare a warmed executor with cold process
startup.

#### Layer 7b — versioned cross-process artifact

Only after Layer 7a establishes the immutable/mutable boundary should a
versioned `.rphpc` artifact preserve bytecode, portable typed IR, profiles and
dependency hashes across process or server restarts. Target-specific native
sections may follow only with architecture, CPU-feature, relocation,
optimization-policy and runtime-helper ABI validation. Any mismatch discards
the affected unit and returns to ordinary compilation; unchecked executable
memory is never restored.

The current default-JIT checkpoint keeps fresh-process startup inside the
one-percent gate and bounds executable mappings, so Layer 7b remains behind the
in-memory cache and a measured restart/deployment workload. Layer 7a may move
ahead of lower-numbered speculative throughput layers when a long-lived server
becomes the explicit product target, but it still consumes the same canonical
bytecode, typed IR, native and materialization contracts rather than creating a
second execution engine.

### Layer 8 — long-lived-runtime memory lifecycle

Status: **production requirement, not current throughput priority; Grade D for
performance ordering**

Preserve deterministic reference counting and the acyclic hot path, then add a
non-moving candidate-driven cycle collector only when long-lived server tests
need it. Scalars and programs that never create cycles must gain no allocation
or poll. Frames, closures, generators, resources, coroutines and published
native state must participate in one root/safepoint contract.

This layer is semantically necessary before broad long-lived-process claims,
but it does not precede the measured core layers merely because it appears in
the memory roadmap. Its admission gate is a stable-RSS long-running corpus plus
the existing one-percent acyclic controls.

## Explicitly deferred or rejected as next steps

| Candidate | Current decision | Evidence needed to reconsider |
| --- | --- | --- |
| Native lowering only for value `foreach` | Do not schedule. The typed runner is already faster than PHP no-JIT and the default-JIT binary adds no benefit in the current row. | A profile showing native-entry/typed-dispatch cost dominates a new representative `foreach` shape. |
| More String or packed-array append fusion | Do not schedule from coverage alone. Existing typed kernels are already close to or faster than the current PHP controls. | Allocator/COW telemetry and a corpus/holdout with material append time. |
| Broad call-frame rewrite | Do not schedule globally. Current native corpora execute millions of logical calls with only 6–9 physical frame lifecycles. | A general non-region workload with counted frame traffic on both hosts; Layer 1 is the bounded route. |
| Broad object virtualization | Do not widen the read-only proof directly. Identity, writes, constructors and destructors need a shared materialization design. | A new escaping corpus graph and Layer 2 virtual-value contract. |
| General straight-region compiler | Static `no_typed_span` sites are not execution-weighted hot gaps. | Sampled corpus time and exact operation mapping. |
| Regex/JSON special kernels | Existing specialized families are migration inputs, not default priorities. | A current representative pipeline rejected by the common IR after Layers 1–3. |
| Handwritten assembly or architecture-only semantics | Rejected as a starting point. | Portable typed/intrinsic implementation, normalized disassembly and dual-host end-to-end proof. |
| Coroutine expansion | Deferred behind core execution, representation and dual-backend gaps. | Explicit coroutine product goal and proof of zero ordinary-call tax. |
| BigInt/BigDecimal, typed numerical buffers, GPU and Trader work | Separate future product/capability programs, not optimizations of the current PHP corpus. | Explicit product goal, compatibility contract and their own corpus. |

## Next decision checkpoint

The next bounded goal should validate **Layer 1 direct-call regions** rather
than implement another loop kernel:

1. add or identify an independent closure-heavy application holdout;
2. reproduce the current ARM64 `direct_call_shape`, frame and slot traffic on a
   clean locked x86-64 run;
3. sample the exact current build and attribute time between call setup,
   capture publication, closure-body execution and cleanup;
4. define one shared function/closure typed call ABI and exact exit envelope;
5. proceed to a small dual-backend implementation only if the holdout and both
   hosts retain material removable cost; otherwise record the rejection and
   select the next Layer 2 corpus graph.

This admission checkpoint has the best current price-to-information ratio. It
tests the largest visible gap, protects the project from a microbenchmark-only
call special case and, if accepted, supplies the foundation required by the
next two performance layers.

## Maintenance

After every accepted performance checkpoint:

- update the completed-foundation row that now owns the capability;
- update evidence grades and the first not-yet-admitted layer;
- preserve exact result links instead of copying raw logs or private-host
  details;
- mark rejected variants and the evidence required to reopen them; and
- regenerate the scorecard before changing the order.

The integrating agent remains the owner of `docs/roadmap.md`. This document may
describe measured performance ordering, but it does not assign shared-file
ownership, change compatibility priority or authorize a direct push to `main`.
