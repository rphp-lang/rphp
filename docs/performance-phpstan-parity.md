# PHPStan instruction and runtime parity

Status: active; partial checkpoints do not complete this goal.

The outcome is the same five-file PHPStan analysis, with identical diagnostics,
exit status and PHP semantics, at the reference PHP instruction count and
analysis time. Startup is measured separately. A result-cache hit is not an
analysis and cannot satisfy this goal.

The user subsequently makes retired instructions the deciding optimization
criterion. Native time, spread and controls remain visible diagnostics, but a
small timing regression alone does not reject a verified reduction in the
application's instruction budget. This supersedes the earlier time-based
one-percent acceptance rule for this task; it does not relax PHP semantics,
correctness, resource bounds or measurement integrity. The instruction target
is compared within the analysis phase, whose reference budget is approximately
10.33 billion instructions.


Baseline: `2ee8e73dd532051b73ddf0ff70255a48923aeec7`, default features,
`max-perf`, PHP 8.5.11 NTS with CLI OPcache/JIT disabled. Both runtimes use the
same PHAR, fixture, process-disabling settings and `zend.exception_ignore_args=0`.
Each analysis receives a fresh temporary directory. The previous accepted
checkpoint measured 11.509 seconds for RPHP's analysis versus 0.926 seconds for
PHP; its memory correction did not resolve the performance gap.

The earlier whole-application profile at `ffe29165` counted 155.357 billion
instructions versus PHP's 15.577 billion. Approximately 30.027 billion belonged
to main dispatch, 21.866 billion to other VM call/frame operations, 21.614
billion to explicit release planning, and 24.004 billion to general containers,
hashing, drop and string support. Those old totals are evidence for investigation,
not the instruction count of the current baseline. A new source-attributed
whole-application profile will establish the current budget.

The completed current-baseline profile counts **152,418,412,307** instructions
for RPHP and **15,606,913,817** for PHP, a 9.766-fold ratio. Both used the
unmodified PHAR and identical fresh analysis settings; diagnostics, stderr and
exit status matched. The RPHP diagnostic build adds line tables to `max-perf`.
All 8,629 RPHP text symbols have the same sizes as the previously accepted
build; native A/B comparisons still use matching flags on both sides. The
profile service completed without OOM or timeout, with a 3.859 GB aggregate
peak including its build. The central opcode match accounts for 0.824 billion
instructions, so dispatch selection alone cannot explain the remaining gap.

Implementation hypotheses must remove repeated runtime work through general
ownership, representation or immutable metadata. No PHPStan-specific names,
types, source patterns or reduced semantic checks are admissible. Existing
diagnostics, aliases, COW, exception/destructor ordering, dynamic binding,
autoload and suspended activations remain part of the semantic envelope.

This integrating task temporarily owns affected compiler, value, runtime and VM
files in its isolated performance checkout. It has no concurrent editing agent.
Each implementation is evaluated against the whole application before broad
test expansion. A small microbenchmark improvement cannot replace that gate.
Focused differential cases cover the changed semantic boundary. Accepted
checkpoints additionally pass formatting, the applicable unsafe policy and
feature checks, plus independent runtime holdouts. Architecture-independent
changes must not introduce architecture-specific behavior; unavailable native
architecture measurements are recorded as unavailable.

Profiles, builds and benchmark cycles use an exclusive lock and a separate
aggregate memory boundary with 6 GiB maximum memory, no swap and process-group
cleanup. Every valid sample, output mismatch, timeout and OOM remains evidence.
Diagnostic line-table builds are identified separately from native acceptance
builds. Native A/B builds use identical profiles and toolchains.

The first implementation checkpoint replaces repeated string-based class
relations with the existing stable declaration IDs. The old application made
18.289 million `class_is_a` calls, responsible for 36.581 million class-name
lookups. Objects already carry their declaration ID; constant `instanceof`
operands can retain successful resolution in their instruction's cache.
Ancestry is immutable after publication. Unresolved declarations and dynamic
class expressions must preserve normal lookup behavior, including later alias
publication; misses cannot be cached as permanent failures. Focused gates cover
parent/interface relations, aliases, anonymous classes, implicit Stringable,
relative scopes, dynamic targets and class type hints.

Reject an implementation if its predicted application budget is not removed,
if it changes observable behavior, or if its regression cannot be justified by
the integrating task. Rejection returns to the application profile; it does not
complete or suspend the parity goal. Commit and push coherent verified
checkpoints while continuing toward the full outcome.

## Accepted checkpoint: resolved class identities

Objects now use their existing class ID for class type checks. Transitive
ancestry retains numeric membership alongside names needed for unresolved
declarations. Constant `instanceof` operands retain successful resolution in
their existing instruction cache; unresolved names continue to observe later
declarations and aliases. Incomplete ancestry is rebuilt when the class table
grows. Dynamic operands and relative scopes retain their normal resolution.

The whole command falls from **152.418 to 147.854 billion Callgrind
instructions (-2.99%)**. Reduced class lookup, case folding and comparison
account for the improvement; new ancestry membership work is included in the
total. Reference PHP uses 15.607 billion instructions under that profiler.

Two native measured runs per RPHP binary in ABBA order, after one warmup per
runtime, give analysis medians **11.521 to 11.141 seconds (-3.30%)**. Every run
has fresh result-cache storage, five analyzed files, twenty identical findings
and the same exit status. PHP's measured analysis is 0.939 seconds. Candidate
RSS increases from 592,744 to 594,696 KiB, about 1.9 MiB for ancestry metadata.

An independent hardware-counter pass counts **150.036 to 145.542 billion
retired user instructions (-3.00%)**, with PHP at 15.474 billion. All counters
ran for 100% of their enabled interval. This separate window measures analysis
medians of 12.159 and 11.881 seconds; these samples are retained separately from
the earlier native window. Both instruction protocols count the complete
command, whereas analysis timers exclude interpreter and PHAR startup. Their
totals must not be mixed.

Validation: 20 instanceof/catch cases under default, no-default and all features;
174 type-hint cases and 12 class-alias cases under default features; two focused
PHP differential programs; formatting, unsafe policy and all-target/all-feature
checks. All pass. Three randomized pairs for each independent holdout show
changes from -0.68% to +1.03%; these small changes are retained, with the positive
whole-application result accepted by the integrating task. No OOM or timeout
occurred. ARM64 native measurements are unavailable; the change introduces no
architecture-specific implementation. Exact samples and build identities are
in [the checkpoint data](performance-phpstan-class-identities-samples.json).

This checkpoint removes approximately 4.5 billion instructions. More than
130 billion excess instructions remain. It does **not** complete parity.

## Active checkpoint: allocation-owned cycle admission

- Outcome: repeated releases of an already recorded owner do not allocate weak
  handles or search the GC root index. The collector remains authoritative.
- Baseline: `1c7a7e2e081937d27755eb98bf829cc35f8cc18f`, clean, identical build
  flags and fixture as above. The source profile attributes 7.314 billion
  exclusive instructions to root admission and 0.821 billion to candidate drop.
- Scope: put an admission generation beside each cycle-capable Rc payload;
  preserve its payload address using a first-field C representation. Metadata
  sits outside mutable object RefCell storage. No instruction, name or input
  recognizers are introduced.
- Semantics: a successful registry admission stamps the owner. Request changes,
  startup enablement, callback recording and completed collection invalidate
  old stamps. Retained callback roots are restamped. Snapshots do not admit
  roots; array separation starts with fresh metadata. Weak owners retain no
  PHP lifetime. All raw Rc operations must use the complete allocation type.
- Ownership: the integrating agent owns value representation and affected weak
  runtime sidecars; no other agent edits this worktree.
- Gates: focused GC, weak ownership, COW and destructor differential tests;
  default/no-default/all-feature checks; unchanged unsafe-policy ceiling;
  same-output fresh PHPStan hardware instruction and timing A/B, independent
  frame/array holdouts, allocation accounting and peak memory. Native evidence
  is x86-64; no architecture-specific implementation is added.
- Rejection: reject this representation if root timing or destructor behavior
  changes, a live root can be suppressed after collection, or application
  instructions/time fail to improve. This remains a partial parity checkpoint.


### Allocation-owned admission result

Accepted as a partial checkpoint with an explicit scalar-control tradeoff.
The source fingerprint is
`a69155320a7313fc3cd56dbfaff9dba1916c6c25917c50ab1cc0fcfad05b7b52`.
All samples, counter evidence and checks are in
[the owner-admission packet](performance-phpstan-owner-admission-samples.json).

The generic allocation wrapper preserves payload identity at offset zero and
keeps admission metadata outside mutable payload storage. A physical array copy
constructs a fresh wrapper. The wrapper intentionally does not implement Clone:
Rc::make_mut could otherwise move an allocation with only weak observers without
resetting its metadata. Live Weak stamping projects only the metadata Cell and
never borrows a mutable PHP payload. All Rc casts, weak sidecars, unique-owner
unwrapping and existing checked allocation reservations use the new layout.

Two interleaved samples per runtime, with a fresh analysis cache each time, give:

| Whole-command / analysis measurement | Baseline | Candidate | PHP |
| --- | ---: | ---: | ---: |
| Hardware instructions, billions | 145.5400 | 138.6911 | 15.4740 |
| Analysis median, seconds | 10.98055 | 10.66777 | 0.91972 |
| Peak RSS median, KiB | 592466 | 598830 | 177666 |

This removes 6.849 billion instructions (4.706%) and 2.849% of analysis time.
Metadata adds eight bytes per cycle-capable allocation; measured peak RSS rises
1.074%. Whole-command instructions include the same time wrapper on every side;
these are a new native measurement window, not Callgrind or phase-only counts.
All five files, twenty findings, stderr and exit status match. The service stays
below 624 MB aggregate memory with no OOM or timeout.

Three-pair holdout medians improve 9.0% for shared-frame ownership and 12.3% for
declared-property foreach; the mixed native scalar control improves 1.3%.
The pure scalar-frame control regresses 4.5%. Its independent five-pair,
five-million-iteration confirmation gives 333.836 versus 350.356 ms (+4.95%).
Hardware counters show 8.36564 versus 8.37064 billion instructions (+0.060%),
one extra branch/instruction per loop iteration, 1.78881 versus 1.88113 billion
cycles, and no median branch-miss increase. Main-executor code size grows by
4,770 bytes. These facts do not fully attribute the native regression; code
layout/frontend throughput remains a hypothesis, not a proven cause.

The integrating agent accepts that isolated time regression and the 1.1% memory
increase against the application instruction/time gain and the two ownership
control gains. The scalar program remains a mandatory follow-up control; this
checkpoint is not an all-workloads speedup. No layout or benchmark recognizer is
added to conceal the regression.

Validation: 178 targeted unit/integration test executions across default,
no-default and all features, including memory limits, wide-frame references,
weak owners and Fiber/GC boundaries; eight direct PHP/baseline differentials;
all-target/all-feature compilation; formatting; unsafe-policy check. The new
Weak metadata projection raises the unsafe-block inventory to 1746/1747 while
unsafe functions remain 321/321. No ceiling is changed. The build/check services
peak at 4.072 GB with no memory-limit or timeout failure. The scalar confirmation
service waited for the test service's exclusive lock; its service elapsed time
is not benchmark execution time.

Parity is not achieved: candidate analysis remains about 11.6 times PHP and
whole-command instructions about 9.0 times PHP. The next bounded implementation
will address committed-frame owner retirement, preserving callback completion
and destructor exception order, instead of rebuilding a frame-wide owner graph.

## Accepted checkpoint: committed-frame owner retirement

- Outcome: release actual owned frame slots on a committed return without
  constructing a graph/count table of the whole frame.
- Baseline: `3290af6b61439ac31e1e68efae18134bf14aefe5`; current source-attributed
  evidence charges about 15 billion inclusive instructions to frame planning.
  The earlier rejected prototype demonstrated removal of repeated maps but
  failed root-destructor exception ordering; it is not an accepted baseline.
- Hypothesis: detaching each owned slot before callbacks makes the current
  reference count authoritative. Borrowed arguments stay outside the bitmap.
- Scope and proof: retain construction receivers, dynamic symbols, PHP return
  values and detached argument readback correctly. Ordinary call completion
  chains exceptions; request shutdown handles failures independently. Carry
  that policy explicitly from engine shutdown entries through destructor
  callbacks, including root, static, surviving and handler-owned objects.
  Do not infer shutdown from a synthetic trace frame or workload identity.
- Ownership: the integrating agent owns the affected VM and callback entry
  files. Accepted class-identity and allocation-header changes remain intact.
- Gates: the previously failing root-destructor differential first, then
  focused return/reference/destructor/exception/callback tests and feature
  checks; same-output PHPStan instruction/time A/B; scalar and shared-frame
  controls retained. Use the current corrected-memory baseline for RSS.
- Rejection: any lost/duplicated callback, exception-order or alias-lifetime
  difference rejects the implementation. Investigate and report performance
  regressions instead of tuning for source names or accepting micro-only wins.


### Retirement entry investigation

The first candidate removes 7.863 billion whole-command instructions and reduces
PHPStan analysis from 10.7990 to 10.0908 seconds, but its scalar control regresses
15.7% in the short holdout and about 20% in independent longer runs. Instruction
and branch counts are unchanged. Native sampling attributes 95% of cycles to the
main executor. Hardware counters show more frontend-empty slots, op-cache misses
and branch misses; the candidate is not accepted on this evidence.

The next revision reuses the existing exact no-owner proof before crossing into
cold destructor code. Committed retirement also no longer passes through the
large live-frame planner. This removes unnecessary calls and stack setup for an
ordinary owner-free frame; it is independent of source shape, types of arguments
and workload names. Retain the first candidate and all measurements as evidence.


### Retirement result

The accepted source fingerprint is
`b8cb2c2f7ff7bd18c40e53fb8e7d43cc573ad61995fe96578c6daa9da4f863db`.
Every application, control and confirmation sample is retained in
[the frame-retirement packet](performance-phpstan-frame-retirement-samples.json).
The first native entry variant remains rejected evidence; its separate safety
and compatibility results do not substitute for the accepted source's checks.

| Same-window measurement | Baseline | Candidate | PHP |
| --- | ---: | ---: | ---: |
| Whole-command hardware instructions, billions | 138.7018 | 130.5969 | 15.4739 |
| Analysis median, seconds | 10.77867 | 10.19991 | 0.92177 |
| Peak RSS median, KiB | 598972 | 598700 | 177762 |

