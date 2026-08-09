# RPHP runtime architecture and coroutine roadmap

This roadmap protects two properties at the same time: the runtime must remain
easy to reason about as its supported PHP surface grows, and source-level
cleanup must not reintroduce abstraction costs into already-proven hot paths.
Refactoring and coroutine work therefore use the same rule as performance
work: preserve canonical PHP execution, measure before and after, and keep the
fast path allocation-free unless the feature itself requires an allocation.

## Structural refactoring track

At the start of this track `src/vm/execute.rs` had 21,595 lines, including an
approximately 1,630-line `NativeMixedBuildState` implementation. Other large
boundaries are the roughly 3,300-line `execute_ex` dispatch function, the
roughly 3,100-line `Compiler` implementation, the 5,500-line quick-IR module,
and the approximately 1,100-line ARM64 straight-region compiler. These sizes
make review and ownership harder even when runtime behavior is correct.

The target is not small files at any cost. The target is one responsibility
per source unit, explicit data flow, and no new dynamic dispatch, allocation,
reference counting, or unpredictable branch in a hot loop merely to improve
source layout.

### Refactoring rules

- Start with codegen-neutral physical splits. `include!` may keep code in the
  original Rust module while responsibilities are separated and private
  interfaces are still changing.
- Move to real modules only after the shared inputs and outputs are narrow and
  stable. Visibility should be reduced deliberately rather than widening most
  runtime internals to `pub(crate)` for convenience.
- Prefer plain data and free functions over a new trait hierarchy. Trait-object
  dispatch is not allowed in an opcode, quick-loop, or generated-code hot path.
- Do not move canonical fallback semantics into a JIT-specific module. Native
  lowering may consume proven IR, but the baseline bytecode executor remains
  authoritative.
- Every structural commit must pass default, no-default-feature, quick-loop,
  type-hint, corpus and ARM64/JIT tests applicable to the moved code.
- Performance-sensitive splits require an A/B release build. Existing corpus
  and holdout medians should remain within normal run variance; unexpected
  movement is investigated before the refactor is accepted.

Source units should normally remain below about 2,000 lines and one manually
maintained implementation or function below about 500 lines. Larger units are
allowed only for a measured hot dispatch body or generated/mechanical tables,
and should carry a short explanation of why another boundary would cost more
than it clarifies.

### Refactoring sequence

1. **Native mixed lowering.** Split operation/catalog/call bookkeeping,
   virtual object-array lowering, scalar/property lowering, and composed typed
   lowering, followed by native admission/kernel construction and runtime
   binding/chunk/side-exit execution. Keep all pieces in the execute module
   initially so visibility and code generation do not change. This first
   vertical physical split is now complete.
2. **Quick runtime.** Separate activation state, String state, array contexts,
   object/method resolution, native admission, native execution and canonical
   deoptimization. The quick-loop dispatcher should orchestrate these pieces,
   not own every implementation.
3. **Baseline opcode dispatch.** Group cold opcode helpers by calls/objects,
   arrays/iteration, exceptions, generators and scalar operations. Keep the
   central dispatch loop inlinable; source extraction must not turn every
   opcode into an unconditional Rust function call.
4. **Quick IR and planning.** Separate stable IR/data definitions from pattern
   recognition and from runtime execution. Compiler proofs must not depend on
   executor state, and native lowering must not inspect PHP source names.
5. **Compiler.** Split declaration/type facts, class/method compilation,
   function bodies, expressions, control flow and call lowering around a small
   shared compilation context.
6. **Native backend.** Separate ARM64 encoding, executable-memory ownership,
   IR validation/liveness, straight-region emission and compiled-code caches.
   This also establishes the boundary needed by a future x86-64 backend.

After each sequence item, update this document with resulting module sizes and
any boundary that intentionally stayed large for performance reasons.

