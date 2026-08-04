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

Source-architecture limits and the bounded post-JIT coroutine branch are
specified separately in
[the runtime architecture roadmap](roadmap-runtime-architecture.md). They are
performance constraints, not cleanup work that may silently add abstraction
cost to the executor.

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

Typed-declaration checkpoint (2026-08-02): exact method contracts now propagate
from class-typed parameters, `$this`, `new`, straight-line assignments, and
inherited declarations. The receiver class and resolved method signature are
guarded once at `InitMethodCall`; downstream integer and string operations use
the existing declaration-proven opcodes without repeating that dispatch guard.
Exact all-`int`, non-reference method arguments also reuse the typed scalar ABI
only when the runtime override retains the same ABI. Nullsafe calls, referenced
receivers, untyped overrides, and failed signature guards retain canonical
execution.

In the method-return fanout workload, an interleaved seven-run no-PGO sample has
an RPHP median of approximately 0.271 s typed versus 0.276 s untyped. PHP
without JIT is approximately 0.071 s versus 0.070 s. The small RPHP typed win
confirms that the facts reach ordinary bytecode, but the remaining roughly
3.8x gap also shows that method/frame execution dominates this workload; tag
checks in the eight downstream modulo operations are no longer the main cost.

Typed scalar control-flow checkpoint (2026-08-02): the shared Long IR now
represents pure two-edge `if` and guard-clause bodies. It accepts ordinary and
compiler-fused equality/range branches, including side-effect-free bit masks,
and executes only the selected scalar return arm. Runtime Long, dispatch, and
checked-arithmetic guards still leave the canonical frame untouched on
failure. The same program has also gained integer modulo, integer XOR, and
immutable local-CV aliases, so results stored in ordinary local variables can
flow through composed functions without rebuilding a frame. Mutated public
parameters remain canonical.

Return-only `: int` functions can use this guarded plan even though their
canonical call strategy remains `Fast` and continues to validate fallback
returns. Eligibility is signature-based: parameters must be untyped, `mixed`,
or `int`, the return must be untyped, `mixed`, or `int`, and references remain
excluded.

In a five-run no-PGO matrix, the integer call chain is approximately 0.086 s
RPHP versus 0.097 s PHP untyped and 0.086 s versus 0.105 s typed: both are now
strict RPHP wins. Integer fanout improves to approximately 0.077 s versus
0.057 s untyped and 0.076 s versus 0.061 s typed. The remaining method-return
fanout gap is approximately 3.5x because its consumer has a mixed object/Long
signature that the scalar-only external ABI cannot yet represent; this is the
next call-layer boundary, rather than another arithmetic opcode gap.

Mixed typed ABI checkpoint (2026-08-02): composed scalar plans now describe
public inputs with separate Long and guarded-object masks. A class-declared
object may be used as a method receiver without ever entering scalar
arithmetic, while integer arguments and results continue through the shared
Long IR. At quick-region entry the runtime validates the declared class
contract, resolves the monomorphic method cache and scalar leaf plan once, and
then evaluates only the Long portion inside the loop. A changed receiver class,
a reference, a failed type contract, an impure method, or an incompatible
override leaves the canonical call frame untouched and resumes normal PHP
execution.

This removes the previously measured frame boundary rather than specializing
the benchmark's arithmetic. In the same five-run no-PGO matrix, method-return
fanout improves from approximately 0.244 s to 0.079 s untyped and from 0.244 s
to 0.078 s typed. PHP without JIT measures approximately 0.069 s and 0.070 s,
so the gap contracts from roughly 3.5x to 1.14x/1.12x. The rest of the matrix
stays within ordinary run-to-run variance. The next decision should therefore
come from profiling the remaining composed-program and string-return costs,
not from adding another method-fanout-specific kernel.

Borrowed typed-String checkpoint (2026-08-02): sampling the typed string fanout
showed roughly three quarters of runtime below the general `execute_ex` loop,
with frame cleanup and repeated return-contract checks also visible. `strlen`
itself was not the dominant cost. Pure fixed-signature functions that select
immutable string literals from guarded Long predicates now expose a borrowed
String leaf plan. Exact `: string` call facts can feed that result into a typed
composed consumer without creating a PHP `Value`, changing a reference count,
or building either function frame.

The typed IR currently observes these borrowed strings through byte-length
operations. A concatenation with a compile-time string literal carries its
checked combined length forward, so `strlen(label($value) . '!')` does not
materialize an intermediate string. Any consumer that needs actual contents,
an impure or unsupported producer, a failed Long/dispatch guard, references,
or an unknown return contract remains on canonical PHP execution. String-aware
operations use a separate composed enum and executor; the established
two-variant Long executor is deliberately not widened by new value kinds.

In the five-run no-PGO matrix, typed string fanout improves from approximately
0.086 s to 0.031-0.033 s, close to PHP's 0.028-0.030 s. Typed string chain with
a literal concat improves from approximately 0.097 s to 0.015 s versus 0.026 s
for PHP, making RPHP about 1.7x faster. The corresponding untyped workloads are
unchanged, which is intentional evidence that exact declarations—not a
benchmark-name special case—license this representation. The next String work
should add reusable borrowed-content operations only when real corpus shapes
need them; array/object and general function-frame gaps remain separate.

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

The sixth corpus-driven slice removes repeated string hashing from associative
reads without introducing a corpus-only kernel. Each string-key `FetchDimR`
site can cache the last ordered-entry position. Every use validates both the
position and the current key against the current array; layout changes, COW,
different arrays, and changing dynamic keys safely fall back through the
canonical index and refresh the hint. Baseline execution and dense straight
regions use the same cache contract, while numeric strings continue through
ordinary PHP key normalization and the integer-key path.

Two final independent best-of-five runs measure 0.281--0.283 s RPHP versus
0.078--0.079 s PHP, about 3.60x and another 5--6 percent below the first
dense-region result. The complete release suite passes, including
reordered-array coverage. The latest no-PGO matrix contains 30 strict RPHP wins
and one timing-level tie. Sampling now leaves short associative-array
construction/destruction and object allocation as the largest runtime-owned
costs outside general opcode execution, so those lifecycle costs are the next
candidates for structural analysis.

The seventh corpus-driven slice adds an allocation-free small ordered-hash
representation. Up to three explicit integer or string entries live directly
inside the existing `PhpArray` allocation; the fourth new key dynamically
promotes to the unrestricted indexed representation. This is not a PHP array
size limit. Existing keys can be overwritten inline, and ordering, COW,
remove/pop/shift, iteration, integer renumbering, and guarded position reads
retain their ordinary semantics across promotion.

Three optional entries fit without enlarging `ArrayStorage` or the 112-byte
`PhpArray`, so packed and larger hash arrays do not carry additional object
size. The common short result avoids its entries-vector and string-index heap
allocations, insertion hashes, and corresponding frees. Two independent
best-of-five runs measure 0.2404--0.2406 s RPHP versus 0.0785--0.0789 s PHP,
about 3.05--3.06x and roughly 14.5 percent below the validated-position result.
The complete release suite passes and the no-PGO matrix remains 30 strict wins
plus one timing-level tie.

Post-change sampling reduces short-array construction and destruction from
roughly 17--18 percent to about 6 percent. Object creation and destruction now
form the largest named runtime lifecycle block at approximately 20 percent
combined. The next no-JIT structural target is therefore declared-object
layout and allocation, while dynamic properties and magic behavior must keep
their canonical fallbacks.

Before opening that object-layout slice, the eighth corpus-era checkpoint uses
declared scalar types as an optimization contract. An isolated matrix showed
that identical typed functions and methods were paying a different, slower
frame protocol: strict typed function calls took approximately 0.274 s versus
0.122 s untyped, and typed methods took 0.215 s versus 0.116 s untyped.

A distinct `FastTypedScalar` ABI now represents fixed-arity all-`int`
parameters with an absent, `mixed`, or `int` return. It remains separate from
the original untyped `FastScalar`, preserving that ABI's metadata and
discriminants plus its dedicated baseline call/return path. Existing
compiler-proven Long plans, composed scalar calls, and monomorphic scalar
methods accept the typed ABI because their Long guards and
checked arithmetic already prove the declaration. Other scalar hints use
compact call/return checks, while failures and complex hints retain canonical
validation and errors. Hot and macro tiers check nested typed calls and typed
returns, including discarded results, before committing.

The final generic strict typed function is approximately 0.132 s, a 52 percent
reduction from the initial RPHP result. Typed scalar-plan functions and methods
are effectively equal to their untyped RPHP controls: 0.0448 s versus 0.0446 s
for functions and 0.0435 s for both method variants. They are respectively
about 47 percent and 16 percent faster than PHP 8.4 without JIT.

Two independent order-pipeline runs are 0.2344--0.2369 s RPHP versus
0.0781--0.0787 s PHP, slightly better than the preceding 0.2404--0.2406 s
checkpoint and therefore showing no tax on untyped real code. Both complete
release configurations pass, the focused type suite contains 72 tests, and
the no-PGO matrix remains 30 RPHP wins with one marginal 1.05x packed-array
build loss. Work can now return to declared-object layout and allocation with
the scalar type boundary ready for the later typed-region JIT.

The ninth corpus-era checkpoint carries that scalar contract across function
boundaries. Exact `int`, `string`, and `bool` facts now flow from immutable
declared parameters and statically resolved named returns through literals,
safe assignments, and exact-result operations. Conservative invalidation at
branches, defaults, aliases, references, globals/statics, and foreach keeps the
canonical interpreter authoritative.

The existing 16-byte instruction stores the fact in unused padding. Baseline
and hot execution consume specialized Long arithmetic/modulo/xor, string
concat/strlen, and scalar echo instructions without repeating Value-tag probes.
When a statically resolved argument already proves its callee hint, the call
site also skips only the redundant type guard; arity, error, overflow, and
fallback behavior remain unchanged. Unknown or overflow-capable values still
take the canonical checker.

The typed integer return-chain workload improves from approximately 0.318 s to
0.251 s and the typed string chain from 0.105 s to 0.092 s. Operation-derived
facts also improve equivalent untyped code, as intended. Typed code is not yet
uniformly faster than untyped code because validation boundaries and heap
string returns remain measurable, but its declarations now eliminate
downstream work and form reusable input for the future JIT.

Two independent order-pipeline runs are 0.2207--0.2273 s RPHP versus
0.0788--0.0806 s PHP, improving the preceding 0.2344--0.2369 s checkpoint.
Both release configurations pass, the type suite contains 80 tests, and the
31-case no-PGO matrix remains 30 RPHP wins with one marginal 1.03x array-build
loss. The next type-specific slice is guarded monomorphic method-return
propagation; the broader corpus priority remains declared-object layout and
allocation.

The next three type slices make those declarations useful across richer call
graphs. Statically known method return contracts now attach one receiver-class
dispatch guard to the call and propagate exact scalar results into all later
consumers. Conditional scalar bodies compile to guarded programs instead of
falling back merely because they contain branches. Composed scalar calls can
mix declared object receivers and Long arguments, so object identity is
guarded once while nested scalar leaves remain frame-free. Finally, immutable
typed string results can stay borrowed inside typed consumers: string length
and literal-concat length observations avoid materializing intermediate PHP
Values and frames. The typed string chain is approximately 0.015 s RPHP versus
0.026 s PHP, while typed string fanout falls to approximately 0.033 s and is
now close to PHP's 0.028 s.

The following corpus checkpoint extends the compact call ABI beyond scalars.
Fixed user calls with exact `array` and declared-class parameters no longer
enter the cold diagnostic/variadic path. Exact object facts propagated from
typed parameters can prove a nested argument at compile time; otherwise a
successful one- or two-class tuple is retained under stable class IDs and the
next monomorphic call needs only an integer guard. A new runtime class ID or a
failed type still executes the canonical hierarchy check and produces the
ordinary `TypeError`. Array returns use the same fast return boundary.

`$this` ownership is now independent of the scalar planner. Every synchronous
user method may borrow the receiver from its caller unless it directly returns
that slot; generator frames remain owning because they outlive the initiating
call. Declared objects also share their immutable class name with the class
layout instead of allocating an identical String per instance. An experimental
three-property inline object store was rejected after measurement: removing a
property-vector allocation enlarged every object enough to worsen cache
locality and regress this corpus. The unrestricted compact vector therefore
remains the better current representation.

To keep this work representative, the corpus now runs identical untyped and
typed order/service pipelines. Before the compact object ABI, the typed flow
was approximately 0.287--0.290 s RPHP versus 0.084--0.085 s PHP, compared with
approximately 0.225 s for untyped RPHP. After compact class/array validation,
borrowed `$this`, monomorphic class guards and shared class names, final
best-of-five runs are 0.219--0.224 s untyped and 0.228--0.230 s typed, versus
0.081--0.083 s and 0.083--0.085 s in PHP. Typed overhead in the real-code flow
is therefore about three to five percent and its RPHP/PHP ratio (2.72--2.74x)
is nearly the same as untyped (2.68--2.72x).
The separate 31-case no-PGO microbenchmark matrix has 29 RPHP wins and two
marginal losses at 1.03x and 1.06x; no corpus-only fused kernel was introduced.

The next corpus profile separated metadata work from irreducible allocation.
Every literal `new ClassName` site had already cached its constructor, but object
creation still hashed the same class name and rebuilt declared-property defaults
for every instance. Registered classes now own one resolved property template
and a stable class-ID index to boxed metadata. A warm monomorphic `new` therefore
does an integer lookup and clones the template; abstract/interface/enum checks
and the canonical name lookup remain on the first/cold resolution.

Synchronous by-value calls now use a compiler-proven borrowed heap-argument
mask. Parameters that are rebound, returned directly, used in try regions, or
mutate String/Array storage are excluded. Read-only Strings and Arrays and
ordinary Objects borrow the caller's Value without an Rc increment. If a nested
call exposes such a variable by reference, the VM first materializes an owned
slot and restores normal reference semantics. This extends the `$this` borrowing
model to application DTOs and immutable request data without weakening COW.
Finally, a hot frame that fully side-exits both the specialized executor and its
comparison resume is demoted to baseline. Unsupported heap/string bodies no
longer pay a failed tier-entry guard on every later call.

The untyped corpus consequently moved from approximately 0.219--0.224 s to
0.188--0.189 s and the typed corpus from 0.228--0.230 s to 0.200--0.202 s. Their
corresponding PHP runs were 0.079 s and 0.082--0.083 s, for ratios of
2.37--2.39x and 2.43--2.46x. The no-PGO
microbenchmark matrix remains at 29 RPHP wins out of 31; the two losses are the
already-marginal array-build and scalar-method cases.

The counters explain both the gain and the remaining gap. Object clones fell
from 3.0 million to 2.0 million, String clones from 3.0 million to 2.0 million,
cleanup fast-skips rose from effectively zero to 1.0 million, and scanned frame
slots fell from 21.0 million to 11.0 million. But the workload still pushes and
pops 2.0 million complete frames and zeroes 40 MB of CV storage. It also creates
500,000 request objects plus their property vectors and 500,000 short result
hash arrays. Sampling now places object/array allocation and destruction next
to baseline opcode/frame dispatch as the dominant named costs.

The next structural target is therefore a guarded application region capable
of spanning non-escaping DTO construction, nested monomorphic calls, declared
property reads, and short result extraction. The general win must come from
scalar replacement and frame elimination under side-exit guards—not from a
QuoteRequest/QuoteService-specific kernel. A one-allocation object/property
representation is also worth revisiting only if it avoids enlarging every
PhpObject header; the earlier inline-property experiment regressed cache locality.

The first frame-elision step toward that region is a small read-only
object/Long method program. It recognizes fixed-signature methods made only of
declared property reads, checked integer arithmetic/comparisons, forward
branches, local assignments, and a scalar return. Object arguments and typed
class declarations are validated at the call boundary. Each property read is
guarded by the canonical `FetchObjR` class/visibility cache; a cold cache,
different layout, reference, non-Long property, unsupported edge, or overflow
side-exits before observable state changes. This is a method-shape proof, not a
corpus class or method-name specialization.

Both typed and untyped `DiscountPolicy::rate()` now use that plan after their
property caches warm. Corpus frame pushes fall from 2.0 million to 1.5 million
and CV zeroing from 40 MB to 32 MB. Best-of-five wall time remains within noise
at approximately 0.187--0.188 s untyped and 0.199--0.200 s typed, versus about
0.079 s and 0.082--0.083 s in PHP. This proves that the policy frame is
removable, but also that it is not the dominant remaining cost. The next
general extension should cover immutable String guards and pure binary scalar
built-ins such as `intdiv`, which can remove the independent tax-policy frame;
only then should the measurements decide whether to compose the whole quote
region or return to allocation/layout work.

That String/binary-scalar extension now lowers literal equality guard chains
whose return arms are checked integer multiply+`intdiv` expressions. The
generic guarded program remains available for side exits and future lowering,
while the no-JIT executor uses a compact string-select representation instead
of interpreting the same PHP opcodes through a second dispatch loop. Calls
whose argument expressions contain an immediately-consumed, warmed declared
`FetchObjR` may borrow that property directly. The caller's class/read-safety
cache guards the layout and visibility; polymorphic, dynamic, magic, reference,
type-mismatched, and cold cases execute the original fetch/send/call sequence.

Consequently both `TaxPolicy::amount()` variants become frame-free even though
their region argument originates at `$request->region`. Full frame pushes fall
again from 1.5 million to 1.0 million, String clones from 2.0 million to 1.5
million, and the timed body executes 500,000 fewer `FetchObjR`, `SendVal`,
`DoFcall`, `Return`, multiplication, and direct-`intdiv` opcodes. Best-of-five
corpus time improves to approximately 0.184 s untyped and 0.195 s typed, versus
roughly 0.080 s and 0.083 s in PHP (about 2.30x and 2.36x). The remaining one
million complete frames are principally request construction and
`QuoteService::quote()`; the workload also still allocates 500,000 request
objects and 500,000 result arrays. The next measurement should therefore test
composition of the quote region and non-escaping request scalar replacement,
not add another leaf-only method kernel.

