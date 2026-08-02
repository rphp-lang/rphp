# RPHP roadmap: no-JIT core, typed IR, JIT, compatibility

Status: accepted project direction, 2026-08-01

## Objective

Build a PHP runtime whose supported language subset is semantically correct,
systemically fast without a JIT, and structured so the same guarded execution
plans can later be lowered to native code. Compatibility breadth and production
hardening expand after the execution architecture is proven, while differential
correctness tests and representative application workloads remain active from
the beginning.

The project does not need to preserve the Zend C ABI. That freedom should be
used deliberately for compact values, a purpose-built call ABI, frame elision,
inline caches, typed execution plans, and precise deoptimization.

## Non-negotiable correctness contract

1. Baseline bytecode is the semantic source of truth.
2. Every optimized operation has an exact baseline resume position.
3. Guards run before the operation they protect mutates observable state.
4. Completed operations remain committed; fallback never repeats a side effect.
5. Type changes, overflow, references, COW, magic behavior, exceptions, and
   polymorphic dispatch leave the optimized path before behavior can diverge.
6. Removing every optimization cache must leave a correct executable program.
7. Supported behavior is continuously compared with reference PHP.

## Phase 1: optimal supported PHP core without JIT

Optimize runtime costs that a JIT cannot repair by itself:

- `Value` representation and scalar access;
- packed/hash arrays and strings;
- objects, declared properties, and copy-on-write behavior;
- function and method ABI;
- frame allocation, cleanup, and frame-free calls;
- monomorphic inline caches;
- memory ownership and lifecycle;
- guarded fallback and precise deoptimization.

The goal is not to win every microbenchmark. The exit criterion is that core
runtime primitives are stable, remaining hot costs are measured on both
microbenchmarks and representative programs, and no large structural overhead
is being delegated to a future JIT.

### Current performance workstream

Extend `QuickScalarCall` in this order:

1. scalar expressions in call arguments;
2. composed scalar user functions;
3. monomorphic scalar methods guarded by receiver class and method cache.

This targets the largest current no-JIT gaps: scalar methods, composed function
calls, and part of application-like object dispatch. String-append and
array-append kernels follow because their relative gaps are large but their
current absolute benchmark cost is smaller.

Each layer requires:

- a compiler/planner proof;
- runtime identity, arity, type, and dispatch guards;
- exact overflow/type/side-effect fallback tests;
- an interleaved release benchmark against `php -n`;
- a regression check on existing loop, call, and object workloads.

Implementation checkpoint (2026-08-01): all three layers are implemented.
On the current machine, a no-PGO release run is approximately 0.078 s RPHP
versus 0.101 s PHP for the composed function benchmark, and 0.090 s RPHP versus
0.093 s PHP for the scalar-method benchmark. Scalar argument preparation is
compiled once into a compact typed plan instead of rescanning argument bytecode
inside each loop iteration. The current 31-workload suite has 31 strict RPHP
wins and no PHP wins. These numbers are directional and must be remeasured after
related runtime changes; completing the microbenchmark matrix is not a
substitute for representative application coverage.

Isolated packed-array append still has structural headroom at approximately
0.0030 to 0.0036 s versus 0.0018 s, even though faster RPHP reads make the
combined array build-and-sum workload a 0.0037 s versus 0.0040 s RPHP win. This
is retained as a measured runtime cost, not as a reason to add a benchmark-only
kernel before the real-code corpus establishes its importance.

## Phase 2: unified typed execution IR

Consolidate the existing scalar, composed-scalar, property, recursion, and
quick-loop plans into a small typed IR. A representative vocabulary is:

```text
GuardLong
GuardClass
GuardFunction
LoadProperty
StoreProperty
AddLong
SubLong
MultiplyLong
CallScalar
Loop
Exit
Deopt
```

The no-JIT executor must run this IR first. That validates its semantics,
guards, side exits, and profitability before native code generation exists.

Exit criteria:

- calls, methods, properties, and loops share one deoptimization contract;
- plans can describe the majority of measured hot time without benchmark-only
  shapes;
- plan construction is outside the hot execution path;
- baseline execution remains independently correct.

Implementation checkpoint (2026-08-01): the first vertical slice is unified.
Scalar function and method bodies, plus quick-loop scalar call arguments, now
use the same `ScalarLongProgram`, `ScalarLongOp`, and context-independent
`ScalarLongSource`. An `Input` is bound to a public argument by the function
executor and to a CV slot by the quick-loop executor; constants and temporaries
retain identical semantics in both contexts. The former duplicate quick scalar
source, operation, and argument-plan types have been removed.

