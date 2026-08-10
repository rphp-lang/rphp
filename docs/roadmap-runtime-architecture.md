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

Eleventh checkpoint (2026-08-09): the range-proof test boundary is split
without touching production code. `jit/straight_range.rs` falls from 1,724 to
877 lines and retains only the interval analysis plus one test-only module
declaration; its 831-line test implementation now lives in
`jit/straight_range_tests.rs`. The module path and private test access remain
unchanged. All thirteen ARM64 and nine x86-64 focused tests pass, as do the
248/273-test host library suites, complete x86 integration matrix and
all-feature/all-target compilation on both hosts. The ordinary ARM64 release
remains byte-identical at SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`;
the metadata-normalized x86-64 release remains byte-identical at
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.

An attempted production split of the 863-line standard-library registration
block was deliberately rejected. It preserved executable sizes and every
text-symbol address, and the ARM64 1,003-pair regex gate ranged from -0.78 to
+0.38 percent, but two pinned x86-64 callback workloads regressed by 6.95 and
7.78 percent after diagnostic-string layout changed. None of that candidate
remains. Future production splits must therefore keep the two-host performance
gate even when source, symbol sizes and function addresses appear equivalent.

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

### Milestone 4b non-blocking I/O readiness checkpoint (2026-08-09)

Milestone four now also has a scope-owned Unix stream-pair foundation. The
feature-only API adds `coroutine_stream_pair()`, explicit
`coroutine_wait_readable(stream)` and `coroutine_wait_writable(stream)`
suspension points, plus non-blocking `coroutine_stream_read(stream, length)`
and `coroutine_stream_write(stream, data)`. Read and write return `false` for
`WouldBlock`; writes may report a partial byte count, and reads are capped at
8 MiB per call. The descriptors never escape as raw file descriptors and are
closed with their lexical scope. This is intentionally the scheduler
substrate, not yet a generic PHP stream or TCP compatibility layer.

`scheduler/io.rs` owns descriptors, FIFO direction waiters and reusable poll
buffers. A small internal Darwin/Linux `poll(2)` binding keeps this slice on
the standard library and adds no external dependency. The scheduler samples
OS readiness only inside a coroutine scope. It blocks in `poll` only after
runnable work is exhausted and passes the nearest timer as its timeout, so an
I/O wait cannot stall either an already-ready task or a timer. Deterministic
stream ordering and one in-flight waiter per readiness edge prevent a
level-triggered descriptor from overtaking earlier work or waking multiple
readers for one available byte.

The scheduler was split further while adding this policy: `driver.rs` owns the
combined ready/timer/I/O progress loop and `lifecycle.rs` owns scope teardown,
leaving the central scheduler focused on task transitions. Four new PHP
scenarios prove reader/writer progress, runnable-before-I/O fairness, combined
timer/I/O progress and cancellation of an unjoined I/O waiter. Two lower-level
tests cover byte preservation and the single-in-flight readiness rule. The
complete all-feature/all-target matrices pass on ARM64 and x86-64; the focused
coroutine matrix contains seven unit tests and sixteen PHP scenarios, with
three release benchmarks kept ignored during normal testing.

Across five warmed release runs, ARM64 medians are 53.72 ns per PHP
suspend/resume cycle, 149.72 ns per bounded-channel value and 2,522.24 ns per
stream-readiness round trip. Pinned x86-64 medians are 78.36 ns, 149.46 ns and
4,357.96 ns respectively. The default ARM64 executable remains byte-identical
at SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`.
The x86-64 text/data/BSS sizes remain 2,931,803/49,784/2,504 bytes and its
metadata-normalized executable remains byte-identical at
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.

This completes the bounded single-threaded milestone-four substrate. Adapters
for broader PHP streams and TCP belong to the compatibility phase; optional
multi-threaded scheduling remains a separate decision and must not impose
thread-safety costs on ordinary PHP execution.

### Phase 5 TCP listener adapter checkpoint (2026-08-09)

The first compatibility adapter promotes the milestone-four readiness core
from synthetic Unix pairs to real TCP server sockets. The feature-only API now
adds `coroutine_tcp_listen(address)`, which accepts a numeric socket address
and returns `[listener, bound_address]`, and
`coroutine_tcp_accept(listener)`, which returns `[stream, peer_address]` or
`false` for `WouldBlock`. Passing port zero is supported, so callers can use
the resolved address returned by the operating system. Listener readability
uses the existing suspension API, while accepted streams use the unchanged
non-blocking read, write and direction-readiness operations.

Descriptors remain owned by the lexical coroutine scope. `scheduler/io.rs`
now distinguishes byte streams from listeners and byte streams from Unix and
TCP transports without trait objects or dynamic allocation in the readiness
loop. Both listener and accepted sockets are non-blocking; descriptor IDs and
poll registration remain deterministic, readiness waiters stay FIFO, and the
one-in-flight guard still admits one logical waiter for one level-triggered
edge. Invalid combinations such as writable waits or byte I/O on a listener,
and accept on a byte stream, fail explicitly. Address parsing uses
`std::net::SocketAddr`, deliberately excluding blocking DNS resolution. No
external library or Cargo dependency is added.

The source boundary was tightened during the adapter work. Unix/TCP handlers
now occupy the 178-line `runtime/coroutine/api/io.rs`, leaving `api.rs` at 350
lines. Descriptor policy remains in the 479-line `scheduler/io.rs`, while its
four lower-level tests moved to a 127-line sibling file; the main scheduler is
474 lines. Real loopback coverage proves listen/wait/accept/read/write,
runnable-before-network fairness, pre-connection `WouldBlock`, scope
cancellation and descriptor-kind rejection. The focused PHP suite now passes
20 scenarios with three ignored release benchmarks, and the complete host
matrices pass 250 ARM64 and 275 x86-64 library tests plus all integration
targets.

Nine order-alternated release pairs against the milestone-four checkpoint show
no actionable cost in the existing coroutine paths. ARM64 candidate medians
are 52.53 ns per suspend/resume, 148.36 ns per channel value and 2,488.13 ns
per Unix-readiness round trip; paired median changes are -0.50%, +0.42% and
-0.05%. Pinned x86-64 candidate medians are 79.17 ns, 148.36 ns and 4,404.80
ns; paired changes are +0.97%, -2.11% and +0.04%. The default ARM64 executable
remains byte-identical at SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`.
The x86-64 text/data/BSS sizes remain 2,931,803/49,784/2,504 bytes and its
metadata-normalized executable remains byte-identical at
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.

This checkpoint is the inbound server foundation, not a generic PHP stream
claim. Non-blocking outbound connect, explicit descriptor lifecycle APIs and
broader PHP stream adaptation remain later Phase 5 slices.

An attempted explicit `coroutine_stream_close(descriptor)` slice was rejected
before admission. Its idle-only contract was correct: closing one stream end
delivered EOF to its peer, listener close allowed immediate rebinding, and a
descriptor with queued or in-flight readiness waiters was rejected rather than
falsely waking them. The implementation used only the standard library and
passed the complete correctness matrices, but adding the handler repeatedly
perturbed feature-binary layout. Two order-balanced 20-pair ARM64 checks of
cold-module variants regressed the existing suspend/resume control by 3.99%
and 6.23%, while channel and readiness controls remained flat or improved.
The default executable hashes stayed exact and x86-64 did not reproduce the
suspend loss, but the ARM64 result is sufficient to reject the slice. No close
API or lifecycle source file remains. A future lifecycle design must first
stabilize feature API/code placement or combine close with a broader adapter
whose measured benefit justifies that one-time layout movement.

### Static coroutine API registration checkpoint (2026-08-09)

The first placement-stability prerequisite is accepted. Core and Unix-specific
API descriptors now live in immutable `CORE_API_DEFINITIONS` and
`PLATFORM_API_DEFINITIONS` slices. Registration iterates those slices directly
into the owned internal-function vector instead of allocating, populating and
extending a temporary definitions vector first. The runtime therefore performs
one registration allocation instead of two, and a future adapter entry changes
the static descriptor data rather than expanding the construction sequence.
The complete API unit remains 357 lines and no dependency is introduced.

Performance evaluation now uses an even, order-balanced protocol so neither
binary receives an extra first or second position. Across twenty ARM64 pairs,
candidate medians are 50.76 ns per suspend/resume, 146.52 ns per channel value
and 2,465.87 ns per readiness round trip; the mean of the two order-specific
paired medians is -0.73%, -0.68% and -0.16%. Pinned x86-64 records 79.86 ns,
146.87 ns and 4,436.61 ns, with balanced changes of +0.55%, -1.20% and +0.24%.
All three existing paths therefore remain within the one-percent regression
ceiling on both architectures. Complete all-feature/all-target and no-default
matrices pass with the unchanged 250/275 host library counts and 20 passing
plus three ignored coroutine scenarios.

Default execution remains byte-identical: ARM64 retains SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`,
while x86-64 retains text/data/BSS sizes 2,931,803/49,784/2,504 bytes and the
metadata-normalized SHA-256
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.
This checkpoint stabilizes registration allocation and construction shape; a
new API still requires its own two-host layout gate.

That distinction is now measured directly. Repeating the idle-only close
candidate on top of this static-registration checkpoint still moved the
order-balanced ARM64 suspend/resume control by +3.96%, while channel was
-0.59% and readiness +0.25%. The registration construction was no longer the
variable, so the remaining sensitivity comes from linking the new handler and
descriptor path itself. The retry was rejected at the ARM64 gate without an
unnecessary x86 run, and again leaves no source or API behind.

### Internal outbound TCP connect checkpoint (2026-08-09)

The non-blocking outbound socket foundation is accepted below the PHP API
boundary. A private `scheduler/io_connect.rs` module creates numeric IPv4 or
IPv6 sockets directly through the supported Darwin/Linux ABI, immediately
wraps each raw descriptor in a standard-library `TcpStream` for RAII, and
starts `connect` only after enabling non-blocking mode. Linux creates the
descriptor atomically with `SOCK_CLOEXEC`; Darwin applies `FD_CLOEXEC` and
`SO_NOSIGPIPE`, matching the safety contract of Rust's own TCP constructor.
Interrupted connects retry, `EISCONN` completes, and `EINPROGRESS` or
`EALREADY` enters writable-readiness progress. No crate dependency was added.