A subsequent DTO-constructor plan recognizes fixed-signature bodies that only
copy positional arguments into declared `$this` properties and return. At a
literal `new` site, the already-created object's exact class ID and each warmed
`AssignObjProp` write cache prove the destination slots. All arguments and type
hints are validated before any write; named arguments, references, dynamic or
magic properties, cache/layout changes, and unsupported constructor work retain
the ordinary frame. The direct path clones each source exactly once into the
new object's property vector and materializes the constructor's null result,
preserving ownership without the temporary argument/property clone cycle.

The request constructor therefore removes another 500,000 complete frames:
the corpus now pushes about 500,000 frames in total, executes only cold-start
constructor `SendVal`/`AssignObjProp`/`Return` opcodes, scans about 9.0 million
frame slots instead of 11.0 million, and performs 500,000 fewer Object clones.
Untyped time remains near the preceding checkpoint at roughly 0.183--0.187 s;
typed runs improve to about 0.191--0.192 s. The remaining repeated frame is now
`QuoteService::quote()` itself, which also accounts for the surviving 32 MB of
local-CV zeroing. That makes a guarded multi-call/multi-result quote region the
next frame target; request and result-array allocation remain separate scalar-
replacement/data-layout targets even after that frame is gone.

The following checkpoint adds that region without recognizing the corpus class
or method names. The compiler now proves a compact read-only object method that
reads warmed declared properties, composes monomorphic callees already backed
by `ObjectLongFunctionPlan`, performs checked Long arithmetic/`intdiv`, and
returns a small literal-key associative array. The plan keeps canonical
property and method cache positions as runtime guards. Argument declarations,
receiver classes, nested target identities, property layouts, references and
every checked arithmetic step are validated before the result array is
allocated. A mismatch therefore replays the untouched bytecode; dedicated
tests cover a polymorphic nested-method switch and an overflowed result edge.

On the order pipeline this removes the final repeated `QuoteService::quote()`
frame. VM statistics fall from about 500,000 pushed frames and 32 MB of local
zeroing to 9 cold/startup frames and 288 bytes, while the nested policy calls
remain accounted through the same scalar-call counters. Best-of-five no-PGO
times improve from approximately 0.183--0.187 s to 0.164 s untyped and from
0.191--0.192 s to 0.171 s typed, versus approximately 0.081 s and 0.084 s in
PHP (about 2.03x and 2.04x). The complete release suite passes and the separate
31-workload matrix remains at 29 strict RPHP wins; its two marginal losses are
array construction at about 1.09x and the standalone scalar-method case at
about 1.06x.

The profile has consequently crossed a useful boundary: full PHP frame
machinery is no longer a repeated corpus cost. The largest remaining costs are
now materialization boundaries -- one request object and one returned
three-entry array per iteration, plus the ownership writes/clones around them.
The next corpus slice should therefore scalar-replace the non-escaping request
and then let adjacent consumers read the region's three scalar outputs without
materializing the intermediate result array. Those are allocation/escape
analysis problems, not another call-dispatch kernel.

That scalar-replacement checkpoint is now implemented in both directions. The
caller analysis marks a small associative result only when it is assigned to a
local, immediately reduced through constant string keys into checked Long
consumers, and has no other use. A compatible `ObjectArrayFunctionPlan` can
then publish its scalar values directly: no result `PhpArray`, hash lookup, or
temporary array ownership traffic is needed. If a key, class, cache, value type,
or arithmetic check fails, execution resumes through the original bytecode
before any observable write.

The adjacent request object is virtualized under a similarly narrow escape
proof. A positional constructor backed by `PropertyInitMethodPlan` is mapped
to declared property slots, including declared defaults, and the composed
object-call plan reads those slots from temporary scalar storage. Class hints,
constructor types, method/property caches, visibility/read safety, and the
absence of `__destruct` remain runtime guards. Requests that escape, constructors
with additional work, references, dynamic or magic behavior, and polymorphic
edges retain canonical allocation and calls.

On the order pipeline, best-of-five no-PGO time improves from about 0.164 s to
0.115 s untyped and from about 0.171 s to 0.128 s typed, versus approximately
0.079 s and 0.084 s in PHP (about 1.45x and 1.53x). Hot result-array clones fall
to four process-wide cold/startup clones and request-object clones to eight;
only nine cold/startup frames remain. The full release suite passes, while the
independent 31-workload matrix still records 29 RPHP wins and only the same two
small losses (about 1.08x array construction and 1.06x scalar method).

The remaining corpus gap is consequently no longer primarily PHP object or
array materialization. It is the outer loop's general opcode/control dispatch
(`Mod`, branches, jumps, equality and accumulator writes), plus the remaining
string ownership boundary. The next systematic step should compose this
already-guarded virtual pipeline into a closed scalar loop region, so those
operations stay in native Rust locals across iterations while all existing
guards and canonical side exits remain reusable.

That complete loop composition is now selected through the general
`QuickLongOpsLoop` graph. Escape markers are established before loop planning,
and the typed vocabulary now admits checked literal/slot arithmetic,
less-than-or-equal branches, immutable string selection, and a composed
virtual-constructor/ObjectArray operation. Compiler temporaries produced inside
the region are distinguished from CVs that carry state across iterations, so a
conditionally produced TMP is not incorrectly required to contain a stale
entry value. Overflow and every unsupported edge still publish the retained
CV/TMP/string state and resume at the exact original opcode.

Loop-invariant constructor class/signature, method identity, declared-property
layout, argument representation, and receiver guards are resolved once at
region entry. Each iteration consequently keeps subtotal, level, accumulator,
branch, virtual-property and result values in the scalar slot file. Only the
nested value-dependent object plans and checked business arithmetic execute;
the request/result objects, their frames, caller opcode dispatch, and the
per-iteration region string clone are absent.

Best-of-five no-PGO corpus time falls from about 0.115 s to 0.080 s untyped and
from about 0.128 s to 0.085 s typed. On the same run PHP measures about 0.080 s
and 0.083 s respectively: the representative no-JIT application pipeline is
therefore at parity (approximately 0.99x and 1.02x), rather than 1.45--1.53x
behind. The 500,000-iteration statistics show one successful quick-region
entry, 499,967 quick iterations, zero guard failures/deoptimizations, only 46
String clones and 138 ordinary Value writes. The full release suite passes and
the independent matrix remains at 29 RPHP wins out of 31; the same isolated
array-build and scalar-method workloads remain the two small losses.

This is the intended decision boundary for the no-JIT architecture: a mixed
typed application region can now carry control flow, arithmetic, strings,
objects, composed calls, scalar replacement and precise side exits as one
guarded program. The next systematic work should validate this representation
against additional independent application corpora and consolidate duplicated
plan lowering/execution into the unified typed IR. Once that breadth is stable,
the minimal JIT can lower these already-proven regions instead of inventing a
second semantic optimizer.

The first independent validation corpus is now a stateful ledger rather than a
variant of the order pipeline. It keeps one aggregate object alive, computes a
conditional fee through `intdiv`, and passes that scalar result into a method
which mutates four declared Long properties. It therefore exercises a different
lifetime and data-flow shape: no transient request DTO and no result array in
the hot loop. Typed and untyped mirrors live beside the order corpora and the
corpus runner verifies all four workloads against PHP.

The shared scalar IR now represents direct `intdiv`, including a conditional
method whose two paths join at one trailing return. `QuickLongOpsLoop` can keep
the result of a guarded monomorphic scalar method in its slot file and can
represent literal Long assignments used to merge branch state. Method identity,
receiver class, property layout and property slots are resolved once at region
entry. Property mutations then read and write the already-proven Long slots in
place; checked operations remain transactional and any mismatch resumes at the
original PHP opcode before an observable partial update.

On the 500,000-iteration ledger this reduces pushed PHP frames from roughly
500,000 to seven process-wide cold/startup frames. The closed region records one
entry, 499,967 iterations, zero guard failures and zero deoptimizations. Ordinary
Value writes fall to 180 and Long clone/drop traffic to 167/166, instead of
millions of replacement Values. Stable no-PGO measurements put the untyped
ledger at about 0.0261 s versus 0.0249 s in PHP (approximately 1.05x) and the
typed ledger at about 0.0264 s versus 0.0261 s (approximately 1.01x). The order
corpora remain at parity, the full release suite passes, and the independent
31-workload matrix still records 29 RPHP wins.

This result supports the architecture rather than a benchmark-specific kernel:
the same scalar program, guarded method call and declared-property transaction
are selected from ordinary method bodies. The remaining ledger delta is now
small enough that the next work should consolidate scalar/property/object plan
execution into one typed-region representation and measure its dispatch cost,
instead of adding another corpus-shaped fast path.

The first consolidation checkpoint now separates semantic call ABI from
executable body shape. Direct scalar methods, declared-property getters and
property mutators share `QuickTypedMethodCall`: one receiver/method guard, one
eight-slot argument description, one resume point and one next target. Argument
materialization and exact side-exit publication are also shared. The executable
IR deliberately retains three body-specific variants, however, so a proven body
kind is not redispatched inside every hot iteration.

This distinction is performance-critical. A prototype with one dynamically
tagged method-body variant was semantically clean but slowed the 15-million-call
method-chain workload from roughly 0.094 s to 0.131 s. Keeping specialized
executable variants removed that branch, and passing the common argument payload
by reference removed an additional eight-operand copy. The retained design
measures about 0.0846 s on the same workload versus about 0.1525 s in PHP
(approximately 0.55x); application-like object dispatch measures about 0.0709 s
versus 0.0883 s (approximately 0.80x). The 31-workload matrix remains at 29
strict RPHP wins, while both application corpora retain their previous parity
and exact VM counters.

The architectural rule for subsequent consolidation is therefore explicit:
unify guards, operands, ownership publication and deoptimization contracts in
the semantic typed IR, then lower them to specialized executable forms before
entering the loop. The future JIT can consume the same semantic nodes without
forcing the no-JIT executor to pay a generic body-dispatch cost.

An intentionally unseen routing/admission holdout then tested whether this
architecture generalizes beyond the named corpus. Its loop combines typed
`int + string -> int` methods, short-circuit scalar control flow, changing
string hash keys, two existing-entry updates, and aggregate branches. The
initial implementation created 1.5 million frames and did not select a quick
region: ordinary release measured about 0.179 s and the first maximum-build
prototype about 0.153 s, versus about 0.0654 s in PHP (2.35x).

The resulting extension remains shape-based. `ObjectLongFunctionPlan` now
accepts immutable positional Strings, `strlen`, boolean TMP assignments and
pure scalar CFG methods without requiring an object/property input. General
quick regions carry mixed Long/String method arguments under the same warmed
receiver-class and method-cache identity guard as the established scalar
forms. Dynamic hash replacement is admitted only after an exact-key Long fetch
proves that the entry exists. A generic fetch/arithmetic/store superinstruction
then performs one mutable lookup on an array whose COW uniqueness is guarded at
region entry; structural writes such as `[]` push disable cached entry pointers.
Overflow, missing/non-Long values, references, COW aliases, changed receiver
classes, and plan mismatches retain canonical side exits.

The complete holdout loop now enters once, completes 749,967 quick iterations
with no guard failure or deoptimization, and reduces actual frame pushes from
1.5 million to six. The array superinstruction alone improves the ordinary
release path by roughly another seven percent. A same-batch eleven-run result
is 0.1048 s ordinary release, 0.0913 s maximum PGO build, and 0.0660 s PHP; the
machine-specific build is therefore about 1.38x behind PHP instead of 2.35x,
while the unseen RPHP workload is about 1.68x faster than the preceding maximum
build. Output is identical in all three modes.

The maximum-build prototype is deliberately separate from developer release:
fat LTO, one codegen unit, `target-cpu=native`, and matched-LLVM PGO trained on
60 `bench_*`/`corpus_*` workloads. The holdout filename is outside those globs,
so this measurement tests profile and runtime generalization rather than
training on the answer. The full release correctness suite passes, including
new planner, COW, structural-write, overflow and mixed-method regressions.

Sampling before the array fusion attributed about 27 percent of the remaining
holdout time to the two generic object/Long plan interpreters; most of the rest
was dispatch inside the general quick operation loop. The next no-JIT step is
therefore not another routing-specific kernel. It is a common lowering that
reduces typed method-plan and quick-operation dispatch while keeping the same
guards and side exits. This is also a concrete signal that the project is
approaching the typed-region JIT decision gate below.

That lowering has now passed its first decision cycle. Two tempting generic
ObjectLong fusion layouts were rejected after exact A/B builds: embedding large
fused payloads slowed the holdout by roughly five percent, while a side table or
compact fusion marker was neutral to twelve percent slower. Both forms made the
ordinary method dispatch representation costlier than the semantic operations
they removed. Canonical ObjectLong IR therefore stays compact.

The retained quick-loop lowering only combines proven materialization edges.
General checked arithmetic followed immediately by a CV assignment is one
`BinaryAssign` operation, including literals, subtraction and multiplication.
A typed method result whose canonical TMP is consumed by the following assign
is written directly to the destination CV; the dead TMP needs neither a larger
opcode nor a second dispatch. Exact same-layout A/B runs improve the unseen
holdout by about 11.6 percent and another five percent respectively.

Two high-level typed method plans remove the remaining branch-materialization
tax without changing canonical PHP bytecode. `ObjectLongModuloAnySelect`
recognizes a bounded short-circuit disjunction of checked modulo/equality terms
with two constant Long returns. `ObjectLongWeightedStringScore` recognizes a
checked weighted integer score with String length, mutually exclusive literal
adjustments and scalar threshold adjustments. They are selected solely from
types, operands and CFG edges; names and the holdout file never participate.
Every overflow, zero divisor, type mismatch or cache mismatch side-exits before
an observable result. The two plans improve the same workload by about 15--16
percent and 18 percent in isolated A/B runs.

After these changes all release tests pass. On nine interleaved 7.5-million
iteration runs with identical output, PHP 8.4.12 CLI with OPcache/JIT disabled
has a 0.6721 s median, ordinary RPHP release 0.7171 s, and the untrained
`max-perf` fat-LTO build 0.6298 s. The production-ceiling no-JIT build is thus
about 6.3 percent faster than PHP on this intentionally excluded holdout; the
ordinary developer release remains about 6.7 percent behind. The holdout has
moved from roughly 0.179 s per 750,000 iterations to about 0.063 s in the
maximum build, approximately 2.8x faster than the initial RPHP path.

PGO remains optional rather than intrinsically superior. The 60-workload
profile still excludes the holdout and predates coverage of the new high-level
plans. Its profile-use build records about 0.653 s against a same-batch 0.663 s
PHP median, but direct comparison consistently loses to the untrained
fat-LTO binary. A performance build should therefore publish both modes and
accept PGO only after an independent validation matrix confirms a net win.

Final sampling assigns about 81 percent of CPU to the general quick-loop
executor itself and about 15 percent to the two already-specialized method-plan
entries; hash lookup and key normalization are only a few percent combined.
The dominant remaining tax is therefore typed-region dispatch, not frames,
arrays, strings, or an unfused PHP operation. This satisfies the JIT decision
gate: further no-JIT work should be limited to independently recurring
superinstructions, while the main performance effort moves to lowering this
same guarded typed IR as a native region.

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

PHP JIT measurements must load the normal CLI configuration and explicitly
enable tracing JIT:

```sh
php -dopcache.enable_cli=1 -dopcache.jit_buffer_size=100M \
    -dopcache.jit=tracing benchmark.php
```

The no-JIT PHP control is run explicitly with
`php -dopcache.enable_cli=0 -dopcache.jit_buffer_size=0 -dopcache.jit=off`.
Do not use `php -n` for the PHP JIT lane: it is the isolated no-configuration
baseline used by the older no-JIT benchmark scripts, not the production-like
tracing-JIT invocation. Every JIT batch must first verify
`opcache_get_status(false)['jit']` reports both `enabled` and `on`.

Also track compilation latency, code-cache memory, runtime memory, and
deoptimization frequency.

Implementation prototype checkpoint (2026-08-03): the first native platform
boundary is proven on macOS ARM64 behind the opt-in `jit-prototype` feature.
RPHP now owns a minimal binary encoder for register `ADD`, `MUL`, and `RET`,
maps code through direct operating-system FFI, writes it while the page is RW,
invalidates the instruction cache, seals it RX, and calls it through the ARM64
C ABI. The 12-byte demonstration computes `(a + b) * c`; byte-encoding and
live-execution tests both pass without LLVM, Cranelift, DynASM, an assembler,
or a new package dependency.

This prototype is intentionally disconnected from PHP execution and does not
yet implement PHP overflow or deoptimization behavior. The next vertical slice
is a restricted `ScalarLongProgram` lowering with checked arithmetic and an
explicit success/side-exit result, still exercised outside the production VM
before a guarded quick region may call it.

Typed-program prototype checkpoint (2026-08-03): that restricted lowering is
now complete. Existing straight-line `ScalarLongFunctionPlan` instances with up
to eight inputs and eight operations lower directly to ARM64. Inputs and the
single transactional output use a small pointer ABI; temporaries stay in
caller-saved registers. `Add`, `Subtract`, and `Multiply` detect signed overflow
in native code and branch to one shared side exit before publishing the output;
`BitwiseXor`, arbitrary 64-bit constants, and prior temporaries use the same
lowering. Invalid temporary/input references, conditional plans, division, and
modulo are rejected before executable memory is created.

The demonstration now builds `(a + b) * c` as the real shared typed IR, emits
60 native bytes, returns `Value(36)` for `[7, 5, 3]`, and returns `SideExit` for
an overflowing input. Nine focused tests cover byte encoding, live ABI calls,
transactional overflow exits, validation, constants, and 40,000 deterministic
differential arithmetic cases. The next step is to add the remaining scalar
guards needed by representative plans, then attach a compiled native region to
an existing guarded quick-plan cache; native entry must happen outside the hot
per-operation dispatch loop, with the canonical resume point retained.