The change removes 8.105 billion instructions (5.843%) and 5.370% of analysis
time, with equivalent memory. All five files and twenty findings match PHP.
It performs actual slot retirement before callbacks, preserves borrowed owners,
retains construction receivers until completion and moves dynamic symbols in
insertion order. Detached callback argument readback precedes retirement.
Explicit ordinary/shutdown completion policy preserves exception replacement
chains and independent request-handler dispatch, including nested destructors
and last-alias order. Existing resumable boundaries keep their planner.

Three-pair holdouts improve 50.3% for shared frames, 4.6% for scalar frames and
10.3% for declared-property foreach; the mixed scalar loop changes -0.75%.
An independent five-pair, five-million-iteration scalar confirmation gives
340.226 versus 323.938 ms (-4.787%), 17 fewer instructions per iteration and
essentially unchanged branch misses. The main executor shrinks by 163 bytes.

Hardware startup-subtracted shared-frame counts fall from 4,974.941 to
2,754.941 instructions per iteration, versus PHP's 516.001. Scalar counts fall
from 1,671.342 to 1,654.342, versus 395.501. Separate Callgrind attribution puts
2,045.440 shared-frame instructions in the main executor and 367.004 in committed
retirement, inclusive. This is still a large ordinary-operation gap.

The deep-destructor release phase regresses: five-pair controls show +5.0% at
2,048 nodes and +2.9% at 8,192. Independent 16,384-node confirmation gives
24.773 versus 25.388 ms (+2.48%). Build plus release is 30.944 versus 31.026 ms
(+0.27%), and whole-program hardware instructions fall 1.62%. Source accounting
at 2,048 nodes attributes 233 instructions per node to newly completed detached
destructor callbacks, which the old implementation skipped. The integrating
task accepts this phase-specific tradeoff against restored PHP behavior and the
application, instruction and ownership gains. It remains a mandatory control.

The exact accepted source passes 95 focused test executions across default,
no-default and all features; 22 direct PHP differentials; all-target/all-feature
compilation; formatting and the unsafe-policy gate. These cover callback and
exception completion, references, dynamic/owned frame boundaries, generators,
Fibers and native resource callbacks. The unsafe inventory stays within its
existing ceiling at 1747 blocks and 321 functions. No JIT lowering or ABI is
changed; native timings apply only to x86-64. All aggregate jobs stay below
4.23 GB with no OOM or timeout. The confirmation service waited for the gate's
exclusive lock; its service elapsed time is not benchmark duration. Cleanup ran
in both local checkouts, and no private benchmark host was configured.

PHPStan remains 11.1 times slower in actual analysis and uses 8.4 times the
whole-command instructions. A fresh native profile still charges about 22% of
cycles to the main executor, 5% to class lookup and several percent to temporary
release and its snapshot/graph machinery. Continue from those general costs;
this accepted checkpoint does not complete the parity goal.


## Rejected checkpoint: resolve type scopes when consumed

- Outcome: concrete/scalar return contracts do not allocate and resolve unused
  lexical class metadata. Relative contracts keep exact existing resolution.
- Baseline: `895941b2446631149dc2ffc32481f62e2ba08466`. The source profile counts
  4.105 million return-check calls into caller scope, costing 1.181 billion
  inclusive instructions before checking a type. The latest native profile
  still charges 2.09% of cycles to caller scope and 5.02% to class lookup.
- Hypothesis: a shared type-check context carries either supplied class names
  or a live return frame. Only the existing self/parent/static type cases
  project the scope they consume; recursion retains the same context.
- Semantics: preserve union/intersection order, aliases, strict/weak conversion,
  closures, traits, lexical versus late-static scope, diagnostics and fallback
  names for unresolved relative contracts. No additional workload/type admission
  path or preemptive scope-string creation is introduced.
- Ownership: integrating task owns the canonical VM type checker. Frame
  retirement and allocator metadata are already accepted and remain baseline.
- Gates: focused relative return, trait, closure, argument and coercion tests;
  direct PHP differentials; relevant feature checks; fresh identical-output
  PHPStan instruction/time comparison and existing ownership/scalar controls.
- Reject on a changed PHP result or if application work does not decrease;
  investigate every independent holdout regression before acceptance.

### Scope-context result: rejected

The candidate preserves the focused PHP results and removes 0.987 billion
whole-command instructions (130.610 to 129.623 billion, 0.76%). Analysis medians
are 10.09469 and 9.99448 seconds. However, independent five-pair confirmation
at five million iterations gives 594.742 versus 639.114 ms for shared frames
(+7.46%), 319.835 versus 326.467 ms for scalar frames (+2.07%), and 1079.030
versus 1176.009 ms for relative `self` returns (+8.99%). Property-foreach
controls also regress 1.62%. All valid samples are retained in private evidence.
The compiler changes generated layout even in untyped controls; their instruction
and branch-miss counts barely change. The small application gain does not justify
these regressions. The implementation is removed; the trait/closure/relative-type
PHP differential remains useful regression coverage. The accepted runtime stays
at the committed-frame-retirement checkpoint. This rejection does not complete
the parity goal.

## Rejected checkpoint: automatic-collection instruction boundary

- Outcome: ordinary bytecode dispatch does not materialize exception/collection
  state before a collection has been requested; collection remains at precisely
  the same per-instruction boundary.
- Baseline: `895941b2446631149dc2ffc32481f62e2ba08466`. The main executor accounts
  for 22.21% of native cycles and roughly 30 billion instructions in the prior
  source profile. Exact baseline disassembly loads executor exception state and
  combines it with the TLS collection flag on every opcode. The collector body,
  admission vectors and destructor handling are inlined into this same function.
- Hypothesis: a cold, non-inlined boundary handles existing exception exclusion,
  origin publication, collection and throw dispatch only after the pending flag
  is set. The ordinary loop needs only the pending-flag probe.
- Semantics: preserve opcode-by-opcode polling, interrupt cadence, prior-opcode
  exception origin, pending-exception exclusion, frame restoration before a VM
  error propagates, and handled/unhandled collection exceptions. No GC batching,
  workload detection, frame-layout change or JIT change.
- Ownership: integrating task owns the baseline dispatcher and the extracted
  frame-access boundary. One additional annotated unsafe block separates the
  existing raw-frame accesses into two functions; its invariant is unchanged.
- Gates: existing cycle, finalization and interrupt checks; direct automatic-GC
  PHP differentials; exact-source interleaved PHPStan and ordinary-call controls;
  relevant feature checks, formatting and unsafe inventory. Native evidence is
  x86-64; this architecture-neutral extraction has no new encoder or ABI.
- Reject if results or callback order change, ordinary instruction counts do
  not improve, or independently confirmed holdout regressions remain unexplained.

### First boundary layout and origin representation

The first extraction removes 1.890 billion PHPStan instructions but leaves
analysis timing effectively unchanged. Five-pair confirmation shows scalar
frames regressing from 328.035 to 349.680 ms (+6.60%), despite 108 fewer
instructions per iteration. Branch misses rise from 451,720 to 817,030. Shared
frames regress 1.30%, property foreach improves 1.79%, and relative self-return
improves 6.71%. This version is not accepted.

Disassembly also exposes redundant live state: the previous instruction is an
`Option<*const Instruction>`, which carries a separate discriminant even though
valid instruction pointers are non-null. The revised boundary carries
`Option<NonNull<Instruction>>`. This preserves the initial no-previous state,
uses the existing live-instruction proof, and removes its separate tag from
ordinary dispatch without changing collection timing. Re-run the same gate
against the original accepted baseline; do not pool the two implementations.

### Compact boundary result: rejected

The revised source removes 2.075 billion application instructions and lowers
shared-frame instructions from 2754.941 to 2595.441 per iteration. However,
independent confirmation still regresses relative self-return from 1.07111 to
1.13747 seconds (+6.20%); scalar medians are 325.909 versus 330.906 ms (+1.53%,
with overlapping spread). The small deep-release control also regresses while
the larger control is equivalent. Shared frames improve 3.47% and property
foreach 1.33%. PHPStan medians are 10.19427 and 10.10724 seconds. All valid
samples and the passing semantic gates remain preserved as rejected evidence.
Both GC-boundary implementations and their unsafe-inventory increase are removed.
The executor remains the accepted frame-retirement implementation.

## Accepted instruction checkpoint: integer identity hashing for weak sidecars

- Outcome: weak-object release membership uses the existing request-seeded
  integer identity hasher rather than SipHash byte-stream processing.
- Baseline: `895941b2446631149dc2ffc32481f62e2ba08466`. The source profile records
  18.404 million weak-release identity hashes and 2.705 billion inclusive
  instructions in the generic hash function. The current native profile still
  attributes 0.82% of cycles to that hash specialization.
- Hypothesis: use the already tested function-identity hasher for the five
  request-local weak identity maps/sets. Keys are engine-created allocation
  addresses; exact key equality, seeds, ownership and sidecar behavior remain.
  Map/set and enclosing sidecar sizes must stay unchanged.
- Semantics: retain weak target/owner distinction, key counts, reference clearing,
  cloning, iterator ownership, GC ephemerons, destructor ordering and replacement.
  No new membership shortcuts, release scheduling or VM dispatch changes.
- Ownership: integrating task owns `runtime/weak.rs`; no other implementation
  is active. The prior two dispatch variants remain rejected.
- Gates: focused weak/GC tests and direct PHP differentials, default/no-default/
  all-feature checks, same-output PHPStan hardware/time/RSS, and the established
  independent call, property and deep-release controls. No new unsafe code.
- Reject on changed behavior, unbounded sidecar cost or independently confirmed
  unexplained regressions. This is a partial cost reduction, not PHP parity.

### Weak-identity result and native timing tradeoff

The exact candidate source is
`b7d00904d58d205207e9a7bcbb6d609002a18eee4f2edd749d2fa62f25239d47`.
[All valid samples and investigation results](performance-phpstan-weak-identity-samples.json)
remain available. An observable WeakReference/WeakMap lifecycle improves from
978.395 to 859.385 ms (-12.16%) and from 16.324 to 13.292 billion instructions
(-18.57%) over five shuffled pairs. An earlier unobserved object loop was
virtualized; it is retained only as excluded attribution evidence.

Independent PHPStan confirmation reduces whole-command instructions from
130.6175 to 128.4230 billion (-1.68%). Actual analysis changes from 10.24875
to 10.31784 seconds (+0.67%), against PHP's 0.94292 seconds and 15.4740 billion
instructions. All outputs, five files and twenty findings agree; memory remains
equivalent. This is an instruction-cost improvement, not a PHPStan time win.

Five-pair independent holdouts regress 2.21% for shared frames, 3.64% for scalar
frames and 4.64% for relative self returns. These controls execute essentially
identical instruction counts. Normalized disassembly confirms all 73,261 main
executor instructions and its 341,653-byte size are unchanged, but link placement
differs. Hardware counters show increased empty frontend slots; branch misses,
opcode-cache misses and instruction-cache misses do not increase. Using the same
executable path preserves the slowdown. This supports native layout sensitivity
without establishing the exact hardware cause.

The integrating task accepts the measured weak-sidecar and application instruction
reduction with this explicit timing tradeoff. The unconditional performance gate
has not passed; retain all three regressed controls for the next checkpoint.
The exact source passes 41 focused test executions, six direct PHP differential
programs, all-target/all-feature compilation, formatting and unsafe enforcement.
No unsafe invariant, value/frame ABI or JIT lowering changes. Native evidence is
x86-64 only. All aggregate jobs completed within 6 GiB without OOM or timeout;
cleanup ran in both checkouts and no private benchmark host was configured.

## Accepted checkpoint: bounded temporary snapshot proofs

- Outcome: capturing live-read proofs before temporary release does not allocate
  a list for an ordinary statement and does not linearly search that list for
  each retired slot.
- Baseline: `f937b0f6` (seeded weak identities). Source accounting attributes
  2.985 billion instructions to the snapshot-capture closure, including
  1.725 billion in its filtering iterator and 1.23 million vector growth calls.
  The accepted native profile still identifies temporary release as a hot cost.
- Hypothesis: store proofs for the first 64 slots of the release range in one
  word, with the existing growable list only for proven slots beyond that range.
  Compact frame ownership already supplies the slots to visit directly.
- Semantics: perform exactly the existing identity proof, after callbacks and
  before any source temporary is dropped; preserve slot order, GC root admission,
  callback/exception exits and wide-frame ownership. No proof survives PHP
  callbacks. No frame, Value or JIT ABI change and no extra unsafe invariant.
- Ownership: integrating task owns the temporary-release implementation and its
  focused GC/alias tests. No other implementation checkpoint is active.
- Gates: compact/wide and overflow proof coverage, GC-root/temporary/finalization
  suites in relevant feature configurations, direct PHP differentials, same-output
  application instructions/time/RSS, and existing call/property/deep controls.
  Reject semantic changes or an unaccounted confirmed control regression; do not
  compound a rejected representation with unrelated dispatcher changes.

### Temporary snapshot result

The measured source fingerprint is
`8bdbd64e59f0a0c4478aa4ef9bf41cd6b4d6cc03aa955d9fc6c416b0a92069c1`.
[The complete sample packet](performance-phpstan-temporary-snapshot-samples.json)
retains both application windows and all controls. Independent confirmation
changes PHPStan from 128.4387 to 128.3484 billion whole-command instructions
(-0.0703%), and actual analysis from 10.34398 to 10.30752 seconds (-0.35%).
The first window gives a larger 1.29% time reduction; do not pool it with the
confirmation or describe the latter as a large application win. Peak RSS stays
equivalent (598546 versus 598776 KiB). PHP remains at 0.96281 seconds and
15.47394 billion instructions in the confirmation window.

Five-pair controls improve 0.91% for shared frames, 4.90% for scalar frames,
2.44% for property foreach and 7.23% for relative self returns. The shared
control nevertheless adds 6.5 instructions per iteration (2754.94 to 2761.44),
while scalar instruction counts are unchanged. Deep-release controls improve
about 0.83%. A first short mixed-loop regression triggers independent longer
confirmation: 245.602 versus 247.162 ms (+0.64%), within the one-percent gate.
Code placement still affects native controls; instruction and time results are
reported independently.

The representation avoids overflow allocation for proofs within the first 64
slots of a statement, including a statement located in a wide frame. This is
covered by a boundary test and eight direct PHP programs with nested property
chains; the whole application's allocation count is not measured. The change
passes 73 focused test executions and 29 direct differential programs across
GC roots, weak observations, references, aliases, destructor exceptions, Fibers
and temporary foreach owners. All-target/all-feature compilation, formatting
and unsafe enforcement pass. Main executor size stays 341653 bytes; the release
helper grows from 18369 to 18429 bytes. No frame ABI or JIT lowering changes;
native performance evidence remains x86-64 only.

