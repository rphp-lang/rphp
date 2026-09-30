# Literal method resolution memo checkpoint

Status: accepted bounded instruction/time checkpoint. PHPStan parity remains open.
Baseline: clean `52ac7731`; production source remains
`f24637cce89489cd2a8c323d6dd2b1db4ce2af85fb7cdf0af5dc43d8cee260d7`,
with exact accepted executable
`26410682002e68c683bd673d477d5a79c93920d2ea3b5ef883dff692889b2f6e`.
Analysis-only instructions remain 79.8590 billion versus PHP's 10.3365 billion.

## Evidence, outcome and hypothesis

The exact analysis-only profile records 1,186,869 cold method initializations,
with 4.2727 billion inclusive instructions. Those costs include scope probes,
method metadata, formatted callable names and function lookup; they overlap
callers and cannot be added to the whole profile again.

A separate fresh-cache diagnostic snapshot at the same baseline counts
1,205,232 cold resolutions over the complete command. There are 1,112,036
literal, non-stateful resolutions in apparently stable caller scopes, covering
only 18,113 unique site/receiver-class keys: 1,093,923 repeat earlier keys.
These classification counts are an opportunity bound, not promised admissions
or native performance measurements. Diagnostic output agrees with baseline and
PHP. Simple getter frames are also measured, but are outside this checkpoint.

Retain a complete already-proven literal instance-method cache state when the
receiver class changes. Refill the existing primary cache on a repeated key
instead of rebuilding class/method strings and re-running immutable resolution.
Use the existing request-local member memo representation, also used by property
reads/writes. Primary-cache hits keep their current execution path. Only a miss
can consult the memo; a restored entry resumes the same InitMethodCall before
any PHP effect. Do not change call/return admission, binding, frame ownership,
Value/opcode/frame/JIT layout, method metadata or PHP semantics.

## Envelope, ownership and boundedness

A memo entry requires a literal name, a nonzero receiver class, a concrete
instance method, no magic call and a scope that the canonical caller_scope
probe proves fixed by an ordinary function pointer. Rebound closures, unresolved
trait composition and dynamic scopes retain full resolution. Stateful method
hooks, including the inherited GlobIterator policy, and static methods called
through objects are excluded. Preserve the complete method flags, generic and
trait-composition metadata in the primary entry. Generic contracts continue to
be instantiated and checked for each receiver; no object-specific contract is
memoized. Existing return-dispatch guards execute after refill.

Cache keys use the live request-owned cache allocation, instruction index and
receiver class. Opcode identity separates method and property entries. No PHP
callback may occur while borrowing the memo, and no restored pointer may outlive
its request-owned function descriptor. New method admission stops at 65,536
existing memo entries; property policy is unchanged and any miss retains the
canonical lookup. The method memo itself therefore cannot grow past this bound.

The sole integrating task owns runtime member-memo metadata, the cold method
resolver, its baseline dispatch miss boundary, focused tests and this evidence
in the isolated performance worktree. No other implementation goal is active.

## Gates and stop conditions

Run focused method-resolution/visibility, trait, rebound-closure, generic-member,
stateful GlobIterator and reference-return tests in the required aggregate
memory boundary. New fixtures must agree with PHP and the exact baseline before
candidate timing. Check default/no-default/all-feature configurations for this
slice, formatting, unsafe inventory and all-target compilation. Build fresh
independent PGO from the unchanged eighteen inputs; exclude PHPStan and holdouts.

Compare exact analysis-phase instructions, whole-command counters, fresh-cache
PHPStan output, time/RSS and established shared/scalar/property/regex controls.
Add an independent alternating-receiver-class method holdout with both successful
and fallback contracts. Retain every valid sample. Confirm any timing change
outside the one-percent control envelope independently, with hardware frontend
counters if needed. The preceding borrowed-return experiment is rejected for
such a native regression and cannot become this checkpoint's baseline.