Native-leaf integration checkpoint (2026-08-03): `IntDivide` and `Modulo` now
lower through ARM64 `SDIV` and `MSUB`. Explicit native guards side-exit for a
zero divisor and `PHP_INT_MIN / -1`, matching the existing `checked_div` and
`checked_rem` typed contract; canonical execution therefore still owns PHP's
division error and the modulo result for the overflow corner case. The focused
matrix now covers all six `ScalarLongOpKind` variants and 60,000 deterministic
differential arithmetic inputs.

Each straight-line scalar function plan also owns a lazy native cache in the
prototype build. It remains interpreted for its first 63 evaluations, compiles
once at the 64th, and records native entries and side exits. One-operation
leaves deliberately stay on the existing inlined Rust path. A real PHP test
warms a two-operation typed function, proves subsequent native entry, then
forces overflow and observes the original canonical `TypeError` plus one native
side exit. Default and prototype release suites both pass.

The first isolated profitability check disables quick-loop fusion so it
measures the leaf boundary itself. Across seven interleaved runs, the
two-operation scalar-call workload improves from a 0.1710 s median to 0.1558 s
(about 8.9 percent), while the three-operation typed workload improves from
0.0969 s to 0.0794 s (about 18 percent), with identical results. This licenses
the cached leaf entry but does not replace the larger goal: quick loops already
compose such leaves, so the next native slice should lower a guarded whole
region with conditions and looping rather than reintroducing one native call
per inner operation.

Whole-region loop checkpoint (2026-08-03): the first guarded quick region now
executes as native ARM64 rather than calling native code once per scalar
operation. The initial shape is the general `$accumulator += $induction` loop
with an exclusive Long bound. Its compact ABI receives only a three-Long state
snapshot and a non-zero iteration budget. The generated block performs the
condition, checked accumulation, checked increment, backedge and state commit;
it returns distinct statuses for completion, budget exhaustion, sum overflow,
and increment overflow.

Execution is deliberately chunked at 32 iterations, matching the established
quick executor's interrupt cadence. Between chunks the VM can service its
interrupt flag. Completion publishes the final CV/TMP state and jumps to the
original exit. Sum overflow leaves the accumulator transactional and resumes
at `sum_ip`; increment overflow publishes the completed sum and resumes at
`increment_ip`. An invalid backend status discards the current chunk and
returns to the canonical header from the last known-good state. Compilation is
attached lazily to an already-hot `QuickLongAccumulateLoop`; unsupported term
shapes continue through the unchanged Rust quick executor.

Direct ABI tests cover multi-chunk progress, early completion, zero-budget
rejection and transactional overflow. Real PHP tests prove native entry for a
100,000-iteration loop and prove that a native sum overflow resumes bytecode
and produces the original typed-return `TypeError`. The focused ARM64 suite now
has 15 tests, and both the default and `jit-prototype` release matrices pass.

The first isolated region benchmark uses the existing 10-million-iteration
`$sum += $i` workload. Across seven interleaved runs with identical
`49999995000000` output, the existing Rust quick region has a 0.02030 s median
and the native region a 0.00571 s median: approximately 3.55x faster, or a 71.9
percent time reduction. Local PHP 8.4.12 CLI with OPcache/JIT disabled records
a 0.02518 s median on the same script, so this narrow RPHP native region is
about 4.41x faster than PHP without JIT.

Constant-term widening checkpoint (2026-08-03):
`QuickLongTerm::InductionPlusConst` now uses the same whole-region lowering.
The invariant addend is materialized once before the native backedge rather
than passed through the state ABI or reconstructed per iteration. The original
induction-only machine loop remains unchanged. The widened block performs a
separate checked term addition and returns `TermOverflow` before either the
term TMP or accumulator is published; the VM resumes exactly at the plan's
`term_ip`. Sum and increment exits retain their previous transaction boundary.

A two-call PHP test first warms `plusTwo(0, 100)` and then enters native code at
`PHP_INT_MAX - 1`, forces term overflow, replays the original term instruction,
and observes the canonical typed-return `TypeError`. Together with direct ABI
coverage and an ordinary 100,000-iteration PHP loop, the focused ARM64 suite
now contains 17 tests.

On the existing 50-million-iteration `bench_scalar_loop.php` workload, seven
interleaved runs produce a 0.09967 s median for the Rust quick region, 0.03520 s
for the widened native region, and 0.17060 s for local PHP 8.4.12 without JIT.
Native execution is therefore about 2.83x faster than RPHP's no-JIT region and
4.85x faster than PHP, with identical `1250000025000000` output. The next step
is to move this chunk/status ABI from these two handwritten recurrence shapes
into the more general typed loop IR.

General typed-loop IR checkpoint (2026-08-03): the first
`QuickLongOpsLoop` subset now lowers through the same native-region model. It
accepts a validated `<` loop header, an inner `<` condition, one checked
conditional accumulator add, and the fused checked post-increment/backedge.
Unlike the recurrence-specific three-Long ABI, this lowering addresses the
existing 64-slot unboxed Long state directly. Bound and cutoff operands are
loaded or materialized once before the native backedge, while induction and
accumulator stay in registers for each 32-iteration chunk.

The native block writes only induction and accumulator state. The VM maps its
status back through the original `QuickLongOpsLoop` metadata, reconstructs only
the TMP/CV outputs that were actually defined, maintains the Long/Bool dirty
masks, and resumes `add_resume_ip` or `post_resume_ip` on checked overflow. An
exact chunk-boundary completion is normalized to the exit before interrupt
handling. A never-taken inner add therefore does not fabricate its result TMP,
and an invalid or failed native chunk can be discarded at its last published
boundary.

Direct tests cover slot operands, chunks, skipped additions, completion,
transactional add overflow, and invalid aliasing. Real PHP tests prove native
entry, canonical overflow replay, a never-taken body, and exact chunk-boundary
completion. The focused ARM64 suite now contains 21 tests.

Seven interleaved runs of `bench_branch_loop.php` record medians of 0.02250 s
for the existing Rust quick-IR kernel, 0.00702 s for the native IR region, and
0.02842 s for local PHP 8.4.12 without JIT, all producing
`12499997500000`. This first general IR lowering is about 3.21x faster than
RPHP without JIT and 4.05x faster than PHP. The next useful IR operation is
the modulo/equality condition already represented by the same conditional
kernel, followed by general straight-line checked `BinaryAssign` bodies.

Modulo/equality widening checkpoint (2026-08-03): the general native region
now also accepts `(induction % constant) == invariant`, including a slot-based
invariant. The ARM64 block materializes the divisor and comparison operand once,
computes each remainder with `SDIV` plus `MSUB`, and branches directly around
the checked accumulator update. This is the same existing `ModConst` and
`ConditionalAddAssign(Eq)` typed IR; no benchmark-specific source pattern was
introduced.

The native ABI now returns a per-chunk `addition_executed` bit alongside its
status. That lets the VM publish the conditional add TMP only when native code
actually defined it, including alternating modulo branches and never-taken
bodies. On completion, interrupt, add overflow, or increment overflow, the VM
reconstructs the exact last remainder and equality TMP from the original IR
metadata. A separate condition side exit preserves the pre-operation state for
a zero divisor and `PHP_INT_MIN % -1`, then resumes the canonical modulo
instruction. Direct tests exercise both guards and cached reuse; real PHP tests
cover a slot equality operand, native add overflow, normal completion, and the
canonical `MIN % -1` prologue. The focused ARM64 suite now contains 25 tests.

The original seven-run `max-perf`, native-CPU RPHP measurements of
`bench_modulo_branch_loop.php` produced a 0.00646 s JIT median and 0.02268 s
without JIT. The corrected same-batch PHP invocation records approximately
0.01171 s with tracing JIT and 0.03596 s without JIT, rather than the invalid
0.03567 s value previously labelled as PHP JIT.

An independent eight-run, order-rotated rerun of all four modes produces
identical `24999995000000` output and medians of approximately 0.00686 s RPHP
JIT, 0.01136 s PHP tracing JIT, 0.02203 s RPHP without JIT, and 0.03520 s PHP
without JIT. On this admitted region, native RPHP is therefore about 3.21x
faster than its Rust quick executor and 1.66x faster than PHP's tracing JIT in
the same batch. The first process run remains a visible cold outlier, so
compilation and cold-start latency must continue to be tracked separately from
warm medians. The next structural operation remains general straight-line
checked `BinaryAssign` bodies; expanding coverage is more valuable than further
tuning this modulo loop.

Straight-body widening checkpoint (2026-08-03): a second general
`QuickLongOpsLoop` native shape now accepts a linear body of up to eight
existing `ModConst` and checked `BinaryAssign` nodes. `BinaryAssign` supports
addition, subtraction, and multiplication with slot or literal operands. The
detector validates the ordinary `<` header, every linear successor, the fused
post-increment/backedge, immutable bound state, output aliasing, and the exact
operation capacity. It is independent of source names and constants.

Each successful native operation writes its TMP/CV outputs into the private
64-Long shadow state in program order; PHP values are still committed only at
an interrupt, completion, or side exit. A failed operation writes neither its
result nor destination and returns its body-operation index through the native
control ABI. The VM maps that index to the corresponding `resume_ip`, commits
only prior successful outputs, and replays exactly the failing PHP instruction.
The wrapper snapshots only the finite set of mutable shadow slots, so an
invalid backend status can discard its current chunk without copying the full
slot file. Modulo zero, `PHP_INT_MIN % -1`, add/subtract/multiply overflow,
empty loops, prefix commits, chunk boundaries, dynamic bounds, and real PHP
fallback are covered. The focused ARM64 suite now contains 29 tests.

Eight order-rotated `max-perf`, native-CPU runs of the new
`bench_binary_assign_loop.php` produce identical `419,729999927,1` output. The
corrected medians are approximately 0.01273 s for RPHP with the prototype JIT,
0.10016 s for RPHP without JIT, 0.03261 s for PHP 8.4.12 with tracing JIT, and
0.07348 s for PHP without JIT. The native region is therefore about 7.87x
faster than RPHP's general Rust executor and 2.56x faster than PHP's tracing JIT
on this four-operation body. PHP's own tracing JIT improves its no-JIT result by
about 2.25x, confirming that the former near-identical PHP figures came from an
incorrect JIT invocation.

Scalar-expression widening checkpoint (2026-08-03): the same straight native
region now accepts checked arithmetic whose result remains in a TMP and feeds a
later operation. Existing `Binary`, `Add`, and `AddAssign` nodes lower directly;
the existing two-add `AddAddAssign` fusion expands into two ordered native
operations. This removes the former requirement that every arithmetic result
be immediately materialized into a CV. Detection still depends only on typed
IR, control-flow edges, operands, and the eight-operation capacity.

Each expanded native operation retains its own canonical resume IP. The control
ABI's failed-operation index therefore commits only successful TMP/CV prefixes
and resumes the exact arithmetic instruction that overflowed. Direct tests
cover a three-operation TMP chain and a second-operation overflow after a
successful unpublished prefix. A real PHP test combines literal/TMP arithmetic
with a CV-only two-add chain and proves that both enter the same native region.
The focused ARM64 suite now contains 31 tests, and both full default and
`jit-prototype` release matrices pass.

Ten order-rotated `max-perf`, native-CPU runs of the previously unseen
`bench_scalar_expression_chain_loop.php` produce identical
`729999940,10000004` output. The medians are approximately 0.01376 s for the
widened RPHP JIT, 0.10712 s for the preceding RPHP JIT binary that rejected the
shape, 0.10696 s for current RPHP without JIT, 0.03863 s for PHP tracing JIT,
and 0.06251 s for PHP without JIT. The new region is therefore about 7.79x
faster than the previous fallback and 2.81x faster than PHP tracing JIT on this
shape. The first native run is a visible 0.02892 s cold outlier. An eight-run
A/B check of the already-supported `bench_binary_assign_loop.php` records
0.01237 s for the widened binary and 0.01260 s for its predecessor, showing no
regression in the original materialized shape within normal run variance.

Guarded scalar-method composition checkpoint (2026-08-03): a direct typed
method call can now be composed into the same native loop instead of returning
to the Rust quick executor once per iteration. Admission reuses the existing
object tag, receiver-class, warmed method-cache, exact function identity, and
arity guards. The first deliberately narrow shape accepts direct CV or Long
constant arguments and a straight `ScalarLongFunctionPlan`; a nested method
call, branch, unsupported argument source, or changed target remains on the
authoritative quick/canonical path.

Lowering consumes the existing typed scalar plan rather than matching source
names or benchmark constants. Runtime invariant CV arguments live in the
native slot state, while induction and accumulator CVs stay live across the
region. The straight ARM64 emitter now covers checked add, subtract, multiply,
integer divide, modulo, bitwise XOR, and an explicit move operation. Division
by zero, `PHP_INT_MIN / -1`, arithmetic overflow, method-body failure, sum
failure, and increment overflow all retain distinct side exits. A method-body
exit resumes at its canonical initializer; a later sum or increment exit also
publishes every already-successful PHP-visible prefix.

Tests exercise a real 100,000-iteration method loop, exact-target rejection for
a second class with the same method name, method-body overflow, caller-sum
overflow, and the widened division/modulo/XOR/move emitter. The focused ARM64
suite now contains 36 tests, and both full default and `jit-prototype` release
matrices pass.

Ten order-rotated `max-perf`, native-CPU runs of the new
`bench_scalar_method_native_loop.php` produce identical
`3649999705000000` output. Medians are approximately 0.02018 s for the widened
RPHP JIT, 0.10013 s for the preceding RPHP JIT binary that rejected this shape,
0.11733 s for current RPHP without JIT, 0.04898 s for PHP 8.4.12 tracing JIT,
and 0.12138 s for PHP without JIT. The composed region is therefore about 4.96x
faster than the previous RPHP JIT fallback, 5.81x faster than RPHP without JIT,
and 2.43x faster than PHP tracing JIT on this admitted direct-method shape. The
first native measurement is a modest 0.02354 s cold outlier.

Nested scalar-call flattening checkpoint (2026-08-03): the same region builder
now recursively consumes compiler-proven Init/Send/DoFcall trees and emits one
linear native scalar program. Each call's direct CV/Long-constant arguments,
single checked caller-side arithmetic argument, straight scalar body, and
output feed the next call without constructing an intermediate PHP call frame.
Invariant caller CVs are assigned stable runtime slots and shared across the
tree; native temporaries use separate dynamically allocated slots.

The compiled cache stores the exact ordered identity list for all inner and
outer targets, not a hash or only the root method. A changed inner receiver
class therefore cannot reuse code compiled for a structurally identical outer
call. Any arithmetic failure inside the tree resumes the root initializer and
replays the pure call tree canonically; failure after the final tree result
resumes only the caller sum. Successful profiling counts are recorded for every
target, including repeated targets, in the same post-order as the established
Rust recursive evaluator.

New regressions cover `add(mul(...))`, checked caller-side arithmetic feeding a
nested tree, a changed inner target under a stable outer target, and overflow
inside the nested method followed by canonical root replay. Together with the
direct-method cases, the focused ARM64 suite now contains 40 tests. Both full
default and `jit-prototype` release matrices pass.

Ten order-rotated `max-perf`, native-CPU runs of the existing
`bench_scalar_method.php` retain identical `37499992500000` output. Medians are
approximately 0.00951 s for the nested RPHP JIT, 0.11157 s for the preceding
direct-method JIT that rejected the tree, 0.10195 s for RPHP without JIT,
0.04641 s for PHP 8.4.12 tracing JIT, and 0.09360 s for PHP without JIT. The
flattened tree is therefore about 11.73x faster than the preceding fallback,
10.72x faster than RPHP without JIT, and 4.88x faster than PHP tracing JIT on
this admitted two-method shape. The first native run is a 0.01485 s cold
outlier.

A separate ten-run alternating A/B of the already-admitted direct-method
benchmark records approximately 0.01784 s for the nested-capable build and
0.01991 s for its direct-only predecessor. The generalized target-list guard
and dynamic slot allocator therefore do not regress the original direct shape.

Conditional scalar-call checkpoint (2026-08-03): `ScalarLongFunctionPlan`
selects now lower into the same native method-call tree. The straight native IR
has forward-only `BranchUnless` and `Jump` operations, signed `==`, `!=`, `<`,
and `<=` predicates, and the existing predicate-only bitwise-AND form. True and
false arithmetic remain separate control-flow ranges and write one join slot,
so an overflow or invalid division in the inactive arm is never evaluated.

Validation rejects backward or out-of-range native targets and verifies that a
false edge cannot read a true-edge temporary. A failure in the selected shared
or arm operation uses the existing root-call side exit; exact receiver and
method identities remain guarded before native entry. Tests cover all four
predicate encodings, masked parity, nested `outer(conditionalInner(...))`, an
inactive overflowing arm, selected-arm canonical replay, and a polymorphic
conditional receiver. The focused ARM64 suite now contains 46 tests, and both
full release matrices pass.

The larger control-flow operation representation exposed a separate dispatch
cost: method configuration and the exact target list were being copied and
compared at every 32-iteration chunk. Region preparation now performs those
guards once per activation and hands the chunk loop a stable compiled-program
reference. The same prepare/call split is used by the general straight-loop
cache. This is safe because receiver CVs are already invariant and method
identity is immutable during one pure scalar region activation.

Ten order-rotated `max-perf`, native-CPU runs of the new
`bench_scalar_method_branch.php` produce identical `199999980000000` output.
Medians are approximately 0.01443 s for the conditional RPHP JIT, 0.15425 s for
the preceding JIT that rejected the select, 0.13882 s for RPHP without JIT,
0.05727 s for PHP 8.4.12 tracing JIT, and 0.13100 s for PHP without JIT. The new
region is therefore about 10.69x faster than the preceding fallback, 9.62x
faster than RPHP without JIT, and 3.97x faster than PHP tracing JIT on this
masked two-arm method.