The native IPv4/IPv6 layouts remain local and have compile-time 16/28-byte
size checks. Their platform family constants and Darwin length bytes are
selected with `cfg`; ports use network byte order, while IPv6 flow and scope
fields follow the standard-library ABI conversion. Completion inspects
`SO_ERROR` through `take_error` and then `peer_addr`. The socket is represented
by the existing TCP descriptor from creation onward, so the kernel remains the
only pending-state authority. A successful finish is idempotent, refusal is a
fatal connect result, and the descriptor is not exposed to PHP while pending.

This state-free design is the important performance result. An initial third
`ByteStream` variant and a later descriptor-state boolean were both correct,
but their order-balanced gates moved at least one existing control above the
one-percent ceiling: observed holdouts included +1.84%/+2.09% ARM64
suspend/resume and +1.42% x86-64 readiness. Neither form remains. In the final
shape, the accepted baseline and candidate have identical release `.text`
bytes on both hosts: ARM64 SHA-256
`f8fb9858de9b6f011d8e13483774a8de7a65cfd6cccde7516f5dd998da9261d7`
and x86-64 SHA-256
`f96f0ea3f953d2e0292984d16ecf77b967d8bcc57e6cceb2c8a65cbe69b4e5f1`.
Section sizes are also exact; only feature-build metadata differs.

Successful loopback progress and refused completion pass on Apple ARM64 and
Linux x86-64. Complete all-feature/all-target matrices pass 252 and 277 host
library tests respectively, including six descriptor/connect tests, and all
20 coroutine scenarios with three ignored release benchmarks. Complete
no-default matrices remain green. The default release stays byte-identical on
ARM64 and metadata-normalized x86-64 at the established hashes. This is
an internal substrate checkpoint, not a user-visible connect claim: admitting
the coroutine handler, its cancellation state and a numeric-address PHP
surface is the next independently gated slice; blocking DNS remains excluded.

### Numeric coroutine TCP connect API checkpoint (2026-08-09)

The internal substrate is now admitted through feature-only
`coroutine_tcp_connect(address)`. The function accepts an explicit numeric
IPv4/IPv6 address and port, starts the private non-blocking socket, and returns
its positive descriptor only after the connection has completed. It remains a
child-coroutine operation; DNS names are rejected before scheduler admission
so no blocking resolver work enters the executor.

An in-progress call records one continuation in a separate I/O map and blocks
the task with `WaitReason::TcpConnect(descriptor)`. The continuation owns the
task id plus the suspended caller and return-slot pointers needed by the VM's
canonical heap-slot writer. Writable readiness checks `SO_ERROR` before
publishing the descriptor and waking the task. A spurious writable edge is
acknowledged and rearmed without resuming PHP. Refusal remains a fatal connect
result and removes the private descriptor; scope cancellation drops the queued
waiter and socket before its suspended frame is cleaned, so no stored pointer
can outlive the frame it targets.

This adds no crate or Cargo feature. The ABI implementation remains 364 lines,
while its three descriptor tests moved to a dedicated 102-line
`io_connect_tests.rs` module. That split preserves the complete ARM64 and
x86-64 release test executables byte for byte. PHP loopback tests prove that a
connect can suspend while another logical task runs, then exchange data through
the returned numeric stream; refusal and DNS rejection are covered separately.

Two order-balanced 20-pair ARM64 gates put the existing
suspend/channel/readiness controls at +0.37%/-0.70%/+0.82% and
-0.15%/-3.13%/-1.44%. Pinned x86-64 records -2.85%/-0.19%/+0.14%. Every
regression result remains below the one-percent ceiling. Complete
all-feature/all-target matrices pass 253 ARM64 and 278 x86-64 library tests,
including seven descriptor tests and 22 coroutine E2E scenarios with three
ignored benchmarks. Both no-default matrices pass 161 library tests. Default
execution remains exact at ARM64 SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`
and metadata-normalized x86-64 SHA-256
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`;
the x86-64 text/data/BSS sizes remain 2,931,803/49,784/2,504 bytes.

### Optional TCP connect timeout checkpoint (2026-08-09)

`coroutine_tcp_connect` now admits an optional non-negative integer timeout in
milliseconds. The one-argument form remains unbounded. The scheduler checks
deadline arithmetic before opening the socket, starts the same non-blocking
numeric connection, and schedules a timer only when that connection actually
enters the in-progress state.

The deadline deliberately uses the existing `Readiness` timer heap instead of
adding a second driver-wide deadline structure. On successful writable
completion, the connect continuation cancels its timer before the VM return
slot receives the descriptor. A due timer is routed by the task's current wait
reason: ordinary `Timer` waiters wake normally, while `TcpConnect(descriptor)`
removes the private socket/continuation and reports a fatal timeout. The timer
therefore cannot outlive a successful suspended frame or keep an idle scope
sleeping after connect completion. Scope cleanup retains its idempotent
descriptor cancellation.

This shape followed two measured rejections. An unconditional secondary-map
scan regressed ARM64 channel/readiness by +2.13%/+1.38%; gating the scan still
left readiness at +1.10%. The accepted timer-heap representation records
-2.47%/+0.35%/+0.20% on ARM64 and -11.21%/-3.08%/-0.79% on pinned x86-64 for
the established suspend/channel/readiness controls. No regression exceeds the
one-percent ceiling. The figures are admission controls rather than speedup
claims.

Tests cover invalid PHP timeout values, successful timed loopback progress,
deadline cancellation, expiry routing and private-descriptor cleanup. Driver
tests were moved to `driver_tests.rs`, keeping production `driver.rs` at 118
lines. Complete matrices pass 255/280 host library tests, 23/3 coroutine E2E
scenarios and 161 no-default library tests on both architectures. Default
release hashes and x86-64 section sizes remain exact at the established
baselines. No dependency or Cargo feature changed. Explicit close, DNS and a
generic PHP stream adapter remain separate Phase 5 work.

### Isolated close-section retry rejected (2026-08-09)

The post-timeout baseline received one final standalone close experiment. All
new lifecycle code was kept non-inline and cold in a dedicated ARM64 Mach-O
`__TEXT,__rphp_life` section, including the PHP handler, scheduler bridge and
idle-only `IoSet` removal. The contract remained sound: stream close delivered
EOF, listener close allowed immediate rebinding, and descriptors with queued
or in-flight readiness work were rejected.

Despite the separate `0x308`-byte section, the ordinary feature-test `__text`
layout still changed and the order-balanced ARM64 suspend control regressed
5.84%. Channel/readiness were -0.48%/-0.23%; pinned x86-64 was
+0.97%/-0.37%/+0.10%. The ARM64 result exceeds the one-percent ceiling, so the
entire candidate was removed. Local and server feature executables were then
rebuilt bit-identically at the accepted timeout hashes. This rules out another
standalone placement retry; explicit close should be reconsidered only within
a broader stream-adapter checkpoint.

### Coroutine stream-policy module checkpoint (2026-08-09)

The accepted post-timeout cleanup gives stream and listener operations an
explicit internal ownership boundary. Pair creation, TCP listener creation,
accept, readiness admission/queueing and byte reads/writes now live in the
155-line `scheduler/io_stream.rs` submodule. The shared descriptor registry,
poll driver, connect integration and lifecycle bookkeeping remain in
`scheduler/io.rs`, which falls from 489 to 366 lines. The methods are visible
only to the enclosing coroutine scheduler; no public API, Cargo feature or
dependency changes.

This real-module split changes feature-test executable layout on both hosts, so
it was admitted as a production refactor rather than assumed to be cosmetic.
An order-balanced 20-pair ARM64 gate records +0.45% suspend/resume, +0.00%
channel and -0.61% readiness. Pinned x86-64 records +0.21%/-0.15%/-0.78%.
All remain below the one-percent regression ceiling. The clean feature-test
executables have SHA-256
`ba47663ee205fbcae00518bc4d8b38fe10ae37796d03b5a2850b2f7da6cc5569`
on ARM64 and
`f77d83f30063f5d582302279c3dd9f23033d1cc31c184e7e227dd46eaaf55a3d`
on x86-64.

Both hosts pass the seven descriptor/connect tests, 23/3 coroutine E2E
scenarios, complete all-feature/all-target matrices and complete no-default
matrices. The established 16 MiB test-thread stack is used only for the
pre-existing recursive Ackermann debug test; no product stack setting changes.
Default releases remain exact at the established ARM64 and metadata-normalized
x86-64 hashes, with unchanged x86-64 text/data/BSS sizes. This checkpoint is
source-ownership cleanup only and makes no runtime speedup claim.

The gate also closes a benchmark-process hole: `cargo build --release` does
not refresh an ignored release test executable. Benchmark admission now
requires the release-test no-run build (`cargo test --release --features
coroutines --test e2e_coroutines --no-run`) in a fresh target directory before
hashing or timing. Rebuilding this checkpoint that way exposed and replaced
one stale intermediate artifact; the figures and hashes above are from the
corrected clean two-host comparison.

The repository now enforces that protocol through
`benches/run_coroutine_gate.sh CANDIDATE_ROOT BASELINE_ROOT [CPU]`. The runner
creates two fresh temporary Cargo target directories, builds both release test
executables with `--no-run`, alternates candidate/baseline order for an even
number of pairs, reports the mean of the two order-specific median ratios and
returns failure above the configurable one-percent default ceiling. It uses
only the existing toolchain and standard shell utilities.

### Cohesive stream lifecycle adapter rejected (2026-08-09)

A broader standard-library-only adapter then tested four operations together:
TCP local/peer address inspection, Unix/TCP half-close, and explicit idle
descriptor close. Close rejected queued, in-flight or connecting descriptors;
shutdown delivered EOF, listener close permitted immediate rebinding, and
address pairs matched across a real loopback connection. Nine descriptor tests
and 25 coroutine E2E scenarios passed on both hosts without a new dependency.