One fresh Callgrind profile of this exact candidate validates the same PHP output
and attributes 29.394 billion instructions to the main executor, 5.582 billion
to temporary release, 3.115 billion to class lookup and 2.603 billion to release
tree classification, as self costs. Its 130.537 billion total is source attribution,
not another native hardware sample. All jobs stay below 4.07 GB without OOM or
timeout. Cleanup ran in both checkouts; no private host was configured. The
integrating task accepts this bounded representation while keeping the overall
PHP time/instruction parity goal active.

## Rejected checkpoint: execution frequency at ordinary ownership boundaries

- Outcome: the compiler can optimize ordinary ownership, call and frame work
  without an explicit assertion that those routines are rarely executed.
- Baseline: `8bef72b7`. Exact source profiling charges 5.582 billion self
  instructions to temporary release, 2.833 billion to full call entry,
  1.963 billion to committed frame retirement and 1.452 billion to bitmap
  replacement. These routines and scalar replacement/frame pop carry `cold`
  attributes despite millions of ordinary invocations. Existing disassembly
  shows full register-saving prologues even for no-owner overwrite paths.
- Hypothesis: remove only the six frequency annotations and retain existing
  explicit non-inlining boundaries. Let the normal optimizer choose block
  placement and helper inlining. This is an experiment; source annotation alone
  does not prove that it causes the instruction/time gap.
- Semantics and ownership: the integrating task changes attributes in the
  executor, call-frame and slot-write source only. No runtime predicate, PHP
  operation, ownership order, error boundary, ABI, encoder or allocation changes.
- Gates: exact-source default ownership/GC/type regressions, remaining feature
  checks, direct PHP differentials, hardware application counts and phase timing,
  all existing independent controls, code size and memory. Native evidence is
  x86-64; no ARM64 performance claim.
- Reject if application instructions/time do not benefit, generated code expands
  without an offsetting measured benefit, or independently confirmed control
  regressions remain outside the agreed band. Do not stack unrelated source
  changes on an unsuccessful frequency experiment.

### Ordinary frequency result

Removing six `cold` attributes preserves all 29 direct PHP differential outputs
and the first 26 focused default test executions, but fails the performance gate.
The application window changes whole-command instructions from 128.3341 to
128.0681 billion (-0.21%) while analysis changes from 10.1562 to 10.2799 seconds
(+1.22%). Independent five-pair controls regress 5.97% for shared frames, 4.08%
for scalar frames and 5.59% for relative self returns. Their instruction counts
improve slightly, so removing source frequency annotations does not address the
native execution cost. Property and deep-release controls improve modestly.

The candidate source is `596aa3405b03101428cea4f39a8a64ab8f0402e805f9028d0edf99b1ec819d00`.
Both bounded jobs complete without OOM or timeout; the largest memory peak is
3.75 GB. All six annotations are restored exactly to the accepted baseline.
Remaining feature gates are not run for this rejected candidate. No unrelated
implementation is stacked on it.

## Deferred checkpoint: borrowed release-tree observation

- Outcome: inspecting a live ownership tree does not manufacture strong owners
  and an accounting table for ordinary arrays, properties and closure captures.
- Baseline: `8bef72b7`, exact source Callgrind 130.537 billion instructions.
  Release classification costs 2.603 billion self instructions; its child queue
  costs 2.021 billion inclusive over 8.835 million calls, with additional counted
  snapshots, queue-map growth and Value drops. The frequency experiment is rejected.
- Hypothesis: pending ordinary edges can store non-owning copies of Value bits
  while the original root pins the entire read-only graph. Only opaque native and
  generator visitors retain the existing owned handles and their count corrections.
  Descendants of such owned handles also remain owned, preserving transient edges.
- Semantic envelope: no PHP callbacks, graph mutation, owner retirement or escape
  is permitted during classification. Copies retain pointer provenance but borrow
  allocations, never storage slots; object marker mutation therefore crosses no
  property borrow. Shared-owner encounter proofs, reference alias deduplication,
  traversal order, deep-drop markers and every callback-capable type remain intact.
  Count corrections cover only handles actually retained by the observer.
- Ownership: integrating task owns the release classifier, native-edge visitor
  boundary and focused lifecycle tests. No frame, Value or JIT ABI changes.
- Gates: pin/opaque transient-edge and shared-alias tests, deep release, GC roots,
  generator and destructor exception coverage; PHP differential outputs; relevant
  default/no-default/all-feature gates, unsafe inventory and focused sanitizer
  diagnostics if the candidate survives measurement. Exact-source PHPStan hardware
  counts, phase times, RSS, independent controls and code-size comparison.
- Stop: reject lost callbacks, root-count changes, dangling snapshots, unbounded
  traversal storage, or confirmed unexplained control regressions above one percent.
  A reduced instruction count alone does not establish application parity.

### Pre-existing shared-reference lifetime failure

The focused default gate exposes a crash in the new mixed reference, closure and
native-container regression. Reference PHP prints `held|leaf|cleared`; the exact
accepted baseline also crashes before output (SIGSEGV), while the checked test
build rejects an invalid Rc strong-count precondition. The failure is preserved
and is not attributed to the borrowed observer. The observer passes its two unit
checks but receives no performance or acceptance claim; its source is restored
before isolating the lifetime defect.

## Accepted correctness checkpoint: shared-reference release lifetime

- Outcome: a deep ownership DAG built from explicit reference pairs, ordinary
  object properties, closure captures and native array ownership releases a leaf
  once, after its last external owner, without using freed Rc storage.
- Baseline: `8bef72b7`, directly reproduces SIGSEGV for the added PHP differential.
  The checked build stops in Value clone during retained temporary-container
  discovery. Reference output is `held|leaf|cleared`.
- Scope: reduce the reproducer and fix the actual ownership transition. Preserve
  destructor/exception/GC order; do not guard on the fixture or disable an existing
  runtime mode. The integrating task owns the necessary shared runtime files.
- Gates: exact PHP differential, bounded memory diagnostics and relevant ownership,
  deep-release, reference and GC feature coverage. Recheck application time/counts
  and independent controls before accepting the correction.
- Stop: do not restore the deferred observer or combine another performance
  experiment until the baseline lifetime defect is understood and fixed.

The reducer shows that one reference-array layer suffices; native containers and
closures are unnecessary for the memory defect. `AddArrayElement` duplicated CV
promotion and moved the unowned bits of a borrowable heap argument into a new
owned cell. The first candidate reused `materialize_reference_alias` to acquire
the real edge before promotion and preserve the shared main-scope mirror
protocol. That runtime variant is rejected below; the accepted fix changes the
compiler's argument ownership proof instead.

A separate baseline control also finds that invoking even a direct CV Closure
keeps it weakly observable after its PHP owner is unset. It reproduces without
reference arrays, so the promotion regression tests exercise closure alias
storage without invocation; the independent invocation-lifetime failure remains
recorded for the next correctness checkpoint. Do not treat that failure as fixed
by argument promotion.

### Runtime-promotion variant and proof correction

The first repair passes 144 focused feature test executions and 54 PHP programs;
three Memcheck cases report zero errors, versus the baseline's freed Rc storage.
However, expanding the canonical promotion helper in the main executor adds
508 code bytes (341653 to 342161). Independent scalar controls regress 11.66%
with unchanged instructions; shared frames regress 1.19% and relative self returns
2.81%. The application instruction count is essentially unchanged. This runtime
variant is not accepted and is restored before the next implementation.

The ownership proof is the more appropriate boundary: `build_borrowable_heap_args`
excludes in-place mutation of an array destination, but misses a reference-array
source in operand two. The candidate clears that parameter's borrow bit during
compilation. A caller then supplies the owned argument required by existing CV
promotion. This changes no steady-state opcode body, runtime guard or reference
representation. The same differential, memory and feature gates apply.

### Accepted compiler proof and explicit performance limit

The compiler repair passes 144 focused test executions across default,
no-default and all-feature configurations, all-target/all-feature compilation,
formatting and unsafe enforcement. All 54 direct programs match PHP; three
minimal/deep reference-escape programs have zero Memcheck errors with system
heap routing. The baseline's freed-Rc-storage failure is fixed without a new
unsafe block or executor branch. Leak detection is not part of this diagnostic.
The [complete measurements](performance-phpstan-reference-escape-samples.json)
retain both application windows and every valid control sample.

In the independent application window, instructions change from 128.3341 to
128.3188 billion and analysis from 10.5240 to 10.2595 seconds; reference PHP is
0.9732 seconds and 15.4742 billion instructions. RSS stays approximately 585 MiB.
The earlier window is slower for all runtimes and is reported separately.
These measurements do not establish an instruction speedup or PHP parity.

The relative-self return control regresses 3.87% in the first five-pair window
and 9.05% in a separate five-pair confirmation. Its median instruction count is
unchanged. The normalized 73,261-instruction main-executor sequence is identical
and still 341653 bytes; linked addresses shift by 64 bytes. The precise hardware
cause of the time difference is not established. Shared-frame confirmation is
within one percent (+0.72%); scalar, property, mixed and deep controls do not
show a confirmed regression above the bound.

The integrating task explicitly accepts this narrow correctness tradeoff to
remove a reproducible use-after-free and crash. This is not acceptance as a
performance improvement. Relative-self remains an independent regression
control. All jobs stay below 4.17 GB with no OOM or timeout; cleanup ran in both
local checkouts and no private host was configured. No frame ABI or JIT lowering
changes; native evidence remains x86-64 only. The separate echo callable-lifetime
failure is still open and does not block resuming the borrowed release-tree
experiment after this reference-escape repair.

## Accepted checkpoint: borrowed release-tree observation resumed

The deferred classifier change resumes on `aabbd1e8`, after the reference-array
escape proof and its failing deep DAG regression are fixed. The semantic proof,
files, gates and rejection conditions above remain unchanged. Baseline and
candidate use separate exact binaries; the corrected compiler is present on
both sides. The two native transient-edge/shared-owner unit tests and the fixed
deep reference regression are mandatory. This checkpoint makes no claim to fix
the separately recorded echo callable-lifetime behavior.

The root-pin proof also covers native visitor effects: `NativeObjectState` is
crate-private, and every current `for_each_value` implementation only visits
retained fields or delegates to another read-only visitor. Generator traversal
can construct transient snapshots, which remain owned here. None of these
visitors invokes PHP, edits ownership edges or retires a queued ordinary child.
Only traversal metadata, including deep-drop markers, may change during the
inspection. A future visitor that mutates that graph would require a different
borrowing boundary before it could be used by this classifier.

### Borrowed observer results and accepted limits

The [exact-source samples](performance-phpstan-borrowed-release-tree-samples.json)
identify source `e3a57f989f2b346314bea0d305c251bae9ef9f6018fad8b97bb9f3d5d8516f81`.
PHPStan instructions fall from 128.3313 to 125.8905 billion in the first window
and from 128.3294 to 125.8780 billion in the independent confirmation (about
1.91%). Analysis changes from 10.3310 to 9.9112 seconds and, independently, from
10.3244 to 10.1982 seconds. Every application run produces the same findings;
reference PHP in the second window is 0.9679 seconds and 15.4742 billion
instructions. RSS is effectively unchanged (598284 to 598330 KiB in the second
window). The wide candidate time spread remains visible; windows are not pooled.

The scalar-frame control regresses 5.54% initially and 4.07% in an independent
five-pair confirmation; property reads regress 2.18% and 2.70%. Their median
instruction counts are unchanged. Shared frames improve 0.21%, relative-self
returns 6.67%, the long mixed control 1.44%, and deep releases about four percent.
The integrating task explicitly accepts the two time regressions for this
targeted 2.45-billion-instruction application reduction, favorable application
windows and deep-release benefit. This is not an all-workloads speedup; scalar
and property controls remain required for subsequent work.

The main executor and temporary-release helper remain 341653 and 18429 bytes.
The classifier shrinks from 5783 to 5716 bytes. After excluding relocation
addresses and disassembler comments that label anonymous data tables, the main
executor has the same 73,261-instruction sequence. Code and table placement
change; these observations do not establish the precise hardware cause of the
two time regressions. A fresh native cycle sample of this exact candidate
attributes 23.08% to the main executor, 5.32% to class lookup and 2.27% to the
temporary-release body plus 1.82% to its snapshot closure. This is sampled cycle
attribution, not an additional instruction measurement.

All 383 focused feature test executions and 61 direct PHP programs pass, as do
all-target/all-feature compilation, formatting and unsafe enforcement. Three
minimal/deep PHP programs and both transient/shared ownership unit regressions
report zero Memcheck errors. The single new unsafe block is the documented
root-pinned Value-bit copy; opaque handles remain owned. No frame, Value or JIT
ABI changes. Allocation counts for the whole application and ARM64 native timing
are not measured.

The first expanded CLI differential run fails because host PHP and RPHP use
different default `zend.exception_ignore_args` settings. Both explicit settings
produce identical PHP/baseline/candidate output; the corrected differential
runner pins the same setting on both sides. The failed job remains recorded,
and no runtime change or retry without memory limits hides it. All jobs stay
below 4.25 GB with no OOM or timeout. Cleanup ran in both local checkouts; no
private benchmark host was configured. PHP time/instruction parity remains open.

## Rejected checkpoint: statement root-owner bound

Baseline is clean `4ad0c998`, with 125.8780 billion instructions and 10.1982
seconds of PHPStan analysis in the independent window above. The exact native
profile still charges about four percent of cycles to temporary release and
its snapshot closure. The earlier instruction profile attributes 12.03 billion
inclusive instructions to the unchanged release helper over 18.10 million calls.

The hypothesis is a general physical-ownership proof: if every callback-capable
root in an ordinary release range has more strong owners than the total number
of owning handles in that range, none can be final. Each allocation loses at
most that many direct edges; because none becomes final, no children retire.
Strings and native resources without VM callbacks need no such proof. Owned
references compare their cell's count, never their target's count. Unknown
ownership fails conservatively. A failed proof changes no state. Successful
proofs still use the original drop order, snapshot checks and GC admission.
The foreach protocol and pending-call/operand cleanup retain their established
paths; this does not cache a proof across any callback.