Ten-run alternating regression checks also show the prepared dispatch
improving existing admitted methods: the direct-method median moves from about
0.01904 s to 0.01363 s, and the nested-method median from about 0.00950 s to
0.00696 s. The unrelated general `bench_binary_assign_loop.php` region remains
effectively flat at 0.01224 s versus 0.01219 s, within run variance.

Standalone conditional-call checkpoint (2026-08-03): the same structured
`ScalarLongFunctionPlan` select now lowers through the per-plan hot-call cache,
not only when a surrounding loop can absorb it. The native leaf emits the
shared arithmetic prefix, predicate, selected true or false operation range,
and one transactional output. All four signed comparison forms and the
predicate-only bitwise mask share the region encoder. The inactive arm is not
executed; overflow or an invalid division in the selected arm returns the
existing `SideExit` status and lets the canonical call produce PHP's result or
error.

Validation independently proves shared/true/false operation boundaries,
predicate temporary availability, and that false-edge operations and outputs
cannot read a true-edge temporary. Hot conditional plans enter native code on
their 64th call. Straight one-operation leaves retain their specialized Rust
path, while even a zero-arithmetic conditional leaf is admitted: an isolated
10-million-call check measured about 0.20892 s with the native select versus
0.22500 s without JIT, a small but repeatable gain. Direct ABI, inactive-arm,
selected-overflow, cache-hotness, unrolled real-call, and deliberately
uncomposed-loop tests bring the focused ARM64 suite to 52 passing tests.

The new `bench_scalar_call_branch_standalone.php` contains a strict-comparison
and unreachable echo edge specifically to keep its loop outside native region
composition while retaining ordinary statically resolved calls. Ten
order-rotated `max-perf` runs produce identical `199999980000000` output and
medians of approximately 0.21464 s for the new RPHP call JIT, 0.32185 s for the
preceding JIT that rejected selects, 0.26959 s for current RPHP without JIT,
0.04832 s for PHP 8.4.12 tracing JIT, and 0.12429 s for PHP without JIT. The
new leaf is therefore about 1.50x faster than the previous RPHP fallback and
1.26x faster than current RPHP without JIT, but remains about 4.44x slower than
PHP tracing JIT. A separate straight two-operation call A/B improves from
about 0.23114 s to 0.21058 s, so the shared emitter refactor does not regress
the already-supported leaf.

This comparison locates the next bottleneck more precisely. Once `route()` is
native, RPHP still executes the surrounding VM loop backedge, strict condition,
call-site scan, argument capture, native ABI transition, result publication,
and accumulator op ten million times. PHP tracing JIT keeps that caller state
inside one trace. The next widening should therefore join a hot caller and its
typed scalar callee across an otherwise unsupported control-flow edge, rather
than micro-tuning the arithmetic leaf or adding another source-shaped loop
kernel.

Unified scalar-call region checkpoint (2026-08-03): direct scalar functions
now enter the same whole-loop native call region as monomorphic scalar methods.
The recursive region builder already understood both `InitFcall` and
`InitMethodCall`; only its outer admission gate and method-specific cache naming
excluded the function term. The cache, prepared dispatch, slot ABI, exact
target-identity list and root-call resume state are now explicitly shared
`call` concepts, preventing later widening from accidentally selecting only
one dispatch form.

Simple function calls retain their compact `ScalarFunctionCall` argument plan
and fused no-JIT executor. A genuinely nested function/function or
function/method expression is recognized separately as a generic
`ScalarCallTree`, so adding tree coverage does not slow the common leaf. The
tree detector records all Long and object input guards, the native builder
recursively lowers checked caller argument expressions and every proven scalar
callee, and an inner arithmetic failure resumes the original root initializer.
Tests cover a direct function region, `addNative($i + 1,
mulNative($i, 2))`, a mixed function/method tree as one native region, and
canonical root replay after an inner function overflow. The focused ARM64
suite now contains 56 passing tests.

Eight order-rotated `max-perf`, native-CPU runs of
`bench_scalar_call_loop.php` produce identical `100000000000000` output and
medians of approximately 0.01330 s for RPHP JIT, 0.04886 s for RPHP without
JIT, 0.03325 s for PHP tracing JIT, and 0.07560 s for PHP without JIT. The
whole-call region is therefore about 3.67x faster than the Rust quick executor
and 2.50x faster than PHP tracing JIT. Its first native run is a visible
0.02786 s cold outlier; the following measurements stay near 0.0131--0.0134 s.

The new `bench_scalar_function_tree.php` independently exercises two nested
functions plus a checked caller-side argument expression. Eight matched runs
produce identical `37499997500000` output and medians of approximately
0.00776 s RPHP JIT, 0.10382 s RPHP without JIT, 0.03775 s PHP tracing JIT, and
0.07632 s PHP without JIT. Flattening the entire guarded tree is about 13.38x
faster than RPHP's no-JIT recursive quick path and 4.87x faster than PHP tracing
JIT on this admitted shape.

Cold-edge trace-guard checkpoint (2026-08-03): an uncommon arbitrary PHP block
no longer has to invalidate an otherwise pure scalar caller/callee region. The
planner recognizes a strict Long comparison whose forward conditional jump
lands on the loop increment and skips at least one cold instruction. It records
the original operands, expected hot result, condition TMP, and comparison
resume IP as a `QuickLongTraceGuard`; the skipped range stays canonical and may
contain effects such as output.

The shared native straight IR now has a general `Guard` operation over all four
existing scalar condition kinds and either expected Boolean result. A mismatch
uses the ordinary indexed operation side-exit ABI. In a call-accumulate region,
the call and checked sum precede the guard while increment follows it, so the
VM publishes the successful call/term/sum prefix, leaves induction unchanged,
and replays the original comparison and complete cold block. The Rust quick
executor uses the same transaction boundary. Successful completion, interrupt,
and increment-overflow paths publish the expected condition TMP; exact call
accounting includes the current call when a later guard exits.

Direct native tests prove that a guard preserves prior shadow outputs and exits
before increment. PHP tests cover a never-taken `=== -1` edge and a dynamically
taken `$i === $needle` edge: the latter executes its canonical echo once,
continues the loop with the correct sum, and records exactly one native side
exit. Together with planner coverage, the focused ARM64 suite now contains 58
passing tests.

Ten order-rotated, native-CPU runs of the former standalone holdout
`bench_scalar_call_branch_standalone.php` produce identical
`199999980000000` output in all six compared modes. Medians are approximately
0.01386 s for guarded RPHP JIT, 0.15436 s for guarded RPHP without JIT,
0.24506 s for the preceding RPHP JIT, 0.30967 s for the preceding RPHP no-JIT
binary, 0.04975 s for PHP tracing JIT, and 0.12603 s for PHP without JIT. The
joined native region is about 17.69x faster than its preceding JIT fallback and
3.59x faster than PHP tracing JIT. The Rust quick path improves about 2.01x but
remains roughly 1.22x slower than PHP without JIT, locating its remaining gap
in typed-plan execution rather than call frames.

Eight-run alternating regressions show no cost on regions without a trace
guard. The direct scalar-call median moves from about 0.01324 s to 0.01287 s,
and the nested function tree from 0.00779 s to 0.00747 s; both differences are
normal favorable variance with identical output.

General typed trace-guard checkpoint (2026-08-03): `QuickLongOps` now carries
the same exact cold-edge contract instead of requiring every instruction in a
closed mixed loop to have a typed implementation. A new `TraceGuard` accepts a
strict Long comparison over guarded CV/TMP/literal operands, selects the
forward conditional edge as the speculative hot edge, and leaves every
skipped instruction in canonical bytecode. Other typed control-flow targets
must still resolve to admitted operations, so no reachable fast edge can enter
the skipped range accidentally.

The general Rust executor commits dirty Long and Boolean slots, redirected
string state, direct unique-array updates, object effects, and exact call
accounting before resuming the original comparison. This was tested with one
region containing a monomorphic method with a borrowed String argument,
dynamic string-key hash replacement, an internal routing branch, and a taken
cold `echo`. The update preceding the guard is visible exactly once and the
loop safely re-enters its typed region afterward.

Straight scalar `QuickLongOps` lowers the same operation to the shared native
`Guard` ABI. Per-operation metadata records the original resume IP plus the
condition TMP and expected value. On a side exit, only guards completed before
the failed operation are materialized for the current iteration; successful
completion, interrupt, or increment overflow publishes all completed guard
conditions. Publication occurs only when control returns to the VM, avoiding
per-chunk overhead on existing guard-free native loops. A taken-edge test uses
two loop-carried variables and compares the just-updated second value, proving
that prior native outputs are committed before canonical replay.

Eight order-rotated `max-perf`, native-CPU runs of the new
`bench_general_trace_guard_loop.php` produce identical
`49999995000000:10000000` output. Medians are approximately 0.01066 s for RPHP
JIT, 0.07446 s for RPHP without JIT, 0.29012 s for the preceding RPHP JIT,
0.26121 s for the preceding RPHP no-JIT binary, 0.03070 s for PHP tracing JIT,
and 0.05926 s for PHP without JIT. The general native region is about 27.22x
faster than its preceding fallback and 2.88x faster than PHP tracing JIT. The
Rust typed executor improves about 3.51x, although it remains approximately
1.26x slower than PHP without JIT on this all-scalar shape.

The independent `bench_mixed_trace_guard_loop.php` exercises the mixed path.
Its eight-run medians are about 0.02920 s RPHP JIT, 0.02764 s RPHP without JIT,
0.10269 s preceding RPHP JIT, 0.10366 s preceding RPHP no-JIT, 0.00966 s PHP
tracing JIT, and 0.02868 s PHP without JIT, all with identical
`250002000000:250002000000` output. General admission therefore improves the
no-JIT mixed path about 3.75x and places it slightly ahead of PHP without JIT.
The RPHP JIT build still executes this object/string/hash vocabulary in Rust
and remains roughly 3.02x behind PHP tracing JIT, making native mixed-operation
lowering the next concrete coverage gap.

Twelve alternating regressions keep the existing guard-free binary loop flat
at about 0.01393 s versus 0.01389 s. The specialized scalar-call trace region
also remains flat at 0.01394 s versus 0.01391 s. Planner, mixed no-JIT, native
updated-value side-exit, and direct native guard tests cover both executors.

Direct typed String-input checkpoint (2026-08-03): the shared composed typed
IR can now represent a borrowed public String argument directly, rather than
requiring every String value to originate in a nested user call. A method such
as `int + strlen(string)` therefore carries one general typed program with
separate Long and String argument masks. The warmed receiver class and method
cache are still checked once at mixed-region entry; no method identity or
unchecked heap pointer is embedded in the program.

The first runtime implementation exposed an important executor constraint:
reusing the general nested-call evaluator allocated several large temporary
arrays per iteration and regressed no-JIT performance. The final direct-method
executor instead stores borrowed String lengths in the same eight scalar input
lanes and uses one compact operation-temporary array. It supports checked Long
arithmetic, String length and literal-length composition without constructing
a PHP frame or a 64-slot object-plan frame. Programs containing nested calls
retain the established guarded evaluator and are not selected by this direct
method path.

Twenty-one sequential native-CPU `max-perf` measurements of
`bench_mixed_trace_guard_loop.php` produce identical output and medians of
approximately 0.02426 s for RPHP JIT and 0.02526 s for RPHP without JIT,
compared with 0.02840 s and 0.02726 s for the preceding binaries. The general
typed method input therefore improves the mixed workload about 14.6 percent
in the JIT build and 7.3 percent without JIT. PHP tracing JIT remains near
0.00951 s, confirming that the majority of the remaining gap is the enclosing
String-state and dynamic-hash region rather than the now-compacted method
frame. Compiler, mixed-region, 109 core, 108 type-hint, 107 quick-loop and 59
ARM64 prototype tests cover the checkpoint.

Finite String/hash native checkpoint (2026-08-03): admitted mixed regions can
now keep a finite immutable String state as small activation-validated tokens.
Generated ARM64 selects compile-time String lengths from those tokens and uses
them to inline direct call-free typed methods. Dynamic hash accesses select
pre-resolved runtime entry pointers from a per-activation context table; the
generated code does not embed borrowed heap addresses. Admission still
requires a unique COW array, an existing Long entry for every finite key, a
warmed receiver class/method cache, and the same guarded typed method target.
Structural array mutation and unrestricted String contents retain the general
executor fallback.

The whole accepted loop is one native region: String assignment, typed method
arithmetic, hash load/update/store, scalar control flow, and trace guards.
Visible PHP slots are separated from private native shadow slots. Before each
chunk, mutable shadow values and entry payloads are snapshotted so a failing
operation can restore only the current chunk and resume at the exact canonical
PHP operation. Stores completed before a later cold trace guard remain visible
exactly once. Completed calls and guard condition temporaries are likewise
published according to the failed native operation rather than approximately
per chunk.

Twenty-one sequential native-CPU `max-perf` measurements of
`bench_mixed_trace_guard_loop.php` produce identical
`250002000000:250002000000` output and medians of approximately 0.00280 s for
the new RPHP JIT, 0.02550 s for the preceding direct-typed-input RPHP JIT,
0.00979 s for PHP tracing JIT, and 0.02942 s for PHP without JIT. This admitted
mixed region is about 9.1x faster than its preceding RPHP path and 3.5x faster
than PHP tracing JIT. Fifteen-run regression medians remain approximately flat:
the general scalar trace loop is 0.01032 s versus 0.01022 s, the scalar-call
loop is 0.01226 s versus 0.01252 s, and the unadmitted routing holdout is
0.06442 s versus 0.06608 s. The checkpoint passes 109 core, 108 type-hint, 107
quick-loop, and 62 ARM64/JIT tests, including a taken cold edge after an array
store and direct context validation.

The mixed routing holdout remains intentionally outside this first native
mixed shape. Its loop carries three String routes, updates two hash arrays, and
performs two object calls; one method contains `intdiv` plus internal branches
and the second contains compound branch logic. The finite String and existing
entry representation is sufficient for its data, but the current native method
lowerer deliberately admits only one direct call-free composed typed body.
Current and preceding RPHP JIT medians remain approximately flat at 0.06442 s
and 0.06608 s; the earlier PHP tracing-JIT median was 0.02575 s. This separates
the next coverage gap from the data representation already solved here: widen
the shared native region to multiple monomorphic calls, internal typed-method
control flow, and selected scalar builtins without weakening exact side exits.

Multi-method mixed-region checkpoint (2026-08-03): the same native builder now
lowers the compiler-proven `ObjectLongWeightedStringScore` and
`ObjectLongModuloAnySelect` plans. These are general semantic IR shapes, not
benchmark-name checks: the first covers checked weighted arithmetic,
`intdiv`, finite String adjustments and scalar threshold adjustments; the
second covers short-circuit modulo/equality predicates with two Long return
arms. Both `ObjectLongMethodCall` and all-Long `ScalarMethodCall` adapters can
participate, so one region may contain several independently guarded
monomorphic calls. Ordinary `Assign`, literal assignment and `BinaryAssign`
around those calls now lower through the shared native operations as well.

The region limit grows from 12 to 48 native operations, while mutable snapshots
remain bounded by the actual 64-slot shadow namespace rather than by operation
capacity. Large kernels are borrowed by the execution adapter instead of being
copied. Compiled programs also precompute a 16-bit required-context mask, so
ordinary scalar chunks no longer scan their operation list to prove that no
hash context is needed. Callee String literals are related to the caller's
finite token catalog by guarded contents during lowering; generated code still
contains only the resulting token and never a callee or heap pointer.

The previously unseen 28-operation routing holdout is now one native region
covering three String routes, two objects/methods, two dynamic hash arrays,
internal method control flow and `intdiv`. Twenty-one pre-throttling
native-CPU `max-perf` runs preserve
`290394364,154183816,54660174,384960,192495,64134,108411` and give medians of
approximately 0.00800 s for the new RPHP JIT, 0.06227 s for the preceding RPHP
JIT, 0.02634 s for PHP tracing JIT and 0.06525 s for PHP without JIT. The new
path is therefore about 7.8x faster than the preceding RPHP path and 3.3x
faster than PHP tracing JIT on this independent application shape. The earlier
mixed native benchmark remains near 0.0026-0.0027 s and the scalar-call loop
near 0.0123-0.0126 s. A 100-million-iteration scalar trace check measures
approximately 0.357 s for the widened build versus 0.381 s preceding it,
confirming that the larger admission envelope does not add a per-iteration
cost to existing generated code. The checkpoint passes 109 core, 108
type-hint, 107 quick-loop and 63 ARM64/JIT tests; its end-to-end holdout test
requires one native entry, multiple chunks and zero side exits.

Application-corpus native checkpoint (2026-08-03): the shared mixed-region
builder now lowers both remaining corpus boundaries without class, method or
benchmark-name recognition. A non-escaping constructor plus ObjectArray
method pipeline is scalar-replaced through nested monomorphic calls, checked
arithmetic, finite String selection, result-entry selection and its immediate
Long consumers. No request object or result array is materialized in the hot
loop. Constructor, outer method and nested method identities remain guarded,
and every call is accounted only after the complete virtual transaction has
succeeded.

The same builder also accepts ordinary scalar method plans followed by a
declared-property mutator. Long properties are activation-resolved and seeded
into private native shadow slots; the compiled program contains neither a PHP
object pointer nor a property address. All mutations in one method are
calculated first and published to persistent property shadows only through
non-failing moves. Completion, interrupt and exact side-exit paths then commit
those shadows to the current activation's guarded object. An overflow after an
earlier property update therefore replays the canonical method once with no
partial or duplicate mutation. Add, subtract, set, min and max property plans
share this transactional lowering.