The candidate was rebuilt from scratch in separate target directories before
measurement. Against the clean stream-policy baseline, an order-balanced
20-pair ARM64 gate recorded +2.67% suspend/resume, +1.44% channel and +0.05%
readiness. Pinned x86-64 recorded +10.99%/+3.99%/+0.04%. The first two paths
exceed the one-percent ceiling on both hosts, so the complete API, scheduler,
policy and test candidate was removed. No lifecycle or metadata handler
remains. A future generic stream surface must first isolate API-handler code
placement from the existing coroutine paths; adding more correct operations to
the same static table is not sufficient.

### Coroutine context prototype test split (2026-08-09)

The standalone milestone-one/two context prototype now keeps its model and
driver in the 451-line integration root, while its 588-line test module lives
in `tests/e2e_coroutine_context/tests.rs`. The original `tests::*` paths and
all eight test names remain unchanged. This is a test-only ownership boundary;
the production crate, Cargo configuration and dependency graph are untouched.

Both no-default and all-feature modes pass six tests with two ignored release
benchmarks on ARM64 and x86-64. Isolated ARM64 runs record 12.36 ns per
hand-off and 11.30/11.98 ns for shallow/deep-wide scaling; x86-64
records 12.77 ns and 12.89/12.73 ns. The default and coroutine feature release
hashes remain exactly at their accepted baselines on both hosts.

### Coroutine integration-test ownership checkpoint (2026-08-09)

The coroutine integration target now retains only its shared process and
loopback harness in the 78-line root. Core structured-concurrency/channel
scenarios live in the 281-line `e2e_coroutines/core.rs`, non-blocking stream
and TCP scenarios in the 293-line `e2e_coroutines/io.rs`, and the three
release gates in the 135-line `e2e_coroutines/benchmarks.rs`. Direct
`include!` boundaries preserve all 26 root test names and the existing exact
benchmark filters; no product source, Cargo feature, crate or external library
changes.

Both ARM64 and x86-64 pass 23 scenarios with the same three ignored
benchmarks. The fresh-source, order-balanced 20-pair gate compares this split
against the archived one-file target. ARM64 records -0.056% suspend/resume,
+0.242% bounded channel and -0.253% readiness; pinned x86-64 records
-0.382%/-0.162%/+0.616%. Every result remains below the one-percent regression
ceiling. The split is accepted as test ownership cleanup only and makes no
runtime speedup claim.

### Codegen-stable coroutine core-API ownership checkpoint (2026-08-09)

The eight structured-concurrency, channel and timer handlers now live in the
141-line `api/core.rs`, while scope-root invocation, suspend mechanics, shared
argument/result helpers and registration remain in the 227-line `api.rs`.
The root uses a private `include!` boundary deliberately: handlers retain their
existing `api::*` codegen identity and the registration table retains its exact
order. No PHP API, runtime state, Cargo feature, crate or external library
changes.

A conventional Rust submodule was tested first and removed. Although it passed
255 ARM64 and 280 x86-64 library tests plus 23/3 E2E scenarios on both hosts,
its new symbol identities moved the feature-test layout and the ARM64 gate
measured +5.957% suspend/resume, -3.813% channel and +0.882% readiness. The
suspend result exceeds the one-percent ceiling.

The accepted include boundary passes the same functional matrix. Its
fresh-source, order-balanced 20-pair gate records +0.306%/-0.040%/-0.224% on
ARM64 and +0.188%/-0.515%/+0.355% on pinned x86-64 for the same three controls.
Clean feature-test SHA-256 values are
`2333dd6ee8bcc9ddb300be6e113b050e37b07d83401af43cdd399c8a15061b3d` and
`1922d968ed64ecb069aa1a23290895644a5daf4b3472f165367d34805a7a0e42`.
Default production releases remain exact at the established host hashes.

### Scheduler operation ownership split rejected (2026-08-09)

A follow-up codegen-stable extraction placed the contiguous channel, timer and
I/O adapter methods behind a macro expanded inside the existing
`CoroutineScheduler` implementation. It reduced `scheduler.rs` from 506 to
355 lines and kept every method inherent on the original type. The candidate
added no API, state, crate or external library and passed 255/280 library tests
plus 23/3 coroutine E2E scenarios on both hosts.

The fresh-source 20-pair gate accepted ARM64 at
-0.562%/+0.422%/-1.671% for suspend/channel/readiness, but pinned x86-64
recorded -0.097%/+1.470%/+1.099%. Channel and readiness both exceed the
one-percent ceiling, so the macro, included file and all source changes were
removed. The scheduler remains a measured hot layout boundary; future cleanup
should target tests or genuinely cold resources rather than repackage these
adapter methods.

### Asynchronous resolver transport prototype (2026-08-09)

A standalone 310-line integration prototype now proves the standard-library
transport needed for non-blocking hostname resolution without touching the hot
scheduler. One named worker thread receives owned host/port jobs over `mpsc`,
runs `ToSocketAddrs` away from the caller, publishes owned address vectors and
signals a non-blocking `UnixStream` wake descriptor compatible with the
existing `poll(2)` driver. Monotonic job ids and a `BTreeSet` pending filter
make cancellation discard late results without hiding later completions.

Four tests on both hosts cover localhost resolution on a distinct thread,
64 numeric jobs with unique ids, cancellation ordering, invalid input and
explicit worker shutdown; one release benchmark remains ignored by default.
Ten thousand numeric jobs cost 513.19 ns/job on ARM64 and 1,022.91 ns/job on
x86-64, including submission, worker resolution and completion delivery. The
prototype uses no crate or Cargo change, and default release hashes remain
exact.

This is not yet a DNS-capable coroutine API. A running OS resolver call cannot
be forcibly cancelled through `ToSocketAddrs`; cancellation currently filters
its completion, and shutdown waits for the worker. Production integration must
therefore bound worker ownership and prove that an idle resolver adds no hot
scheduler tax before hostname input is admitted.

### Bounded resolver pool prototype (2026-08-09)

The resolver transport now models a production-safe admission boundary more
closely. Two fixed workers share a bounded 64-job `sync_channel`; scheduler-side
submission uses `try_send`, so saturation returns `WouldBlock` instead of
blocking the caller. Worker count and capacity are constructor parameters for
tests, while job ids, cancellation filtering and the single pollable wake
descriptor retain their original contract. The standalone target is 431 lines
and still uses only `std` plus the local `poll(2)` declaration.

Injected resolver tests prove that a fast job completes on worker two while
worker one is deliberately blocked, and that a one-worker/one-slot pool rejects
the next submission immediately while preserving both admitted completions.
Both hosts now pass six tests with one ignored benchmark. Ten thousand numeric
jobs measure 524.42 ns/job on ARM64 and 827.74 ns/job on x86-64; the bounded
pool therefore adds no material ARM cost and improves the observed x86
transport result. Default production releases remain bit-identical.

The remaining limitation is explicit: two simultaneous blocking OS resolver
calls can occupy both workers, and shutdown must still join them. Scheduler
integration must create the pool lazily, surface queue saturation without
losing the suspended frame, and retain completion filtering until scope
cancellation has released every continuation.

### Lazy production hostname resolution checkpoint (2026-08-09)

The bounded prototype is now integrated into the feature-only coroutine TCP
path without adding a crate, Cargo feature or external resolver/event library.
`coroutine_tcp_connect` accepts either a numeric socket address or a
`hostname:port` target. Numeric targets retain their direct non-blocking path;
hostname targets submit owned work to a process-wide pool of two named
standard-library workers behind a 64-entry `sync_channel`. Submission remains
`try_send`, so saturation fails immediately instead of blocking the scheduler.

Every scheduler that first uses DNS creates only its private completion queue
and non-blocking `UnixStream` wake pair. The read side is registered as a
private descriptor in the existing `IoSet`, so resolver completion uses the
same `poll(2)` set and readiness queue as streams rather than adding a second
poll or an unconditional scheduler scan. Pool threads are created lazily and
live for the process lifetime. Scope cancellation removes the continuation and
disarms its wake waiter without joining a worker; an already-running
`ToSocketAddrs` call is not forcibly cancellable, but its late owned result is
discarded when no waiter remains.

One absolute deadline is computed before DNS submission and is retained while
resolved addresses are attempted in order. An asynchronous refusal therefore
continues to the next IPv6/IPv4 candidate under the original timeout. Success
writes the descriptor into the suspended VM return slot, cancels the timer and
wakes the exact task; exhaustion, resolver failure, timeout and scope cleanup
release every private continuation and socket.

The first correct integration polled a separate optional resolver fd from
every scheduler pass. ARM64 rejected it at +9.452% suspend/resume, +1.361%
channel and +1.110% readiness. Registering the lazy wake source inside the
existing descriptor set removes that inactive-path tax. The final fresh-target
20-pair gate records -0.333%/-1.938%/-0.401% on ARM64 and
-2.797%/-3.112%/-0.644% on pinned x86-64 for the same controls. These are
admission results, not speedup claims.