First checkpoint (2026-08-03): `NativeMixedBuildState` is physically split
into core/object (441 lines), virtual object-array (681), scalar/property (360)
and composed typed (175) lowering units. They remain in the original execute
module through conditional `include!`, reducing `execute.rs` to 19,965 lines
without widening private APIs. The native-CPU `max-perf` binary retains exactly
the same byte size. In 101 order-randomized A/B runs, four corpus workloads
move between -0.16 and +0.20 percent and the independent routing holdout moves
-0.48 percent, with identical output. This is treated as measurement noise and
establishes the process for the remaining structural slices.

Second checkpoint (2026-08-03): native mixed admission/kernel construction
(676 lines) and runtime binding, chunk execution and exact side exits (443)
are separated as well. `execute.rs` falls again to 18,852 lines and the
native-CPU `max-perf` binary remains byte-for-byte the same size. In a new 101
order-randomized A/B against the preceding refactor binary, all outputs remain
identical; the four corpus deltas range from -0.63 to +0.07 percent and the
independent routing holdout is +0.81 percent. The whole native-mixed vertical
slice is therefore physically separated with no statistically actionable
performance change.

Third checkpoint (2026-08-04): the quick-runtime track begins by separating
object/method resolution and call accounting (416 lines), borrowed packed/hash
array state (117), retained String state and dynamic-key cache (246), and the
guarded virtual object-array activation (352). All remain in the execute module
through `include!`; `execute.rs` falls to 17,731 lines and the native-CPU
`max-perf` binary again retains the same byte size. The first 101-run A/B put
typed order and both ledger workloads between -0.61 and -0.28 percent, with
the routing holdout at -0.44 percent. Untyped order initially
reported +1.20 percent, so the checkpoint was not accepted on that batch; a
dedicated 301-pair randomized rerun measured +0.02 percent with overlapping
quartiles and identical output. No actionable performance change remains.

Fourth checkpoint (2026-08-04): the quick-kernel layer is physically divided
into its data model (213 lines), cached and straight array access (131), pure
kernel pattern recognition (553), shared scalar/method/deoptimization helpers
(212), array-loop execution (753), and conditional/branch execution including
the ARM64 conditional path (666). The six units remain private items in the
execute module through `include!`; `execute.rs` falls to 15,222 lines. A
mechanical reconstruction check confirms that all 81,932 moved source bytes
are identical to the preceding revision. The native-CPU `max-perf` binary has
the same file and Mach-O segment sizes. In 101 order-randomized A/B pairs, all
five outputs are identical and the four corpus workloads plus independent
routing holdout range from -0.47 to +0.51 percent. Core, default, no-default,
quick-loop, type-hint, corpus and ARM64/JIT matrices all pass, so the movement
is treated as normal process noise rather than an actionable regression.

Fifth checkpoint (2026-08-04): the remaining 1,132-line
`run_quick_long_ops_loop` dispatcher is isolated as one 1,137-line source unit
including its structural header. It intentionally stays a single Rust
function: after the specialized kernels are attempted, its generic operation
loop is still a measured hot path, so source decomposition must not introduce
unconditional function calls per operation. `execute.rs` is now 14,089 lines,
and a reconstruction check verifies all 44,242 moved source bytes. The
native-CPU `max-perf` binary and Mach-O segments retain the same sizes. Across
101 randomized A/B pairs, four corpus workloads and the routing holdout range
from -0.19 to +0.22 percent with identical output. All applicable feature
matrices pass; the dispatcher extraction is therefore codegen-neutral within
measurement resolution.

Sixth checkpoint (2026-08-04): the remaining quick-loop support block is split
into induction execution (111 lines), scalar/composed call preparation (286),
ARM64 native call and accumulate lowering/execution (1,136), and the canonical
accumulate runtime (896). The 20-line slot-commit helper intentionally remains
next to the include list because it is shared by several later quick-kernel
units. `execute.rs` falls to 11,674 lines, and all 94,078 moved source bytes
reconstruct exactly. The native-CPU `max-perf` binary and Mach-O segment sizes
remain unchanged. The four corpus workloads move between -0.01 and +0.43
percent in 101 randomized A/B pairs. Routing initially reports +1.41 percent,
so it is checked independently rather than accepted: 301 randomized pairs
reduce the median delta to +0.07 percent and paired median to +0.35 percent,
with overlapping quartiles and identical output. Default, all-feature,
no-default, corpus, quick-loop, type-hint and ARM64/JIT matrices all pass.