Twenty-five order-rotated native-CPU `max-perf` before/after runs preserve the
complete output. The untyped order pipeline falls from approximately 0.07957 s
to 0.00459 s (17.3x) and its typed counterpart from 0.09413 s to 0.00498 s
(18.9x). The subsequent property slice moves the untyped ledger from 0.02021 s
to 0.00254 s (8.0x) and the typed ledger from 0.02016 s to 0.00257 s (7.8x),
while both already-native order variants remain flat around 0.00446--0.00449 s.
A separate 21-run four-mode batch, after verifying PHP reports tracing JIT as
`enabled:on`, records RPHP JIT/PHP tracing-JIT medians of 0.00535/0.05960 s,
0.00533/0.05788 s, 0.00275/0.01066 s and 0.00387/0.01255 s for untyped order,
typed order, untyped ledger and typed ledger respectively. RPHP without JIT
remains slightly ahead of PHP without JIT in untyped order and both ledger
workloads; typed order is approximately 3.9 percent slower. The checkpoint
passes 109 core, 108 type-hint, 107 quick-loop and 67
ARM64/JIT tests, including both complete corpus variants, rebinding one cached
program to distinct activation objects, and a deliberate mid-method property
overflow with exactly one native side exit.

Power-of-two modulo lowering checkpoint (2026-08-04): all three ARM64 integer
remainder emitters now recognize a positive or negative constant whose
magnitude is a power of two. Scalar functions, general straight regions and
the conditional loop region avoid `SDIV`. The shared signed lowering applies a
sign bias around the bit mask, preserving truncating PHP results such as
`-3 % 2 == -1`, and handles `PHP_INT_MIN % -1 == 0` and a divisor of
`PHP_INT_MIN` without a native side exit. A zero-remainder conditional such as
`($i % 2) == 0` lowers further to one hoisted mask and an ARM64 `TST` in the
loop body. Non-power-of-two and dynamic divisors retain the guarded
`SDIV`/`MSUB` path.

The focused suite now contains 69 ARM64/JIT tests. New differential cases span
both divisor signs, values from `PHP_INT_MIN` through `PHP_INT_MAX`, direct and
assigned straight operations, negative-induction conditional loops, and a
machine-code assertion that the optimized regions contain no `SDIV`.

End-to-end measurement is intentionally recorded as neutral rather than
claimed as a benchmark win. In 101 randomized before/after process pairs, the
10-million-iteration modulo benchmark moves from a 7.216 ms median to 7.219 ms
(+0.04 percent); a 31-pair 100-million-iteration amplification moves from
71.923 ms to 71.744 ms (-0.25 percent). Output is identical in every run. The
native function still returns to Rust every 32 iterations for interrupt and
state publication checks, so the next performance experiment is to measure
and safely amortize that chunk boundary. Constant-remainder lowering remains
valuable general code quality and removes a division bottleneck once larger
regions keep execution native for longer.

Native safepoint amortization checkpoint (2026-08-04): the shared native-loop
budget is now named for its actual contract and increases from 32 to 1,024
iterations between VM interrupt checks. All admitted native regions are
non-blocking and contain at most 48 bounded operations per iteration. The VM
still publishes exact state and checks `vm_interrupt` after every exhausted
budget; overflow, guards and other side exits remain instruction-precise and
do not wait for the budget boundary.

The value comes from a 21-run randomized matrix over 32, 64, 128, 256, 512 and
1,024 iterations, followed by 2,048 and 4,096 holdouts. Moving from 32 to
1,024 reduces the medians of sum, branch, modulo, straight binary, scalar-call
and expression-chain loops by 7.6, 21.5, 20.5, 19.1, 14.3 and 18.0 percent.
Untyped order, ledger and independent routing application workloads improve
7.7, 9.3 and 7.6 percent. Every output is identical. The larger 2,048 and
4,096 variants provide no consistent additional benefit, establishing 1,024
as the conservative plateau rather than a benchmark-specific maximum.

At 1,024 iterations the slowest measured application region remains below
9 microseconds between safepoints; the experimental 4,096 build remains below
36 microseconds. Focused integration checks now require roughly 98 native
chunks for 100,000 iterations in simple accumulate, scalar-call, conditional,
straight and mixed regions, preventing a silent return to high-frequency
runtime crossings while retaining multiple interrupt boundaries.

A final 51-run six-mode comparison records 4.819 ms for the sum loop and
5.449 ms for modulo on RPHP JIT, versus 2.308/2.340 ms for forced-scalar C and
8.906/11.320 ms for PHP tracing JIT. RPHP therefore reaches about 48 percent
and 43 percent of scalar-C throughput while running these workloads 1.85x and
2.08x faster than PHP tracing JIT. The prior 32-iteration RPHP binary records
5.265/6.884 ms in the same batch.

Native accumulate range-proof checkpoint (2026-08-04): simple induction and
induction-plus-constant accumulation now has separate checked and range-proven
native programs in the per-plan cache. Before each 1,024-iteration chunk, the
dispatcher derives the exact executed length and evaluates the arithmetic
progression in `i128`. It checks both term endpoints and the extrema of every
accumulator prefix sum; because those prefix sums are convex, the only required
points are the two endpoints and the transition after the final negative term.
This covers negative-to-positive loops as well as ordinary positive loops
without replaying the chunk in Rust.

When the proof succeeds, the hot native body uses ordinary ARM64 additions and
contains no term, sum, or increment overflow branches or overflow stubs. If it
fails, the same cache lazily compiles and calls the previous transactional
checked program, preserving the exact canonical PHP resume instruction. The
choice is per chunk rather than per function, so one activation can safely use
both variants. Edge matrices plus 100,000 deterministic randomized states
match an iterative checked reference, real negative accumulation takes only
range-proven chunks, and real sum and term overflow still produce one precise
side exit. The full all-feature suite passes.

On the native-CPU `max-perf` build, a 201-run median for the 10-million-iteration
sum loop is 2.412 ms, down 49.9 percent from the preceding 4.819 ms checkpoint,
with identical `49999995000000` output. Against the earlier same-machine
forced-scalar C median of 2.308 ms, this is approximately 95.7 percent of C
throughput and only 4.5 percent more elapsed time; it is about 3.69x faster than
the earlier PHP tracing-JIT median of 8.906 ms. Separate 51-run batches at
2.641 and 2.434 ms show the expected short-run frequency variance while
preserving the structural gain.

Range-proven endpoint lowering follow-up (2026-08-04): a non-empty proven
chunk now receives its exclusive induction end in the native ABI. It enters
the body without a redundant PHP-bound header check and terminates with one
induction/end comparison rather than maintaining a second per-iteration
safepoint countdown. ARM64 immediate `ADD`, checked `ADDS`, and checked `SUBS`
encodings also remove the materialized constant-one register. The common hot
body is consequently the same four-instruction scalar shape emitted by Clang:
accumulator add, induction increment, end comparison, and back edge.

The VM proves the complete remaining activation once on entry when possible;
all later 1,024-iteration chunks derive only their exclusive end. Plans that
cannot prove the full remaining range continue to evaluate each chunk and use
the checked program where required. Instrumented integration checks require
one proof evaluation for positive, negative, and constant-term safe loops, two
for an immediately overflowing sum, and preserve the existing exact side exits.

In 201 order-rotated A/B pairs, the preceding per-chunk-proof binary records a
2.416 ms median and the endpoint/prove-once binary 2.388 ms; the paired median
change is -1.09 percent. A same-run scalar-C comparison is 2.259 ms versus
2.388 ms, putting RPHP at approximately 94.6 percent of C throughput and 5.7
percent more elapsed time. A 100-million-iteration holdout remains separated
by nearly the same 5.55 percent (22.573 ms C, 23.826 ms RPHP). Because the hot
instruction shape is now equal and the percentage does not shrink with a
longer loop, the remaining simple-loop gap is attributed primarily to the
roughly 1,024-iteration Rust/native safepoint crossings, not JIT compilation or
per-iteration arithmetic. The next focused experiment is therefore an in-native
interrupt poll that keeps the same safepoint interval without returning to Rust
when no interrupt is pending.

In-native accumulate safepoint checkpoint (2026-08-04): the full-range-proven
program now receives the runtime `AtomicBool` interrupt address and keeps the
1,024-iteration polling contract inside generated ARM64. Its inner chunk still
has the same four instructions as scalar C. At a chunk end it first honors
semantic loop completion, otherwise performs the relaxed byte-sized atomic
load; only a set flag publishes state and returns `ChunkExhausted` to Rust. An
unset flag selects the next bounded chunk using the unsigned induction distance
and continues natively. This handles ranges crossing signed zero without an
overflow assumption.

Direct ABI tests cover uninterrupted completion, a flag already set at entry,
the exact 1,024-iteration published boundary, clearing the flag, and resuming
to an identical final state. VM accounting derives the number of passed
safepoints from the native induction delta, so the existing roughly 98-chunk
integration assertions and interrupt observability remain meaningful even
though the common activation uses one Rust/native call.

Across 201 order-rotated pairs, the prove-once program returning at every
safepoint records 2.391 ms and the in-native poll 2.322 ms; the paired median is
-2.57 percent. The same-run forced-scalar C median is 2.256 ms versus 2.321 ms
for RPHP: approximately 97.2 percent of C throughput and only 2.9 percent more
elapsed time. A 100-million-iteration holdout gives 22.571 ms C and 23.213 ms
RPHP, the same 2.85-percent separation. The 50-million constant-term loop is
essentially flat within variance (paired -0.37 percent), as expected for its
longer dependency chain. The simple induction loop is therefore close enough
to the scalar hardware ceiling that further benchmark-specific unrolling would
not justify weaker interrupt latency or larger generated code; the reusable
next step is to bring internal safepoint polling and range reasoning to wider
typed loop IR where measurements show a material application benefit.

Conditional typed-loop range checkpoint (2026-08-04): the same complete-range
proof and in-native safepoint contract now covers the general conditional Long
accumulator IR used by both less-than branches and modulo-equality filters. The
proof is deliberately independent of the predicate: over the complete ordered
induction range it sums every possible negative contribution and every possible
positive contribution separately. If the initial accumulator remains inside
`i64` at both conservative extremes, every prefix of every predicate-selected
subset is safe. A failed proof keeps the original checked ARM64 program and its
exact PHP overflow resume instruction.

The proven program removes per-iteration sum and induction overflow branches,
keeps the condition itself unchanged, and polls the VM interrupt flag every
1,024 iterations without returning to Rust when the flag is clear. The simple
accumulator and conditional compiler share one ARM64 polling-backedge emitter,
so completion priority, unsigned signed-zero-crossing distance, and interrupt
publication are now one backend invariant rather than two handwritten copies.
Instrumentation verifies one range-proof evaluation and one native call for a
100,000-iteration activation while retaining roughly 98 virtual safepoint
chunks. Unsafe branch and modulo overflow cases prove nothing, use zero proven
chunks, and retain their precise checked side exit.

Four direct proof/ABI tests include 20,000 deterministic arbitrary-subset
states, interrupt at the exact first safepoint, resume to the identical final
state, and modulo selection. All 70 ARM64 JIT integration tests pass. In 101
order-rotated `max-perf` A/B pairs against the preceding binary, the branch
loop moves from 10.189 ms to 6.817 ms (paired median -33.61 percent) and the
modulo branch from 7.480 ms to 4.987 ms (paired median -33.30 percent), with
identical outputs. Absolute clock rates varied between batches, but an earlier
61-pair run showed the same -33.7-percent relative result. In a separate
31-run comparison, RPHP recorded 4.174 ms versus PHP tracing JIT's 13.777 ms
for the branch loop (3.30x faster), and 4.467 ms versus 13.253 ms for modulo
(2.97x faster), again with identical outputs.

Straight typed-loop interval checkpoint (2026-08-04): complete-range proof and
in-native safepoint polling now extend to the general straight Long IR. A
separate interval analyzer propagates `i128` bounds through moves, add,
subtract, multiply, integer divide, modulo, bitwise XOR, and materialized or
assigned intermediate results. It derives the induction interval from the
actual remaining activation, so one proof covers arbitrary composed scalar
expressions rather than a benchmark-specific instruction sequence.

Admission remains deliberately conservative. Every checked intermediate must
fit in `i64`; zero divisors and the `PHP_INT_MIN / -1` edge are rejected. A body
output read before its first current-iteration write is loop-carried and is
therefore left for a future recurrence proof. String tokens, hash operations,
guards, and control flow also retain the existing transactional checked path.
An accepted activation removes arithmetic and induction overflow branches,
uses one Rust/native call, and polls the shared interrupt flag inside generated
ARM64 every 1,024 iterations. A rejected activation keeps exact PHP operation
replay and checked side exits unchanged.

Four focused analyzer tests cover composed affine/modulo expressions,
overflow and recurrence rejection, interval-transfer edge samples, and exact
interrupt publication/resume at iteration 1,024. Instrumented real-PHP tests
require one proof evaluation and one native call for both a straight binary
body and a non-materialized scalar-expression chain, while deliberate
loop-carried overflow proves no range and exits at the canonical operation.
The complete checkpoint passes 122 library tests, 70 ARM64/JIT integration
tests, and all four stateful application-corpus tests.

In 101 order-rotated `max-perf` A/B pairs against the preceding conditional
checkpoint, `bench_binary_assign_loop.php` moves from 11.279 ms to 11.120 ms
(paired median -1.48 percent), while
`bench_scalar_expression_chain_loop.php` moves from 12.375 ms to 9.916 ms
(paired median -19.92 percent); every output is identical. A separate 31-run
comparison records 10.675 ms for RPHP versus 32.348 ms for PHP tracing JIT on
the binary body (3.03x faster), and 9.774 ms versus 38.320 ms on the expression
chain (3.92x faster). The small binary-body gain is expected because its modulo
and slot traffic dominate; the composed chain demonstrates the reusable value
of proving multiple checked intermediates once outside the loop.

Forward scalar-control-flow checkpoint (2026-08-04): the straight-loop planner
now maps ordinary typed `if`/`else` edges and forward jumps into the same native
IR instead of requiring a physically linear PHP body. The interval analyzer is
a small forward dataflow pass: each program point carries the range of every
Long slot plus a definitely-written mask. Branch joins take the interval hull
and intersect that mask, allowing a value written on every incoming edge to
feed later composed arithmetic.

This also defines the safety boundary without guessing. If any incoming edge
can reach a read before writing that iteration's value, the slot is
loop-carried and the complete-range proof is rejected. The already-compiled
transactional ARM64 region then runs with its checked 1,024-iteration chunks.
Only forward branches are admitted, so the dataflow converges in one ordered
pass; backedges remain exclusively the validated outer loop. String/hash
state, trace guards, and general recurrences continue to use their established
paths.

A new previously unseen benchmark assigns a scalar expression in both arms and
feeds the joined value to another composed expression. Across 101
order-rotated `max-perf` runs, identical `49999993,149999990` output moves from
145.845 ms on the preceding RPHP binary to 9.621 ms, a 15.16x speedup (paired
median -93.39 percent). PHP tracing JIT records 34.110 ms, making the new RPHP
path 3.55x faster. A 101-run linear binary control remains flat within noise
at 11.368 versus 11.328 ms (paired -0.44 percent), and the expression-chain
control is likewise flat in a separate 31-run batch. The checkpoint passes
124 library, 72 ARM64/JIT, and all four application-corpus tests, including a
partially-written branch that must retain checked native chunking.

Rejected recurrence-proof experiment (2026-08-04): a closed-form `i128`
prototype safely proved direct additive loop-carried values, including multiple
independent recurrences and induction-dependent deltas. Randomized checked
execution and deliberate overflow side exits confirmed the proof itself. It
was not retained because the current slot-based native body could not turn the
removed guards and Rust/native crossings into higher throughput.

On the two-recurrence composed benchmark, 101 order-rotated runs measured
19.067 ms for the proof-enabled endpoint-polling program versus 11.567 ms for
the existing checked chunks (paired +62.32 percent). A separate countdown
backedge improved the prototype only to 18.152 ms versus 11.324 ms (paired
+59.38 percent). Keeping checked arithmetic inside the polling program and
lowering small constants directly as ARM64 immediates also failed the
profitability gate; the immediate experiment was neutral on the binary body
and regressed the expression chain by 3.79 percent. All prototype code and
benchmarks were removed before commit.

The result changes the next recurrence prerequisite. General scalar liveness
must first stop publishing dead temporary slots on every iteration, keep
eligible loop-carried values in registers across the backedge, and publish
only at safepoints or exact side exits. Range proof should then be reapplied to
that register-resident IR. Until those representation changes demonstrate a
measured win, direct recurrences deliberately retain the already faster
transactional checked path.

Linear scalar forwarding checkpoint (2026-08-04): the first liveness step now
keeps the most recently computed Long value in its ARM64 result register for
immediate consumers. A result can represent both the operation temporary and
its assigned CV destination, so either name avoids a redundant shadow reload.
Admission is intentionally narrow: only complete-range-proven, physically
linear bodies containing modulo, move, binary, and binary-assign operations
use forwarding. Structured branches, strings, hashes, guards, and checked
side exits remain on their previous lowering.

This checkpoint does not remove a single shadow store. Every operation still
publishes exactly the same slots at exactly the same point, which keeps VM
resume, safepoint, and error semantics unchanged while isolating the benefit
of register dataflow. A structural encoder test distinguishes forwarded
`MOV`, already-resident zero-instruction reuse, and the original non-resident
`LDR` path.

Across 101 order-rotated `max-perf` A/B pairs, the scalar expression chain
moves from 9.804 ms to 8.774 ms (paired median -10.54 percent) and the binary
assign body from 11.017 ms to 8.756 ms (-19.79 percent), with identical output.
PHP tracing JIT records 38.178 ms and 32.264 ms respectively, leaving RPHP
4.35x and 3.68x faster. The deliberately excluded forward-branch control is
flat at 9.570 versus 9.580 ms (-0.10 percent paired). In 101-run application
controls, typed order is flat at +0.09 percent and typed ledger at +0.22
percent, both within sub-percent noise and with identical business results.
The next profitable step is multi-value residency and liveness-driven dead-TMP
store elimination, but only after publication requirements are represented
explicitly in the native IR.