`ScalarLongProgram<Operation, OUTPUT_CAPACITY>` gives each program an exact
compile-time inline output capacity: function and composed bodies use one slot,
while quick argument plans use the guarded scalar ABI maximum of eight. This
keeps the common IR without the extra allocation and 2–4 percent regression
measured with exact-length boxed outputs.

The second vertical slice is also complete. Composed bodies use the same generic
program container and express calls through `ScalarLongCall`. Baseline and quick
executors share one inline-cache identity, `FastScalar` ABI, and arity guard;
only their post-guard execution strategy differs. The composed-call workload is
approximately 0.075 s RPHP versus 0.101 s PHP.

The third vertical slice adds `FunctionCache` and `MethodCache` dispatch to the
same call guard. Quick scalar methods and nested scalar call trees now share the
receiver-CV, receiver-reference, class-id, method-cache, target ABI, and arity
checks. A mismatch exits before typed execution and resumes the canonical call
initializer.

The fourth vertical slice extends the existing general `QuickLongOpsLoop` with
guarded property mutators, property getters, and the composed
`mutator(getter())` evaluation order. Target, receiver class, method cache, ABI,
and declared-property slot guards are resolved at region entry. Completed
object mutations commit immediately; retained scalar slots are committed at an
interrupt, side exit, or normal region exit. No class names, method names, or
benchmark constants participate in recognition.

This closes the previous largest gap across three different workloads:
application-like object dispatch is approximately 0.068 s RPHP versus 0.088 s
PHP, property R/W is 0.082 s versus 0.100 s, and method chain is 0.089 s versus
0.152 s. Overflow, impure targets, receiver-class changes, references, and
property type changes retain canonical fallback.

The fifth vertical slice composes a quick call site's scalar argument program
with a guarded leaf function body when the region is entered. Public body
inputs are substituted with argument outputs, body temporaries are shifted
after argument-expression temporaries, and the resulting program is evaluated
by one typed executor with a fixed inline temporary buffer. Failed composition,
target guards, type changes, and arithmetic overflow retain the existing exact
canonical fallback.

This removes the intermediate argument array and second typed-plan traversal
from the hot iteration. In the full no-PGO suite, the direct scalar-call ratio
improves from approximately 1.12x to 1.05x and the nested scalar-call workload
from approximately 1.06x to 0.98x. The latter is now a strict RPHP win without
adding a nested-loop-specific recognizer.

The sixth vertical slice specializes execution mechanics without adding new
PHP expression shapes. Fused scalar programs containing up to four operations
use unrolled steps instead of a nested operation-dispatch loop. A leaf program
also records its single guarded target once per region activation; only
composed programs construct nested-call bookkeeping. Programs longer than four
operations retain the generic interpreter, and all paths retain the same typed
IR and canonical side exits.

Sampling identified the nested typed loop and outlined bookkeeping closure as
the dominant costs before this change. In the full no-PGO suite, direct scalar
call improves further to approximately 0.046 s RPHP versus 0.075 s PHP and
nested scalar call to 0.011 s versus 0.019 s. The composed call-heavy workload
remains approximately 0.077 s versus 0.100 s.

The seventh vertical slice adds `ArrayPushLong` to the general typed loop IR.
The planner accepts Long CV, temporary, and literal values and records mutated
array slots separately from immutable array inputs. Runtime verifies type,
reference state, and unique COW ownership once at region entry, then retains the
mutable `PhpArray` and appends Long values without repeated opcode dispatch,
cloning, or refcount checks. Shared and reference-backed slots retain canonical
COW behavior through baseline fallback; interrupts and arithmetic side exits
commit completed appends exactly once.

Profiling separated the old array workload into a slow append phase and an
already-fast read phase. With the typed append operation, 499,967 of 500,000
benchmark appends execute in one guarded region with zero deoptimizations. The
combined array build-and-sum workload improves from approximately 0.0060 s to
0.0036 s RPHP versus 0.0038 s PHP, while the rest of the 31-workload suite
retains its previous wins.

The eighth vertical slice adds guarded string append to the same typed loop IR.
Literal and loop-invariant string sources are accepted; destination strings are
tracked separately from immutable string inputs. Runtime verifies type,
reference state, and unique COW ownership once at region entry and retains the
mutable `String` for direct `push_str` execution. Shared strings, references,
non-string sources, and self-append retain canonical conversion and COW behavior
through baseline fallback.