Seventh checkpoint (2026-08-04): the baseline-dispatch track begins by moving
already-cold helpers into execution entry/API (494 lines), include/throw control
(237), object and call initialization (990), foreach and generators (399),
named arguments (137), nullsafe/clone object values (104), and concatenation
(73). The central `execute_ex` loop is deliberately untouched, so the split
adds no opcode-level call or abstraction. `execute.rs` falls to 9,262 lines,
and all 101,015 moved source bytes reconstruct exactly. The native-CPU
`max-perf` binary and Mach-O segment sizes remain unchanged. Across 101
randomized A/B pairs, all four corpus workloads and the routing holdout retain
identical output and range from -0.25 to +0.30 percent. Default, all-feature,
no-default, corpus, quick-loop, type-hint and ARM64/JIT matrices all pass.

Eighth checkpoint (2026-08-04): the 3,289-line `execute_ex` body is isolated as
a 3,292-line `baseline_dispatch.rs` source unit including its structural
header. This is an intentional exception to the normal source-unit size: it
remains one Rust function and one dispatch loop, so the refactor adds no
per-opcode call, dynamic dispatch or allocation. `execute.rs` falls to 5,974
lines, and all 172,265 moved source bytes reconstruct exactly. The native-CPU
`max-perf` binary and Mach-O segment sizes remain unchanged. In 101 randomized
A/B pairs, the four corpus workloads and routing holdout retain identical
output and range from -1.00 to +0.46 percent, with routing at +0.07 percent.
The negative edge is treated as layout or measurement noise rather than an
optimization claim. All applicable feature matrices pass.

Ninth checkpoint (2026-08-04): nine semantically cold opcode bodies move behind
explicit `#[inline(never)]` helpers: `CallUserFuncArray`, static-property fetch,
`instanceof`, constant fetch/definition, default/global/static binding, closure
creation and closure capture. Hot arithmetic, branches, arrays, `InitFcall`,
`DoFcall`, method dispatch and `Return` remain directly in `execute_ex`.
`baseline_dispatch.rs` falls from 3,292 to 3,115 lines and the cold helper unit
is 249 lines; `execute.rs` is 5,975 lines. The release file grows by 992 bytes
while its Mach-O segment sizes remain unchanged. In the first 101 randomized
corpus/holdout pairs, four results range from -0.49 to +0.70 percent and typed
order reports +1.72 percent. A required independent 301-pair typed-order check
instead measures -0.10 percent by aggregate median and -0.86 percent by paired
median with overlapping quartiles, so the regression is not reproduced. Five
opcode-focused 101-pair holdouts all improve on this build, from -1.57 percent
for static binding to -16.55 percent for default-parameter binding. These
holdouts support the compact-dispatch design but are not treated as universal
speedup claims. An additional 228 focused callable, closure, constant,
`instanceof`, interface and global/static tests pass alongside every standard
feature matrix.

Tenth checkpoint (2026-08-06): the quick-IR/planning track is physically split
without changing its module or private interfaces. Stable IR/data definitions
remain in the 1,136-line `quick.rs`; array planning (542 lines), Double-call
planning (434), scalar/induction planning (270), accumulate planning (813),
long-region helpers (375), long-region construction (1,712) and tests (2,022)
are separate `include!` units. All moved non-boundary source lines reconstruct
byte-for-byte; only six blank separators became include boundaries before the
six corrected test feature guards.
Those guards now keep quick-plan selection assertions and integration markers out of
`--no-default-features`, where the compiler intentionally cannot select that
tier. The native-CPU `max-perf` `__text` section remains exactly 1,780,708
bytes; only diagnostic source-path strings change. In 101 order-alternated A/B
pairs, the four corpus workloads and routing holdout retain identical output
and range from -0.13 to +0.61 percent. This is normal measurement noise, so
the structural split is accepted with no runtime optimization claim. Clean
native-CPU x86-64 builds likewise retain identical `.text` sizes in both
configurations (2,127,818 bytes without JIT and 2,340,314 with JIT); 101
CPU-pinned pairs range from -0.23 to +0.09 percent without JIT and from -0.08
to +0.09 percent in the JIT build. The complete x86 matrix passes 232 library,
113 quick-loop and 32 JIT tests plus every end-to-end suite, and the
no-default-feature all-target check remains green.