The integrating task owns `src/vm/execute/call_frames.rs` and focused tests.
No other agent edits this checkout. Validation requires alias counts on both
sides of the bound, owned-reference indirection, ordinary shared/final roots,
weak observations, exceptions, fibers and GC ordering; default, no-default and
all-feature focused tests; direct PHP comparisons; formatting and unsafe gates.
Application A/B, the existing independent controls and code size decide whether
the extra proof pays for itself. Reject semantic differences, unchanged target
cost or reproducible target regressions. No Value/frame/JIT ABI change is planned;
native measurement remains x86-64 only.

The first ordinary-only experiment passes 62 focused checks and 53 direct PHP
programs, but reduces application instructions only 0.134% (125.8881 to 125.7197
billion); analysis changes from 10.0390 to 10.1077 seconds. It is superseded
without acceptance. The same all-root proof now also precedes operand/nested
planning, after the mandatory pending-call cleanup. It does not mix partial
survival proofs with callbacks: every callback-capable root must survive, or
the original alias-aware/shallow-drop test runs unchanged. The source profile
already covers these modes; they account for most temporary-release calls.


### Owner-bound result: rejected

The [two complete measurement packets](performance-phpstan-statement-bound-rejected-samples.json)
retain both exact builds and every valid sample. The extended variant lowers
application instructions only from 125.8827 to 125.6644 billion (0.173%). Analysis
medians are 10.1198 and 9.8872 seconds, with reference PHP at 0.9451 seconds and
15.4742 billion instructions. This single window is not independently confirmed.

The shared-frame control regresses 4.61% in the short window and 3.13% in five
longer pairs. It executes 2770.94 versus 2761.44 instructions per iteration:
failed-proof work has a measurable steady-state cost. Relative self-return
regresses 5.79%, adding 18 instructions per iteration and substantially more
branch misses. Scalar and property controls improve, while deep-release
controls are slightly slower. A tiny application instruction reduction does not
justify the extra checks and observed regressions. Both implementations and
their tests are removed; `4ad0c998` remains the accepted source baseline.

Each variant passed 62 focused checks and 53 direct PHP programs. The remaining
feature matrix is deliberately not run after rejecting the performance
hypothesis. All four bounded jobs completed without OOM or timeout; maximum
aggregate memory was below 3.87 GB. No new unsafe block or ABI change was made.

## Active checkpoint: measured compiler frequency optimization

- Baseline: `cd2cecb9`, whose Rust source is identical to accepted `4ad0c998`.
  Use the exact accepted binary and identical max-perf, target and alignment
  flags. The executor is 341653 bytes; the exact native profile assigns 23.08%
  of cycles to it. Independent prior controls expose time changes even when
  normalized executor instructions do not change. These facts motivate a
  compiler-layout experiment, without claiming that layout explains the whole
  application gap.
- Outcome/hypothesis: let measured branch/function frequencies guide LLVM
  placement and inlining, reducing executed work or cycles across ordinary
  interpreter behavior. Change no opcode, PHP semantic rule, frame layout or
  source-level admission guard. Keep CPU targeting identical on both sides.
- Scope/ownership: the integrating task owns the isolated build and evidence
  pipeline. Use an LLVM profile tool matching the compiler. Train on a fixed
  selection of existing array, string, object, callback, type and application
  corpus programs. Exclude PHPStan and the existing scalar/shared/property/self
  comparison programs from training. Record the selection before the build,
  its source hashes, raw and merged profile hashes, and all compiler warnings.
- Gates: identical direct PHP output, exact-source application instruction/time
  A/B with a fresh cache, existing independent controls and native code size.
  Retain every measured sample; confirm regressions and meaningful target wins
  independently. All builds and training inherit the aggregate memory limit.
  The source and JIT encoders are unchanged; native evidence remains x86-64.
- Stop rule: reject on output differences, unusable/mismatched profile data,
  unbounded build/training, or no repeatable target gain. Do not add the target
  to training or change its workload merely to make the result favorable.

### Compiler-frequency result: accepted build-only gain

The [complete PGO packet](performance-phpstan-pgo-samples.json) identifies the
unchanged Rust source, both executables, all 18 training inputs, raw/merged
profiles and every valid sample. PHPStan and every measured control are excluded
from training. CPU targeting remains the rustc default on both sides; this is
not a native-CPU comparison. The matching profile tool is LLVM 22.1.8 from the
Rust 1.98.1 toolchain.

| Separate application window | Baseline analysis | PGO analysis | Baseline instructions | PGO instructions | PHP analysis |
| --- | ---: | ---: | ---: | ---: | ---: |
| First, two samples per runtime | 10.0006 s | 9.5923 s | 125.8957 G | 122.7327 G | 0.9713 s |
| Independent confirmation, two per runtime | 10.0700 s | 9.4804 s | 125.8959 G | 122.7432 G | 0.9876 s |

These are medians within each window, never pooled. Counters cover the whole
command; phase timers cover actual analysis. Every run has a fresh result cache,
five analysed files, the same findings and full counter coverage. The confirmed
instruction reduction is 2.50%, with analysis 5.85% faster in that window. PHP
still uses only 15.4741 billion instructions. Median peak RSS is 598860 versus
596236 KiB in the confirmation. This is a partial improvement, not PHP parity.

Five long pairs improve shared-frame time by 0.85%, scalar-return time by 16.74%,
property reads by 5.03% and relative-self return by 19.05%. Incremental shared
iterations still require 2636.86 versus PHP's 516.00 instructions; scalar
iterations require 1640.29 versus 395.50. Five pairs at each depth improve
2048/16384-node release by about nine percent. The short mixed control is 0.54%
slower, below the one-percent rejection threshold. Two additional untrained
application holdouts agree with PHP; their short timings do not justify a
broad speedup claim.

All 53 direct differential programs and all 18 final training-output checks
pass. No Rust source, ownership invariant, ABI, JIT encoder or feature behavior
changes. The unchanged source already passed its accepted feature gates; no
new full matrix is run for a compiler-profile change. Native evidence is x86-64
only. The executor grows from 341653 to 395497 bytes, while `.text` shrinks from
14837138 to 12130674 bytes and temporary release from 18429 to 14702 bytes.
PGO's benefit cannot be reduced to making every function smaller.

The first merge also contained 21 automatically emitted build-script profiles;
that executable was not benchmarked. The measured rebuild uses exactly the 18
recorded runtime profiles. Its 26 missing-function warnings all name
`build_script_build`; there are no runtime mismatch warnings. The instrumented
build took 263.71 s and the corrected profile-use build 203.15 s. Maximum
aggregate memory across the jobs was 4.79 GB, without OOM or timeout.

`scripts/pgo-build.sh` now encodes the measured default CPU/alignment policy,
uses the fixed `scripts/pgo-workloads.txt` manifest, separates Cargo build
profiles from runtime profiles, and retains artifacts in a fresh task directory.
Run it inside the required aggregate memory boundary; set
`CARGO_PROFILE_MAX_PERF_DEBUG=line-tables-only` to reproduce this checkpoint's
debug setting. An explicit `RPHP_PGO_TARGET_CPU` changes CPU targeting for both
stages and needs its own comparison. Shell syntax and manifest hashes pass;
the equivalent two-stage commands produced the measured artifact, while the
updated shell entrypoint has not received a second full rebuild. Cleanup ran in
both local checkouts; no private benchmark host was configured.

## Application-level work diagnostic

Accepted source is `bb46dcca` (Rust unchanged from `4ad0c998`), with the separately
identified PGO executable. The remaining gap is 122.74 versus 15.47 billion
whole-command instructions. Existing native profiles identify runtime costs,
but do not directly establish whether PHPStan executes the same number of PHP
function bodies on both interpreters.

Create one private diagnostic archive with identical fixed integer counters at
named-function and ordinary-closure body entries. Exclude abstract declarations
and arrow expressions; retain a source-position map. Initialize scalar counters
before archive bootstrap, serialize them only at shutdown, and run both
interpreters with the same input, fresh cache and explicit INI settings. Keep
all generated code and raw output outside tracked source. Confirm the original
application findings, ordinary stderr and exit status before interpreting any
counts. Instrumented elapsed time is not performance evidence: added PHP code
changes execution and ownership. Counts from generators represent body entries,
not allocation of a generator object.

The integrating task owns this diagnostic only. No runtime implementation or
new language behavior is admitted. Runs inherit the standard memory boundary
and benchmark lock. Reject the probe if it changes observable application
results or cannot match entry IDs between interpreters. Use count differences
to select the next measured runtime boundary; do not turn workload names into
runtime recognition or optimize the diagnostic instrumentation itself.

The full-array counter probe is rejected after a SIGSEGV. Scalar counters reduce
probe overhead but reproduce the same crash. Reference PHP preserves all
original findings with both probes. Both accepted non-PGO and PGO executables
crash; native debugging locates a corrupted suspended frame during
`restore_detached_scope_globals`, with ordinary integer Value bytes in its
header. No OOM occurs. Four simple direct/nested/closure/static callback
reducers agree across all runtimes but do not reproduce it. Temporary local
caller-descriptor and frame-retirement invariants then identified the first
stale logical caller. These assertions are absent from the repaired runtime;
the failing probes remain diagnostic evidence, not performance samples.

### Diagnostic root cause and bounded repair contract

The current source frame and its function descriptor remain valid across the
callback and its retirement. The invalid ancestor is a detached trace-table
entry inherited at a reused stack address, not corruption of the current
frame. A diagnostic assertion at `pop_vm_call_frame` catches the first leftover
entry in `force_close_generator_activation`: its branch without a pending
`finally` pops the materialized generator frame but does not discard the
logical caller installed by `materialize_generator_frame`. This failure is
observed before the later invalid ancestor is dereferenced.

The repair scope is that generator retirement boundary. Remove the trace entry
only after local cleanup/destructors have used the live caller and before stack
storage is reusable. Keep all temporary diagnostic assertions out of the final
runtime. Ownership remains with this integrating task in the same isolated
performance worktree. No language rule or execution-tier admission changes.

The acceptance gate is a regression checking that newly allocated frames do
not inherit a force-closed generator's caller, for both unstarted and suspended
generators and both source-origin conventions. It must fail before the repair.
Run the relevant generator/callback checks, then complete the identical-archive
application-count comparison with matching findings. Ordinary uninstrumented
phase/counter measurements remain the performance evidence. A callback-output
or lifecycle regression rejects the change; incomplete logical counters must
not be presented as evidence of equal application work.

### Generator retirement repair: accepted

The no-finally close path now discards its detached caller after frame
local/destructor cleanup and before the stack allocation can be reused. This
matches the existing ordinary generator retirement protocol. No production
unsafe block, ABI, opcode, JIT admission or PHP behavior is added.

The new lifecycle regression fails on the unrepaired runtime. After the fix it
passes in default, no-default-feature and all-feature builds; its four cases
cover unstarted/suspended generators and both caller-origin conventions. The
93 existing frame-retirement, generator and native-callback tests also pass.
Formatting, the unsafe diff gate and all-target/all-feature checking pass. Test
unsafe operations stay in test support and do not relax production inventory.
All build and execution jobs use a verified 6 GiB aggregate boundary without
swap, OOM or timeout. The first build attempt was stopped by the production
unsafe inventory when test code was colocated in the runtime file; moving the
regression to test support resolved that packaging issue without changing the
gate.

The identical instrumented archive now completes with the same 20 findings,
ordinary stderr and exit status as original PHP. Its 23557 entry IDs span 3014
modified source files. Reference PHP executes 9605646 counted function bodies;
RPHP executes 9590291, a difference of -0.160%. There are 773 differing entry
IDs, with 54903 extra and 70258 fewer RPHP entries. Some differences involve
native-versus-polyfill selection and reflection-driven Nette dependency
hashing; their causes have not all been established. Parser traversal counts
include several exact matches. These counters rule out an order-of-magnitude
increase in function entries, not all possible differences inside functions.
Arrow expressions and native function bodies are outside this probe.

The [repair evidence packet](performance-phpstan-generator-trace-samples.json)
retains exact source/executable/probe hashes, validation commands, counter
summaries and every ordinary native sample. With two fresh-cache samples per
runtime, identical non-PGO settings give these medians:

| Runtime | Analysis | Whole-command instructions | Peak RSS |
| --- | ---: | ---: | ---: |
| PHP | 1.0208 s | 15.4743 G | 177566 KiB |
| Previous RPHP | 10.0370 s | 125.8746 G | 598904 KiB |
| Repaired RPHP | 9.9619 s | 125.8717 G | 598574 KiB |

The instruction difference is negligible and these short timing distributions
do not establish a performance gain. This is a correctness repair enabling the
application-work diagnostic. Earlier PGO results use another build policy and
are not mixed into this source-change comparison. PHP parity remains open.

## Active checkpoint: writable property-array ownership

- Baseline: clean `7ab760c0`, exact non-PGO repair executable and source hashes
  in the preceding packet. A hardware-counter growth probe writes integer and
  string keys to local arrays and declared object arrays with identical output.
  At 4000/8000 string-key entries, the property form takes 0.8133/3.1604 billion
  RPHP instructions versus PHP's 31.5914/35.0982 million. The RPHP local-array
  control takes 32.1782/55.2092 million. Previous application Callgrind data
  attributes 1.891 million array copies to mutable array access; it motivates
  this boundary but does not establish the current application's saving.
- Hypothesis: the canonical writable property fetch makes an ordinary array
  snapshot, leaving the property as a second owner. Each later dimension write
  consequently copies the entire growing array. Expose stable storage through
  the existing engine-internal reference protocol instead. PHP-visible copies
  must still separate, while a compiler temporary must not create a PHP alias.
- Scope: initialized ordinary array properties after existing visibility,
  readonly and hook validation. Keep hook/overloaded results, scalar auto-init
  and richer reified contracts canonical. Reuse one storage-cell protocol in
  cold and cached reads; no workload recognition, new opcode, ABI or JIT rule.
  The integrating task owns the property executor and any necessary diagnostic
  completion changes in this worktree.
- Semantics: preserve explicit aliases, object-clone and array COW, observable
  reference cardinality, typed writes, lazy rollback, exception/destructor
  order and synthetic writeback retirement. A baseline diagnostic reducer also
  proves that current writeback overwrites an error handler's replacement or
  unset. Reference PHP retains those callback effects. Establish and test this
  completion boundary before accepting an in-place representation.
- Gates: focused PHP differential reducers and existing affected property,
  array and reference tests; relevant feature builds, formatting, unsafe policy
  and all-target checking; exact non-PGO application A/B with fresh caches,
  growth probe and independent controls. Preserve all valid samples. Native
  evidence is x86-64; no architecture encoder changes are proposed.