Sampling attributed approximately 79 percent of the former string workload to
the generic opcode executor and 9 percent to repeated COW checks, while buffer
growth was negligible. In the no-PGO suite, 199,967 of 200,000 appends execute
in one typed region with zero deoptimizations. String concat improves from
approximately 0.0017 s RPHP versus 0.0012 s PHP to 0.0010 s versus 0.0013 s,
bringing the current microbenchmark matrix to 31 strict RPHP wins out of 31.

## Phase 3: representative real-code corpus

Start before the JIT and continue through all later phases. Add progressively
larger programs covering:

- DTO and service-object flows;
- routing and middleware-like dispatch;
- dependency-container patterns;
- collections and transformations;
- serialization and parsing;
- business calculations with exceptions and mixed data.

This phase is deliberately concurrent with performance work. It prevents the
runtime and IR from being optimized only for synthetic loops while avoiding a
premature requirement to run an entire framework.

Implementation checkpoint (2026-08-02): the first corpus workload is an
order/quote service pipeline combining a request DTO, constructor and service
object dispatch, declared properties, associative return data, strings,
branches, and integer business calculations. Its deterministic aggregate is
checked against PHP 8.4 and its performance runner is intentionally separate
from the 31-workload microbenchmark matrix.

The initial no-PGO run was approximately 0.521 s RPHP versus 0.082 s PHP for
500,000 quotes. VM statistics showed no quick-region entry, 3,000,005 call
frames, 500,003 object allocations, 3,500,000 property reads, and 2,000,004
associative-array reads. Sampling then exposed repeated textual constructor
resolution inside every `NewObj`: formatting, lowercasing, allocating, and
hashing the same `Class::__construct` name. A per-opcode cache, guarded by the
stable class ID and supporting negative entries for classes without a
constructor, reduced `find_function` calls from approximately 500,014 to 14
and runtime to approximately 0.421 s. This is a general object-allocation
improvement, not corpus-name recognition. The next measured costs are object
allocation/lifecycle and construction/destruction of short associative return
arrays.

The full release test suite passes after this change. A concurrent no-PGO
microbenchmark rerun has 30 strict RPHP wins out of 31; the isolated packed
array build-and-sum workload is the single marginal PHP win at approximately
1.06x, consistent with the already-recorded packed-append headroom and
unrelated to constructor execution.

The second corpus-driven runtime slice specializes array-literal allocation
without recognizing the workload. The compiler selects packed, string-hash, or
neutral initial storage from static keys, passes exact literal capacity only
when the target representation is known, and leaves dynamic or sparse-integer
literals allocation-neutral.
Known string-key literals start in hash storage at final capacity. Insertion
borrows the source `Value` key and shares its immutable `Rc<String>` bytes with
the entry and index; later source mutation still detaches through COW. This
removes the packed-to-hash transition, intermediate `ArrayKey` string, duplicate
key bytes, and repeated capacity growth for common DTO/result arrays.

The order pipeline improves further to approximately 0.33 s RPHP versus 0.080 s
PHP, about 4.15x, and the full no-PGO microbenchmark matrix returns to 31 strict
RPHP wins out of 31. Compared with the original 0.521 s corpus baseline, the
first two general runtime corrections remove roughly 36 percent of RPHP time.
Fresh sampling now attributes most remaining time to general opcode execution;
property reads, object allocation, and short-hash lifecycle are secondary. The
next structural target is therefore a guarded application region spanning the
outer loop, object calls, properties, and result extraction, with canonical
side exits, rather than another workload-specific hash kernel.

The third corpus-driven slice removes a separate call-ABI limitation before
building that region. The metadata-driven frame-free internal ABI now has a
two-operand opcode in addition to its unary form, and `intdiv` is its first
binary client. Positional direct calls compile to one `DirectInternalCall2`;
namespace shadowing and named arguments retain canonical call resolution, while
`call_user_func_array` uses the same registered direct handler when its callback
and argument shape are eligible.

In the order pipeline this replaces 1,000,000 `InitFcall`/send/`DoFcall`
sequences and reduces call-frame creation from 3,000,005 to 2,000,005. Runtime
improves from approximately 0.33 s to 0.316 s versus 0.080 s PHP, about 3.95x.
The modest time reduction despite a million removed frames confirms that the
remaining dominant cost is cross-opcode application execution, not one
especially expensive internal frame. The complete release test suite and all
31 no-PGO microbenchmark wins remain intact.