Both hosts pass 168 no-default library tests, 255 ARM64 or 280 x86-64
all-feature library tests, 24 coroutine scenarios with three ignored release
benchmarks, and the complete all-feature/all-target compile matrix. Clean
feature-test SHA-256 values are
`52f85dd6c78789a3f5718ca095767de842bde3811ece7e1bd7776bc3478bbbb2`
on ARM64 and
`863e991e667d667133a756c697fd7085b4c7d95dd1b6473def19135084b94a0d`
on x86-64. The default ARM64 release remains exact at
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`;
the fresh x86-64 candidate and exact baseline are byte-identical at
`1b42d96b831bc0d717e28f46bbb49896f56891aac4385917a7dea07941f7070d`.
Generic PHP stream-resource integration remains a separate Phase 5 slice.

### Resolver ownership split rejected (2026-08-09)

A post-checkpoint cleanup separated the 376-line resolver into a 181-line
scheduler continuation and 203-line worker transport module, and moved 73
lines of resolver wake policy out of `io.rs`, reducing that root from 435 to
380 lines. The API, algorithm, queue dimensions and dependency graph were
unchanged. Both hosts passed 168 no-default tests, their complete 255/280
all-feature library sets, 24/3 coroutine E2E scenarios and every all-target
compile.

Code layout still failed the admission rule. A preliminary ten-pair ARM64 gate
was inside the ceiling at -2.500%/+0.162%/+0.969% for
suspend/channel/readiness, but the required 20-pair run measured
-5.228%/-0.866%/+1.244%. Pinned x86-64 passed at
+0.845%/-0.118%/-0.657%; the ARM64 readiness result alone rejects the split.
All module and lifecycle changes were removed locally and on the server, which
are restored exactly to `4cf4abb`. The accepted combined resolver and wake
ownership remains in place until a stronger code-placement boundary can make
this cleanup neutral on both architectures.

### Non-blocking UDP coroutine adapter checkpoint (2026-08-09)

Phase 5 now adds a packet-preserving UDP adapter without a socket crate or
external event loop. `coroutine_udp_bind(numericAddress)` creates a
scope-owned non-blocking `std::net::UdpSocket` and returns its descriptor plus
resolved local address. `coroutine_udp_send_to(socket, data, numericAddress)`
returns the sent byte count or `false` for `WouldBlock`;
`coroutine_udp_recv_from(socket, length)` returns `[data, peerAddress]` or
`false`. The existing readable/writable wait functions provide suspension and
retry, so the adapter adds no parallel readiness mechanism.

Datagrams are a distinct descriptor kind rather than pretending to be byte
streams. Packet boundaries and sender addresses survive receive, while
stream-only read/write and TCP accept operations reject a UDP descriptor.
Receive allocation is capped at 65,535 bytes. Bind and peer addresses are
numeric-only so no resolver call can occur inside send or bind. The policy is
isolated in an 85-line `io_datagram.rs`, with a 37-line scheduler bridge and a
111-line PHP adapter; 95 lines of lower-level tests stay outside production.

Real loopback coverage proves bidirectional ping/pong, peer identity,
`WouldBlock`, shared readable and writable readiness, descriptor-kind errors,
runnable-before-datagram fairness, numeric-address validation and the receive
cap. Both hosts pass 168 no-default library tests, 257 ARM64 or 282 x86-64
all-feature library tests, 26 coroutine scenarios with three ignored release
benchmarks and complete all-feature/all-target compilation.

The fresh 20-pair gate against the accepted DNS source records
-2.861%/-0.288%/-0.486% on ARM64 and +0.079%/-0.529%/+0.001% on pinned
x86-64 for suspend/channel/stream readiness. Every control remains under the
+1% ceiling; negative values are not treated as UDP speedup claims. Clean
feature-test SHA-256 values are
`6d9b3278845b051844d2613ba4b7b17b96c41d4fe85752b79f478dec633e60b0`
on ARM64 and
`7f038c8bfbe1be43af5380c2071880a45acbec68185b0dbab96c9046567108cb`
on x86-64. Default releases remain exact at
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`
and `1b42d96b831bc0d717e28f46bbb49896f56891aac4385917a7dea07941f7070d`.
No dependency or Cargo configuration changed.

### Generic PHP stream resources and dense coroutine registries (2026-08-09)

The first generic stream slice is accepted using only Rust's standard library.
A PHP resource remains a 16-byte scalar `Value` containing one request-local
integer id; the lazily created request registry owns the actual backend. This
keeps resource assignment and destruction out of the ordinary heap-value
reference-count path. Explicit `fclose()` drops the backend immediately and
request shutdown drops every entry still open. No `ExecutorGlobals` field,
crate, Cargo feature or external stream/event library was added.

`PhpStream` owns either `std::fs::File` or `Cursor<Vec<u8>>` and admits bare
paths, `file://`, `php://memory`, `r/w/a/x/c` modes plus `+`, read, write,
flush, seek, position and EOF. The PHP surface is `fopen`, `fread`, `fwrite`,
`fclose`, `fflush`, `feof`, `ftell`, `fseek`, `rewind`, `is_resource`,
`get_resource_type`, `get_resource_id` and `SEEK_SET/CUR/END`. Seven E2E
scenarios cover memory and real files, every admitted creation policy, alias
invalidation, closed-resource identity, seek/EOF behavior, invalid wrappers
and the large-frame fallback. The intentionally recorded remaining lifetime
difference is that dropping the last resource zval without `fclose()` retains
its backend until request shutdown.

The admission work also removes two measured scheduler costs instead of
hiding feature layout movement. Monotonic coroutine and channel identifiers
now index private dense `Vec` registries rather than `HashMap`s. Pinned context
allocations retain stable addresses, explicit zero-cost reserve words preserve
the established later scheduler-field offsets, channel blocking reuses the
already validated active task, and `resume` no longer repeats an executor
identity check already performed by every entry path. A packed PHP array append
also returns through one direct `Vec::push` fast path rather than dispatching
the storage enum twice.

Fresh-source, order-balanced 20-pair coroutine gates against `1f28a2b` record
-18.517%/-55.960%/-3.457% on ARM64 and
-19.220%/-39.062%/-3.990% on pinned x86-64 for suspend/resume, bounded channel
and stream readiness. These are direct dense-registry wins, not neutral layout
claims. The new dependency-free default-runtime gate builds candidate and
baseline in fresh targets and runs scalar, packed-array, string, order and
ledger workloads under the same +1% ceiling. Pinned x86-64 records
-0.197%/-0.987%/-0.144%/+0.742%/+1.115%; the sole failure is not reproduced
by an independent 20-pair ledger rerun at +0.508%. A thermally drifting ARM64 full batch
put only String above the ceiling at +1.668%; its independent 20-pair rerun is
+0.202%, while scalar/array/order/ledger in the full batch are
-0.174%/-2.370%/-0.265%/-1.566%. The focused array rerun is -2.829%.

Both hosts pass 167 no-default library tests, 265 ARM64 or 290 x86-64
all-feature library tests, all seven stream scenarios and complete
all-feature/all-target compilation. Clean default release SHA-256 values are
`de0fec7335fa71d1cdc5720637dbe970de4be0cfde604f455971465aa638af31`
and `a0228fe3e34a6a367c96a71a9f4a2322db2d963d7f83d3f4a0b487a3bf6c23e0`;
clean coroutine integration hashes are
`813117642d8b990b574eae5e5de5f2db87fbd722deeb50912c27dd98aa606909`
and `bccc63469e4967ddf5a37cf6aa37f12399108446fac69d9a788b2e4d2c2dc796`.
The next stream slices may add `php://temp`, line-oriented reads and metadata,
but must retain this standard-library-first ownership boundary.

### `php://temp` spill and no-JIT admission checkpoint (2026-08-10)

The generic request-owned registry now supports bounded `php://temp` streams
using only `std`. A stream starts as `Cursor<Vec<u8>>`, defaults to a 2 MiB
memory limit and accepts `php://temp/maxmemory:N`. The first write that would
cross the limit copies the complete logical contents into a unique temporary
file opened with owner-only permissions, restores the cursor and then applies
the write. The file is unlinked by `fclose()`, request shutdown or backend
drop; a stream that stays below its limit never touches disk. Seek, tell, EOF,
read, write and flush retain one API across both representations.

The performance admission work remains dependency-free. Exact String append
and packed-array push loop kernels bypass general typed dispatch. A checked
32-iteration induction-plus-constant fold shares the existing interrupt
cadence and rejects any range whose overflow behavior cannot be proven.
Virtual object-array regions retain invariant nested dispatch contracts at
entry while rechecking varying values, and tiny scalar-plan ranges execute as
one validated unrolled block. Every uncertain guard still exits before the
corresponding canonical PHP operation is skipped or replayed.

Against `178ef41`, the final fresh-build runtime gate records
-95.649%/-18.367%/-57.683%/-1.716%/-1.326% on ARM64 and
-96.604%/-12.450%/-59.513%/-10.019%/-2.207% on pinned x86-64 for scalar,
array, String, order and ledger. Coroutine controls remain within +1% at
+0.367%/-0.841%/-0.741% and -1.869%/-0.086%/+0.343% respectively. Both hosts
pass 174 no-default library tests, 272/297 all-feature library tests, eight
stream scenarios, four corpus scenarios and all-target compilation. No Cargo
file or dependency changed. Clean ARM64/x86-64 default release SHA-256 values
are `859e22b8a1fc8d52a3b495b037d68ee347ffe1c560dd5a391f722d4f08c03e2f`
and `83043325ba2545a2c6517bdaceb4d7cbc368c26a80b649dcb4e773dafb85dab4`;
the matching coroutine integration values are
`40dc251671dca21c978d25cf17978080be51d3cc8f74d6ff2c5d5a6b80a21fac`
and `8eb5a0592f9c5b1fc888d925fb26cf13717b8a3825929bac8818cd3ffe11c2c1`.

The next stream boundary is line-oriented input and metadata. Final-alias
backend release before request shutdown remains a separate resource-lifetime
decision rather than being conflated with stream I/O compatibility.

### Line-oriented streams and scheduler-owned suspension checkpoint (2026-08-10)

`fgets()` and `stream_get_meta_data()` complete the next bounded stream slice
using only `std`. Line reads retain newlines, implement PHP's `length - 1`
ceiling, span an arbitrary number of fixed 8 KiB scratch reads and seek back
over-read data. The final unterminated line sets EOF after its probe. Plain
files preserve their input mode and URI; `php://memory` exposes normalized
binary modes and `MEMORY`; `php://temp` exposes `TEMP` before and after spill.
Backend-specific metadata key sets match PHP 8.5 rather than filling absent
keys with synthetic defaults.

The hot coroutine control is now less sensitive to unrelated runtime code
placement. The suspension request no longer uses a second thread-local. Its
one-byte state replaces one word in the existing 48-byte dense context-registry
reserve, so later scheduler fields and the opt-in feature boundary retain
their size. `request_suspend()` rejects a nested sidecar and
`take_suspend_request()` consumes it exactly once. Execution-state exchange
also avoids moving exception, named-variadic, generator and pending-invoke
storage when both sides are empty; any non-empty side still takes the original
full swap. Unit tests cover the slow exchange and both suspension kinds.