- Stop rule: reject if internal aliases escape into PHP-visible sharing, any
  callback or hook effect changes incorrectly, the growth remains quadratic,
  or the real application/control measurements reject the tradeoff. Do not
  equate a synthetic asymptotic fix with achieving PHPStan parity.

### First property-container candidate: not accepted

The first candidate passes 40 focused tests and the original direct PHP
reducers. Its 8000-entry string-key property control falls from 3.1604 billion
to 60.8835 million instructions, removing the observed quadratic growth. The
six ordinary application samples retain matching output: median instructions
fall from 125.8801 to 116.5892 billion (-7.38%), while median analysis changes
from 10.4233 to 9.1542 seconds. Both individual baseline times (10.0590 and
10.7877 seconds) remain in the packet; they are not filtered. Peak RSS grows
from 598812 to 614766 KiB as mutable properties retain stable reference cells.

This source is not accepted. Three long pairs expose shared-frame time +7.40%
and relative-self return +12.56%; the latter also executes 470 million more
instructions over five million iterations. The inline property-reader change
has therefore affected unrelated execution. A 14-case diagnostic expansion
also finds three remaining baseline discrepancies when a handler publishes an
array copy or a non-aliased object clone. The revised candidate restores the
ordinary cached-read body, outlines container acquisition, and tracks newly
published owners on the diagnostic completion edge. It must pass the same
application/control gates with its own exact source and executable hashes.

The outlined-acquisition revision matches all 14 diagnostic combinations and
owner/clone/cycle lifetime output. Its confirmed whole-command application
counts are approximately 116.01 versus 125.88 billion. Independent five-pair
controls nevertheless confirm shared-frame +5.10%, scalar +2.36% and
relative-self +5.77% time despite fewer instructions, so it remains unaccepted.
A separate three-pair hardware frontend probe records shared-frame instruction
cache misses 8.423 to 19.743 million and decoded-operation cache misses 36.293
to 76.938 million. Scalar instruction cache misses grow 2.168 to 7.741 million.
The next revision moves diagnostic key conversion/completion out of the main
executor, retaining the same semantic protocol and normal conversion branch.

The first diagnostic extraction is rejected before performance measurement.
Its cloned illegal key remained a Rust local during exception cleanup, hiding
the final temporary object owner from the VM's destructor proof. A separate
payload reducer also exposes missing destructors when the error handler
replaces or unsets the array. The corrected completion renders diagnostic
text and relinquishes the key mirror before any callback or throw. Array
guards use canonical owner retirement before frame unwinding; a detached final
property cell uses the same protocol before synthetic writeback is consumed.
The added regressions check destructor order before catch-variable rebinding
and twelve replacement/unset/copy/exception combinations with nested objects.
These lifetime corrections are required for acceptance, not performance gains.

The completed lifetime revision passes 54 focused tests and all direct PHP
reducers, including twelve throwing, resurrecting and reentrant destructor
completions. A further correction discards the write when every PHP owner of
the private cell has disappeared; otherwise replacing an element of an already
detached array changed the child-destructor order.

The non-PGO performance gate still rejects this revision. Its independent
application pair reduces median analysis from 10.6735 to 9.6263 seconds and
instructions from 125.8861 to 116.0464 billion. Five independent control pairs
show shared frames +0.66%, scalar returns +8.12%, ordinary property reads
-0.73% and relative-self returns +10.60%. Scalar instruction-cache misses grow
from 2.225 to 14.040 million despite fewer executed instructions. Neither this
control failure nor the earlier source variants are discarded.

The next comparison changes only the build policy to the already accepted PGO
pipeline. Train the exact repaired baseline and this exact source separately
on the unchanged 18-program manifest, excluding PHPStan and all acceptance
controls. Preserve their distinct profiles, source hashes and binaries. This
checks whether compiler frequency information resolves the measured layout
regression; it does not grant acceptance to an ordinary non-PGO build or permit
adding the failed controls to training.

### Shared assignment and unset completion

The fifth revision's PGO build was stopped before measurement after extending
the same diagnostic reducer to `unset`. Assignment had passed, but unset still
used its old completion boundary: payload destructors could run inside the
handler or disappear during exception cleanup. One reentrant destructor case
terminated with a PHP fatal error (exit 255), not a native segmentation fault.
The complete independently trained baseline was retained; the interrupted
candidate and its profile were not reused for the final source.

Assignment and unset now share cold key-diagnostic completion. Diagnostic
guards retain the array through the handler and retire actual owners through
the canonical destructor protocol before exception unwinding. When a handler
removes every PHP owner, completion consumes the private operand cell and
leaves a completed-writeback sentinel. A destructor that recreates the property
must not cause the old synthetic writeback to overwrite it. An initial shared
implementation still failed three unset/throw cases; its failed gate is retained.
The final revision passes all twelve unset completion cases and the 55-test
focused default-feature packet.

A separate nested-dimension reducer exposes an existing limitation in the
baseline transaction machinery: replacing or unsetting the containing property
inside a diagnostic can lose nested payload retirement. The old baseline and
the final candidate are both checked and reported separately. This checkpoint
does not claim to repair nested transaction semantics; no previously passing
case in this reducer becomes a failure.

### Independently trained PGO comparison

The final source fingerprint is
`1a35c2f38b9ae02404c798d4a5f742beae10f9ef9a13988585d003c8b1bc302c`.
Baseline and candidate use separate fresh profiles from the unchanged
18-program training manifest, with PHPStan and the acceptance controls excluded.
Both use `max-perf`, default features, the compiler's default CPU target,
line tables and the same function-alignment setting. The baseline build takes
276.7 + 216.7 seconds; the candidate takes 264.6 + 202.3 seconds. Profile-use
warnings concern build-script functions, with no runtime profile mismatch.

Two fresh-cache application samples per runtime in the first window give
9.7383 to 8.7517 seconds and 122.7254 to 113.8993 billion instructions. An
independent reversed-order window confirms:

| Runtime | Analysis median | Whole-command instructions | Peak RSS median |
| --- | ---: | ---: | ---: |
| PHP | 0.9750 s | 15.4741 G | 177774 KiB |
| Baseline RPHP | 9.5289 s | 122.7315 G | 595918 KiB |
| Candidate RPHP | 8.8385 s | 113.8939 G | 610104 KiB |

The confirmed reduction is 7.25% analysis time and 7.20% instructions. The
additional stable property cells cost about 13.9 MiB peak RSS (+2.38%). Every
sample analyzes five files with twenty identical findings, ordinary stderr and
exit status. Counters run throughout their enabled interval. Instructions count
the whole command; analysis timers exclude startup. The two windows remain
separate, with no sample filtering.

Independent five-pair controls retain shared-frame time +1.76%, relative-self
return +8.62%, scalar return -1.02% and ordinary property reads -2.94%. The two
regressing controls execute fewer instructions (-0.13% and -0.15% respectively).
PGO reduces some earlier layout regressions but does not remove all of them.
This result is a bounded application tradeoff, not an all-workloads speedup;
the shared-frame and relative-self controls remain mandatory follow-up evidence.
The non-PGO failures above remain separate evidence and are not replaced by
these PGO numbers.

The integrating task accepts this partial checkpoint with those two explicit
control regressions and the measured memory increase. Removing quadratic array
copying and 8.84 billion whole-application instructions justifies that bounded
tradeoff; further arbitrary source rearrangement is not an acceptance tactic.
All samples, rejected revisions, build identities and diagnostic differences
are retained in [the property-array evidence packet](performance-phpstan-property-array-samples.json).

Validation passes: 55 focused default tests, seven new regressions in each of
the no-default and all-feature configurations, all-target/all-feature checking,
formatting and the unchanged unsafe inventory. Three focused Valgrind programs
cover re-entry, destructor completion, object clone and GC lifetime with zero
reported memory errors and exact PHP output. Build/check jobs remain within
their verified 6 GiB aggregate boundaries without OOM or timeout. The PGO main
executor grows from 395479 to 398522 bytes; the executable grows by 13432 bytes.
ARM64 native performance is unavailable, and no architecture-specific code is
introduced.

The full parity goal remains open: this candidate still needs roughly 7.36 times
PHP's instructions and 9.07 times its analysis time. A fresh instruction-sample
profile puts 32.39% in the main executor and 5.33% in class lookup, with other
frame, method and ownership costs distributed across helpers. These samples
guide the next bounded investigation; they are not exact per-function budgets.

## Accepted checkpoint: register-sized symbol hash tails

- Baseline: clean `02db8434`, using the accepted independently trained PGO
  executable above. Class lookup has 5.33% and caller-scope lookup 1.73% of the
  whole-application instruction-overflow samples. Disassembly of class lookup
  shows a zeroed stack word populated by partial overlapping stores for the
  seven-byte hash tail, then reloaded for mixing; other lengths retain a
  variable-size copy. The most frequent class-lookup sampled PC is immediately
  after that tail load. Sampling skid prevents assigning its count to a single
  instruction, but the temporary and copy are independently visible in code.
- Outcome and hypothesis: assemble the same zero-padded little-endian word
  directly from bounded integer loads. Remove transient stack storage and
  variable-size library copies from a shared symbol-table primitive.
- Scope: `SymbolHasher::write` only. Preserve every hash bit, per-write chunk
  boundary, integer-key operation and table iteration order. Keep all loads
  within the input slice on every architecture, without additional unsafe code
  or name/length distributions learned from the target workload.
- Ownership: this integrating task owns the runtime helper and its focused
  hash compatibility tests in the same isolated worktree.
- Gates: frozen hash vectors across short tails, chunk boundaries, unaligned
  starts and multiple writes; default/no-default/all-feature checks and unsafe
  policy; independent fixed-manifest PGO training; same-output PHPStan A/B and
  the four retained controls. Inspect generated code as well as timings.
- Stop rule: reject if hash values change, source introduces out-of-bounds or
  architecture-specific behavior, or measured application results fail to
  justify the change. A sample share alone is not an expected speedup.

The safe Rust implementation combines overlapping, in-bounds integer loads.
Explicit little-endian decoding preserves the original zero-padded word for
every one-to-seven-byte remainder. The generated class-lookup tail has two
register loads followed by shift/OR, with no temporary stack word or variable
copy. Other lookup work still uses stack storage; this is not a claim of a
stack-free class resolver.

Independent application confirmation gives analysis **8.5596 to 8.3404 seconds
(-2.56%)** and whole-command instructions **113.8856 to 113.5080 billion
(-0.33%)**, with PHP at 0.9330 seconds / 15.4740 billion. RSS medians are 610334
and 610342 KiB. All outputs, five files and twenty findings match. Both initial
and confirmation windows are retained in
[the symbol-hash packet](performance-phpstan-symbol-hash-samples.json).

The independent five-pair controls give shared frames -1.32%, scalar return
+0.43%, property reads -1.57% and relative-self return -14.14% in time. Their
instruction counts increase by 0.43-1.47%; this is visible in the packet, not
described as a universal instruction-count improvement. No confirmed control
time regression exceeds one percent. The large self-control timing gain is
not attributed exclusively to the hash tail: source changes also alter compiler
inlining and register/code placement.

All 26 focused default tests pass, including frozen hash vectors with unaligned
starts, every tail width and incremental/integer writes. The two new hash tests
also pass in each no-default and all-feature build. Formatting, unchanged unsafe
policy and all-target/all-feature compilation pass. No new allocation, unsafe
operation or architecture-specific implementation is added. Main-executor code
grows by 806 bytes, class lookup by 650 bytes and the full executable by 461936
bytes, including debugging information. PGO training remains independent and
unchanged; no runtime profile mismatch occurs. Memory boundaries record no OOM
or timeout.

### Analysis-only hardware counters

A separate diagnostic archive now enables hardware counters at analysis entry
and disables them at analysis completion, using acknowledged perf control FIFOs.
The first probe required exact acknowledgement bytes and rejected a control
response; that run is excluded. Accepting NUL/line terminators and the tool's status-line format
produces identical ordinary PHPStan output. This probe is not the archive used
for the native acceptance timings above.

One valid phase measurement per runtime counts **10.3310 billion** user
instructions for PHP, **94.3811 billion** for the property-container baseline
and **94.1042 billion** for the hash candidate. Startup is outside those counts.
The remaining analysis-only instruction ratio is approximately **9.11x**.
Optimizing startup therefore cannot close the main gap. A phase-only native
cycle/caller profile of the property-container baseline is retained separately
to choose the next structural change. It is diagnostic evidence, not a new
timing comparison or proof that the parity goal is complete.

## Active investigation: repeated class and scope resolution

The analysis-only cycle sample has 6.67% in class lookup, 2.33% in lexical scope
resolution and 1.99% in method metadata lookup. The central executor accounts
for 32.80%, spanning many different operations. The recorded DWARF stacks do
not reliably recover callers for the main Rust functions; therefore the flat
profile cannot assign these costs to particular callers.

An isolated diagnostic source copy counted exact call sites of class, method
and scope resolution, with identical PHP output. Object type matching made
8,668,922 target lookups, class constants made 3,058,407 lookups before their
cache, and lexical scope resolution was called 7,026,626 times through its
string-returning wrapper. Whole-command counts include startup and are not
phase-only instruction budgets. The instrumented copy is never an acceptance
binary. The next checkpoint targets immutable type declarations, the largest
counted class-lookup caller; class constants and lexical scope remain separate.


## Rejected checkpoint: resolve immutable named type metadata

- Outcome: repeated parameter, return and property checks reuse the published
  target class identity and classify built-in named hints once at construction.
- Baseline: `5e9b0b38`, source SHA-256
  `854e5c7ffa73fdceef75736532e2020ba5ca240b7130884ac372eb4743cd5565`.
  An isolated call-site probe records 8,668,922 class lookups from object type
  matching and 3,058,407 before class-constant caching. These are whole-command
  counts, not instruction budgets. Probe output matches PHP. This checkpoint
  addresses named type metadata only; class-constant dispatch remains separate.
- Hypothesis: type declarations keep immutable spelling but currently hash and
  resolve that spelling for every check. An owned, shared descriptor can retain
  a positive class ID, guarded by a never-reused executor identity, without
  increasing the size of the general type-hint enum or frame/signature layout.
- Semantic envelope: misses and unpublished classes retain canonical lookup;
  aliases share published class identity. Relative `self`/`parent`/`static`
  continue to use their lexical/called scopes. No cache owns an executor or
  class pointer. Cloned hints may share metadata, including across requests,
  because every cached ID checks its executor identity. Reflection, spelling,
  equality, unions, intersections, coercions and diagnostics remain unchanged.