Overlapping scalar residency checkpoint (2026-08-04): a separate backward
liveness pass now tracks the current value version of each straight Long slot.
The range-proven linear lowering uses three caller-saved registers that are
otherwise dead after polling-mode entry to preserve values whose lifetime
crosses a later operation. Allocation is bounded and failure is harmless: if
all three cache registers are occupied, the existing shadow load remains.
Version-killing on every output prevents an older value of the same slot from
being reused after reassignment.

As in the first forwarding checkpoint, every shadow store remains. Structural
tests verify both the cache-register `MOV` and the original `STR`, while a real
PHP integration test confirms that an overlapping four-expression body uses
one complete-range-proven native entry with exact output. A new permanent
benchmark covers this previously unseen lifetime shape.

In 101 order-rotated A/B pairs against the one-register checkpoint, overlapping
lifetimes move from 15.340 ms to 11.236 ms (paired median -26.78 percent), with
PHP tracing JIT at 62.390 ms and RPHP therefore 5.55x faster. Regions that do
not need another live register remain flat: expression chain is -0.17 percent
and binary assign -0.07 percent. Typed order and typed ledger application
controls are also flat at +0.10 and +0.03 percent with identical results. This
isolates the next remaining memory cost cleanly: dead temporary stores, not
reloads of live overlapping values.

Dead scalar temporary publication checkpoint (2026-08-04): native compilation
now receives an explicit mask of externally visible Long outputs. The VM
derives it from the OpArray CV boundary; compiler TMP slots remain internal.
The existing planner invariant already proves that a TMP produced by a region
is redefined before every valid later use, while CVs retain loop and user
state. A backward value-version liveness mask then preserves the shadow store
for any TMP that is read later and removes it only when that exact version is
dead after the operation.

The optimization is restricted to complete-range-proven, physically linear
scalar bodies. Checked bodies still compile with a full publication mask, so
operation overflow, increment overflow, and precise side-exit replay are
unchanged. Induction and post-result publication are also unchanged. At normal
completion or an interrupt safepoint, the VM commits the same CV publication
mask rather than copying stale internal TMP slots. A structural execution test
proves that the dead result `STR` is absent, the paired destination `STR`
remains, and interrupt/resume preserves the visible destination while leaving
the excluded TMP untouched.

Across 101 order-rotated A/B pairs against the multi-register checkpoint,
binary assign falls from 8.712 ms to 5.828 ms (-33.16 percent), expression chain
from 9.051 ms to 6.538 ms (-27.43 percent), and overlapping lifetimes from
11.556 ms to 6.451 ms (-44.03 percent), all with identical output. PHP tracing
JIT records 32.265, 39.478, and 63.844 ms, making RPHP 5.54x, 6.04x, and 9.90x
faster. The excluded forward-branch control is flat at -0.12 percent. Typed
order and typed ledger controls are +0.55 and +0.09 percent respectively,
without a result change. This confirms that per-iteration publication, rather
than arithmetic throughput, was the dominant remaining cost in these scalar
regions.

Immediate-consumer temporary checkpoint (2026-08-04): the publication pass now
also recognizes a non-visible TMP value whose final use is in the immediately
following operation. The producer result is guaranteed to remain in `x8`
until that consumer has loaded both operands, even when every auxiliary cache
register is occupied. The store can therefore be omitted without relying on a
successful register-allocation decision. If the value remains live beyond the
consumer, its shadow store is retained conservatively.

Version handling is explicit: a next operation that rewrites the same slot
kills the old value only after consuming its operands, while liveness after
that operation refers to the new version. Structural execution tests verify
that both the producer TMP and final assignment TMP stores disappear, the CV
destination store remains, and stale TMP sentinels cannot affect the result.
Interrupt/resume and the full checked fallback contract remain those of the
preceding publication checkpoint.

In 101 order-rotated A/B pairs against that checkpoint, expression chain falls
from 6.412 ms to 5.132 ms (-19.91 percent), binary assign from 5.894 ms to 5.656
ms (-3.96 percent), and overlapping lifetimes from 6.310 ms to 6.086 ms (-3.54
percent). Against PHP tracing JIT, RPHP is respectively 7.59x, 5.77x, and
10.32x faster. Typed order is flat at +0.02 percent and typed ledger at -0.20
percent, again with identical business outputs.

Last-result safepoint publication checkpoint (2026-08-04): the visible CV
aliases produced by the final scalar operation now remain in ARM64 `x8` across
the polling backedge. Generated code writes them to shadow state only in the
completion and interrupt exit blocks. The backedge does not clobber `x8`, and
every admitted native activation executes at least one iteration, so the exit
always observes the exact final body value. All earlier CVs, induction, and
post-result state retain their prior publication rules.

The existing dead-TMP interrupt test now also exercises this deferred visible
result: it interrupts exactly after 1,024 iterations, validates the published
CV while the excluded TMP remains untouched, resumes, and validates final
completion. Across 101 A/B pairs against immediate-consumer publication,
expression chain is -0.65 percent, binary assign -2.94 percent, and overlapping
lifetimes -4.63 percent. Typed order is flat at -0.05 percent and typed ledger
at +0.43 percent. This establishes the exit-publication mechanism; the larger
payoff requires assigning earlier final CV values to dedicated registers.

Multi-CV safepoint publication checkpoint (2026-08-04): a reverse definition
pass identifies the final body value for every published CV. Up to three
values defined before the final operation receive fixed `x4`, `x5`, and `x11`
registers; the final operation can retain `x8`. Reserved publication registers
are excluded from temporary-cache allocation before their definition and join
normal resident operand lookup afterwards. Additional values beyond capacity
retain their original per-iteration shadow stores.

Each fixed register is written only by its final defining operation and is
flushed to all aliased CV slots in both native completion and interrupt exits.
Temporary aliases retain their independent liveness rules. A direct four-CV
test validates all three cache-register moves, exact values after an interrupt
at iteration 1,024, and exact final values after resume; the real-PHP overflow
test confirms that checked fallback remains unchanged.

In 101 A/B pairs against last-result-only publication, the two-CV expression
control is flat at +0.34 percent, the three-CV binary body improves 2.19
percent, and the four-CV overlapping-lifetime body improves 12.25 percent to
5.128 ms. PHP tracing JIT records 63.242 ms on the latter, making RPHP 12.33x
faster. Typed order is flat at +0.01 percent and typed ledger at -0.16 percent.
The fixed-register exit contract is now sufficient to revisit loop-carried
recurrence values without reintroducing per-iteration publication.

Register-resident recurrence checkpoint (2026-08-04): the straight range
proof now admits up to three independent additive or subtractive loop-carried
Long CVs. The recurrence delta may be a constant, the induction value, or an
invariant Long slot. For every candidate, an `i128` prefix envelope starts at
the activation's current CV value and includes the worst contribution of every
remaining prefix. Acceptance therefore proves every intermediate update, not
only the final closed-form result. Multiplicative, dependent, branch-carried,
multiply-written, or capacity-exceeding recurrences retain checked chunking.

The proof returns an explicit carried mask to the backend. Each carried CV is
loaded once into `x4`, `x5`, or `x11` before the native loop entry, consumed
from that register, moved back after its defining operation, and kept across
the polling backedge. Completion and interrupt exits reuse the multi-CV
publication contract, so no recurrence shadow store occurs inside the loop.
Unsafe activations compile and execute the existing checked program with its
canonical operation side exit.

A two-recurrence direct ABI test covers interrupt publication at iteration
1,024 and exact resume to completion. A real PHP test requires one proof and
one native call, while the existing overflow test still rejects the proof and
exits at the precise PHP operation. A matrix over positive and negative
induction ranges, add/subtract deltas, and initial values near both `i64`
limits asserts that no overflowing prefix is ever accepted. The checkpoint
passes 132 library tests, 74 ARM64/JIT integration tests, all four application
corpus tests, and `cargo check --all-features`.

In 101 order-rotated `max-perf` runs against the multi-CV publication commit,
the permanent two-recurrence benchmark improves from 6.485 ms to 2.615 ms
(paired median -59.74 percent, 2.48x faster). PHP tracing JIT records 30.860
ms, making RPHP 11.80x faster. The non-recurrence binary and overlapping-value
controls remain flat at +0.02 and +0.16 percent. This reverses the earlier
slot-based recurrence prototype's roughly 60-percent regression and confirms
that range proof became profitable only after register residency and exit-only
publication were established.

Composed recurrence delta checkpoint (2026-08-04): direct recurrence proof no
longer requires a single constant, induction, or invariant operand. When the
delta is a temporary, a bounded backward definition walk reconstructs its
acyclic scalar expression from earlier move, modulo, binary, and binary-assign
operations. Each intermediate receives the same conservative `i128` interval
transfer used by the straight-body proof before the resulting delta interval
is folded into the all-prefix recurrence envelope.

The walk is definition-order based rather than syntax based, so ordinary PHP
such as `$sum = $sum + (($i * 3) + $offset)` benefits without a dedicated
benchmark pattern. It deliberately refuses a dependency on any loop-carried
CV, later definition, control-flow merge, unsupported operation, or unsafe
intermediate. A real overflow case keeps its sum at zero while a nested
multiply eventually overflows; the proof rejects it and the checked native
program side-exits at the exact operation after reaching hotness.

In 101 order-rotated `max-perf` runs against the direct-recurrence commit, the
new permanent composed-recurrence holdout improves from 8.648 ms to 3.149 ms
(paired median -63.80 percent, 2.75x faster). PHP tracing JIT records 35.865
ms, making RPHP 11.39x faster. The already optimized direct-recurrence control
is flat at +0.16 percent and the ordinary binary control at +0.04 percent.
The checkpoint passes 133 library tests, 76 ARM64/JIT integration tests, all
four application corpus tests, and `cargo check --all-features`.

Forward-dependent recurrence checkpoint (2026-08-04): recurrence discovery is
now separated from proof order. Every carried CV is first mapped to its unique
defining operation, then definitions are proven in physical body order. The
all-prefix interval of an earlier updated CV becomes an available operand for
a later recurrence or its composed delta DAG. This admits triangular state
updates such as `$a += 1; $b += $a` while preserving the current-iteration PHP
ordering semantics.

A carried dependency whose definition is current, later, repeated, branched,
or cyclic has no previously proven interval and is rejected. The proof remains
conservative: the later recurrence multiplies the complete earlier prefix
envelope by the remaining iteration count, so acceptance bounds every possible
intermediate even when this is wider than the exact polynomial result. A
matrix around both `i64` limits asserts that no overflowing dependent prefix
is accepted. The direct ABI interrupt/resume test now also requires the second
fixed register to consume the first register's newly updated value.

In 101 order-rotated `max-perf` runs against the composed-delta commit, the new
permanent dependent-recurrence holdout improves from 7.288 ms to 2.659 ms
(paired median -63.65 percent, 2.74x faster). PHP tracing JIT records 30.863
ms, making RPHP 11.61x faster. The composed recurrence control improves 0.99
percent and the direct recurrence control is flat at -0.38 percent. The
checkpoint passes 134 library tests, 77 ARM64/JIT integration tests, all four
application corpus tests, and `cargo check --all-features`.

Topological recurrence proof checkpoint (2026-08-04): physical operation
order no longer doubles as dependency proof order. Discovery first records
every recurrence node and delta expression. A bounded solver then repeatedly
proves any unresolved node whose carried dependencies already have all-prefix
intervals. With at most three fixed recurrence registers, convergence is
strictly bounded; no-progress means a cycle, unsafe expression, or unsupported
shape and retains checked chunking.

This admits reverse-order acyclic PHP such as `$b += $a; $a += 1`. Proof order
may solve `$a` first, but generated ARM64 remains in source order, so `$b`
observes the old `$a` exactly as PHP requires. Both the direct ABI
interrupt/resume test and a real PHP integration test assert this old-value
semantics. The dependent safety matrix now covers forward and reverse physical
orders near both `i64` limits, while an explicit two-node cycle remains
rejected.

In 101 order-rotated `max-perf` runs against the forward-dependent commit, the
new permanent reverse-dependent holdout improves from 11.371 ms to 2.635 ms
(paired median -76.79 percent, 4.32x faster). PHP tracing JIT records 30.454
ms, making RPHP 11.56x faster. The forward-dependent control is flat at +0.04
percent and the composed recurrence control at -0.16 percent. The checkpoint
passes 134 library tests, 78 ARM64/JIT integration tests, all four application
corpus tests, and `cargo check --all-features`.

Structured conditional recurrence checkpoint (2026-08-04): the recurrence
proof now admits forward control flow when every carried value still has one
unique additive or subtractive definition. A guarded update uses the same
all-prefix envelope as an unconditional update; treating any subset of at most
the remaining iteration count is conservative and therefore covers a skipped
arm. To avoid claiming general CFG liveness, a structured recurrence delta may
only use a constant, induction, invariant slot, or already-proven carried CV.
A branch condition that reads carried state is also rejected until condition
lowering can consume fixed resident registers directly.

The ARM64 backend separates fixed carried residency from linear temporary
residency. Structured scalar code loads each carried CV into `x4`, `x5`, or
`x11` once, but continues to materialize ordinary temporaries in shadow state
across branches. Executing a recurrence replaces its fixed register; skipping
it naturally preserves the old value. Only carried CVs are deferred to native
completion or interrupt exits, so a never-executed temporary alias remains
untouched. This gives conditional loops the optimization without an unsound
assumption that every path defines every temporary.

Compiler fusion is normalized at the native boundary: `ConditionalAddAssign`
lowers to a forward `BranchUnless` and checked `BinaryAssign`. A real PHP loop
with one conditional and one unconditional recurrence now enters one straight
native region, evaluates its range proof once, and completes all 98 safepoint
chunks without a side exit. A near-limit counterpart rejects the proof and
uses the existing checked operation side exit, preserving canonical PHP
overflow behavior.

In 101 order-rotated `max-perf` runs against the topological-recurrence commit,
the new permanent conditional-recurrence holdout improves from 65.542 ms to
5.303 ms (paired median -91.90 percent, 12.36x faster). PHP tracing JIT records
28.083 ms, making RPHP 5.30x faster. The existing linear recurrence control is
flat at -0.26 percent and the structured non-recurrence expression control is
flat at -0.03 percent; the dedicated one-recurrence branch control moves by
+0.69 percent. The checkpoint passes 136 library tests, 80 ARM64/JIT
integration tests, all four application corpus tests, and
`cargo check --all-features`.

Resident recurrence-condition checkpoint (2026-08-04): structured branch
conditions can now consume the exact current value of a carried CV. A simple
source operand returns its fixed register directly; induction likewise returns
the induction register instead of first copying it. Bitwise-and conditions
keep their result temporary but obtain either input through the same resident
lookup. The branch therefore observes source-order state whether it appears
before or after that recurrence's defining operation.

The range proof's existing state already assigns every carried CV its proven
all-prefix envelope and treats conditional execution as a conservative subset,
so no new arithmetic assumption is required. The former backend-driven
rejection of carried branch inputs is removed. Direct tests cover a source
condition, a bitwise carried condition whose update never executes, exact
safepoint publication and resume. The real PHP integration makes `$count`
both a condition input and an unconditional recurrence; its near-limit variant
still rejects the proof and takes the checked overflow side exit.

In 101 order-rotated `max-perf` runs against the structured-conditional
checkpoint, the permanent carried-condition holdout improves from 8.148 ms to
5.187 ms (paired median -36.21 percent, 1.57x faster). PHP tracing JIT records
28.827 ms, making RPHP 5.56x faster. The induction-guarded conditional
recurrence control is flat at +0.06 percent and the dedicated branch control
is flat at -0.18 percent. The checkpoint passes 136 library tests, 80
ARM64/JIT integration tests, all four application corpus tests, and
`cargo check --all-features`.

Dominated conditional-delta checkpoint (2026-08-04): composed recurrence
deltas may now use scalar temporaries defined inside forward control flow. A
bounded reachability check removes each candidate definition from the native
body graph; the definition is accepted only when its use becomes unreachable,
which proves that every path to the use executes that definition. The same
check is applied recursively to every temporary in the delta DAG.

This is deliberately narrower than general CFG liveness. A branch-dominated
multiply and add inside the selected arm can feed its recurrence, while an
otherwise identical branch that can jump directly to the recurrence without
executing those definitions is rejected. Existing interval transfers still
prove every intermediate multiply, add and all-prefix recurrence value. The
main range-state pass independently verifies that each temporary is definitely
written on paths reaching its consumer.

A real PHP loop now range-proves `$sum += ($i * 3) + $offset` inside a guard
over another recurrence and completes in one native activation. Its near-limit
counterpart reaches native hotness, rejects the proof and exits from the exact
checked overflowing operation. In 101 order-rotated `max-perf` runs against
the resident-condition checkpoint, the permanent conditional composed
recurrence holdout improves from 10.925 ms to 5.824 ms (paired median -46.60
percent, 1.88x faster). PHP tracing JIT records 37.204 ms, making RPHP 6.39x
faster. The simple carried-condition recurrence and linear composed recurrence
controls are both flat at +0.02 percent. The checkpoint passes 136 library
tests, 81 ARM64/JIT integration tests, all four application corpus tests, and
`cargo check --all-features`.

Branch-local temporary residency checkpoint (2026-08-04): structured
range-proven scalar bodies now retain the latest operation result in `x8`
inside one basic block. Fallthroughs after a conditional branch, jump
successors and every explicit branch target start a new block and clear only
the compile-time `x8` mapping; fixed carried registers remain valid on every
path. This admits immediate expression chains without claiming that a
temporary is live across a CFG merge.