Fresh order-balanced 20-pair gates against `bc54dfe` record
-7.926%/-3.242%/+0.088% on ARM64 and -7.885%/-1.509%/-2.045% on pinned
x86-64 for suspend/resume, bounded channel and stream readiness. The default
runtime gate records -0.966%/-24.187%/-47.041%/-16.267%/-4.400% on ARM64
and +0.560%/-45.612%/-17.061%/-10.892%/-3.445% on x86-64 for scalar,
packed-array, String, object-order and ledger workloads.

Both architectures pass 178 no-default tests, 280/305 all-feature library
tests, 26/3 coroutine scenarios, 11 stream scenarios, 118 quick-loop
scenarios, four corpus scenarios and complete all-feature/all-target builds.
Default release SHA-256 values are
`3816f11807ad36b2a251130e193c52ed2f8e60f7a1a880760ab697f8432785b1` and
`31d18d9a1af594f9c37c4f8348aa5aeb695d6510085fff93eefa2b0fa409999c`;
coroutine integration values are
`49fd48cb26d833bd84b1a57136a592a893f4511429ab415f94bd43ee55bfd801` and
`192c6d57fa7a93a933181eed19f045bb693572f2bf5044376b4f92b97a2f0b26`.
No Cargo file, crate or external stream, buffering or scheduler library was
added. Final-alias resource destruction remains explicit future work.

### Opt-in final-alias resource lifetime checkpoint (2026-08-10)

The final-alias boundary is now implemented behind the independent
`resource-lifetime` feature. A resource-enabled `Value` still occupies exactly
16 bytes: its payload points to a standard-library `Rc<ResourceHandle>` that
stores request scope, stable resource id and an indirect close callback.
Assignment increments that handle, explicit `fclose()` removes the backend
immediately, and dropping the last still-open alias removes it automatically.
Dropping aliases after `fclose()` is a no-op, while request shutdown remains a
safety net for every registry entry.

Resource is included in `needs_cleanup()`, small-frame ownership bitmaps and
the large-frame fallback scan only when the feature is enabled. Raw-copy fast
paths therefore reject owned resource aliases exactly as they reject strings,
arrays and objects. Backend payloads are removed from the thread-local
registry before their destructors run, so a destructor can release another
resource without re-entering an active `RefCell` borrow. Tests cover final
alias release, explicit-close idempotence, nested resource destruction and the
unchanged 16-byte `Value` layout.

The first default-linked version was rejected: a fresh balanced ARM64 gate
kept scalar, array, String and order controls within the ceiling but moved the
ledger workload by +2.102 percent. The admitted feature boundary restores the
default release image to the exact `f5d4e68` `__TEXT` size (2,818,048 bytes)
and the exact monitored hot-symbol addresses. The CPU-pinned x86-64 gate
records +0.106%/+0.378%/-0.274%/-0.549%/-0.191% for scalar, packed array,
String, order and ledger, all below the +1% ceiling. Both hosts pass 195
default and 186 no-default library tests; all-feature coverage is 296 on ARM64
and 321 on x86-64. The 14 stream scenarios pass with and without
`resource-lifetime`, and complete all-feature/all-target compilation succeeds.
The implementation adds no crate, `Cargo.lock` is byte-identical, and no
external resource or stream library is used.

Making final-alias destruction a production default now requires the same
two-host runtime admission as other compatibility extensions. Until then,
users that need deterministic early backend release can enable
`resource-lifetime`, while ordinary builds retain the established scalar
resource codegen and request-shutdown contract.

### Opt-in `stream_get_contents()` checkpoint (2026-08-10)

The next compatibility slice adds bulk stream reads without changing the
resource registry or introducing a buffering crate. `PhpStream::read_contents`
accepts an optional byte limit and absolute offset, uses one fixed 8 KiB stack
chunk and grows its caller-owned result through `Vec::try_reserve`. It therefore
works uniformly for memory, spilled temporary and file backends without
preallocating an attacker-sized requested length. Existing backend `read` and
`seek` operations remain the sole owners of cursor and EOF state.

The PHP surface is isolated behind `stream-contents`. It implements PHP 8.5's
`null`/`-1` unlimited length, zero-length, bounded-length, negative current-
cursor and non-negative absolute-offset behavior. Wrong types, closed handles
and lengths below `-1` use the exact covered exception surface; unreadable
streams return `false`. A private `streams::checked_args` module now owns
stream-resource validation and weak integer conversion for both this handler
and the checked CSV handlers. The common `ValueError` class registrar is gated
by the internal `value-errors` feature implied by both public error surfaces.

Default ARM64 code generation remains the exact 2,818,048-byte `f5d4e68`
`__TEXT` image with identical monitored hot addresses and lockfile. X86-64
text/data/bss and those addresses also match exactly. Its full pinned runtime
gate passed four workloads at -0.020%/+0.049%/-0.241%/-0.267%; the only noisy
ledger result was +1.933% with large bidirectional outliers and its mandatory
isolated 20-pair rerun passed at +0.571%.

ARM64 passes 195 default, 186 no-default and 299 all-feature library tests;
x86-64 passes 195, 186 and 324. Stream coverage is 14 default, 17 bulk-read,
15 checked-reader, 20 checked-writer, 14 final-alias and 23 all-feature
scenarios. Both hosts complete the all-feature/all-target build. No dependency
was added, `Cargo.lock` is byte-identical and the implementation uses only the
standard library.

### Opt-in stream-to-stream copy checkpoint (2026-08-10)

`stream-copy` adds `stream_copy_to_stream()` without expanding `PhpStream` or
the resource registry. One 8 KiB array lives on the handler stack. Each loop
iteration releases the source registry borrow after reading, then borrows the
destination for as many writes as needed. This keeps the existing single-
payload `RefCell` access valid even when source and destination ids are equal,
avoids a heap copy buffer and preserves the source cursor when a later write
fails.

PHP 8.5 differential coverage defines the admitted semantics. Omitted, zero
and negative offsets use the current position; positive offsets seek from the
start. `null` and negative lengths are unbounded, zero performs only a requested
positive seek, and an exact limit does not set EOF. Both stream arguments are
validated before scalar options, closed resources produce parameter-specific
errors, and read, seek or write failures return `false`. The private checked-
argument helper was generalized by argument index/name and remains compiled
only for opt-in stream/CSV error surfaces.

Default ARM64 remains the exact 2,818,048-byte `f5d4e68` `__TEXT` image with
the same monitored addresses and lockfile. X86-64 text/data/bss and hot
addresses also match exactly; its pinned 20-pair gate passes at
-0.797%/-0.138%/-0.795%/+0.309%/-0.332% for scalar, packed array, String,
order and ledger. Test matrices remain 195/186/299 on ARM64 and 195/186/324 on
x86-64. The dedicated and combined stream matrices pass 17 and 26 scenarios,
and both hosts complete all-feature/all-target compilation. The slice is
standard-library-only and leaves `Cargo.lock` byte-identical.

### Opt-in file-content bridge checkpoint (2026-08-10)

The old default `file_get_contents($filename)` body remains in the composition
root for exact compatibility and codegen. With `file-contents`, registration
selects a new five-argument handler in `stdlib/file_contents.rs`. It opens an
ordinary `PhpStream`, applies absolute or end-relative seeking and calls the
same fixed-chunk bulk reader used by `stream_get_contents()`. The backend
method is gated by either feature, so enabling the file surface does not also
register the stream function.

The extended handler supports current/absolute paths and `file://`, nullable
or bounded length, positive and negative offsets, weak scalar conversions and
named parameters. It validates all typed arguments before the empty-path
`ValueError`, matching PHP's observable error order. Invalid context resources
and all five parameter types use the probed exact exceptions. Configurable
include-path search and valid stream-context resources remain future substrate;
the accepted feature does not pretend those facilities exist.

Results grow fallibly behind one 8 KiB stack chunk rather than from file
metadata, keeping memory proportional to bytes successfully published. Shared
integer/error helpers are visible across the stdlib subtree, but the resource-
specific helper bodies remain feature-gated. This adds the new policy outside
the 9k-line stdlib composition root without moving the established default
handler.

ARM64 default codegen remains the exact 2,818,048-byte `f5d4e68` `__TEXT`, hot
addresses and lockfile. X86-64 text/data/bss and addresses are exact, and its
pinned runtime gate passes at -0.489%/-0.414%/-0.400%/-0.084%/+0.431%.
Library matrices pass 195/186/198/299 on ARM64 and 195/186/198/324 on x86-64
for default/no-default/file/all features. Filesystem E2E passes one default and
three expanded scenarios; 26 stream scenarios and all-feature/all-target builds
also pass. The implementation uses only `std` and leaves `Cargo.lock`
byte-identical.

### Opt-in file-write bridge checkpoint (2026-08-10)

The existing two-argument `file_put_contents()` remains the complete default
handler. The independent `file-write` feature selects a four-argument handler
and publishes `FILE_USE_INCLUDE_PATH`, `LOCK_EX` and `FILE_APPEND`. PHP 8.5.9
differential probes define weak flag conversion, append/replacement behavior,
ordered scalar arrays, readable stream data, `file://`, named arguments and
the observable validation order for flags, context resources, source streams
and empty paths.

The write policy is isolated in `stdlib/file_contents/write.rs`; the sibling
parent retains the read handler and small shared frame helpers. An open source
stream is read through alternating request-registry borrows and one fixed 8 KiB
stack buffer. Strings and array fields are also emitted incrementally, so the
handler does not allocate a combined payload. Non-stringable object/closure
warning behavior remains conservative (`false`), valid stream-context
resources do not yet exist, and `FILE_USE_INCLUDE_PATH` uses the ordinary path
resolver until configurable include paths land.

`LOCK_EX` uses `std::fs::File::lock`. Replacement under a lock opens in
non-truncating create mode, acquires the lock first and only then truncates and
rewinds; append keeps the same lock across all writes. Memory and temporary
wrappers reject the regular-file lock flag. A permanent test holds the lock
against a competing file descriptor and verifies that truncation occurs inside
the exclusion interval.