## Post-JIT coroutine branch

After the minimal typed-region JIT is stable and before broad compatibility
work, take a short, bounded architecture branch for cheap coroutines. The goal
is Go-like inexpensive logical tasks and context hand-off, not immediate Go API
or scheduler compatibility.

The defining requirement is pay-for-use behavior:

- a program that creates no coroutine performs no coroutine allocation and
  gains no scheduler check on ordinary calls or every opcode;
- a function that cannot suspend retains the existing frame ABI and remains
  eligible for the same no-JIT and JIT optimizations;
- suspend/resume is constant-time with respect to call depth and live-slot
  count: contexts are detached by pointer/state exchange, never by copying an
  ExecuteData chain or an OS stack;
- stack/storage segments are allocated lazily and pooled after completion.

### Runtime model

`CoroutineContext` should own a VM stack segment chain, top `ExecuteData`,
current opline, exception/unwind state, result/status and scheduler linkage.
The active executor keeps one pointer to the current context. Detach/attach
swaps that pointer and the active stack bounds; it does not walk frames or
touch dormant values.

The compiler propagates a `may_suspend` fact transitively through known calls.
Ordinary functions stay unchanged. Only suspension-capable boundaries need a
resumable activation and suspension metadata. Unknown/dynamic calls use a cold
guarded path rather than imposing a branch on every proven non-suspending call.

The first scheduler is cooperative and single-threaded. It proves frame
lifetime, reference-count ownership, exceptions, cancellation and exact resume
positions without requiring every RPHP value to become `Send`. Readiness-based
timers and non-blocking I/O come next. M:N work stealing is a later optional
step after object/value thread-safety has an explicit design; it must not be
smuggled into the first context-switch primitive.

Generators and coroutines should share stack/context storage where useful, but
their PHP-visible semantics remain separate. A generator yields a sequence to
its caller; a coroutine suspends a schedulable execution context.

### JIT contract

The initial JIT admits no implicit suspension inside a native region. A known
suspension operation ends the region at an exact side exit, publishes live
values, and resumes through canonical VM state. Later, explicit JIT safepoints
may use compact spill maps, but non-suspending regions retain their current
machine code and contain no coroutine poll.

Cancellation or preemption polling may occur at already-existing interrupt
checks and selected loop backedges. It must not become a new per-opcode test.
Native calls, property shadows and virtual objects must be fully published
before a context becomes externally resumable.

### Milestones

1. Add an internal context object and deterministic two-context
   suspend/resume test with no public API.
2. Move suspended frames to lazy pooled VM stack segments and prove cleanup,
   exception and `finally` behavior under repeated resume/drop.
3. Add a minimal PHP-facing spawn/suspend/resume/join API and structured parent
   ownership. API naming is decided only after the runtime primitive is
   measured.
4. Add bounded channels and a readiness scheduler for timers and non-blocking
   I/O. Blocking system calls must not silently stall all logical tasks.
5. Evaluate optional multi-threaded scheduling and work stealing separately;
   reject it if thread-safety costs leak into single-threaded PHP execution.

### Milestone 1 checkpoint (2026-08-09)