Reject on changed visibility, scope, diagnostics, argument/callback order,
generic/reference semantics, replayed effects, pointer lifetime failure,
unbounded method cache growth, no instruction reduction or a confirmed control
regression without an accepted evidence-backed tradeoff. Native evidence is
x86-64 only; ARM64 evidence remains unavailable.

## Accepted result

Reuse complete primary method-cache words from the existing request-local
member memo. Publication follows canonical method dispatch and visibility,
with a literal concrete instance method, immutable caller scope and no stateful
hook. Restoration happens only on a receiver-class mismatch, before argument
or PHP effects, and retries the same InitMethodCall through its unchanged
primary-cache path. Matching-class return-contract failures do not retry.
The primary generic, trait-scope and return-dispatch flags round-trip intact.

The diagnostic snapshot proves 1,072,607 restores in the complete PHPStan
command. Canonical cold resolutions fall from 1,205,232 to 132,694; 17,765
method states are admitted and the measured member memo peaks at 33,530 entries.
These are actual coverage counts, separate from native measurements.
Alternating public/private dispatch and reference-return fixtures restore entries;
rebound closures and GlobIterator fixtures record zero restorations. Six exact
PGO fixtures match PHP and baseline. The known last-owner callback gap remains
byte-equal to baseline and is not a PHP pass.

| Confirmed metric | Baseline | Candidate | Reference PHP |
| --- | ---: | ---: | ---: |
| Analysis instructions | 79.8377 billion | 76.7068 billion | 10.3380 billion |
| Analysis time, median | 7.3305 s | 6.9115 s | 0.9248 s |
| Whole-command instructions, median | 98.4659 billion | 95.3183 billion | 15.4743 billion |
| Maximum RSS, median | 618,146 KiB | 620,102 KiB | 177,438 KiB |

Phase instructions improve 3.92% in independent confirmation after 3.94%
initially. Confirmed analysis time improves 5.72% and whole-command instructions
3.20%, with 1,956 KiB additional RSS. All five files, twenty findings, ordinary
stdout/stderr and exit status agree, using fresh result-cache storage each time.

The independent alternating-class holdout, excluded from PGO, improves 31.06%
instructions and 38.73% time. Existing inherited-method controls improve 34.76%
instructions and 37.47% time. The separately scaled confirmation programs are
never pooled with initial controls. All valid samples, medians and ranges are
retained in [the evidence packet](performance-phpstan-method-resolution-memo-samples.json).

There are explicit timing tradeoffs: scalar-frame return is 4.34% slower in
confirmation while its instructions decrease 0.034%; regex is 1.13% slower
with 0.008% fewer instructions. The scalar hardware diagnostic records frontend
no-op slots rising from 0.6796 to 1.7396 billion. This supports an instruction-
supply effect, but no specific alignment or placement cause is proven.
Shared-temporary timing varies between windows, so no improvement is claimed.
The integrating task accepts these visible tradeoffs under the user's
instruction priority and the measured application/dispatch gains. This is not
an all-program speedup claim.

All 115 focused default/no-default/all-feature executions pass, including
visibility, scope, trait, reference, stateful and generic-receiver boundaries.
Formatting, unsafe inventory and all-target/all-feature compilation pass.
No new unsafe operation or Value/opcode/frame/JIT layout is introduced.
Fresh PGO uses the same eighteen independent programs, excluding PHPStan and
all controls. Source is
`bdf1e318157656cd70248ffbc058565247689600f8df2f21c1c446a78818b3d9`;
executable is
`5718f9bfb75856c4d0c44624c928a9118fcc2c17f5fa6fcac90886086b194565`.
Executable size grows 6,584 bytes; the main executor remains 390,257 bytes.
Preparation peaks at 5,306,122,240 bytes below the verified 6 GiB aggregate
boundary, with no OOM or swap. Cleanup runs in both local checkouts; no private
benchmark host is configured. Exact binaries, source snapshots, PGO and raw
samples remain preserved. Native evidence is x86-64 only. Parity remains open
at 7.42 times PHP's analysis instruction budget.