ARM64 default codegen remains the exact 2,818,048-byte `f5d4e68` `__TEXT`, hot
addresses and lockfile. X86-64 retains text/data/bss
3384879/51792/2112 and hot addresses `0xce550`/`0xd1c40`; its pinned 20-pair
gate passes at +0.001%/-0.126%/+0.446%/+0.500%/-0.618%. Library matrices pass
195/186/196/300 on ARM64 and 195/186/196/325 on x86-64 for
default/no-default/file-write/all features. File E2E passes 2 default, 5
write-only and 7 combined read/write scenarios; the 26 stream scenarios and
all-feature/all-target builds pass on both hosts. No crate was added and
`Cargo.lock` remains byte-identical.

### Opt-in streaming `file()` lines checkpoint (2026-08-10)

`file-lines` replaces only the feature build's registration of the old
one-argument eager `file()` function. Its three-argument handler lives in
`stdlib/file_contents/lines.rs`, opens the established `PhpStream` backend and
reuses one line vector across the complete input. Lines longer than 8 KiB use
the existing fallible `read_line` growth and cursor contract rather than a new
buffered reader.

PHP 8.5.9 probes define `FILE_USE_INCLUDE_PATH=1`,
`FILE_IGNORE_NEW_LINES=2` and `FILE_SKIP_EMPTY_LINES=4`. LF is removed first,
then a preceding CR, and only then does skip-empty test the line. Consequently
`FILE_SKIP_EMPTY_LINES` alone retains a newline-only record while combining it
with `FILE_IGNORE_NEW_LINES` removes that record; spaces, tabs and `"0"`
remain. Unknown bits raise the exact `ValueError`, parameter types precede flag
value validation, a validly typed but non-context resource follows it, and an
empty path is checked last.

The feature supports bare/current/absolute paths and `file://`. Configurable
include-path search and valid Stream-Context resources remain absent. A
pre-existing general nested-call bug with adjacent named arguments was exposed
during testing and deferred to the following codegen-gated call-frame
checkpoint rather than being hidden in the filesystem handler.

Default ARM64 remains the exact 2,818,048-byte `f5d4e68` `__TEXT` and hot
addresses. X86-64 remains exact at 3384879/51792/2112 text/data/bss and
`0xce550`/`0xd1c40`; its fresh pinned 20-pair gate passes at
-0.095%/+0.197%/+0.005%/-1.697%/+0.404%. ARM64 library matrices pass
195/186/195/300 and x86-64 195/186/195/325 for
default/no-default/file-lines/all features. File E2E passes 3 default, 5
line-feature and 10 combined scenarios; 26 stream scenarios and full
all-feature/all-target compilation pass on both hosts. The implementation adds
no crate and leaves `Cargo.lock` byte-identical.

### Reused named-call frame safety checkpoint (2026-08-10)

Named sends can target declared parameters out of order, but `push_call_frame`
intentionally leaves the supplied argument prefix uninitialized for the common
positional `SendVal` path. Reusing that stack region therefore let duplicate
and required-hole checks inspect stale bytes. The compiler now records each
named send's source position in `extended_value`; the first named send preserves
the preceding positional prefix and writes `Undef` to every remaining declared
parameter before any check reads it. The work is outlined in the cold
`prepare_named_call_frame` helper, so ordinary positional sends and frame
allocation gain no loop or initialization.

Dynamically resolved invokable objects also require a hidden `$this` slot.
Their pending receivers now form packed `(call frame, receiver)` pairs inside
the existing `pending_invoke_this: Option<Value>` side-state slot. This keeps
the established `ExecutorGlobals` layout and one cheap `is_none()` hot check,
while nested argument calls can no longer consume an outer receiver. On the
first named send only the already-written positional prefix moves right; named
destinations already include the method offset. Cleanup and coroutine exchange
retain the same single owned side-state value.

Regression coverage grows to 49 named-argument E2E cases and includes reused
internal functions, nested calls, user functions, methods, static methods,
constructors and nested `__invoke` calls with mixed arguments. File E2E now
keeps its original nested `file(filename: ..., flags: 6)` form. Default,
no-default and all-feature library matrices pass 195/186/300 on ARM64 and
195/186/325 on x86-64; 10 file, 26 stream and 6 active coroutine-context tests
plus all-feature/all-target compilation pass on both hosts.

ARM64 keeps the exact 2,818,048-byte `__TEXT` and monitored quick-loop/string
addresses. Its fresh 20-pair gate is
-0.088%/-0.045%/-0.386%/-2.942%/-0.382%. Linux places the new named helpers and
three established cold/post-loop helpers in a 0x988-byte `.rphp_cold` section;
the quick loop and array kernel addresses remain exact. X86-64 text/data/bss is
3387875/51816/3240 and its pinned gate passes at
+0.092%/+0.291%/-0.753%/-0.192%/-0.209%. No crate, feature or lockfile changes.

### Executor call-path ownership split (2026-08-10)

The baseline/quick composition root no longer owns five unrelated call-path
implementations inline. `src/vm/execute.rs` falls from 6,917 to 1,877 lines;
the original 5,045-line sequence is retained in place through private
`include!` boundaries for frame-slot/property helpers, direct scalar calls,
direct object calls, composed scalar calls and call-frame/exception lifecycle.
The split does not create Rust submodules, change visibility, reorder an item
or introduce a second execution abstraction. It makes the ownership boundary
visible while preserving the established monomorphization and call identities.

The moved source is mechanically identical apart from one file-ownership
comment per child. Default, no-default and all-feature library matrices pass
195/186/300 tests on ARM64 and 195/186/325 on x86-64. Forty-nine named-call
scenarios, six active coroutine-context scenarios and complete all-feature/
all-target compilation also pass on both hosts.

ARM64 retains the exact 2,818,048-byte `__TEXT` size and monitored quick-loop
and String-commit addresses. Its fresh 20-pair default runtime gate records
+0.302%/-0.190%/-0.454%/-1.697%/+0.156% for scalar, packed array, String,
order and ledger. On x86-64 GNU `size` moves 192 bytes from bss accounting to
text accounting while preserving the exact 3,442,931-byte total; the monitored
hot group shifts uniformly by `0xc0`. The pinned gate nevertheless passes at
-0.290%/+0.285%/-0.822%/-0.204%/-0.252%. No manifest, lockfile, feature, crate
or external library changes.

### Opt-in Stream Context substrate checkpoint (2026-08-10)

Phase 5 now has a request-owned `stream-context` resource behind the independent
`stream-context` feature. `stream_context_create()` normalizes wrapper options
into nested PHP arrays and retains the supported `notification` callback
parameter after validating it through the existing callback resolver.
`stream_context_get_options()` and `stream_context_get_params()` expose the
stored state for both Context resources and open streams.

The expanded `fopen()`, `file_get_contents()`, `file_put_contents()` and
`file()` handlers accept only a valid Stream Context resource and preserve PHP
8.5.9 validation order and exact covered TypeError/ValueError messages. PHP
uses the supplied Context while opening but does not copy its state into the
new stream; the stream therefore starts with its own empty Context state.
Default handlers, resource layout and registration remain compiled exactly as
before when the feature is off.

The first checkpoint deliberately limits the surface to creation, reads and
validated file opens. Mutators follow as a separate measured slice below;
wrapper-specific consumption of stored options and configurable include-path
lookup remain independent work. The implementation uses only existing runtime
types and the standard library; no crate is added and `Cargo.lock` is unchanged.

ARM64 keeps the exact 2,818,048-byte default `__TEXT`, quick-long-loop and
String-commit addresses. Its noisy full 20-pair gate reported
-2.007%/-2.011%/+0.273%/+0.491%/+1.610%; because the order workload ranged
from roughly 65 to 149 ms despite exact static layout, the mandatory isolated
ledger rerun was used and passed at +0.347%. X86-64 remains exact at
3388067/51816/3048 text/data/bss, the 0x988-byte `.rphp_cold` section and the
post-refactor hot addresses. Its pinned 20-pair gate passes at
-0.287%/+0.217%/-0.693%/-0.694%/+0.049%.

Default/no-default/all-feature library matrices pass 195/186/300 on ARM64 and
195/186/325 on x86-64. Stream Context-only coverage passes 16 stream scenarios;
the all-feature surface passes 28 stream and 11 file scenarios, and complete
all-feature/all-target compilation passes on both hosts.

### Mutable Stream Context state checkpoint (2026-08-10)

PHP 8.5.9 differential probes corrected the initial stream-attachment
assumption and define the mutable surface. `stream_context_set_option()` accepts
the legacy two-argument option-array form or the four-argument wrapper form;
`stream_context_set_options()` exposes the non-deprecated array form, and
`stream_context_set_params()` updates callbacks and nested options. Context
resources and streams own independent state, and an ordinary stream lazily
creates its empty Context state on the first mutation.

Option updates merge by wrapper and option name. Empty or numeric-only inner
arrays add nothing, invalid top-level shapes raise the PHP ValueError, and weak
string conversion plus omitted/null argument distinctions follow the probed
error order. Parameters retain the last valid `notification` callback and can
merge an `options` array. PHP applies a valid callback before validating that
array, so a later options error deliberately preserves the callback update.
Unknown parameter keys remain ignored.

The 628-line prototype was split before admission: the create/get/open owner is
374 lines and `context/mutate.rs` owns 301 lines of setters and their type
policy. There is no second state abstraction. The runtime gate now optionally
accepts prebuilt candidate and baseline executables, allowing thermal-noise
reruns to use the identical paired statistic without two preceding release
compilations.

The default ARM64 `__text` bytes match checkpoint `7ec3224` at SHA-256
`feae31bd9f8de1ce4b08aaf5da5a106c28c61c6592ee91d6d0429cd35a528a2a`;
`__TEXT` remains 2,818,048 bytes and all four monitored loop/string symbols are
exact. Only Mach-O UUID and five line-metadata bytes differ in loaded segments.
The historical full batch was thermally unstable at
-1.168%/+1.439%/+1.459%/-5.998%/-1.068%, so it is not used to invent a runtime
delta for byte-identical code.

X86-64 likewise retains exact 3388067/51816/3048 text/data/bss, monitored
addresses and 0x988-byte `.rphp_cold`; its `.text` SHA-256 matches `7ec3224` at
`66b53177492bbcc339cf7383a7770de330eed12d40b85e1e767b18db29096204`.
The build-free CPU-pinned 20-pair gate against that immediate checkpoint passes
at +0.078%/-0.036%/-1.033%/-0.144%/+0.691%. A separate historical `f5d4e68`
String comparison now reproduces a pre-existing +5.077% cumulative drift in
both identical checkpoint binaries; it is not attributed to this feature-only
slice.