The fourth corpus-driven slice starts unifying the baseline object path with
the guarded machinery already used by the hot tier. A bytecode audit of the
outer application loop confirms why extending one more loop pattern would not
help: the current closed-loop planner would have to accept the entire region,
whose first unsupported arithmetic operation is followed by object allocation,
constructor execution, a service call, and result extraction. Partial guarded
application regions with exact resume points remain the structural goal.

Before adding that region boundary, the measured common-case `FetchObjR` path
now stays in the main dispatch loop. Once a declared public property site has
resolved, a class-ID guard and stable-slot load replace the outlined resolver;
cache misses, visibility rules, dynamic properties, references, COW values,
and `__get` retain the canonical slow path. This is a general DTO and service
object optimization and applies outside the corpus. On the order pipeline it
improves the best no-PGO time from approximately 0.320 s to 0.310 s, about
3 percent, while the complete release suite passes and the 31-workload no-PGO
matrix remains 31 strict RPHP wins out of 31.

The fifth corpus-driven slice implements the first guarded application region
between call events. Constructors and the service method still execute through
the canonical call engine. After the service returns its associative result,
the compiler can select a straight-line typed region for repeated
`FetchArrayLong` and `AddAssign` operations and resume baseline at the following
comparison. The operation graph, target encoding, hotness counters, Long and
array guards, and exact side-exit positions are shared with `QuickLongOpsLoop`;
the new work is region selection outside a closed loop.

Straight-region input analysis is read-before-write aware, so temporaries
produced inside the region are not incorrectly guarded as pre-existing inputs.
This permits activation in fresh function frames as well as in one long-lived
loop frame. Calls, returns, control-flow edges, array mutation, and observable
side effects remain hard boundaries in this first slice.

The initial generic typed-op execution was correct and completed 499,968
regions with no guard failures or deoptimizations, but regressed the corpus to
approximately 0.343 s. This established a profitability rule now enforced by
the compiler: a short straight region is installed only when its typed graph
has a preselected dense execution shape. The first shape compresses one to four
array-fetch/add-assign pairs, plus an optional trailing fetch, without using
source names or literal values. It preserves already-completed additions and
resumes at the current fetch or addition on a non-Long value or overflow.

With dense selection moved entirely out of the hot path, the order pipeline is
approximately 0.299 s RPHP versus 0.079 s PHP, about 3.78x. This is roughly
3.7 percent faster than the 0.310 s cached-property baseline and 6.7 percent
faster than the 0.320 s result before the two latest application slices. The
complete release suite passes, including fresh-frame and mid-region type
side-exit tests, and the no-PGO matrix remains 31 strict RPHP wins out of 31.

## Phase 4: minimal typed-region JIT

Lower only already-proven typed plans at first:

- integer and floating-point arithmetic;
- conditions and loops;
- scalar functions and methods;
- monomorphic declared properties;
- calls inlined under identity/class guards;
- side exits into the baseline interpreter.

Do not build an opcode-by-opcode JIT. The benefit must come from keeping values
in registers, hoisting guards, eliminating dispatch and frames, and compiling
whole typed regions.

Benchmark four modes, including cold and warm behavior:

1. PHP without JIT;
2. RPHP without JIT;
3. PHP with JIT;
4. RPHP with JIT.

Also track compilation latency, code-cache memory, runtime memory, and
deoptimization frequency.

## Phase 5: compatibility breadth and production use

Once the execution architecture and minimal JIT are proven, broaden support
substantially:

- references and complex aliasing;
- exceptions and `finally` edges;
- magic methods and wider object semantics;
- inheritance, traits, closures, and generators;
- standard library and Composer-oriented behavior;
- extension strategy;
- diagnostics, profiling, deployment, security, and operational hardening.

Compatibility is expanded here, not first introduced here: every earlier
phase already preserves the supported subset and exercises selected real code.

## Decision gates

Proceed from no-JIT optimization to unified IR when new handwritten kernels
mostly duplicate combinations already expressible by typed plans.

Proceed from typed IR to JIT when:

- guards and exact deoptimization are stable;
- representative hot regions are expressible;
- the remaining cost is dominated by typed-plan dispatch, call boundaries, or
  repeated guards that native code can remove.

Do not postpone a runtime data-structure problem merely because a JIT exists.
Strings, arrays, object layout, allocation, COW, and lifecycle remain runtime
responsibilities.