- Ownership: the sole integrating agent owns runtime, type metadata, VM type
  checking and the mechanical constructor conversions in compiler/stdlib.
- Gates: focused PHP differentials for late aliases, relative scopes and type
  failures; cross-executor shared-hint tests; relevant default/no-default/all
  feature tests and all-target compilation; unchanged unsafe policy; identical
  independent PGO training; interleaved PHPStan and retained controls, RSS,
  compile cost and code sizes. Native measurements are available on x86-64;
  the implementation remains safe portable Rust with no new native lowering.
- Stop rule: reject stale identity reuse, changed observable behavior, excessive
  metadata allocation/RSS, or no justified native application improvement.
  Preserve all failing measurements. No source/class/workload recognition.


### Named type descriptor result: rejected

The prototype preserves all 232 focused tests and three exact PHP differential
programs. Positive class identities are guarded by a unique executor ID;
relative names keep their lexical/called scopes. Type-hint size stays unchanged,
and the type checker shrinks from 2669 to 1428 machine-code bytes. The prototype
adds no unsafe operation. The complete source, binary and patch are retained
privately; all measured samples are in
[the named-type packet](performance-phpstan-named-types-samples.json).

Whole-command instructions fall from 113.5345 to 112.5220 billion (-0.89%) in
independent confirmation. The separate analysis-only counters fall from
94.0861 to 93.1415 billion (-1.00%), compared with PHP's 10.3326 billion.
However, application timing changes from 8.2677 to 8.3555 seconds (+1.06%) in
the first window and from 8.3674 to 8.2774 seconds (-1.08%) in confirmation.
A consistent application timing gain is not established. Confirmation RSS
increases from 609930 to 612414 KiB, approximately 2.4 MiB.

Shared-frame control time regresses 2.36% initially and 1.50% in independent
five-pair confirmation. Other confirmed controls change by -1.91% for scalar
returns, +0.88% for property reads and -9.69% for relative-self returns. All
instruction and branch counts remain visible. The integrating task declines a
tradeoff exception for this candidate: its approximately one-percent application
instruction reduction does not justify the confirmed shared-frame regression
without a consistent application time gain. Broader feature-test execution is
not pursued after rejection. Production source is restored exactly to the
accepted `5e9b0b38` fingerprint; no prototype metadata remains in the runtime.

## Active investigation: exact executor instruction costs

The new analysis-only retired-instruction sample places 36.57% inside the central
executor but only 2.56% inside class lookup. These differ from cycle percentages:
lookup can cost more elapsed cycles than its instruction share. The rejected
prototype reduces the class-lookup sample share to 1.83%, consistent with its
small total instruction reduction; it cannot explain or eliminate the remaining
roughly ninefold gap.

Instruction samples also accumulate at value cleanup classification, operand
access, pointer writes and frame bookkeeping. Inlining and sampling skid prevent
those source-line samples from being treated as exact budgets. One full
Callgrind run of the accepted executable will provide exact function, source and
instruction-address costs before another implementation is selected. It uses the
unmodified archive, verifies ordinary output against PHP, and retains the same
6 GiB aggregate memory boundary and exclusive lock. This is profiling, not a
new native timing claim or completion of the parity goal.


The exact accepted-binary profile completes with identical PHP output in a
bounded service, recording 115,338,275,570 instructions. Of these,
37,109,122,139 execute in the central VM and 2,680,528,361 map to the inlined
`Value::needs_cleanup` classification and its helper. That last figure includes
1,893,555,882 instructions inside the central VM. Compiler line attribution is
kept distinct from the total measured application budget. The profile also
counts 274,071,199 visits to the central dispatch selection; optimizing only
method/class-name lookup cannot remove these repeated value/slot costs.
The recorded indirect-jump edges resolve 38,176,316 of those visits to
`ReleaseTemps`; 14,586,441 take the compact-frame bitmap branch and 23,589,875
use the wide-frame path. These are execution counts from the same archived
profile, not another timing run or a promise that all cleanup can be omitted.


## Accepted instruction checkpoint: constructor-owned cleanup metadata

- Outcome: frame writes and retirement read one ownership bit instead of
  reclassifying the same value tag, reference ownership and feature-dependent
  resource representation on every visit.
- Baseline: `d92f2fff` (documentation-only successor of `5e9b0b38`), production
  fingerprint `854e5c7ffa73fdceef75736532e2020ba5ca240b7130884ac372eb4743cd5565`
  and accepted executable SHA-256
  `410a0cadb25886e9b1d68554f123cbd6b11f7c9c22f282527b2bafd023ef3750`.
- Evidence: exact Callgrind classification costs above; native acceptance will
  still use independent hardware-counter and analysis-timer comparisons.
- Hypothesis: payload ownership is fixed when a Value is constructed. Encoding
  it in an unused private type-info bit removes repeated range/reference/resource
  checks without adding storage or an ownership operation.
- Semantic envelope: primitive and borrowed-reference values do not own payloads;
  strings, arrays, objects, closures and owned-reference cells do. Resource
  ownership follows the existing resource-lifetime feature. Clones, provenance
  flags, retyping, COW, aliases and all frame widths preserve the same cleanup
  verdict. Low-byte type tags and the 16-byte Value layout remain unchanged.
- Ownership: the sole integrating agent owns Value metadata and focused cleanup
  tests. No allocator, callback, collection timing or JIT lowering is changed.
- Gates: all constructor families and provenance/reference variants; existing
  compact/wide frame cleanup and lifecycle tests under default/no-default/all
  features, targeted PHP differentials, formatting/unsafe policy/all-target
  compilation, independently trained PGO application and four retained controls,
  memory and code-size checks. Native ARM64 measurement is unavailable.
- Stop rule: reject any changed cleanup verdict, lifetime/order failure or
  confirmed unjustified regression. The classification's full source-attributed
  cost is an upper bound, not a promised instruction reduction.


Every owning constructor now establishes the private cleanup bit; ordinary
copies preserve it and scalar constructors/writers replace the complete
metadata word. Weak upgrades reconstruct the same ownership verdict. The
primitive, borrowed-reference and feature-dependent resource cases retain their
previous semantics. No new unsafe operation, allocation, ownership operation,
frame field or architecture-specific code is introduced.

The exact source passes 137 focused test executions across default, no-default
and all features, including real constructor/provenance/copy/weak-owner cases,
compact and wide frame cleanup, shared owners, release plans and resource
callbacks. Both compact/wide CLI programs match reference PHP. All-target and
all-feature compilation, formatting and the unchanged unsafe gate pass. An
initial extra scalar-writer test was removed after the policy inventory counted
its separate test-file unsafe block as production; that failed gate remains in
the packet. No policy ceiling was raised.

Analysis-only hardware counters fall from **94.1301 to 91.8486 billion
instructions (-2.42%)**, against PHP's **10.3366 billion**. Both independent
whole-command windows confirm approximately **113.546 to 111.200 billion
(-2.07%)**. Output, twenty findings and five analyzed files remain identical.
All samples and the exact profiler/binary identity are retained in
[the cleanup-metadata packet](performance-phpstan-cleanup-bit-samples.json).

This is accepted under the user's explicit instruction-count priority, without
claiming an application time win. Analysis medians change from 8.7113 to 8.9779
seconds in the first window (+3.06%) and from 9.2015 to 9.2349 seconds in
confirmation (+0.36%). The recorded SMT-sibling activity differs across these
samples; none is discarded or corrected. Confirmed RSS is 610934 versus
611108 KiB. Relative-self control time regresses 3.46% while its instructions
fall 0.35%; shared-frame and scalar instructions fall 1.55% and 3.26%, and
property-read instructions rise 0.26%. These limits remain visible controls.

The main executor shrinks from 399328 to 394096 code bytes; the full executable,
including line tables, shrinks from 76331392 to 76264752 bytes. Independent PGO
uses the same 18-case training manifest, excluding PHPStan and the controls.
Both build stages complete in 285.04 and 215.59 seconds. The largest aggregate
memory peak is 4742590464 bytes, with no OOM or timeout. Cleanup runs in both
local checkouts; no private benchmark host is configured. Native measurements
remain limited to x86-64. The remaining analysis instruction gap is about
8.89 times PHP; this checkpoint does not complete the goal.

## Accepted instruction checkpoint: pending-only collection state

- Outcome: ordinary dispatch reads the pending-collection flag before preparing
  collection-specific exception and origin state. Keep every existing safe
  point and interrupt check at the same opcode boundary.
- Baseline: `03474af8`, source fingerprint
  `2779488c7fafbba22b2cbc486ed43f2b71db7d03e6c7822b929c32ea416d7204`,
  executable `72202b7701f5add2db49a5128be031d984ff45454db6ab96403649f4d20a6224`.
  Analysis-only instructions are 91.8486 billion versus PHP's 10.3366 billion.
- Evidence: the current executable still loads and tests exception state before
  the pending TLS probe, carries a separate optional-origin tag and combines
  both predicates on ordinary dispatch. The exact preceding profile counted
  274071199 opcode selections. The older compact boundary reduced instructions
  but was rejected under the previous timing rule; its measurements are not
  current-baseline acceptance evidence.
- Hypothesis: outline collection publication/throw handling after the pending
  probe, and store optional origin as `Option<NonNull<Instruction>>`, derived
  safely from the already-valid instruction reference.
- Semantic envelope: preserve opcode-by-opcode collection opportunities,
  256-opcode interrupt cadence, pending-exception exclusion, previous-opcode
  origin, restoration before VmError propagation, exception handling and frame
  transitions. No batching, name recognition, allocator, Value or frame layout
  change is permitted.
- Ownership and safety review: the sole integrator owns dispatch and this local
  boundary. Extraction adds one annotated unsafe block, with zero new unsafe
  functions or raw-pointer operations; the existing live-frame/op-array and
  no-borrow-across-PHP-reentry obligations are unchanged. This narrowly reviewed
  inventory change is recorded in the unsafe baseline. A safe NonNull conversion
  replaces the earlier prototype's unchecked conversion. Roll it back with the
  candidate if the checkpoint fails.
- Gates: automatic-GC admission/finalization/interrupt tests, six exact PHP
  collection differentials, relevant feature configurations, all-target checks,
  formatting and unsafe inventory; independent PGO training and phase-only and
  whole-command hardware counts, timings, retained controls and code sizes.
  Native ARM64 performance is unavailable; no native lowering changes.
- Stop rule: reject a changed callback/error/GC boundary, unsafe lifetime
  invariant, unbounded memory cost or no confirmed application instruction
  reduction. Time remains a recorded diagnostic under the user's current rule.

Collection-specific exception inspection, frame publication and throw handling
now live behind the existing pending flag. The optional previous instruction
uses one nullable pointer representation. The main executor shrinks from 394096
to 391247 bytes; the outlined collection helper occupies 1285 bytes. This
changes neither safe-point frequency nor interrupt cadence.

All 129 focused feature test executions pass, together with six exact PHP
differentials, formatting, all-target/all-feature compilation and the reviewed
unsafe inventory. The production source fingerprint is
`487803d132f2fe4ba07ecf46c1277e04a7436a6887c846f009041704a4716626`;
the independently trained executable is
`a66c09104063216ca695d1736a1bbfb8531d7765d473537aef81279993c69f6f`.
The two bounded services peak at 4901974016 and 4051984384 bytes respectively,
with no OOM or timeout. Cleanup completes locally; no private host is configured.

Analysis-only hardware counts fall from **91.8334 to 89.9791 billion (-2.02%)**,
against PHP's **10.3365 billion**. Independent whole-command windows confirm
111.1879 to 109.1533 billion and 111.1816 to 109.1485 billion, both approximately
**-1.83%**. All diagnostic output and statuses match. Analysis time medians are
9.1790 to 8.9461 seconds in the first window and 8.8685 to 8.7625 seconds in
confirmation; all samples, including their CPU occupancy, remain in
[the collection-boundary packet](performance-phpstan-gc-pending-samples.json).

The integrating task accepts this partial checkpoint under the user's
instruction-count priority. Confirmed instructions fall 6.34% for shared-frame
release, 7.81% for scalar return, 5.97% for property reads and 3.51% for relative
self return. Timing does not fall proportionally: the shared-frame and relative
self medians regress 4.34% and 12.61%, while scalar return and property reads
improve 6.43% and 1.46%. These explicit tradeoffs remain controls; they are not
discarded as noise or presented as universal speedups. Native ARM64 measurement
remains unavailable. The analysis instruction gap is still approximately 8.70x.

The archived exact profile also permits conservative opcode-body attribution:
7.900 billion instructions inside the main executor are reachable only from
`ReleaseTemps`, before counting called helpers. Shared machine-code tails are
left unattributed to individual opcodes. That profile predates cleanup metadata
and this boundary extraction, so it identifies the next investigation family
rather than claiming the same budget for the newly accepted source. Repeated
ownership, alias and snapshot scans are the next candidate; parity remains open.

## Rejected checkpoint: reusable statement ownership enumeration

- Outcome: statement retirement enumerates owned temporaries once and reuses
  that result while proving aliases, capturing GC snapshots and releasing slots.
- Baseline: `d3132526`, source fingerprint
  `487803d132f2fe4ba07ecf46c1277e04a7436a6887c846f009041704a4716626`,
  executable `a66c09104063216ca695d1736a1bbfb8531d7765d473537aef81279993c69f6f`.
  Analysis costs 89.9791 billion instructions versus PHP's 10.3365 billion.
- Evidence: the retained exact opcode attribution isolates 7.900 billion
  main-executor instructions to temporary retirement, plus separately counted
  helper calls. Repeated ownership checks appear in range existence, shallow
  retirement, identity counting, snapshot capture and final drop. This profile
  predates the two accepted representation changes; current instruction
  improvement must be established by a fresh exact-source A/B.
- Hypothesis: use a relative word of ownership bits for the first 64 slots of
  each release interval, including wide frames. All passes consume the same
  ordered enumeration. Longer intervals retain their existing live tail scan,
  so no allocation, frame-layout change or unbounded metadata is introduced.
- Semantic envelope: preserve compact-frame borrowed-slot exclusion, ascending
  release order, pending calls, original destructor/exception boundaries and
  snapshot proofs before any source is dropped. Re-read wide-frame prefix
  ownership after PHP callbacks. Alias counts and values still read current
  storage; the bitmap never carries a destructor or identity proof.
- Ownership: the sole integrator owns statement retirement and focused tests.
  No compiler, allocator, opcode/JIT format, collection cadence or public API
  change is admitted. Existing raw-pointer obligations and unsafe count remain.