Shadow publication uses an equally bounded rule. A non-published,
non-carried output store is omitted only when that version is dead or every
read before its next definition occurs in the immediately following operation
inside the same block. A consumer at a branch target or any later consumer
forces the store to remain. Direct generated-code tests require the two
resident `MOV` instructions, reject four former TMP stores and preserve all
TMP sentinels across an interrupt and final completion.

In 101 order-rotated `max-perf` runs against the dominated-delta checkpoint,
the conditional composed recurrence falls from 6.034 ms to 5.247 ms (paired
median -12.78 percent). PHP tracing JIT records 38.393 ms, making RPHP 7.32x
faster. The simple carried-condition recurrence improves from 5.209 ms to
4.294 ms (paired median -17.79 percent, 6.79x faster than PHP tracing JIT).
The linear composed recurrence is flat at +0.00 percent and the structured
non-recurrence expression control at +0.24 percent. The checkpoint passes 138
library tests, 81 ARM64/JIT integration tests, all four application corpus
tests, and `cargo check --all-features`.

General structured-local residency checkpoint (2026-08-04): basic-block `x8`
forwarding is no longer coupled to the presence of a loop-carried CV. Every
range-proven scalar body composed of moves, modulo, binary operations and
forward branches may use local temporary residency. Fixed-register allocation
and exit-only publication remain gated by the carried mask, so ordinary
branching assignments continue to store their visible CVs on the selected
path and require no phi publication contract.

This separation lets ordinary `if/else` expression code remove branch-local
TMP loads and stores while retaining the established merge behavior. The real
PHP forward-branch integration still enters one range-proven native region and
produces the same selected and folded values. In 101 order-rotated `max-perf`
runs against the carried-only branch-local checkpoint, the permanent
structured branch-expression benchmark improves from 10.355 ms to 6.586 ms
(paired median -36.58 percent, 1.57x faster). PHP tracing JIT records 34.718
ms, making RPHP 5.27x faster. The conditional composed recurrence is flat at
-0.09 percent and the linear composed recurrence at -0.25 percent. The
checkpoint passes 138 library tests, 81 ARM64/JIT integration tests, all four
application corpus tests, and `cargo check --all-features`.

Structured merge-publication checkpoint (2026-08-04): forward scalar control
flow now computes a definitely-written mask at every operation and at the body
exit. Incoming masks are intersected at merges, so an ordinary visible CV is
eligible for deferred publication only when every predecessor defines it. An
`if/else` result can therefore own one fixed register across both definitions
and its merge consumer, while an `if` without an `else` remains on the checked
shadow-store path because the old value may reach the merge.

Published aliases with the same reaching-definition operation set share one
register. Carried registers are allocated first; up to three remaining groups
use the existing fixed publication registers. Compile-time register validity is
reconstructed from the definitely-written fact at each operation rather than
from physical code-generation order, preventing a true-arm value from leaking
into a separately emitted false arm. Both native exits publish the deferred
groups, including after a safepoint interruption.

The direct ABI test requires both `$selected` definitions to reach the same
register, `$folded` to use a second register, and all per-iteration CV stores to
be absent. It also preserves six TMP sentinels and validates exact values after
an interrupt and final completion. The real PHP forward-branch test still
enters one range-proven region; the partially-written control still rejects the
complete-range proof and uses checked chunks.

This checkpoint is intentionally an architectural prerequisite rather than a
claimed speed win. In 101 order-rotated native-CPU `max-perf` A/B pairs against
a freshly rebuilt `fdf66e0`, the structured branch-expression benchmark is
6.500 ms versus 6.507 ms (paired median +0.14 percent). The conditional
composed recurrence is +0.18 percent and the linear composed recurrence
-0.03 percent. The former shadow store/load/store sequence has been exchanged
for three register moves, leaving the effective operation count approximately
flat on this CPU. The next lowering step should write each selected scalar
definition directly into its fixed merge register and remove those moves.

The checkpoint passes 140 library tests, 81 ARM64/JIT integration tests, all
four application corpus tests, and `cargo check --all-features`.

Direct structured-result checkpoint (2026-08-04): a structured scalar
definition assigned to a fixed carried or merge-publication register now emits
its arithmetic result directly into that register. The previous unconditional
`MOV fixed, x8` remains only as a conservative fallback when one operation
defines multiple fixed groups or its result TMP alias is consumed immediately
inside the same basic block. Register-state metadata records whether `x8` or
the fixed register actually owns the result, so later operand selection cannot
observe a stale path-local value.

Two bounded fallbacks are explicit in generated-code tests. An immediate TMP
consumer keeps `x8`, copies the visible alias to its fixed register, omits the
TMP store and produces the exact result. A structured body with four
independent visible results assigns the first three to `x4`, `x5` and `x11`,
then retains the fourth per-iteration shadow store because the fixed-register
budget is exhausted. Thus direct result selection changes neither the three-
register ABI limit nor correctness outside it.

In 101 order-rotated native-CPU `max-perf` A/B pairs against merge-publication
checkpoint `90e5d57`, the permanent structured branch-expression benchmark
falls from 6.614 ms to 5.251 ms (paired median -21.29 percent). The simple
conditional recurrence falls from 4.312 ms to 4.089 ms (-6.18 percent). The
conditional composed recurrence is flat at +0.17 percent and the linear
composed recurrence at +0.19 percent. A separate 101-run comparison records
RPHP at 5.183 ms versus PHP tracing JIT at 34.503 ms with identical output, or
6.66x faster on this workload.

The checkpoint passes 142 library tests, 81 ARM64/JIT integration tests, all
four application corpus tests, and `cargo check --all-features`.

Direct resident-operand checkpoint (2026-08-04): scalar arithmetic now receives
the register that already owns a resident operand instead of first copying it
to the conventional `x6` or `x7` scratch register. The same selector returns
the induction register directly and loads only non-resident slots or constants
into a scratch. Binary, binary-assign, move, modulo and composed bitwise
conditions all consume the returned register explicitly.

This is safe for the range-proven lowering because ARM64 scalar arithmetic may
use the destination as either source; checked lowering has no resident scalar
results and therefore retains distinct scratch inputs wherever overflow logic
needs them. A direct assembler test now requires no emitted instruction for a
resident lookup, while a shadow lookup must still emit the exact load. The
structured temporary-chain test likewise rejects its former `MOV x6, x8` and
`MOV x7, x8` instructions while preserving results and TMP sentinels.

In 101 order-rotated native-CPU `max-perf` A/B pairs against direct-result
checkpoint `ea87450`, conditional composed recurrence falls from 5.124 ms to
4.252 ms (paired median -16.68 percent) and linear composed recurrence from
3.075 ms to 2.783 ms (-9.71 percent). Simple conditional recurrence improves
by 1.06 percent; the already branch-bound structured expression is flat at
-0.18 percent. Separate 101-run comparisons record 4.295 ms versus 37.342 ms
for PHP tracing JIT on conditional composition (8.69x faster), and 2.797 ms
versus 35.429 ms on linear composition (12.67x faster), with identical output.

The checkpoint passes 142 library tests, 81 ARM64/JIT integration tests, all
four application corpus tests, and `cargo check --all-features`.

Signed small-immediate checkpoint (2026-08-04): `Add` and `Subtract` with a
right-hand constant whose magnitude fits ARM64 imm12 no longer materialize the
constant in a register. Positive and negative PHP constants select the exact
`ADD` or `SUB` opcode (`x + -N` becomes `SUB #N`, `x - -N` becomes `ADD #N`).
The same selection uses `ADDS`/`SUBS` in checked code, preserving the overflow
flag and exact canonical side exit. Constants outside `0..=4095`, including
`i64::MIN`, retain register materialization.

Unit coverage fixes the signed selection at both limits and rejects unrelated
operations. Generated-code checks require exact `ADD x4, x8, #1`,
`SUB x4, x8, #2` and `ADD x5, x8, #11` words in the structured branch body.
The existing real-PHP overflow replay test exercises the new checked immediate
path and still resumes the precise source operation.

In 101 order-rotated native-CPU `max-perf` A/B pairs against resident-operand
checkpoint `baf799f`, conditional composed recurrence improves by 1.95
percent, simple conditional recurrence by 2.66 percent, linear composed
recurrence by 0.29 percent and the structured branch expression by 0.25
percent. The modest but consistent result matches the bounded change: one
instruction disappears only for each eligible add or subtract, while constant
multiplication still needs a register.

The checkpoint passes 143 library tests, 81 ARM64/JIT integration tests, all
four application corpus tests, and `cargo check --all-features`.

Carried-aware multiply strength-reduction checkpoint (2026-08-04): positive
constants of the form `1 + 2^shift` can lower range-proven multiplication to a
single `ADD Xd, Xn, Xn, LSL #shift`. Thus `x * 3`, `x * 5`, `x * 9` and the
same bounded family no longer require constant materialization plus `MUL` when
the result is outside the loop-carried critical dependency chain. Checked code
always retains `MUL + SMULH` and its exact overflow side exit.

The profitability guard is data-flow based. Starting from the carried CV mask,
the compiler computes the conservative transitive closure of operations whose
outputs feed those slots. A dependent multiply keeps `MUL`; an independent
multiply in the same native body may use shifted `ADD`. The fixed-point slot
set stays monotonic so multiple branch definitions and reused shadow slots can
only make the classification more conservative.

This distinction is measured rather than theoretical. Applying shifted `ADD`
to every range-proven multiply improved the branch expression by 11.52 percent
but regressed conditional composed recurrence by 3.02 percent and linear
composed recurrence by 8.64 percent. That global variant was rejected. The
carried-aware variant retains the branch gain while restoring the recurrence
controls: in 101 order-rotated `max-perf` A/B pairs against `d258f88`, the
branch expression improves from 4.944 ms to 4.364 ms (-11.44 percent), while
the three recurrence controls range from -0.51 to +0.04 percent.

Across the whole sequence since `fdf66e0`, fresh 101-run A/B pairs record
-31.94 percent for the structured branch expression, -18.13 percent for
conditional composed recurrence, -10.40 percent for simple conditional
recurrence and -9.79 percent for linear composed recurrence. The current
branch result is 4.437 ms versus 33.804 ms for PHP tracing JIT, or 7.62x
faster, with identical output.

Generated-code tests require shifted `ADD` for all three acyclic branch
multiplications and simultaneously require a normal `MUL` for a recurrence
dependency next to an independent shifted multiply. The checkpoint passes 144
library tests, 81 ARM64/JIT integration tests, all four application corpus
tests, and `cargo check --all-features`.

Invariant scalar operand checkpoint (2026-08-04): the range-proven straight
loop now identifies body inputs that are neither the induction variable nor
written anywhere in the native body. It counts their static uses, selects the
most frequently read slot with deterministic tie-breaking, loads it once into
`x10` before the native backedge target and resolves every later operand use
directly to that register. The checked lowering is unchanged because its
overflow and division guards still own `x10`.

The invariant register is deliberately outside the fixed publication pool.
Consequently the existing three-register capacity for carried values and
structured merge results remains unchanged, the invariant is never published
at an exit, and per-operation temporary liveness cannot evict it. A direct
generated-code test requires exactly one `LDR x10` before the loop, rejects the
former repeated `x6`/`x7` slot loads, executes both structured paths, verifies
the exact published result and proves that shadow TMP slots remain untouched.

Longer native-CPU `max-perf` A/B runs against `51fbd02` show the intended
shape rather than a universal benchmark win. In 301 order-rotated pairs, the
structured branch expression moves from 4.329 ms to 4.166 ms (paired median
-3.92 percent) and conditional composed recurrence from 4.080 ms to 3.979 ms
(-2.32 percent). Linear composed recurrence is neutral at 2.758 versus 2.761
ms. A separate 501-pair run of the smallest conditional recurrence records
3.675 versus 3.682 ms (+1.61 percent). A 16-byte loop-alignment experiment was
neutral relative to the invariant build (+0.05 percent) and was rejected.
This remaining small-body holdout should be handled by a general native
register-pressure/profitability model, not a benchmark-name exception.

The checkpoint passes 145 library tests, 81 ARM64/JIT integration tests, all
four application corpus tests, and `cargo check --all-features`.

Second invariant register checkpoint (2026-08-04): invariant discovery now
ranks the two most frequently read immutable body slots in one bounded linear
scan. The primary invariant retains `x10`. The secondary may use `x9`, but only
after the ARM64 lowering proves that no modulo operation or composed bitwise
condition in the native body can clobber that auxiliary register. Checked code
still allocates neither invariant, and the three fixed publication registers
remain independent of both.

Assembly coverage exercises both directions of the contract. A structured
program with two invariant slots must contain exactly one entry `LDR` into
each of `x10` and `x9`, no corresponding body loads through `x6` or `x7`, the
exact final values and untouched TMP shadows. A second program adds modulo and
requires that `x9` is excluded from the invariant pool, the second invariant
retains its ordinary operand load, and modulo execution remains exact. A real
typed PHP function test passes the bound, cutoff and offset as runtime
arguments and confirms one range-proven native call with no side exits.

The permanent `bench_two_invariant_operands_loop.php` prevents constant
folding by using those function arguments. In 301 order-rotated native-CPU
`max-perf` pairs against the one-invariant commit `f86b723`, identical
`199999959` output moves from 4.433 ms to 4.354 ms (paired median -1.92
percent). Four existing one-or-zero-invariant controls remain within -0.04 to
+0.18 percent. In a separate 101-pair comparison, the resulting RPHP records
4.439 ms versus 10.962 ms for PHP tracing JIT, or 2.47x faster.

The checkpoint passes 146 library tests, 82 ARM64/JIT integration tests, all
four application corpus tests, and `cargo check --all-features`.

Commutative constant normalization checkpoint (2026-08-04): ARM64 lowering
now canonicalizes `Const + Slot` and `Const * Slot` to put the constant on the
right before selecting instructions. This makes the existing signed `ADD`/
`SUB` immediate and carried-aware multiply strength-reduction paths available
to ordinary PHP expressions regardless of source operand order. Subtraction,
division and modulo retain their exact original order; checked addition uses
the same overflow side exit after swapping mathematically commutative inputs.

Helper tests fix the permitted and forbidden swaps. A generated-code test then
requires exact `ADD x8, x3, #7` and `ADD x8, x8, x8, LSL #1` words for
`7 + i` followed by `3 * result`, executes the program and proves that both
temporary shadow slots remain untouched. A real reversed-expression PHP test
confirms the compiler still selects one range-proven native region with no
side exits. The permanent `bench_reversed_commutative_loop.php` keeps this
source-order holdout visible.

In 301 order-rotated native-CPU `max-perf` pairs against `6ef9d01`, the unseen
reversed benchmark preserves `49999993,149999990` output and falls from 5.519
ms to 4.750 ms (paired median -13.48 percent). The already canonical original
is neutral at -0.05 percent. A separate 101-pair run records 6.183 ms for RPHP
and 47.588 ms for PHP tracing JIT, making RPHP 7.74x faster on the reversed
form.

The checkpoint passes 147 library tests, 83 ARM64/JIT integration tests, all
four application corpus tests, and `cargo check --all-features`.

### Cross-architecture JIT freeze

ARM64 performance development is frozen at commit `1aa361e` while the native
architecture is split into target-neutral planning and target-specific
lowering. Bug fixes, correctness work and measurements remain allowed, but no
new general optimization may depend directly on ARM64 register names,
instruction encodings or its physical register count.

The initial x86-64 Linux builder uses the same Rust 1.93.1 toolchain as the
ARM64 development machine. The unmodified source already passes a native
`max-perf --all-features` build, 109 target-independent library tests, all four
application corpus tests and `cargo check --all-features` there. The remaining
38 library tests and all 83 focused JIT integration tests are currently hidden
behind the `aarch64 + macOS` platform gate; making their planning and semantic
parts target-independent is the first parity metric.

The first extraction checkpoint moves the fixed-capacity straight-loop IR,
operation input/output masks and invariant-slot ranking into `jit::straight`.
The shared module contains no register names, instruction words, executable
memory or platform ABI state. ARM64 consumes the same definitions and analyses
without a code-generation change, while native x86-64 compiles and exercises
their target-independent tests before its encoder exists. The checkpoint
passes 149 ARM64 library tests, all 83 ARM64/JIT integration tests, all four
application corpus tests and `cargo check --all-features`; x86-64 passes 111
library tests, a native `max-perf --all-features` build and
`cargo check --all-features`.

The second extraction checkpoint moves the complete straight-loop range proof,
liveness, carried-dependency and publication planning into
`jit::straight::{range,liveness}`. Pure analysis tests now run on both targets;
only assertions over ARM64 instruction words and execution through the ARM64
ABI retain architecture gates. ARM64 remains at 149 library tests, while
x86-64 rises from 111 to 123 library tests and keeps a clean
`cargo check --all-features`. This completes the first frozen workstream item.
The next boundary is a small backend contract followed by the first x86-64
SysV range-proven scalar-loop vertical slice.

The backend bootstrap checkpoint extracts one W^X executable-memory owner for
macOS/ARM64 and Linux/x86-64, with an explicit architecture-specific
instruction-cache boundary. A dependency-free x86-64 encoder then emits and
executes `(a + b) * c` through the System V AMD64 ABI. Tests require the exact
`MOV`/`ADD`/`IMUL`/`RET` bytes, correct REX extension bits and native results on
physical x86-64 hardware. This is deliberately not counted as PHP loop support
yet; it proves encoder, executable memory and ABI ownership before straight IR
lowering is added. ARM64 passes 150 library tests, all 83 focused JIT tests and
the four-test corpus; x86-64 passes 127 library tests and a warning-free
`cargo check --all-features`.