The first context-switch primitive is proven as the standalone executable
prototype in `tests/e2e_coroutine_context.rs`. `CoroutineExecutionState` owns
the main and pending VM stacks, top `ExecuteData`, exception, pending named
variadics, active generator and pending `__invoke` receiver. A pinned
`CoroutineContext` owns that state plus ID, status and result. The opt-in
`CoroutineDriver` owns the active-context pointer and borrows one exact
`ExecutorGlobals` for its full lifetime, so a suspended context cannot be
accidentally resumed against a different executor. Attach, detach and direct
context-to-context hand-off perform a fixed set of swaps and never inspect a
frame or dormant value.

The deterministic two-context test gives each context a real frame, distinct
exception, generator, variadic state and receiver, then switches in both
directions and verifies exact isolation and root-state restoration. Separate
state-machine coverage rejects busy-executor activation, inactive suspension,
completion with a live frame and resume after completion. Dropping a running
context or driver is an invariant failure rather than a dangling-pointer
escape.

Milestone one deliberately remains an integration-test prototype until lazy
stack ownership is ready in milestone two. Several semantically correct
production-linked variants were rejected: adding the active pointer directly
to `ExecutorGlobals` reproducibly regressed two pinned x86-64 regex controls by
4.84 and 3.67 percent, while sidecar/module-placement variants moved other
controls by up to 2.75 and 1.54 percent. Keeping an unused substrate linked
would therefore violate pay-for-use even without an executed branch. The
accepted boundary leaves both ordinary release binaries bit-identical to their
baselines: ARM64 SHA-256 `695b7e472ce1913a59e2fdfed105f5626c4d41ae7deea86027eac000de8dab7d`
and x86-64 `5359c1d345ecd9878c5e6bc0d358a7d97f294b80dc2b93be745a3e6cc533dcb8`.

The permanent one-million-hand-off release benchmark records 13.78 ns/switch
on the Apple ARM64 reference host and 11.44 ns/switch on the pinned x86-64
host, well below the initial 150 ns target. The prototype passes with default,
no-default and all features on both architectures, and all existing release
integration suites plus all-target/all-feature checks remain green. Milestone
two can now replace the eager prototype stacks with lazy pooled segments,
prove repeated resume/drop cleanup plus exception/`finally` behavior, and only
then re-evaluate promotion into the production crate under the same binary and
performance gates.

### Milestone 2 checkpoint (2026-08-09)

The executable prototype now uses lazy pooled stack ownership. A newly created
`CoroutineContext` contains no `VmStack`; its main 256 KiB page and pending-call
16 KiB page are checked out together only on first activation. Completion and
explicit discard return the pair to the driver-owned pool. Two simultaneously
live contexts construct exactly two pairs, 200 intervening hand-offs construct
none, and a third context reuses an existing pair. A separate 64-context cycle
constructs one pair and records 63 pool reuses.

Discard is now an ownership operation rather than a raw storage drop. It walks
the suspended `ExecuteData` chain, cleans bitmap-tracked heap slots, follows
and cleans deferred pending calls on the second VM stack, removes named
variadics, drops exception/generator/receiver state, pops every frame and only
then recycles storage. A context in `Running` or `Suspended` state cannot be
silently dropped; it must be completed or explicitly discarded. The repeated
cleanup test performs 64 suspend/resume/discard cycles with independent
reference-counted strings in both main and pending frames, a live exception,
named variadics and `pending_return_after_finally`. Exact pointer preservation
on the surviving string owners proves cleanup released both frame-owned
references without forcing a later COW detach.

Canonical PHP behavior is exercised separately rather than inferred only from
synthetic frame flags. Thirty-two fresh contexts execute a throwing
try/catch/finally script on one recycled stack pair and produce the exact
repeated `caught finally` output. Default, no-default and all-feature prototype
tests, complete release integration matrices and all-target/all-feature checks
pass on ARM64 and x86-64.

The permanent scaling benchmark also confirms the O(1) contract. On ARM64 a
one-frame/zero-slot context measures 11.25 ns/switch and a 64-frame context
with 32 dormant slots per frame measures 11.05 ns/switch. Pinned x86-64 records
12.53 and 12.42 ns/switch respectively. The ordinary million-hand-off runs are
11.10 ns on ARM64 and 13.01 ns on x86-64. Production remains untouched and
bit-identical at the milestone-one SHA-256 hashes, so lazy pooling and cleanup
still impose zero code-size, allocation or dispatch cost on ordinary PHP.