The follow-up bisection places that short-workload signal at the executor
ownership split: five new source-location identities add 192 read-only bytes
and move otherwise unchanged x86-64 code by `0xc0`. Long append runs show no
steady-state loss. Linker anchoring and bounded String pre-reservation improve
the isolated workload but fail packed-array or order controls, so none is
admitted. Only the five ownership filenames are shortened; contents, include
order, accepted section totals and monitored symbol addresses stay unchanged.
The final 40-pair x86-64 gate is
+0.051%/+0.203%/-5.654%/-1.156%/-0.073%, and an isolated 100-pair String run
confirms -5.526%.

Library matrices remain 195/186/300 on ARM64 and 195/186/325 on x86-64.
Feature-only stream coverage grows from 16 to 17 scenarios and all-feature
coverage from 28 to 29; 11 file scenarios and all-feature/all-target compilation
pass on both hosts. No feature, crate or lockfile change is involved.

### Configurable include-path checkpoint (2026-08-10)

The opt-in `include-path` feature composes the accepted Stream Context and
bounded file surfaces and gives them one request-local resolver. It registers
`get_include_path()`/`set_include_path()` and consumes the same ordered path in
`fopen()`, `file_get_contents()`, `file_put_contents()`, `file()` and
include/require. Absolute paths, wrapper URLs and explicit `./`/`../` requests
bypass lookup; the first existing entry wins, while a new write falls back to
its original path. PHP 8.4.24 probes also lock empty-entry behavior, weak scalar
setters, the non-mutating empty result and the embedded-NUL `ValueError`.

The implementation stores the current `.`-default value under a private
namespace in the executor's existing request-owned state map. It therefore
needs neither an `ExecutorGlobals` field nor a second TLS registry. Resolution
uses only `std::path` and `std::fs`; no crate or lockfile change is involved.
Default ARM64/x86-64 `.text` is byte-identical to `c026124`, with exact section
sizes and monitored addresses. The pinned x86-64 gate is
-0.012%/+0.175%/+0.364%/-0.401%/+0.088%; isolated 40-pair ARM64 array/ledger
controls pass at -0.003%/+0.129% after a thermally noisy full batch.

Library matrices pass 195/186/301 on ARM64 and 195/186/326 on x86-64. Two
focused E2E scenarios join the unchanged 11 file, 29 stream and 12 include
scenarios, and x86-64 all-feature/all-target linking passes.

### Default Stream Context checkpoint (2026-08-10)

The `stream-context` surface now includes `stream_context_get_default()` and
`stream_context_set_default()`. One hidden request-owned resource is returned
for the lifetime of an executor, survives every userland alias being unset and
remains distinct from explicit Context resources and the independent Context
state attached to ordinary streams. Both entry points merge wrapper options
into that stable resource; omitted/null options are accepted only by the
getter, matching PHP 8.4.24 argument policy.

PHP applies valid outer option entries before validating later entries in the
same call. The new `context/default.rs` owner therefore publishes those partial
updates even when a later numeric wrapper key or non-array wrapper value raises
`ValueError`. This policy is intentionally separate from constructors and
ordinary mutators, which continue to validate their complete option shape
before mutation. Numeric inner keys are ignored and an inner array containing
only numeric keys creates no wrapper state.

The singleton handle is retained under a private namespace in the existing
request state map, so it also keeps the resource alive when the optional
`resource-lifetime` feature is enabled. No `ExecutorGlobals` field, secondary
registry, crate or lockfile change is required. Wrapper-specific consumption
of default options by future network transports remains separate work; this
checkpoint establishes the PHP-visible identity, merge, mutation and error
contract.

Default ARM64 and x86-64 executable text remains byte-identical to `c026124` at
SHA-256
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`
and `12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`.
ARM64 retains the 2,818,048-byte `__TEXT`; x86-64 retains
3,388,067/51,816/3,048 text/data/bss. Build-free 20-pair gates record
+0.385%/-0.144%/+0.094%/+0.044%/+0.248% on ARM64 and
+0.035%/-4.891%/-0.838%/+0.280%/+0.348% on pinned x86-64. The large negative
x86 array value came with visible host noise and is not claimed as a speedup;
all five admission ceilings pass.

Library matrices remain 195/186/301 on ARM64 and 195/186/326 on x86-64.
Feature-only/all-feature stream coverage grows to 19/31 scenarios, including
stable identity, merge order, alias lifetime, independence, exact type errors
and partial updates. Complete all-feature/all-target linking passes on x86-64.

### Canonical include-path reporting checkpoint (2026-08-10)

The existing `include-path` feature now registers
`stream_resolve_include_path()`. It returns the canonical absolute identity of
the first existing target or `false`, resolving symlinks and `..` components
for absolute paths, explicit `./`/`../` paths, ordered include-path candidates
and local `file://` URIs. Existing directories are valid results, a
case-insensitive `file://localhost/...` maps to its local path, and unsupported
wrappers remain unresolved.

PHP 8.4.24 distinguishes this reporting function from the open-time resolver:
empty include-path entries are skipped, while an empty filename can resolve
the first non-empty directory itself. The two policies therefore remain
explicit instead of overloading the operational resolver: the original module
is 223 lines and the 127-line `include_path/report.rs` child owns canonical
reporting and its unit contract. Type/NUL validation is shared with
`set_include_path()` after a small local refactor; null, false,
bool, integer and float values retain PHP's weak string conversion, while
arrays/resources and embedded NUL bytes retain exact covered errors.

Only `std::fs::canonicalize` and `std::path` are used. Feature-off ARM64/x86-64
text stays byte-identical to `c026124` at
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`
and `12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`,
with exact 2,818,048-byte `__TEXT` and 3,388,067/51,816/3,048 x86-64 section
totals. Build-free 20-pair gates pass at
+0.339%/-0.213%/+0.494%/-0.491%/+0.196% on ARM64 and
+0.165%/-0.029%/-0.525%/+0.136%/-0.034% on pinned x86-64.

Library matrices pass 195/186/302 on ARM64 and 195/186/327 on x86-64, with
201 tests in the focused include-path build. Include-path E2E coverage grows
from two to four scenarios; 31 all-feature
stream scenarios and complete x86-64 all-feature/all-target linking remain
green. No feature, crate or lockfile change is involved.

### Truthful stream-registry checkpoint (2026-08-10)

The independent `stream-registry` feature adds `stream_get_wrappers()`,
`stream_get_transports()`, `stream_get_filters()` and `stream_is_local()`.
Registry results describe the integrated PHP Stream layer rather than every
unrelated runtime capability: `php` and `file` are the admitted wrappers,
while transports and filters remain empty. Coroutine TCP/UDP descriptors are
intentionally not advertised as PHP Stream transports, and no filter pipeline
is claimed before one exists.

PHP 8.4.24 probes define locality independently of path existence. Plain paths,
`php://`, local file hosts, archive/compression-style and unknown fallback
schemes are local; HTTP(S), FTP(S), data URLs and remote `file://` hosts are
not. Every current `PhpStream` backend is local. Closed streams and other
resource types raise the exact covered invalid-stream TypeError. Scalar/array
weak conversion is retained, and Stringable objects reuse the VM's existing
`__toString()` call path; non-Stringable objects and closures raise PHP's Error
instead of silently using the runtime debug representation.

The 139-line `streams/info.rs` module owns registry policy and one pure locality
test. A feature-only VM bridge delegates object conversion to the existing
magic-method helper, so there is no second invocation mechanism. The default
ARM64/x86-64 text remains byte-identical to `c026124` at
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`
and `12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`;
section totals remain 2,818,048-byte ARM64 `__TEXT` and
3,388,067/51,816/3,048 on x86-64.

Build-free 20-pair gates pass at
+0.558%/-0.589%/+0.304%/-0.891%/-0.272% on ARM64 and
-0.191%/+0.062%/-0.892%/-1.806%/+0.216% on CPU-pinned x86-64. The noisy
negative order result is not treated as a speedup. Library matrices pass
195/186/303 and 195/186/328; the focused feature passes 196 library and 16
stream scenarios, while all-feature stream coverage reaches 33. Complete
all-feature/all-target compilation passes on both hosts. One opt-in Cargo
feature is added, but no crate, external library or lockfile change.

### Bounded arbitrary-ending stream reads (2026-08-10)

The independent `stream-line` feature adds `stream_get_line()` without growing
the default stream path. PHP 8.4.24 compatibility probes define zero length as
unbounded, positive length as a consumed-byte ceiling, and an ending as a byte
sequence that is consumed but omitted from the result. The contract includes
empty, overlapping, multi-byte and NUL endings, EOF/cursor behavior, weak
scalar and Stringable conversion, exact covered argument errors, closed
resources and unreadable streams.

Backend mechanics and PHP argument policy are deliberately separated. The
116-line `stream/get_line.rs` child owns fixed-chunk reading and a local linear
KMP matcher whose partial state crosses chunk boundaries; bytes beyond a match
are rewound so there is no persistent read-ahead buffer. The 121-line
`streams/line.rs` child owns registration, type conversion and return/error
mapping. Stringable conversion reuses the feature-only bridge to the existing
magic-method executor rather than introducing another call path.

Feature-off executable text remains byte-identical to `c026124` on both
architectures, including exact ARM64/x86-64 section totals. Build-free gates
pass at -1.958%/-0.448%/+0.038%/+0.982%/-0.886% on ARM64, with a noisy order
repeat at -0.511%, and +0.603%/-0.105%/-0.967%/-0.740%/+0.181% on pinned
x86-64. Library matrices pass 195/186/305 and 195/186/330; focused/all-feature
stream coverage is 16/35 scenarios and complete all-feature/all-target linking
passes on both hosts. No crate, external library or lockfile change is added.

### Writable stream-length checkpoint (2026-08-10)

The independent `stream-truncate` feature adds `ftruncate()` across regular
files, `php://memory` and `php://temp`. Its handler owns exact weak-size,
invalid-resource and writability policy, while the 42-line backend child owns
fallible resize mechanics. Cursor and EOF state are deliberately outside the
resize operation and therefore remain unchanged as PHP requires.