- Gates: compact/wide/over-64-slot ranges, references, nested alias lifetimes,
  callback mutation, exceptions, GC snapshots and suspendable release; focused
  default/no-default/all-feature gates, exact PHP differentials, formatting,
  unsafe and all-target checks; independent PGO, phase/application instructions,
  all four controls, memory and code sizes. ARM64 native evidence is unavailable.
- Stop rule: reject stale ownership across re-entry, changed release/GC ordering,
  added ordinary allocation or no confirmed application instruction reduction.
  Timing remains visible under the user's instruction-first criterion.

The prototype passes 77 default-feature tests and four PHP differentials, but
fails the primary instruction gate. Analysis-only instructions increase from
**89.9931 to 90.2876 billion (+0.33%)**. Both independent whole-command windows
also increase, approximately 109.147 to 109.509 billion and 109.148 to 109.504
billion. Confirmation time improves from 8.3220 to 8.1979 seconds, which does
not override the instruction rejection. Every sample is retained in
[the rejected enumeration packet](performance-phpstan-statement-owners-samples.json).

The native prefix capture and combined prefix/tail iteration added work despite
reducing the number of source-level scans. Main-executor code grows from 391247
to 393246 bytes and the separate cleanup helper from 14238 to 15906 bytes.
All four controls also use more instructions. No new allocation or unsafe block
was introduced, and neither bounded measurement service hit OOM or timeout.
The remaining feature configurations were not run after independent rejection.
All prototype production and test edits are removed; the accepted runtime stays
at `d3132526`. Exact source, patch, test, executable and failed measurements are
preserved for reproducibility.

An initial reference probe exposes a pre-existing lifetime mismatch: when a
temporary destructor reads an object stored only through `$GLOBALS`, accepted
RPHP omits that object's later destructor after explicit unset. The prototype
has the same mismatch. This failed baseline probe remains separate from the
four successful PHP-equivalence cases and is not counted as a pass or fixed by
this checkpoint. The parity goal remains open.

The next read-only investigation concerns call metadata. In the archived exact
profile, 4,433,669 of 7,672,980 ordinary user-call admission visits encounter a
nonempty request-wide pending sidecar, as do 2,725,326 of 4,034,799 internal-call
visits. That sidecar combines invocation receivers, magic names and late-static
scope for multiple frames in a PHP array. Metadata belonging to an outer frame
can therefore reject an unrelated call. These counts do not prove how many
calls satisfy every later admission check. The full-call body itself costs
2,796,446,015 exclusive instructions, before its helpers; per-call metadata and
ownership are the next structural hypothesis, not a promised saving.

## Accepted checkpoint: call-local side-state admission

- Outcome: a pending receiver, magic name or late-static record belonging to
  another activation does not reject an ordinary callee's existing call ABI.
- Baseline: `123ba34b`, documentation-only successor of `d3132526`; production
  source remains `487803d132f2fe4ba07ecf46c1277e04a7436a6887c846f009041704a4716626`,
  binary `a66c09104063216ca695d1736a1bbfb8531d7765d473537aef81279993c69f6f`.
- Evidence: the exact nonempty-sidecar branch counts above, 8,736,018 full-call
  entries and 17,472,089 pending-receiver probes. Full-call entry can repeat
  generic preparation even when its side record belongs to an outer frame.
- Hypothesis: replace request-wide presence with the same current-record key
  interpretation used by receiver/magic extraction and late-static lookup.
  Reuse existing call strategies and their complete arity, type, effect and
  ownership checks; do not add another execution path or recognizer.
- Semantic envelope: any current-callee record, including late-static state,
  still forces full preparation. Malformed/opaque side state conservatively
  retains the existing full path. Another activation's record is only observed,
  never popped or mutated. LIFO semantics remain identical to existing readers.
  Named arguments and closure-capture tables retain their current checks.
- Ownership: the sole integrator owns the common side-record query and the four
  existing call admission sites. No Value, frame, executor layout, coroutine
  root representation, PHP array storage, unsafe operation or JIT ABI changes.
- Gates: nested invokable/magic argument evaluation, current and outer wide
  late-static scopes, callable checks, named arguments, default/coercive types,
  exceptions and suspended Fiber state; focused feature gates and exact PHP
  differentials, formatting, unchanged unsafe inventory, all-target checks;
  same-manifest independent PGO, full and phase-only instruction A/B, retained
  controls, memory and code size. Native ARM64 measurement is unavailable.
- Stop rule: reject loss of receiver/scope, changed evaluation or diagnostics,
  current-call state admitted without preparation, or no confirmed application
  instruction reduction. Reject rather than broaden type/call strategies.

### Call-local admission result

The existing call strategies now query the key of the current side record.
Records owned by another activation no longer force full preparation; a record
owned by this callee still does. The broader native side-stack representation
considered during investigation was not implemented. The query changes no
owner, array, frame layout, coroutine root or unsafe operation.

Analysis-only instructions fall from **89.9827 to 88.3232 billion (-1.84%)**,
with PHP at 10.3364 billion. Whole-command medians fall from 109.1513 to
107.1336 billion in the first window and from 109.1526 to 107.1495 billion in
independent confirmation. All counters run for their complete enabled interval;
all five files, twenty findings, stderr and exit status match. Exact builds and
all samples are in [the admission packet](performance-phpstan-call-local-state-samples.json).

Analysis times are 8.3260 versus 8.2777 seconds initially and 8.3857 versus
8.2009 seconds in confirmation. The latter reference PHP median is 0.9253
seconds. Confirmation RSS is 610094 versus 610184 KiB. These time and whole-
command instruction scopes remain separate from the phase-only counters.

The integrating task accepts explicit control tradeoffs under the user's
instruction priority. Shared-frame instructions increase 0.29% and scalar-frame
instructions 0.47%; property-read instructions are unchanged and relative-self
instructions decrease 0.08%. Confirmed times change by +0.66%, +3.25%, +1.98%
and -6.79%, respectively. The first scalar timing regression is +6.67%, also
retained. This checkpoint improves the application instruction budget and does
not claim a speedup for every control. Main-executor code grows from 391247 to
392365 bytes; the shared key-query helper is 304 bytes.

Validation records **318 successful focused test executions** across default,
no-default and all features, three exact PHP differential programs, formatting,
unchanged unsafe inventory and all-target/all-feature compilation. Existing
ignored coroutine tests remain identified as ignored. Two initial fixture tests
failed because the embedding helper omitted source context required for TypeError
trace frames. The unchanged programs match PHP and baseline through the CLI;
the corrected source-aware helper keeps the original output assertions. The
failed gate is retained separately and is not counted as a pass.

PGO uses the unchanged independent 18-case manifest. Instrumented and final
builds take 288.56 and 219.89 seconds. The largest aggregate peak is 4,494,381,056
bytes, with no OOM or timeout. Both checkouts run cleanup, the disposable Cargo
build is removed after its saved executable is verified, and no private benchmark
host is configured. Native evidence remains x86-64 only. The remaining phase
instruction ratio is **8.54x**; parity is still open. A fresh exact profile of
this accepted executable will attribute the remaining work before another edit.

### Fresh instruction attribution

An exact Callgrind run of `fcc3736f`'s accepted executable completes with
identical PHP output and **108,942,084,130 whole-command instructions**. This is
a profiler total, separate from native analysis-only counters. Full-call entries
fall to 1,897,830 and receiver-side probes to 3,795,713. Their exclusive costs are
794,901,528 and 144,368,018 instructions. The main executor still accounts for
34,564,586,016 instructions over 274,043,536 central dispatches.

Conservative machine-CFG attribution assigns 7,246,986,683 exclusive main-body
instructions to `ReleaseTemps`, 3,093,499,192 to `Return`, 2,899,427,354 to
`DoFcall`, 2,852,389,706 to property reads and 2,373,652,669 to CV assignment.
Called helpers and shared tails are not included in these opcode costs. The
38,176,316 temporary-release dispatches remain a separate structural problem;
compiler TMP numbering is monotonic and frames above 64 slots use scanning.
No temporary reuse or wide-frame ownership change is implemented here.

The same profile counts 4,078,058 return-check calls into lexical scope,
charging 1,014,553,077 inclusive instructions before type matching. The canonical
return checker eagerly materializes that scope even when the visited contract
does not consume a relative class name. The previous deferred-scope prototype
was rejected under the former timing-first criterion despite fewer instructions;
the next bounded slice reevaluates that general context boundary on this exact
baseline under the current instruction priority.

## Accepted checkpoint: deferred return-type scope

- Outcome: absolute and scalar return checks do not derive unused lexical class
  strings. Existing relative members acquire their lexical/called scope at use.
- Baseline: clean `fcc3736f`, source
  `c7764a71db1cd337aa5cc7b635b4f30db93a7808e125d572f71fc8840fdb4d52`, binary
  `365a810a6d146c4aa28b5d239217e017ec4c032821d567f1c8f137357f49fb0e`.
- Hypothesis: carry the supplied names or live return frame through the canonical
  recursive type checker. Only its existing `self`, `parent` and `static` cases
  project that context. This replaces eager work, adding no signature admission,
  workload recognition, class-ID cache or temporary-slot optimization.
- Semantics: retain member order, null/union/intersection behavior, coercion,
  references, trait composition, bound closures, late-static calls, fallback
  spelling and diagnostics. No scope borrow crosses mutation or PHP re-entry.
- Ownership: the sole integrating task owns the canonical type-check context
  and focused regression cases. No Value/frame/executor layout or JIT ABI change;
  existing live-frame unsafe operations move with their original invariants.
- Gates: existing focused return/type/callable/relative-scope cases, direct PHP
  fixtures, relevant default/no-default/all-feature checks, all-target checks,
  formatting and unchanged unsafe inventory. Fresh independent PGO, whole and
  phase-only instruction comparisons, controls, code size and memory; all timing
  regressions remain evidence. Native ARM64 measurements remain unavailable.
- Stop rule: reject changed observable behavior, eager scope work merely moved
  elsewhere, or no confirmed application instruction reduction. Scope allocation
  and named-type resolution remain separate costs; this does not complete parity.

The initial full-message fixture exposes pre-existing PHP diagnostic differences
in the accepted baseline: trait methods use the trait name instead of the
composing class, and a nullable late-static type uses union spelling. Its failed
gate, original source and exact outputs are retained. A separate fixture checks
scope acceptance, exception class, late alias publication and reference identity
against PHP. The original full-message probe also remains a baseline/candidate
equality gate; passing that gate does not resolve or count as a PHP diagnostic
pass. Existing exact-message type tests remain unchanged. All three return-check
callers already provide dereferenced snapshots, so deferring context does not
change which referenced value is checked.

### Deferred scope measurements

The canonical checker now carries either supplied class names or the live return
frame. Only a visited relative-name member derives its lexical/called class.
Absolute names, scalar members and an accepted null member do not materialize
unneeded scope strings. The checker retains recursive member order and the
same reference, trait and bound-closure behavior; there is no class cache.

Analysis-only instructions fall from **88.3428 to 87.4536 billion (-1.01%)**,
against PHP's 10.3366 billion. Whole-command medians fall from 107.1564 to
106.1890 billion initially and 107.1460 to 106.1858 billion in confirmation.
Analysis times are 8.3268 versus 8.1831 seconds initially and 8.1863 versus
8.1133 seconds in confirmation; PHP measures 0.9451 seconds in the latter
window. Confirmation RSS is 610164 versus 610192 KiB. These are separate
measurement scopes. All five files, twenty findings, stderr and status match.

The relative-self control spends 0.39% more instructions because it does consume
scope; the other three instruction budgets stay effectively unchanged. Confirmed
times change by -13.54% for shared frames, -8.02% for scalar frames, +0.67% for
property reads and -1.65% for relative self. The unchanged instruction budgets
and varying native times remain distinct evidence; they do not multiply the
application's roughly one-percent instruction win. All samples are retained in
[the deferred-scope packet](performance-phpstan-deferred-type-scope-samples.json).

The integrating task accepts this instruction reduction and its explicit
relative-self tradeoff. Validation records **343 focused test executions**
across default, no-default and all features, two exact PHP differential programs,
unchanged baseline full-message diagnostics, formatting, unchanged unsafe
inventory and all-target/all-feature compilation. The initial PHP diagnostic
failure remains unresolved evidence and is not counted as a pass.

The exact source is
`5cee45d9441db00185b852ddab3c6fbae898e4179013cdb8ec2a797c44541db4`;
the executable is
`33e760c108946c16830069e255c89f3fdce754893e71eff4e9854b1c21984cf4`.
Main executor size changes from 392365 to 392413 bytes; return preparation
shrinks from 878 to 387 bytes. Fresh PGO uses the same independent 18 programs;
instrumented and final builds take 259.85 and 197.50 seconds. The aggregate
peak is 4,799,823,872 bytes, with no OOM or timeout. Both local checkouts run
cleanup; the exact executable and source are retained after disposable build
removal. No private benchmark host is configured. Native measurements remain
x86-64 only. The remaining analysis instruction gap is **8.46x**.

## Accepted checkpoint: ownership bitmap for wide-frame prefixes

- Outcome: the first 64 slots of every frame use the existing exact ownership
  bitmap; only slots beyond that prefix retain value scanning. Frame size no
  longer disables ownership metadata for an otherwise representable slot.
- Baseline: clean `9da0a81d`, source
  `5cee45d9441db00185b852ddab3c6fbae898e4179013cdb8ec2a797c44541db4`,
  executable
  `33e760c108946c16830069e255c89f3fdce754893e71eff4e9854b1c21984cf4`.
  Analysis costs 87.4536 billion instructions versus PHP's 10.3366 billion.
- Evidence: the retained exact `fcc3736f` profile, preceding only the accepted
  scope deferral, records 23,589,875 wide-frame temporary-release visits versus
  14,586,441 compact visits. Wide range discovery inspects 39,170,123 values.
  TMP writes and return cleanup also discard the bitmap for the entire frame.
  These counts select the representation hypothesis; they do not predict savings.
- Hypothesis: maintain the already allocated header word for the first 64 slots
  regardless of total frame size. Slot publication, transfer and retirement
  share that rule. Prefix-only release intervals and committed frame cleanup
  can then use ownership bits without constructing another temporary index.
- Semantic envelope: no change to slot numbering, initialization, release order,
  reference/borrow ownership, callbacks, exception handling, GC snapshot proofs,
  coroutine roots or destructor policy. Keep existing small-frame late-static
  encoding and all argument-borrow admission rules. Observe live ownership after
  callbacks; no value/alias proof may persist across PHP re-entry.