The first x86-64 straight-IR vertical slice consumes the shared
`NativeStraightLongLoopConfig` and complete-range proof for a single additive
`BinaryAssign` recurrence. The generated SysV code loads induction and carried
state from the 64-Long shadow, runs the complete proven range, and publishes
the induction, destination and distinct result slots exactly once. Empty
ranges stay in Rust, while an unproven overflow case is rejected before native
entry and leaves every slot unchanged. Physical x86-64 tests cover canonical
and reversed operands, signed induction ranges, exact branch displacements and
publication stores. x86-64 now passes 130 library tests. This slice is not yet
wired into VM dispatch and deliberately has no safepoint polling; the next
increment adds precise checked side exits before production integration.

The first native x86-64 performance checkpoint uses an AMD Ryzen 9 7950X, a
fixed logical CPU, the `max-perf` profile and 101 samples of the equivalent
10-million-iteration additive recurrence. All paths produce
`49999995000010`. The direct x86 slice records a 1.875893 ms median
(1.872263/1.894114 ms p10/p90, 0.187589 ns per iteration); executable-code
creation has a 3.640 microsecond median. The current RPHP CLI path, which does
not dispatch to this x86 backend yet, records 11.761427 ms. PHP 8.3.6 records
33.901930 ms without JIT and 12.964010 ms with CLI tracing JIT enabled and
verified. The isolated slice is therefore 6.27x faster than current RPHP CLI,
18.07x faster than PHP without JIT and 6.91x faster than PHP tracing JIT on
this kernel. RPHP CLI itself is 2.88x faster than PHP without JIT and 1.10x
faster than PHP tracing JIT.

The direct result is a code-generation kernel measurement, not an end-to-end
RPHP claim: it includes the shared range proof and native execution but not VM
dispatch, safepoint polling or a precise checked side exit. The compile cost is
reported separately and would add only 0.194 percent to one measured loop.
`examples/bench_x86_straight.rs` and
`benches/bench_x86_straight_equivalent.php` keep the comparison reproducible.

The precise x86-64 side-exit checkpoint (2026-08-04) emits two entries in one
executable allocation. A successful shared complete-range proof selects the
original unchecked instruction stream, which contains no overflow branch. An
unproven range selects a checked stream that computes each candidate in a
temporary register and uses x86 `JO` before committing carried state. On
failure it publishes the current induction value, the last successful
destination and any distinct pre-operation result, then reports operation zero
through the shared `OperationSideExit` contract. It can therefore resume the
canonical VM at the exact failed PHP operation instead of rejecting native
entry in Rust.

Physical AMD64 tests cover failure on the first operation and after a
successful iteration, including a distinct result slot. The target now passes
131 library tests; ARM64 remains at 150. A fresh independent 101-sample run of
the unchanged range-proven entry records a 1.892093 ms median
(1.883214/1.899284 ms p10/p90), approximately 0.9 percent above the earlier
1.875893 ms checkpoint and within ordinary run-to-run variation. Compilation,
which now creates both streams, records a 3.830 microsecond median.

PHP 8.5 server baseline (2026-08-04): Ubuntu 24.04 on the same AMD Ryzen 9
7950X now has co-installable `/usr/bin/php8.5` version 8.5.9 with OPcache,
while the system `/usr/bin/php` alternative deliberately remains at 8.3.6.
For the same pinned 10-million-iteration PHP kernel and 101 samples, PHP 8.5.9
records a 32.072067 ms median without JIT (31.821966/34.128904 ms p10/p90) and
a 22.450209 ms median with CLI tracing JIT
(22.337198/22.573948 ms p10/p90). `opcache_get_status()` verified that tracing
JIT was enabled. This particular 8.5 build/kernel is slower under tracing than
the earlier PHP 8.3.6 result, so both versioned baselines remain recorded
rather than replacing the faster reference.

The x86-64 chunk checkpoint adds separate unchecked and overflow-checked ABI
entries that accept a non-zero iteration budget in SysV `RSI`. They publish
the same exact shadow state on `ChunkExhausted`, give completion priority when
the final iteration exactly consumes the budget, and can resume through any
number of chunks. The unbudgeted benchmark entry remains byte-for-byte free of
the budget decrement and branch. Zero budgets are rejected before native
entry, while the checked chunk entry retains the precise operation side exit.

The physical target passes 133 library tests. In a pinned 101-sample run, the
unbudgeted entry records a 1.886724 ms median
(1.882603/1.911803 ms p10/p90), while one budgeted entry spanning the same ten
million iterations records 1.874604 ms
(1.871994/1.883493 ms p10/p90). The safepoint-capable loop is therefore not
slower in this kernel; repeated VM chunk returns remain a separate dispatch
cost to measure after integration. Emitting all four entries raises median
code creation to 4.290 microseconds, still reported outside execution time.

The first end-to-end x86-64 VM checkpoint (2026-08-04) connects the backend to
ordinary `QuickLongAccumulateLoop` and conservatively to compatible
`QuickLongOpsLoop` plans. Bounds may now be constants or guarded Long shadow
slots; the generated entry loads a dynamic bound on every invocation and
rejects a bound written by the loop body. This covers the common PHP form
`$n = ...; for ($i = 0; $i < $n; $i++)` without constant propagation or a
benchmark-specific recognizer.

Range-proven execution enters native code once. An internal x86 countdown
checks the VM's byte interrupt flag every 1,024 iterations and returns
`ChunkExhausted` only for a real pending interrupt; loop completion has
priority at the same boundary. Checked, unproven execution retains resumable
chunk entries and precise overflow side exits. A dedicated Linux integration
test requires a dynamic-bound PHP loop to compile, make exactly one native
call, account for approximately 98 logical safepoint chunks, and produce the
canonical result. The physical target passes 136 library tests, that x86 JIT
integration test and all four application corpus tests.

On the pinned Ryzen 9 7950X, 101 end-to-end samples of the existing
10-million-iteration PHP source record 3.818989 ms median
(3.798008/3.839970 ms p10/p90), down 67.53 percent or 3.08x from the prior
11.761427 ms RPHP CLI checkpoint. The same source is now 8.88x faster than PHP
8.3.6 without JIT, 3.40x faster than PHP 8.3.6 tracing JIT, 8.40x faster than
PHP 8.5.9 without JIT and 5.88x faster than the measured PHP 8.5.9 tracing JIT
build. The remaining 1.932 ms above the 1.887 ms direct kernel includes warmup,
range proof, native-code creation, VM publication and the benchmark's timing
calls; it is no longer repeated Rust/native chunk dispatch.

The x86 linear-IR checkpoint then retains the one-register additive recurrence
as a fast specialization while adding a memory-backed general lowering for up
to the shared 48-operation capacity. Sequential `Move`, `Add`, `Subtract` and
`Multiply` operations observe prior outputs in exact PHP order; the live
induction value remains register-resident instead of reading its stale
publication slot. Range-proven code uses the same one-call polling contract.
Checked code commits only successful operations and encodes the exact failed
operation index in its native status, so the VM can resume either a composed
term or its following accumulation instruction. Post-increment result slots
are published at complete iteration boundaries.

Real PHP integration tests cover `$sum += $i + 7` and
`$sum += $i + $offset`, each with a dynamic bound, one native call and one
range-proof evaluation. Physical unit tests additionally cover subtraction,
multiplication, failure in operation one after operation zero committed, and
composed-state publication at an interrupt safepoint. x86-64 now passes 140
library tests and two focused JIT integration tests.

In 101 order-rotated pinned-CPU pairs, the permanent two-operation CV benchmark
records 5.986214 ms with native linear IR
(5.909681/6.139040 ms p10/p90), versus 14.131784 ms for the immediately prior
typed executor (13.947248/14.752865 ms). This is a 57.64 percent reduction or
2.36x speedup. A separate 51-sample control keeps the original specialized
recurrence at 3.824711 ms median, within 0.15 percent of its 3.818989 ms
checkpoint.

The structured-control-flow checkpoint adds all four shared scalar comparisons,
finite `BitwiseAnd` condition operands, forward `BranchUnless` and `Jump`, and
trace `Guard` exits. Every generated branch targets a validated shared-IR
operation boundary. Guard failures publish the exact induction state and carry
the failing operation index through the native status, while all successful
prior shadow writes remain committed. This makes ordinary PHP `if`/`else`
bodies eligible through the existing `QuickLongOps` planner; it is not a
benchmark-only recognizer.

On the pinned x86-64 server, the existing 10M conditional-recurrence benchmark
falls from the immediately prior binary's 97.887754 ms median
(97.493410/98.395109 ms p10/p90) to 6.877661 ms
(6.849051/6.942987 ms), a 92.97 percent reduction or 14.23x speedup over the
typed fallback. The same 101-sample workload records 53.598881 ms in PHP 8.5.9
without JIT and 33.494949 ms with tracing JIT, so this structured RPHP kernel is
4.87x faster than PHP 8.5 tracing JIT on the measured host. x86-64 passes 143
library tests, three real-PHP JIT integration tests and the four-test corpus.

The scalar-arithmetic parity checkpoint moves the generic induction register
away from x86-64's architectural `RAX:RDX` division pair and lowers
`IntDivide`, signed `Modulo`, standalone constant modulo and `BitwiseXor`.
Checked entries guard zero divisors and the `MIN / -1` overflow before `idiv`,
preserving the exact pre-operation shadow and failed-operation index. Constant
power-of-two remainder uses the branchless signed identity
`bias = sign(value) & mask; ((value + bias) & mask) - bias`, so negative PHP
remainders remain truncating rather than being replaced with an incorrect
unsigned mask. The dispatcher now offers eligible scalar regions to native JIT
before the older conditional Rust kernel and retains that kernel as the normal
fallback when native compilation declines the shared IR.

The unchanged 10M `bench_modulo_branch_loop.php` now takes the native path. In
101 pinned, order-rotated pairs it falls from the pre-arithmetic fallback's
26.400328 ms median (26.265383/26.561022 ms p10/p90) to 7.194757 ms
(7.033348/7.470369 ms), a 72.75 percent reduction or 3.67x speedup. An isolated
A/B shows the signed power-of-two lowering reducing the intermediate `idiv`
version from 13.272762 to 7.282972 ms median. PHP 8.5.9 records 50.990820 ms
without JIT and 11.234999 ms with tracing JIT on the same workload, making the
final RPHP result about 1.56x faster than PHP tracing JIT. x86-64 now passes 147
library tests, four real-PHP JIT integration tests and the four-test corpus.

The standalone scalar-function checkpoint then enables the same typed
`ScalarLongFunctionPlan` cache on x86-64 as on ARM64. After the shared 64-call
hotness threshold, straight and conditional function/method leaves lower all
six scalar operations, all four comparisons and bitmask predicates. The SysV
entry receives guarded input, output and private eight-word temporary pointers;
checked failures return before writing output, so the caller can resume the
canonical PHP call transactionally. A real conditional PHP function confirms
the cache enters native code on call 64 and keeps its side-exit counters exact.

On the pre-existing 10M `bench_scalar_call_branch_standalone.php`, 101 pinned,
order-rotated pairs reduce the previous x86 path from 179.379702 ms median
(178.663254/183.989286 ms p10/p90) to 125.528336 ms
(124.723196/128.692627 ms), a 30.02 percent reduction or 1.43x speedup. PHP
8.5.9 records 180.901051 ms without JIT and 63.658953 ms with tracing JIT. The
remaining roughly 1.97x tracing-JIT advantage is therefore at the callsite:
RPHP still crosses Rust/native ABI once per standalone call, while a tracing
compiler can inline the selected leaf into its caller. x86-64 now passes 150
library tests, five real-PHP JIT integration tests and the four-test corpus.

The call-composition checkpoint removes that per-call boundary inside supported
accumulate loops. The existing ARM call-tree builder was target-neutral through
its final `NativeStraightLongLoopConfig`, so its function/method target guards,
nested argument lowering, conditional selects and root-call replay are now
shared with x86-64. The call-target capacity moves into the shared IR ABI. x86
reuses its checked structured-scalar backend in 1,024-iteration chunks and
records logical scalar calls in bulk; overflow before the sum resumes the
canonical root call, while later failure publishes the completed term or sum
at its exact PHP instruction boundary.

Real-PHP tests cover a direct scalar function, nested function tree, nested
method tree and an overflowing call whose canonical replay raises the expected
PHP error. The pre-existing 10M `bench_scalar_call_loop.php` falls from
48.548937 ms median (48.233032/48.994780 ms p10/p90) to 17.046928 ms
(16.949892/17.168283 ms), a 64.89 percent reduction or 2.85x speedup. PHP 8.5.9
records 110.527992 ms without JIT and 35.614967 ms with tracing JIT, so composed
RPHP is about 2.09x faster than PHP tracing JIT on this measured call loop.
x86-64 now passes 150 library tests, nine real-PHP JIT integration tests and the
four-test corpus.

The finite-String/hash-context checkpoint makes the existing mixed-region
builder target-neutral and adds the same `StringToken`, `StringLength`,
`HashLoad` and `HashStore` IR lowering to x86-64. The SysV chunk ABI receives a
per-dispatch table of already guarded Long payload pointers. Generated code
keeps that table in callee-saved `R12`, including across x86's architectural
`RAX:RDX` signed-division pair, and restores it on completion, chunk return and
every precise side exit. Token selection admits at most four guarded String
values; an unknown token exits before the failing operation. Structural array
changes and missing, referenced or non-Long entries are rejected before native
entry, so no heap pointer is embedded in generated code.

The permanent one-million-iteration mixed benchmark combines a typed method,
`strlen($key)`, two alternating String keys, a guarded hash load/update/store
and a cold trace edge. In 51 pinned, order-rotated runs, the immediately prior
x86 binary records a 32.307386 ms median (32.182932/32.695055 ms p10/p90), while
the new contextual region records 2.765656 ms
(2.711773/2.815962 ms), a 91.44 percent reduction or 11.68x speedup. PHP 8.5.9
records 38.563013 ms without JIT and 12.542009 ms with tracing JIT on the same
source, making RPHP about 4.53x faster than PHP tracing JIT for this shape. A
second real-PHP test takes a cold edge after a hash store and proves canonical
replay neither loses nor duplicates that update. x86-64 passes 152 library
tests, fourteen real-PHP JIT integration tests and the four-test corpus; ARM64
continues to pass 150 library tests, 83 JIT integration tests and the corpus.

The same checkpoint also admits the existing composed property, virtual-object
and multi-method builders without another x86-specific runtime path. Three ARM
application holdouts now enter one native x86 region. Across 31 pinned,
order-rotated runs, order/virtual-object processing falls from 88.551044 to
4.112244 ms (21.53x) versus 51.054955 ms for PHP tracing JIT; the stateful
property ledger falls from 24.779081 to 2.021313 ms (12.26x) versus 8.662939 ms
for PHP tracing JIT; and the multi-method routing pipeline falls from 79.590082
to 7.739544 ms (10.28x) versus 26.085854 ms for PHP tracing JIT. Every result
matches across all modes. This validates multiplicative backend reuse: the
ARM-side typed region descriptions required only the common builder gate and
the four contextual IR lowerings, not benchmark- or corpus-specific machine
code.

This is still not full ARM64 feature parity. Finite guarded String/hash,
property and virtual-object contexts now share the backend contract, but
arbitrary dynamic String domains, structural array writes and the remaining
ARM-specific publication/register optimizations continue through the typed or
canonical executor and define the next parity steps.

The frozen workstream is:

1. move native loop IR, range proof, liveness, carried-dependency analysis,
   invariant discovery and publication planning out of the ARM64 module;
2. define a backend contract for physical registers, clobbers, instruction
   selection, executable memory and ABI entry/exit;
3. implement an x86-64 SysV vertical slice covering a range-proven scalar loop,
   branches, exact publication and precise side exit;
4. run one shared semantic and planning matrix against both ARM64 and x86-64,
   with separate generated-instruction assertions per backend;
5. reopen performance development only after new shared optimizations can be
   lowered or conservatively rejected by both backends.

x86-64 performance claims require native hardware. Emulation may validate
encoding and semantics but is not accepted for benchmark comparisons. Windows
x86-64 remains a later ABI backend rather than part of this first parity slice.

### Nice to have: persistent compiled artifacts

After the in-memory typed-region JIT is correct and profitable, consider a
versioned `.rphpc` artifact that can preserve optimization work across process
and server restarts. The first useful form should store portable typed IR,
profiles, source and dependency hashes, and the exact RPHP runtime ABI version.
It may later include target-specific ARM64 and x86-64 native-code sections,
constant pools, relocation records, and runtime-helper imports.

Loading must validate the source graph, RPHP/ABI version, target architecture,
CPU features, and optimization configuration before any cached code becomes
executable. A mismatch discards the affected cache entry and returns to normal
planning or JIT compilation; baseline bytecode remains authoritative. Native
code must not be restored as an unchecked raw memory dump because ASLR and
runtime-helper addresses change between processes.

This is not a prerequisite for the minimal JIT. Its purpose is to remove warmup
and compilation latency for long-lived production deployments. A later build
mode may package the same artifact with the RPHP runtime as a standalone
executable, without requiring an external compiler backend or package.

## Phase 4.5: bounded coroutine architecture branch

After the minimal typed-region JIT is stable, pause feature expansion briefly
to prove a pay-for-use coroutine substrate before compatibility work makes
frame ownership harder to change. The target is Go-like cheap logical tasks
and context hand-off: dormant execution owns lazy VM stack segments, while a
switch exchanges active context pointers in O(1) and never copies a frame
chain or OS stack.

Programs that do not create coroutines must retain the current frame ABI, add
no allocation and regress by no more than one percent. Compile-time
`may_suspend` propagation keeps ordinary calls and native regions free of a
coroutine poll. Known suspension points end a JIT region through an exact side
exit until explicit spill maps are proven. The first scheduler is cooperative
and single-threaded; readiness I/O, channels and optional M:N work stealing are
separate later milestones.

This phase is deliberately short and gated by measurements. The detailed
context model, milestones, correctness requirements and the initial 150 ns
internal hand-off target are maintained in
[the runtime architecture roadmap](roadmap-runtime-architecture.md).

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
