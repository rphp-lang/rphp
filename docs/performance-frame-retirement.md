# Ordered retirement of committed function frames

Status: rejected experiment. All three ordered-retirement prototypes are
preserved as evidence and their runtime changes have been removed. Independent
controls and a shutdown differential reject the last prototype. The separate
[owned-reference cleanup correction](performance-owned-reference-cleanup.md)
starts again from the unchanged baseline. This experiment starts from
`b9c344712b954d882e381774ee258abe60f43c29`; the rejected
[reusable-workspace variants](performance-release-workspace.md) are preserved
separately and their runtime changes were removed before this implementation.

The later corrected redesign is accepted separately in
[the parity checkpoint](performance-phpstan-parity.md#accepted-checkpoint-committed-frame-owner-retirement).
The three variants described here remain rejected historical evidence.

## Measured problem and change

The [ordinary-call reproduction](performance-ordinary-value-flow.md) pays
2,676 instructions per iteration in a whole-frame release planner, including
four hash-table allocations and three vector growths. Its caller still owns
the shared values after every invocation. Reusing planning storage removes
allocations but retains the graph construction and did not pass holdouts.

The prototype makes a committed function return relinquish its owned slots in order.
Each slot is cleared before any PHP callback and uses the existing final-owner
release proof. A shared owner simply loses one reference; a final owner enters
the established object/container release machinery. A later alias reaches its
natural destruction point without a frame-wide reference-count table, candidate
snapshot, or deferred fixed-point scan. Borrowed arguments remain outside the
owned bitmap. Wide frames use the existing initialized-value ownership tags.

The local owner stays alive through destructor callbacks. Throwing destructors
therefore do not discard their remaining children; the same release proof
continues before the next frame owner retires. Ordinary return exceptions keep
their replacement chain. Constructor completion retains its receiver until
local cleanup succeeds. Dynamic symbols move out in insertion order without
cloning their values. An empty symbol table uses the existing empty-aware lookup
instead of an unconditional `HashMap::remove`, which the first profile measured
at 183 instructions per return even for an empty table.

Final object ownership is proved before inspecting child metadata. This
reorders existing pure checks, rather than adding another shape admission
condition. Request shutdown, live-generator-only retirement, and resumable
exception/finally boundaries keep their existing planner. An explicit semantic
phase at the outlined boundary preserves the VM dispatch calling interface.
`Value`, `ExecuteData`, executor-global layout, bytecode and JIT ABI do not grow.

Successful detached callbacks in the prototype complete local retirement after argument
readback. The engine entry point explicitly supplies the callback's completion
policy. Shutdown callbacks use the existing shutdown destructor loop, which
invokes the exception handler independently for each failure. A synthetic
logical caller is not proof that an ordinary caller can receive a replacement
exception. Error-handler, autoload, reflection, tick and ordinary callback
entries retain ordinary completion policy.

The callback route does not repeat exceptional unwinding after an already
completed return. Terminal VM errors still reach unconditional detached-frame
cleanup. No callback borrow spans slot mutation or user re-entry. The new unsafe
block uses the same live-frame allocation and bitmap invariants as existing
cleanup; the unsafe-policy gate remains mandatory.

## First prototype: useful profile, rejected result

The first ordered-retirement prototype passed 96 focused test executions,
14 PHP differential programs, 41 constructor/dynamic-variable/capture/fiber
checks, and an additional ownership differential. On the same CPU-2 protocol,
its measured loop used 3,285.616 instructions per iteration versus 5,380.064
for the baseline. The disjoint costs were:

| Incremental instructions per iteration | Baseline | Prototype |
| --- | ---: | ---: |
| Main VM body, exclusive | 2,121.440 | 2,042.940 |
| Frame-retirement boundary, inclusive | 2,676.000 | 843.002 |
| All other work | 582.624 | 399.674 |
| Total | 5,380.064 | 3,285.616 |

The repeated hash allocations disappeared. The only observed vector growth
was one call across the entire 50,000-iteration incremental profile. Native
500,000-iteration medians were 136.321 ms baseline, 72.229 ms prototype and
12.158 ms PHP, retaining all seven randomized samples per runtime. IQRs were
135.147–137.113 ms, 71.761–72.703 ms and 12.143–12.233 ms respectively.
This is a narrow microbenchmark improvement, not a PHPStan result.

Independent eleven-pair confirmation still rejected the prototype: the mixed
loop regressed from 485.480 to 505.402 ms, and destructor-chain release from
3.161 to 3.266 ms at 2,048 nodes and from 12.672 to 13.124 ms at 8,192 nodes.
Declared-property foreach changed from 277.840 to 279.066 ms within overlapping
IQRs. The mixed-loop instruction profile was effectively unchanged:
9,003.515 versus 9,004.516 instructions per iteration. It does not execute the
new frame-retirement routine inside its loop. The empty-table cost is therefore
not an explanation of that mixed-loop regression; code generation/layout
remains a separate hypothesis. PHPStan was skipped by the declared pilot gate.

An additional shutdown differential found a semantic mismatch after the initial
packet: the prototype chained two local-destructor exceptions before invoking
the handler, while PHP handles each separately. The accepted baseline omitted
these local callbacks entirely. The first attempt to select shutdown behavior
from the saved active-frame pointer failed the new regression test; shutdown
callbacks can carry a synthetic main caller. That failure is retained as a
failed check. The second revision passes completion policy
explicitly from the shutdown engine entry instead.

## Second prototype: application gain, scalar regression

The second prototype removes the empty dynamic-table hash and explicitly
selects completion policy for registered shutdown functions. It uses
3,006.609 instructions per target iteration versus 5,380.075. The seven-run
native medians are 65.633 ms versus 136.001 ms, with PHP at 12.289 ms.
The original holdout pilot passes, allowing the application measurement.

Four fresh-result-cache PHPStan runs per binary in ABBA ABBA order measure
11.840 s baseline versus 10.884 s prototype for the analysis phase (-8.07%).
Peak RSS medians are 1,069,772 versus 592,316 KiB (-44.63%). All twenty findings,
five analysed files and exit status match. PHP's 0.935 s warmup is a single
control observation, not a four-run PHP distribution. This is the only
ordered-retirement revision with an application measurement.

An additional matched scalar-argument control rejects this revision: eleven
randomized runs give 33.081 versus 35.146 ms (+6.24%). Its incremental profile
increases from 1,668.337 to 1,720.194 instructions per iteration. Moving return
selection ahead of the existing owner-free frame exit unnecessarily enters
retirement machinery even when no owner exists. This finding becomes a required
control for the following revision; it is not excluded from the evidence.

## Final prototype: rejected after confirmation

The third revision restores the existing owner-free exit before phase selection
and includes the proven wide-frame reference cleanup correction described below.
Its source fingerprint is
`ae5d7c989bacb654890e57eaa57df40a8928fab87d08e3937941b83da45fec9e`.

| Incremental target instructions per iteration | Baseline | Prototype |
| --- | ---: | ---: |
| Main VM body, exclusive | 2,121.440 | 2,044.940 |
| Frame-retirement boundary, inclusive | 2,676.000 | 573.002 |
| All other work | 582.645 | 399.698 |
| Total | 5,380.085 | 3,017.641 |

The reduction is 43.91%. Four hash allocations and three vector growths per
iteration disappear; one vector growth remains across all 50,000 measured
iterations. Native target medians are 135.335 versus 68.467 ms (-49.41%),
and PHP measures 12.207 ms. The pilot still rejects scalar and deep-release
controls, so PHPStan is deliberately not rerun for this source.

Independent confirmation retains eleven randomized pairs per control:

| Control | Baseline median (IQR), ms | Prototype median (IQR), ms |
| --- | ---: | ---: |
| Scalar argument/alias loop | 33.115 (33.008–33.227) | 34.484 (34.180–34.759) |
| Destructor chain, 2,048 nodes | 3.133 (3.098–3.176) | 3.156 (3.117–3.197) |
| Destructor chain, 8,192 nodes | 12.680 (12.580–12.783) | 13.000 (12.945–13.049) |
| Declared-property foreach | 279.382 | 258.030 |
| Mixed loop | 486.287 | 490.999 |

Scalar retirement itself is restored to exactly 30 instructions per iteration
in both binaries. Total scalar instructions are 1,668.354 versus 1,671.405
(+0.18%); almost the entire difference is three instructions in the main VM
body. This explains why the extra return traversal was removed, but does not
explain the remaining +4.14% native regression. The +2.52% deep-release signal
at 8,192 nodes is also outside its recorded IQR. Code layout and hardware
prediction remain hypotheses: native performance counters are unavailable, and
simulated counters from a different variant are not proof for this binary.
No further inline/layout variants were attempted after this confirmation.

The final prototype passes 150 focused test executions and eighteen initial
PHP differential programs, plus the repeated ownership fixture. A subsequent
root-destructor differential nevertheless fails. For a root destructor that
returns while owning two throwing locals, the outputs are:

```text
PHP:       body|root|drop:first|caught:first|drop:second|caught:second|
Baseline:  body|root|
Prototype: body|root|drop:first|drop:second|caught:second|caught:first|
```

Selecting shutdown policy only for registered shutdown functions is incomplete:
root-object destructor callbacks require the same independent error handling.
The passing packet must not be described as complete shutdown compatibility.
This semantic mismatch independently prevents acceptance, even if the native
tradeoff were approved. The existing baseline bug is preserved as a documented
counterexample rather than a passing test.

## Confirmed reference-cell leak and disposition

The old wide-frame raw cleanup enumerates string, array, object and closure
tags, plus resources in lifetime builds, but omits owned reference cells.
A direct ownership test leaves an externally retained object with strong count
2 after an 80-slot frame is cleaned; the same two-slot frame correctly leaves
count 1. The failing cleanup body is byte-for-byte identical to baseline
`b9c34471`. This is a physical leaked owner, independent of exception-trace
retention or a missing PHP destructor callback.

The separate correction uses `Value::needs_cleanup`, the canonical ownership
predicate already used when slots are published. It includes owned references
and excludes borrowed reference pointers. All return-phase, callback-policy,
dynamic-map and planner-reordering changes have been removed before verifying
that correction. None of the experimental speedups above describe that
smaller candidate.

[Raw samples and verification summaries](performance-frame-retirement-samples.json)
retain every valid measured run from the three prototypes, rejected controls,
failed development checks, fingerprints, executable hashes, build cost and
resource boundaries. Quartiles use linear interpolation within the sample range.

## Verification and measurement boundary

The selected packet covers normal/implicit/typed/finally returns, direct and
container aliases, compact and wide frames, constructor eligibility, reference
capture lifetime, dynamic symbols, weak objects, re-entry, exception chains,
callback `exit()`, generator force-close and suspended destructor release.
Formatting and unsafe-policy checks accompany default, no-default and all-feature
runs. This is a focused packet, not a full compatibility matrix or an ARM64
performance claim.

The existing benchmark protocol is unchanged: Ryzen 9 7950X, x86-64 Linux,
approximately 32 GiB RAM, performance governor, Rust 1.98.1/LLVM 22.1.8,
`max-perf`, fat LTO, one codegen unit, default features and repository alignment.
Reference PHP is 8.5.11 NTS with CLI OPcache/JIT disabled. Callgrind subtracts a
zero-iteration control from 50,000 iterations. Native loops validate output,
warm once, retain seven randomized pairs and pin to CPU 2. A regression gets
an independent eleven-pair confirmation with every valid run retained.
PHPStan runs only after the declared holdout pilot allows it, using the same
five public tool files, fresh result-cache directories and warm OS cache.

All expensive work runs serially under the exclusive benchmark lock, verified
6 GiB aggregate RAM/zero-swap limits, whole-group termination, and cleanup
before/after the job. Failed checks remain visible; no OOM is counted as a pass.
