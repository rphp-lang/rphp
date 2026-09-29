# PHPStan instruction and runtime parity

Status: active; partial checkpoints do not complete this goal.

The outcome is the same five-file PHPStan analysis, with identical diagnostics,
exit status and PHP semantics, at the reference PHP instruction count and
analysis time. Startup is measured separately. A result-cache hit is not an
analysis and cannot satisfy this goal.

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