Milestone three can now begin with a measured production integration of this
substrate and structured parent ownership, followed by the minimal
spawn/suspend/resume/join surface. The internal caller and the substrate should
be admitted together; linking unused coroutine code ahead of that caller has
already proven capable of perturbing unrelated hot-code placement.

### Milestone 3 checkpoint (2026-08-09)

The first production integration is complete behind the non-default
`coroutines` Cargo feature. The PHP surface is deliberately small:
`coroutine_scope(callable)` establishes lexical ownership,
`coroutine_spawn(callable)` returns a positive task ID,
`coroutine_suspend()` cooperatively yields a running child,
`coroutine_resume(id)` reports suspended versus terminal state, and
`coroutine_join(id)` returns the child result or rethrows its exception.
Callbacks are zero-required-argument user functions or closures; generator
functions are rejected until generator/coroutine interaction has a separate
contract.

The implementation is split by responsibility rather than collected in one
runtime file. `runtime/coroutine.rs` owns the PHP boundary and feature-private
thread-local registration, `runtime/coroutine/state.rs` owns detached executor
state and cleanup, and `runtime/coroutine/scheduler.rs` owns task transitions,
the stack pool and structured teardown. The scheduler remains bound to one
`ExecutorGlobals` without adding a field to that structure. Contexts are pinned
boxes, and no mutable scheduler reference survives VM re-entry, so a running
child may safely insert a nested child even if the task map rehashes.

Scope teardown cancels every created or suspended child, cleans its complete
frame chain before recycling storage and propagates an unjoined failure. If
multiple children fail, the lowest task ID wins, making failure selection
deterministic rather than dependent on `HashMap` iteration order. Sixty-four
sequential production tasks construct one lazy stack pair and reuse it 63
times. Seven PHP integration scenarios cover suspend/resume/join, nested spawn
ownership, cancellation, joined and unjoined exceptions, parent
catch/`finally`, deterministic multiple failure selection and suspension below
a multi-frame PHP call chain.

Deep resumption retains the original two-argument `execute_ex` ABI and hot loop.
The feature-only wrapper repeatedly enters the current top frame until the
owned bottom frame completes; an exception may still cross the chain in one
entry, while suspension exits immediately. A feature-private thread-local
control bit distinguishes that suspension from the existing error carrier and
is consumed by the scheduler at the same boundary. It uses an empty
`String::new()` carrier, so steady-state hand-off allocates nothing and the
ordinary executor gains no TLS lookup, branch or extra argument.

The permanent million-cycle PHP API benchmark records 79.74 ns per
suspend/resume on ARM64 and 84.28 ns on pinned x86-64, including PHP internal
function dispatch and scheduler state transitions. This remains comfortably
below the original 150 ns target even though the final benchmark measures more
than the milestone-one internal pointer exchange. Default, no-default,
all-feature, all-target and complete release integration matrices pass on both
hosts.

Pay-for-use is checked against the intermediate regex callback optimization
commit `9f038eb`. On ARM64 the default release binary is byte-identical at
SHA-256 `f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`.
GNU build-id and symbol metadata differ on x86-64 because the Cargo feature list
changes, but executable/data/BSS sizes are identical at
2,931,803/49,784/2,504 bytes; removing only that metadata yields byte-identical
binaries at SHA-256
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.
Thus the default program remains unchanged after the coroutine commit.

The final feature-enabled release is admitted against the pre-phase
`3d546a2` binaries, not against a selectively chosen workload. Across 1,003
alternating pairs, ARM64 regex controls range from +0.54 to -17.86 percent and
pinned x86-64 from +0.07 to -12.57 percent. Across 101 application pairs,
ARM64 ranges from +0.33 to -1.98 percent and x86-64 from +0.47 to -10.41
percent. No control regresses by more than one percent. The callback wins come
from replacing a temporary `echo_to_string()` clone with the existing direct
`append_echo_to()` path; that independent optimization is committed separately
so the source of the gain remains reviewable. An isolated x86 comparison
against that intermediate commit still moves the most layout-sensitive callback
by +1.70 percent, confirming that multi-codegen-unit ELF placement remains a
measurement concern rather than claiming it has disappeared.