- Ownership: the sole integrator owns frame metadata and its existing publishers
  in ordinary, macro and coroutine execution plus focused regressions. Header
  and Value layouts, VM stack geometry, opcode/JIT formats and region admission
  remain unchanged. No heap allocation or wider bitmap is introduced.
- Gates: prefix/tail boundary writes and transfers, 32/64-slot late-static
  boundaries, wide statements and returns, references, nested/callback cleanup,
  fibers and suspended coroutine roots; focused feature and PHP differentials,
  unchanged unsafe inventory, formatting and all-target compilation. Independent
  PGO, whole/analysis-phase instructions, four controls, code size and memory.
  Native ARM64 execution remains unavailable; shared lowered contracts stay fixed.
- Stop rule: reject a missing/stale owner bit, changed callback or GC behavior,
  dependence on a widened borrow/region admission, or no confirmed application
  instruction reduction. Preserve failed gates and every measurement.


### Prefix metadata audit before acceptance

The first candidate passes the five PHP lifetime differentials, but a separate
scope audit rejects it before acceptance. Four surplus-argument programs already
fail on the baseline: a declared compact method can acquire a physically wider
frame. Direct property access incorrectly reads its upper ownership word as a
called-class ID. Tracking owners in a frame wider than 64 slots changes one of
these diagnostics, so the initial candidate and all results remain failed
evidence rather than an accepted speedup.

The repair uses the existing geometry-aware class resolver for static reads and
the embedded-scope accessor for the assignment cache. Deferred scalar fallback
also transfers ownership bits separately from its embedded scope, republishing
the latter for the destination frame. Focused tests cover surplus scalar/heap
arguments, cache reuse, and materialization into 16/40/80/130-slot frames. All
seven scope audit programs must now match PHP, including the four baseline
failures. This is a correction to the existing metadata contract, with no new
PHP behavior or widened execution admission.

### Prefix ownership measurements

The final candidate reduces analysis-only instructions from **87.4294 to
85.8101 billion (-1.85%)**, against PHP's 10.3338 billion. Whole-command
medians fall from 106.1909 to 104.4630 billion initially and 106.1931 to
104.4681 billion in confirmation. Analysis times are 8.6329 versus 7.9881
seconds initially and 9.0176 versus 8.7357 seconds in confirmation. PHP measures
1.0274 seconds in the latter window. Confirmation RSS is 611270 versus 609950
KiB. The phase counter and analysis timer measure the same analysis interval;
whole-command instruction totals include startup. Every output, finding and
exit status matches.

All four controls use fewer instructions: shared frames -2.61%, scalar frames
-2.42%, property reads -3.22%, and relative self -1.84%. Confirmed times change
by -6.46%, **+3.80%**, -1.79%, and **+4.04%**, respectively. The scalar timing
regression also occurs in the first window; relative self improves initially
but regresses in confirmation. Their timing cause is not established. The
integrating task accepts these explicit tradeoffs under instruction priority,
not as evidence that every program becomes faster. All valid samples and both
failed compile gates remain in
[the prefix ownership packet](performance-phpstan-frame-prefix-samples.json).

Validation records **223 focused test executions** across default, no-default
and all features, five lifetime and seven static-scope PHP differentials,
formatting, and all-target/all-feature compilation. Four pre-existing static
scope failures now match PHP. The rejected variant and its different diagnostic
remain failures in the packet. Production unsafe blocks decrease from 1749 to
1748; no new unsafe operation is introduced. Small-frame scope bits, wide TMP
initialization, callback retirement order, borrowing and region admission remain
under the existing contracts.

The exact source is
`d5fa352d4c3a83a53f858e274e8dbc800cb4104ed5ef2b4047056bf20c6966ad`;
the executable is
`34ada5d931f1a4163c8444295914d538698348cbba42ad5c2a69cfa0d31d1405`.
Fresh PGO uses the same independent 18 programs. Instrumented and final builds
take 274.72 and 221.39 seconds. Main executor size decreases from 392413 to
390710 bytes; return retirement grows from 3467 to 3800 bytes. The repaired
candidate's aggregate peak is 5,007,355,904 bytes, with no OOM or timeout. Both
local checkouts run cleanup; the exact source and executable are retained after
disposable build removal. No private benchmark host is configured. Native
measurements remain x86-64 only. The remaining instruction gap is **8.30x**.

A fresh analysis-only instruction sample of this exact candidate selects the
next investigation. Roughly 30.6 billion sampled instructions belong to the
main executor, with further costs in class/method lookup, value retirement and
regular expressions. Three lost samples and unusable callchains prevent exact
or inclusive attribution; this profile is selection evidence only. The hardware
phase counters above remain the acceptance evidence.

## Accepted checkpoint: borrowed regular-expression continuations

- Outcome: joining a nested sequence to its enclosing continuation borrows
  immutable AST nodes instead of cloning their trees into temporary vectors.
- Baseline: accepted `a81a0a1d`, source
  `d5fa352d4c3a83a53f858e274e8dbc800cb4104ed5ef2b4047056bf20c6966ad`,
  executable
  `34ada5d931f1a4163c8444295914d538698348cbba42ad5c2a69cfa0d31d1405`.
  Analysis costs 85.8101 billion instructions versus PHP's 10.3338 billion.
- Evidence: the earlier exact `fcc3736f` profile records 839,693 nested sequence
  joins. Their direct vector clone, extension and destruction edges cost
  752,191,420, 307,033,460 and 440,239,715 instructions. These disjoint call
  edges include their helpers; recursive node-clone totals must not be added
  again. The current phase-only sample still identifies matcher and node-clone
  costs, but its skid and lost samples prevent exact attribution.
- Hypothesis: a copyable cursor over a borrowed node slice and its enclosing
  cursor preserves traversal while eliminating continuation-vector allocation,
  AST cloning and destruction. Runtime capture/restore nodes remain scoped to
  their synchronous matcher call. No compiled regex or result is cached anew.
- Ownership: the sole integrator owns `src/regex.rs` and focused regressions.
  No compiler, Value/frame layout, opcode/JIT contract or execution admission
  changes. This is safe Rust with ordinary borrow-checked stack lifetimes.
- Semantic envelope: retain branch order, capture rollback, subroutine scope,
  control verbs, empty matches, flags, UTF handling and recursion/backtrack
  budgets. No successful or failed path may be skipped or duplicated.
- Gates: focused regex and preg tests, exact PHP differentials for nested
  continuations, groups, recursion and controls, all-target compilation and
  feature checks. Fresh independent PGO with the same 18 inputs; PHPStan phase
  and whole counters, four existing controls and a nested-regex holdout. Reject
  semantic drift or no confirmed application instruction reduction. Keep every
  failure and valid sample. ARM64 native evidence remains unavailable.

### Borrowed continuation measurements

Analysis-only instructions decrease from **85.8000 to 84.4368 billion
(-1.59%)**, against PHP's 10.3366 billion. Whole-command medians decrease from
104.4810 to 103.0372 billion initially and 104.4712 to 103.0310 billion in
confirmation. Analysis times change from 7.9690 to 7.7335 seconds initially and
7.8346 to 7.7266 seconds in confirmation; PHP takes 0.9455 seconds in that
confirmation window. Confirmation RSS is 610194 versus 611260 KiB (+0.17%).
All five files, twenty findings, stderr and exit status match. Analysis-only
counters and whole-command counters remain separate measurement scopes.

The four existing controls have effectively unchanged instruction counts. The
independent nested-regex holdout falls from 3.7270 to 1.8785 billion instructions
(-49.60%) and from 0.2448 to 0.1184 seconds (-51.65%) in confirmation. This is a
narrow control for continuation storage, not a claim about all regex execution.
It is excluded from PGO training. All valid runs are retained in
[the continuation packet](performance-phpstan-regex-continuations-samples.json).

Validation records **337 focused test executions** across default, no-default
and all features, formatting, unchanged unsafe inventory and all-target/all-
feature compilation. A global-operation differential matches PHP exactly; 18
of 19 nested cases also match. The remaining PRUNE case already differs on the
baseline: `~^(?:(a(*PRUNE)b)|(ac))d$~` on `acd` returns a match in RPHP and none
in PHP. Candidate and baseline output remain identical for the complete probe.
The original failed differential gate, fixture and outputs remain retained;
this unresolved compatibility case is not counted as a PHP pass. Initial
formatting and duplicate-definition compile failures are also retained.

The copyable continuation cursor borrows immutable node slices and enclosing
cursors. Capture/restore nodes use stack storage scoped to the synchronous
matcher call. Control quantifiers still clone their repeated inner node; this
checkpoint does not eliminate every AST clone. Branch order, capture rollback,
backtracking budgets and recursion behavior remain under the canonical matcher.

The exact source is
`48432692b66e1c17c67a8657a5a9e972e3229d0dd67903d737624512af4f76b2`;
the executable is
`1303e6fd731a8bd83ac44a7cedcf4792eef1a079e98ea7a441d0977e6f34b841`.
Fresh PGO uses the unchanged 18 independent training programs. Instrumented and
final builds take 287.20 and 210.62 seconds. Main executor size stays 390710
bytes; the sequence matcher shrinks from 6302 to 5689 bytes. Aggregate peak is
4,974,931,968 bytes, with no OOM or timeout. Both local checkouts run cleanup;
exact sources, executables and failed evidence survive disposable build removal.
No private benchmark host is configured. Measurements remain x86-64 only.
The integrator accepts this reduction; the remaining **8.17x** analysis
instruction gap leaves the overarching parity goal open.

## Accepted checkpoint: resolved class method metadata

- Outcome: repeated visibility/staticness/declaring-owner queries use an index
  of declared methods instead of rewalking trait and parent metadata.
- Baseline: clean `e0027706`, source
  `48432692b66e1c17c67a8657a5a9e972e3229d0dd67903d737624512af4f76b2`,
  executable
  `1303e6fd731a8bd83ac44a7cedcf4792eef1a079e98ea7a441d0977e6f34b841`.
  Analysis costs 84.4368 billion instructions against PHP's 10.3366 billion.
- Evidence: the exact earlier profile records 3,932,662 method-info visits
  including recursion. Disjoint callers include private dispatch (1,377,453
  calls, 1.5264 billion inclusive instructions) and ordinary method-call setup
  (1,205,234 calls, 1.4403 billion). Recursive edges are not added again. The
  later phase sample still identifies this unchanged resolver as a hot cost.
- Hypothesis: augment the existing per-class own-method index with resolved
  ASCII metadata. Populate only from the finite declaration graph, through the
  canonical resolver; arbitrary requested names never grow the cache. Preserve
  the canonical resolver for Unicode and incomplete/unregistered hierarchies.
- Semantic envelope: preserve declaration order, abstract filtering, trait
  aliases/adaptations, inherited private methods, native contracts and exact
  owner spelling. An incomplete graph remains canonical until registry growth;
  native contract additions/access/order changes invalidate derived indexes.
  A completed immutable hierarchy survives unrelated class registration.
- Ownership: the sole integrator owns `src/runtime/mod.rs` and focused tests.
  Keep executor, class, callable and Value layouts, opcode caches, callbacks,
  JIT contracts and execution admission unchanged. No new unsafe operation.
- Gates: indexed/canonical equivalence including misses, non-ASCII names,
  aliases, late ancestors and native mutations; PHP dispatch/visibility/trait
  differentials and relevant feature suites; formatting, unsafe and all-target
  checks. Fresh unchanged independent PGO, application phase/whole counters,
  existing controls and a polymorphic inherited-method holdout, RSS and code
  size. Native measurements remain x86-64 only.
- Stop rule: reject semantic drift, growth driven by query count, unsupported
  hierarchy traversal, or no confirmed application instruction reduction.
  Account for one-time population and inherited-metadata memory in the complete
  application measurement. Preserve failures and every valid sample.

### Resolved metadata measurements

Analysis-only instructions decrease from **84.4235 to 82.5389 billion
(-2.23%)**, against PHP's 10.3363 billion. Whole-command medians decrease from
103.0515 to 101.1661 billion initially and 103.0360 to 101.1774 billion in
confirmation. Analysis times are 7.8615 versus 7.6711 seconds initially and
7.8557 versus 7.7273 seconds in confirmation; PHP takes 0.9400 seconds in the
latter window. Confirmation RSS increases from 609978 to 618540 KiB (+8562
KiB, 1.40%). These counters cover different intervals. All five PHPStan files,
twenty findings, stderr and status match.

The independent polymorphic inherited-method holdout uses **14.94% fewer
instructions** and takes 15.24% less time in confirmation. It was excluded from
PGO training. The four original non-regex controls and the prior regex holdout
keep instruction budgets within 0.26% of baseline. Confirmed time changes are
+1.32% shared frames, +2.74% scalar frames, +1.34% property reads, -1.45%
relative self and +3.62% nested regex. The first window has lower times for
the first three controls; the regex regression occurs in both windows. The
integrator accepts that explicit timing tradeoff under the user's instruction
priority, while making no general speedup claim. Every sample is retained in
[the method metadata packet](performance-phpstan-method-metadata-samples.json).

Validation records **516 focused test executions** across default, no-default
and all features, formatting, unchanged unsafe inventory and all-target/all-
feature compilation. Two new PHP differentials match exactly, including class
aliases, private/inherited access, magic fallback and dynamic class declaration.
Two retained broader trait-composition probes already differ from PHP on six
lines each on the baseline; the candidate remains byte-identical to baseline.
They are not counted as PHP passes. The failed initial test compile and an
invalid visibility declaration in the first test candidate remain in the
packet. The declaration-bounded cache preserves the canonical resolver for
incomplete and non-ASCII graphs, and native metadata updates clear descendant
indexes. There is no new unsafe code or execution-tier admission.

The exact source is
`bd14d7b0e4906eb176bfb9db2d0ca454b6b84af232f0fd2e2f38b657c8bc93a4`;
the executable is
`a6ba0267f926a030c8c9d859c8872f4204bc175a42319cb1441be25860bf705f`.
Fresh PGO uses the same 18 independent programs. Instrumented and final
builds take 289.37 and 212.87 seconds. Main executor size changes from 390710
to 390674 bytes; the hot method-info entry is 1130 bytes, with the original
resolver kept separately for fallback. Aggregate preparation peak is
5,063,122,944 bytes and verification peak is 4,297,121,792 bytes, both within
the 6 GiB boundary with no OOM or timeout. Native evidence remains x86-64 only.
The remaining analysis instruction gap is **7.99x**, so parity stays open.
