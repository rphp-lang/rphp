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