Milestone four can now add bounded channels plus a readiness scheduler for
timers and non-blocking I/O. It must preserve the current lexical ownership,
no-poll ordinary executor and single-threaded value contract; work stealing
remains a separate decision.

### Milestone 4a channel/readiness checkpoint (2026-08-09)

The first half of milestone four adds bounded FIFO channels and timer
readiness behind the existing non-default `coroutines` feature. The PHP API
now includes `coroutine_channel(capacity)`, `coroutine_send(channel, value)`,
`coroutine_receive(channel)` and `coroutine_sleep(milliseconds)`. A full
channel suspends its sender, an empty channel suspends its receiver, and the
oldest compatible waiter is resumed. Channel capacity must be positive; the
scope root remains the scheduler driver, so potentially blocking send,
receive and sleep calls are child operations.

The source boundary was tightened before adding scheduling policy.
`runtime/coroutine/api.rs` now owns all PHP handlers and descriptor
registration, while the scheduler delegates channel queues and readiness to
`scheduler/channel.rs` and `scheduler/readiness.rs`. `CoroutineContext` records
an explicit wait reason separately from runnable state. The ready queue is
FIFO, equal-deadline timers use insertion order, and direct resume removes its
queued entry so repeated wake/resume cycles cannot accumulate stale work.

A blocked receive retains the dormant caller frame and result slot. Direct
handoff later writes through one feature-private VM helper that updates the
canonical heap-slot bitmap, so strings, arrays and other owned values retain
normal frame-cleanup behavior. Scope teardown cancels channel and timer
waiters before their frames are released. Joining a waiting task drives other
runnable work, sleeps the executor thread only when no logical task is ready
and a timer is pending, and reports a deterministic deadlock when neither a
ready task nor a future timer can make progress.

Five new PHP integration scenarios cover bounded backpressure/FIFO order,
heap-value handoff to an already waiting receiver, runnable-before-timer
ordering, channel deadlock and scope cancellation of a waiter. Dedicated unit
tests cover sender promotion, ready FIFO behavior, equal-deadline timer order
and removal of directly resumed work. On the ARM64 reference host, the
existing million-cycle API benchmark remains at 79.06 ns per suspend/resume;
the new capacity-one producer/consumer benchmark moves one million values at
166.46 ns per value. Pinned x86-64 records 81.63 ns per suspend/resume and
151.54 ns per channel value. The ordinary ARM64 release binary remains
byte-identical to the milestone-three checkpoint at SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`;
after removing only GNU build-id and symbol metadata, x86-64 is likewise
byte-identical at
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.

Milestone 4b must connect OS readiness for non-blocking I/O without polling
the ordinary executor or allowing a blocking syscall to stall runnable
logical tasks. Multi-threaded scheduling remains outside this branch.

### Performance gates

- Existing non-coroutine benchmark medians may regress by at most one percent,
  with zero additional heap allocations on ordinary calls.
- The internal hand-off must be O(1), perform no syscall and copy no frame or
  value array. A one-million-iteration two-context ping-pong benchmark is kept
  as a permanent regression test.
- The first release target is at most 150 ns per internal suspend/resume
  hand-off on the current Apple ARM64 reference machine in a native-CPU
  `max-perf` build, measured separately from PHP callback work.
- Coroutine creation may allocate one initial context, but steady-state pooled
  creation and every subsequent switch should allocate nothing.
- Suspension depth and number of dormant local variables must not change
  switch time materially; dedicated depth/slot-scaling benchmarks enforce it.

If these gates cannot be met without adding overhead to normal PHP, keep the
primitive opt-in and continue compatibility work rather than weakening the
pay-for-use contract.