Memory and file storage diverge only after shrinking below the logical cursor.
PHP memory storage appends later writes at the new buffer end but advances the
old logical cursor; file storage writes at the retained cursor and zero-fills
the gap. One feature-only boolean on memory-backed streams preserves that
contract and disappears after seek. Temp storage carries the same state only
while memory-backed; an already-spilled temp stream uses file semantics, and
growth beyond the memory ceiling reuses the existing spill transition.

Feature-off code and layouts remain byte-identical to `c026124` on ARM64 and
x86-64. The build-free gates pass at
+0.243%/-0.332%/+0.488%/+0.310%/-0.101% and
-0.239%/+0.157%/-1.395%/+0.154%/+0.132%, respectively. Library matrices pass
195/186/307 and 195/186/332; focused/all-feature stream coverage is 16/37 and
complete all-feature/all-target compilation passes on both hosts. No crate,
external library or lockfile change is added.

### Scheduled interphase: dual-runtime generic metadata and dispatch

Once the active Phase 5 stream checkpoint is closed, the next architectural
interphase implements the latest published Bound-Erased Generic Types RFC
v0.22 through `php-generics-erased` and `php-generics-reified`. Either feature
admits the shared syntax; generic AST types, interned metadata and Reflection
are permanent compiled-in RPHP machinery, while a default parser rejects every
generic declaration and use site. An all-features build contains both runtime
capabilities for differential testing. Because that RFC is currently Declined,
this is an explicit RPHP experiment and stays absent from default PHP-
compatibility claims. Its complete syntax and validation contract is tracked
in the main performance/JIT roadmap.

Generic type parameters and use-site arguments live in compact, interned side
tables owned by declarations and explicit use sites. Objects retain their
current class identity, frames retain their current layout and `Value` remains
16 bytes; no reified argument vector is attached per instance. The executable
signature is produced once by erasing each parameter to its bound or `mixed`,
while the pre-erasure signature remains available to inheritance checks,
Reflection and optimizers.

The reified branch consumes the same symbols and type graph but retains the
canonical argument binding in a sidecar, allowing substituted parameter,
return and property checks. It does not widen `Value`, `FunctionCommon` or the
ordinary frame. Separate erased-only, reified-only and dual-feature builds stay
green so the runtime-model cost is measured rather than inferred.

The first vertical implementation now retains call bindings in a feature-only
LIFO vector placed last in `ExecutorGlobals`, so all established field offsets
remain exact. Named same-compilation-unit turbofish sites are discharged during
linking when arity, bounds and the reified runtime signature are provably
identical to erasure; they emit the same call stream as the ordinary generic
call. Sites requiring stricter substitution keep explicit pre-call and
post-return checks. `ReflectionFunction`, `ReflectionClass` and
`ReflectionMethod` all read the same interned declarations and report enabled
runtime capabilities even though the default parser keeps generic syntax
disabled.

Separately compiled `include` units now link into the same executor-wide
metadata graph. Existing strings are reused from the intern pool, incoming
symbol references are remapped once, and only `CheckGenericArgs` use-site
operands in the incoming op arrays receive a base relocation. Declarations,
reified bindings and Reflection consequently share one stable index space
across files without adding metadata to a frame, object, `Value` or ordinary
call. A collision test deliberately gives the main and included units local
use-site index zero with different bounds and verifies both runtimes plus
Reflection after linking.

The first reified-instance slice keeps canonical class arguments outside the
object. Explicit construction registers `(weak object identity, binding)` in a
feature-only executor sidecar before the constructor runs; cloning transfers
the binding, a weak reference prevents address-reuse aliasing, and
exponentially scheduled stale-entry sweeps retain amortized O(1) construction.
A weak one-entry L0 serves repeated property access without hashing. Typed
class and promoted properties are preserved in the same interned declaration
graph. Erased instances enforce the parameter bound, while reified instances
substitute their actual argument for defaults, constructor writes, ordinary
writes and clones. `ReflectionObject` exposes class parameters in every build
and concrete instance arguments only when a reified binding exists. `Value`,
`PhpObject`, `ExecuteData`, `FunctionCommon` and the 16-byte instruction/cache
layouts remain unchanged.

Inheritance clauses now preserve their generic arguments in the same cold
graph. Registration validates `extends`, `implements` and trait `use` arity,
defaults, bounds and forwarded bound-on-bound conformance, including metadata
merged from separately compiled units. Runtime class lookup and inheritance
layout still consume only the erased ancestor name. Declaration-site variance
now composes through nested generic arguments and inheritance slots, checks
function/method/property polarity and rejects class parameters in static
context; cross-unit targets are revalidated after merge. Class-like metadata
also retains method arity, variadic shape and pre-erasure parameter/return
types. The cold registration linker composes direct and transitive ancestor
bindings, substitutes those prototypes and checks staticness, arity,
contravariant parameters and covariant returns for class, interface and trait
overrides, including declarations merged from an included unit. Executable
method bodies remain erased; a separate runtime view supplies only stricter
linked or reified call boundaries.

Generic instance dispatch now adds a receiver-specific runtime view without
changing those executable signatures. The existing method inline cache uses
one previously free bit to identify methods with a reified substitution or a
stricter inherited link-time boundary. A weak object-binding L0 and a
declaration/use-site-plus-method L0 resolve direct and transitive ancestor
signatures once. Reified calls carry the full substitution; bound-erased calls
carry only slots whose linked child boundary differs from the executable
parent ABI. Fixed, variadic and named arguments plus return values are checked
against that contract. Pending and active contracts use feature-only LIFO
sidecars keyed by the existing frame identity, so nested calls and exception
unwinding remain exact without widening `Value`, `PhpObject`, `ExecuteData`,
`FunctionCommon`, an instruction or its 16-byte inline cache. A turbofish alone
still retains the original erased method dispatch.

For the common reified or linked `int -> int` method shape, an exact
scalar-plan proof checks that every substituted boundary admits `int`. The
already-guarded frame-free Long plan may then discharge the contract without
pending/active sidecar state; a mismatched argument, non-Long result or
arithmetic overflow side-exits to the canonical generic call and its full
checks. A second free IC bit records this proof for a non-reifiable concrete
child, avoiding contract materialization on its warmed success path. The hot
interpreter retains a weak per-frame proof for one reified receiver; changing
objects invalidates it without allowing allocation-address reuse, and returns
to the canonical contract before another specialized call.

Generic construction now resolves the effective own or inherited
`__construct` signature through the same method-contract cache. Canonical
calls validate fixed and variadic reified or linked constructor arguments
before entering the body. A proven property-initializer constructor can
instead validate both the substituted parameter and every generic destination
property, skip its frame, and resume at the existing class-binding return
check. Explicit generic bindings now also move through caller-owned pending and
call-owned active scope sidecars; successful completion, abandoned argument
evaluation and exception
unwinding remove the exact binding, preventing a caught generic call from
reifying a later ordinary call through stale LIFO state.

Generic property lookup now composes the same direct/transitive class and trait
bindings. Own properties take precedence; inherited reified properties receive
the fully substituted child binding, while bound-erased properties use the
child's linked signature: forwarded parameters erase to the child's bound and
unbounded parameters remain `mixed`. This covers
ordinary writes, the constructor property-initializer proof, trait properties,
cache invalidation between distinct bindings and child metadata merged from an
included unit. The cold metadata walk lives in `generics/properties.rs`; the
runtime checks and one-entry binding-plus-name L0 live in
`runtime/generic_properties.rs`. The L0 owns its substituted type, so warm
writes allocate nothing and metadata merges cannot invalidate a borrow. The
existing property IC still stores the concrete child declaration and no
object, `Value`, frame, function, instruction or IC layout changes.

That linked view now remains executable when the child itself is non-generic.
`IntBox extends Box<int>` has no turbofish and no object sidecar, but its
inherited property contract is `int` in both runtime modes; transitive children
and concrete trait bindings behave identically. The shared L0 distinguishes an
erased child declaration from a reified declaration/use-site pair. Cached
zero-parameter children skip the weak reified-object lookup because such a
declaration cannot carry an explicit binding. This closes the property half of
the RFC's general substituted signature.

The method/constructor half now materializes a sparse linked contract only when
the child's substituted erased boundary is stricter than the executable parent
ABI. Concrete and transitive children, concrete traits, forwarded child bounds,
returns, constructors and cross-unit declarations share the same resolver and
pending/active call sidecar as reified methods. The permanent concrete
`int -> int` benchmark remains within +0.382% of a manually typed method on
ARM64 and +0.566% on x86-64 across both feature builds. Method-generic
alpha-renaming and deterministic diamond contract merging remain explicit
follow-up link/runtime steps. Generics-aware JIT specialization is deliberately
last: it starts only after these semantics and both runtimes are closed, and
must consume the canonical metadata with exact guards and deoptimization back
to the established erased/reified paths.

ARM64 and x86-64 release builds additionally align functions to one 64-byte
cache line. This stabilizes the large dispatch entry points against unrelated
cold metadata/drop-glue growth: the unaligned feature-off property control
regressed 12.17% solely after `execute_ex` moved off its favorable boundary,
whereas the aligned final build is -0.612% in 31 paired ARM64 runs. The cost is
about 57 KiB of text (1.6%) on that build, with no source-level padding or
backend-specific dependency.

Ordinary calls continue through the existing call-frame path with no generic
flag test. Only `::<...>` sites emit a validation operation; its
inline cache stores the canonical argument tuple, resolved callee identity and
successful bound result before entering the normal call/new path. This mirrors
the RFC's pay-at-use boundary and gives the quick/JIT tiers stable proof data
without forcing a lookup on non-generic code. Feature-off generic-syntax
rejection, ordinary-program bytecode and hot-layout identity, zero steady-state
allocation at warm sites, a one-percent erased-call ceiling and a five-percent
warm-turbofish ceiling are hard admission gates. The complete executable hash
is diagnostic rather than an identity gate because the engine remains compiled
in even when its syntax is disabled.

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
