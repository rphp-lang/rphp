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

Coverage telemetry checkpoint (2026-08-05): the opt-in `vm-stats` build now
measures the shared quick/JIT admission pipeline instead of treating a fast or
slow benchmark as evidence of coverage. It records static backward-loop
candidates and admitted plan shapes, stable dominant rejection families,
runtime-weighted executions of rejected backedges, straight-region admission
at the exact `no_typed_span`/`no_dense_kernel` stage, optimized-region entries,
and successful native executions plus side exits by shape. Scalar Long/Double
native dispatch is included alongside loop regions. Rejected ordinary `Jmp`
instructions use their otherwise-unused `extended_value` only in a
`vm-stats` build; ordinary builds contain neither the marker nor its runtime
branch or counters.

The four order/ledger corpus variants each expose one hot loop, admit it as a
general typed-ops region, execute it once natively for the remaining 499,967
iterations, and record zero side exits and zero rejected hot backedges. Each
also exposes four cold post-loop `FetchDimR` starts that fail with
`no_typed_span`; these are result formatting, not the timed hot-loop gap. In
contrast, the JSON benchmark records one `json_pipeline` planner rejection and
10,000 executions of that rejected backedge. This confirms both sides of the
report and makes JSON/callback/collection coverage a measurable architectural
extension rather than a benchmark-driven guess. Timings from `vm-stats` are
diagnostic only because the enabled build performs atomic counting.

### Planned JIT extension: fused collection, JSON, and callback pipelines

After the minimal scalar/object JIT is stable, extend the same guarded region
model to high-level library pipelines. The goal is to optimize ordinary PHP
composition such as `array_filter` -> `array_map` -> `array_reduce` ->
`json_encode`, not to add isolated benchmark-only native helpers. A chain is
planned once, keeps eligible elements and aggregates in typed native state,
and materializes an intermediate PHP array or string only when semantics make
it observable.

The collection layer should cover `array_map`, `array_filter`, `array_reduce`,
`array_walk` and then the remaining callback-taking array functions through a
shared callback/pipeline IR. It must preserve PHP key and iteration order,
packed/hash behavior, callback arity, COW, references, mutations, exceptions
and exact partial-progress visibility. Unknown or by-reference callbacks,
structural writes, callback replacement and observable intermediate values
end or reject fusion before behavior can diverge.

Callback lowering is a general JIT capability rather than an array-only
special case. All supported standard-library functions that accept a callback
should use one guarded callback ABI for named functions, closures with
captures, instance/static methods, inherited methods and invokable objects.
Identity, receiver class, method cache, capture layout, signature and
by-reference contracts are guarded at region entry. Monomorphic pure callbacks
may inline into the caller region; stable non-inlinable callbacks use one
specialized call stub; polymorphic or side-effect-ambiguous sites retain the
canonical call protocol.

`json_encode` should gain typed encoders for proven scalar, packed-array,
stable hash/object and repeated-schema inputs, with option/depth guards and a
streaming output buffer that avoids temporary strings. `json_decode` should
specialize repeated input/schema shapes into directly allocated RPHP arrays or
objects and reuse proven key/layout metadata. Invalid UTF-8, numeric corner
cases, depth/errors, flags, `JsonSerializable`, visibility/magic behavior and
throwing modes remain exact side exits or canonical helpers. JSON
specialization must never bypass PHP-visible error state.

Permanent benchmarks must include isolated callback cost, multi-stage array
chains, JSON-only encode/decode, and an end-to-end decode -> callback pipeline
-> encode workload in all four PHP/RPHP JIT/no-JIT modes. Admission requires a
win after allocation counts and peak memory are included, no regression for an
unfused single library call, and differential results against reference PHP
for callbacks, references, exceptions, flags, malformed JSON and mixed
packed/hash inputs.

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
continues to pass 151 library tests, 83 JIT integration tests and the corpus.

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

The first x86 register-residency checkpoint consumes the shared invariant-slot
ranking directly. Pure linear and forward-structured scalar entries load their
two most frequently read, non-written CV slots once into callee-saved
`R13/R14`; every completion, chunk return and exact side exit restores the SysV
register state. Shadow stores remain unchanged, deliberately separating safe
load elimination from the later publication problem. The shared ranking now
excludes `StringToken` shadow outputs as writes, closing a latent distinction
between public output masks and the actual private shadow state.

An initial unrestricted version regressed the already tiny mixed String/hash
region by a paired 3.23 percent because its chunk/context prologue was more
expensive than the saved loads. The final profitability rule mirrors ARM64 and
admits only scalar/structured-scalar bodies. In 201 pinned, order-rotated A/B
pairs, the permanent scalar expression-chain benchmark falls from 13.616800 to
13.463974 ms (paired median -1.11 percent), while the binary body changes from
13.484001 to 13.447285 ms (paired median -0.19 percent). The excluded mixed
benchmark remains flat at 2.778292 versus 2.783298 ms (paired +0.21 percent,
with overlapping p10/p90 ranges). x86-64 passes 154 library tests, fourteen JIT
integration tests and the four-test corpus.

The next x86 residency checkpoint gives the remaining-range program its own
cache entry and consumes the shared `publication_mask` and `carried_mask`
contract instead of silently discarding both. For a proven linear scalar body,
up to three loop-carried CVs live in callee-saved `R13-R15`: they are loaded
once, updated in registers and published only on completion or the 1,024-step
interrupt boundary. Any unused resident register continues to cache a ranked
invariant, so carried and invariant allocation are one capacity decision rather
than mutually exclusive modes. Checked, budgeted and side-exit entries retain
their original exact shadow behavior.

The same polling lowering now consumes the shared linear liveness plan. The
latest path-local scalar result is forwarded through caller-saved `RDX`, and a
dead compiler temporary is not written merely to be read by the immediately
following operation. A dedicated three-recurrence test covers completion,
interrupt publication and resume; the private temporary slots deliberately
remain unpublished. Forward-structured bodies still compile through the
range-proven cache, but conservatively set the backend's effective carried mask
to zero. This preserves all prior structured JIT admission while fixed-register
phi values remain the next x86 parity step.

Across 201 pinned, order-rotated A/B pairs against the preceding x86 binary,
the two-state scalar recurrence falls from 7.725480 to 4.156590 ms (paired
-46.19 percent), the composed recurrence from 7.847550 to 5.091430 ms (paired
-35.15 percent), the dependent recurrence from 7.761480 to 4.154440 ms (paired
-46.48 percent), and the scalar expression chain from 13.404600 to 8.076910 ms
(paired -39.75 percent). Independent 201-run PHP 8.5 tracing-JIT medians on the
same pinned CPU are respectively 39.147100, 48.911100, 38.472900 and 56.242000
ms. The checkpoint passes 155 x86 library tests, all fourteen x86 JIT
integration tests and the four-test corpus.

The following x86 checkpoint extends the same fixed-register contract across
forward-structured scalar control flow. A carried CV owns one of `R13-R15` for
the whole polling entry: an executed definition replaces its register value,
while a skipped definition deliberately retains the previous iteration value.
Branch conditions therefore read current carried state directly and multiple
forward definitions merge into the same physical register without a shadow
round trip. This is the structured phi case; checked and unsupported guarded
bodies still select the conservative shadow lowering.

Structured path-local temporaries reuse the shared basic-block and local-output
analyses. `RDX` forwards the latest scalar result only inside one basic block,
and every branch fallthrough, jump successor and merge target invalidates that
mapping. A temporary proven local to the block is not stored in the shadow;
visible and non-local aliases remain materialized. Dedicated x86 tests cover a
carried condition with a skipped update, exact interrupt/resume publication,
and a multi-operation branch expression whose private temporary slots remain
unchanged.

In 201 pinned, order-rotated three-way runs, comparing the preceding committed
x86 binary, the intermediate carried-phi binary and the final phi-plus-local
binary, conditional recurrence moves from 6.907940 through 5.496030 to
5.420450 ms (final paired -21.52 percent). Conditional composed recurrence
moves from 9.803530 through 8.182290 to 7.632020 ms (-22.15 percent), a guard
reading another carried recurrence from 6.852630 through 5.475040 to 4.465580
ms (-34.82 percent), the general structured branch expression from 11.642900
through 11.656800 to 7.979390 ms (-31.47 percent), and modulo branch from
7.248160 through 6.902930 to 6.689550 ms (-7.65 percent). Same-host PHP 8.5
tracing-JIT medians are respectively 32.898900, 48.981000, 34.140800,
41.789100 and 11.091900 ms. x86-64 passes 157 library tests, all fourteen JIT
integration tests and the four-test corpus.

The next x86 publication checkpoint admits visible structured phi values that
are definitely overwritten on every path but are not carried into the next
iteration. The three fixed publication registers are allocated in semantic
priority order: carried state, safe visible phi groups, then invariants. Slots
defined by the same operation set may share one register because every native
operation exposes one scalar result and its output aliases. These registers are
not treated as initialized on entry; they become readable only after the
forward dataflow proves their definitions on every predecessor reaching an
operation. Empty ranges use a separate completion path and do not publish
uninitialized deferred values.

Admission is deliberately stricter than definite exit assignment alone. A
candidate is rejected when an input could observe a path-specific shadow value
before a safe merge, except for an immediate same-block consumer forwarded by
`RDX`. Visible-phi deferral is also an explicit remaining-range compilation
mode, so the ordinary five-entry program retains its existing invariant
allocation. Tests cover mutually exclusive definitions, two result/destination
alias groups sharing fixed registers, absence of per-loop `RAX` stores to all
four visible slots, empty-range preservation, and rejection of a real
stale-shadow merge.

Across 201 pinned, order-rotated pairs against the preceding structured-state
binary, `bench_straight_branch_expression_loop.php` moves from 7.943150 to
7.819410 ms (paired -1.52 percent) and the routing application holdout from
7.681850 to 7.560010 ms (-1.56 percent). Conditional composed, modulo branch,
order corpus and ledger corpus controls remain within +0.16, +0.27, +0.16 and
+0.33 percent respectively. The same-host PHP 8.5 tracing-JIT branch-expression
median is 41.789100 ms, making the new x86 path 5.34x faster. x86-64 passes 159
library tests, all fourteen JIT integration tests and the four-test corpus.

The x86 direct structured-result checkpoint makes fixed publication registers
the actual arithmetic destination instead of computing in `RAX` and copying
afterwards. Range-proven polling entries lower move, add, subtract, multiply,
XOR and power-of-two signed remainder directly into `R13`-`R15`. Checked and
budgeted entries retain the old temporary result so an overflow side exit can
only publish the last successful state; general `IDIV` also remains on its
architectural `RAX:RDX` pair.

x86 two-operand arithmetic needs one target-specific rule that ARM64 does not:
the right operand is captured in `R8` before the fixed destination is
overwritten by the left operand. This preserves a carried value when the same
register is also the expression's right source. A same-register `MOV` is now a
code-generation no-op. After a direct definition, `RDX` path-local forwarding
is emitted only when the immediately following operation in the same basic
block consumes an output alias not represented by any fixed publication
register. Generated-instruction tests require the direct `ADD/SUB` encodings,
forbid the former `RAX` copies and dead `RDX` forwarding, and retain `RDX` for a
live immediate TMP alias.

Across 201 order-alternated, CPU-pinned A/B samples against `e77dcee`, the
simple conditional recurrence falls from 5.426884 to 3.852129 ms (-29.02
percent), conditional composed from 7.696867 to 6.784201 ms (-11.86 percent),
carried condition from 5.426884 to 4.793882 ms (-11.66 percent), and structured
branch expression from 7.889509 to 7.645130 ms (-3.10 percent). Expression
chain is flat at -0.01 percent and general modulo at +0.06 percent. Routing,
order corpus and ledger corpus controls remain within -0.26, -0.05 and -0.11
percent, respectively. The previously recorded same-host PHP 8.5 tracing-JIT
medians make the new simple conditional path 8.54x faster and branch expression
5.47x faster. x86-64 passes 161 library tests, all fourteen JIT integration
tests, the four-test corpus and `cargo check --all-features`.

The x86 direct resident-operand checkpoint changes operand selection from
"always copy into `RAX`/`R8`" to "return the register that owns the value".
Induction, fixed carried/publication values and the path-local `RDX` result can
therefore feed arithmetic, moves and structured conditions directly. Memory
and constants still materialize in a scratch register, and the destructive
left operand is copied only when it does not already own the result register.
If a latest result in `RDX` becomes an `IDIV` divisor, it is explicitly moved
to `R8` before `CQO` claims the architectural dividend pair.

The x86 cost model deliberately retains one apparent copy. Arithmetic from one
long-lived fixed publication register into another is re-banked through `R8`;
on the native Zen 4 host, direct fixed-to-fixed dependent recurrence was 24.5
percent slower despite identical results. Scratch-result arithmetic and
comparisons still consume fixed registers directly. Generated-code tests cover
direct `CMP R13,R14`, direct `ADD RAX,R13`, the fixed-to-fixed `R14` to `R8`
re-bank, and the required `RDX` divisor evacuation before `CQO`.

Variable-length code exposed a separate front-end contract. Removing one
three-byte move shifted a structured polling loop and initially regressed the
simple conditional workload by 24.7 percent. Its hot loop now starts at an
absolute 32-byte boundary computed from the final executable-code offset.
This alignment is CFG-gated to structured polling regions: applying it to
linear regions instead moved dependent recurrence onto a slower layout by
24.6 percent. A machine-code test derives the loop start after the empty-range
branch and requires its final offset modulo 32 to be zero.

Across 201 order-alternated, CPU-pinned A/B samples against direct-result
checkpoint `68ae3f9`, conditional composed falls from 6.753206 to 4.834890 ms
(-28.41 percent), carried condition from 4.834414 to 3.593445 ms (-25.67
percent), general modulo branch from 6.716967 to 5.767822 ms (-14.13 percent),
two invariants from 7.681370 to 6.988764 ms (-9.02 percent), linear composed
from 4.176617 to 3.868580 ms (-7.38 percent), simple conditional from 3.843784
to 3.608227 ms (-6.13 percent), and expression chain from 8.086443 to 7.697344
ms (-4.81 percent). Dependent recurrence and branch expression remain within
-0.05 and -0.04 percent; routing and order corpus improve 0.05 and 0.06 percent,
while ledger corpus is within -0.12 percent. Previously recorded same-host PHP
8.5 tracing-JIT medians make conditional composed 10.13x and carried condition
9.50x slower than the new RPHP paths. x86-64 passes 164 library tests, all
fourteen JIT integration tests, the four-test corpus and
`cargo check --all-features`.

The x86 signed-immediate instruction-selection checkpoint removes per-iteration
constant materialization from both the straight-loop backend and standalone
`ScalarLongFunctionPlan` lowering. `ADD`, `SUB` and `XOR` select the sign-extended
`imm8` or `imm32` group-1 form; constant multiply selects three-operand `IMUL`,
which also avoids copying the left source into the result register. Constants
outside signed 32-bit range retain the existing `MOVABS` plus register operation.
Checked arithmetic still tests the flags from the selected instruction with the
same precise `JO` side exit. Encoder tests require exact high-register `imm8`
and `imm32` bytes, and an executable standalone-scalar test requires an
`IMUL r64,r64,129` while checking both its value and overflow side exit.

Across 201 order-alternated, CPU-pinned A/B samples against `7029898`, the
previously unseen scalar expression chain falls from 7.723331 to 6.138325 ms
(-20.52 percent), conditional composed recurrence from 4.827738 to 4.034996 ms
(-16.42 percent), and linear composed recurrence from 3.860712 to 3.850698 ms
(-0.26 percent). Scalar recurrence and modulo controls remain within +0.16 and
+0.05 percent. Longer 401-pair corpus controls put routing at -0.78 percent,
ledger at +0.06 percent and order at +0.95 percent, where positive percentages
denote a regression.

Two isolated builds reject a benchmark-shaped immediate threshold. Disabling
constant `IMUL` moves order to -0.32 percent but loses the expression-chain gain
(only -0.16 percent) and regresses conditional composed by 10.19 percent.
Restricting `IMUL` to `imm8` preserves the two large wins but order still
regresses 0.54 percent. The backend therefore keeps the complete architectural
`imm8`/`imm32` selection instead of encoding a constant-value heuristic. The
next x86 front-end investigation is structured block placement/alignment; the
next instruction-selection slice is immediate comparisons and encodable
power-of-two remainder masks.

The x86 immediate-condition and remainder-mask checkpoint completes that
instruction-selection slice. Straight-loop guards/branches and standalone
scalar selects compare a right-side signed `imm8`/`imm32` constant directly,
while condition-local bitwise AND embeds an encodable constant from either
commutative operand. Wider values retain register materialization, so the
backend never changes a 64-bit mask through x86 sign extension.

The signed power-of-two remainder kernel is now one shared lowering helper for
both native paths. Recognition suppresses the previously dead `MOVABS` of the
original divisor. Masks through signed 32-bit range use two immediate `AND`
instructions; wider masks load exactly one `MOVABS` mask and keep the same
branchless PHP truncation identity. Generated-code tests require exact `AND`
and `CMP` bytes, absence of the dead divisor, and the wide-mask fallback;
execution tests cover negative remainder semantics in both forms.

Across 201 order-alternated, CPU-pinned A/B samples against `a4db596`, the 10M
modulo branch falls from 5.762339 to 4.827499 ms (-16.22 percent) and the
standalone bitwise scalar branch from 15.509605 to 15.393257 ms (-0.75
percent). Structured branch expression, conditional composed recurrence and
the scalar expression chain remain within +0.04, +0.04 and +0.06 percent.
Order corpus improves 0.41 percent; ledger and routing controls regress only
0.13 and 0.20 percent. The previously recorded same-host PHP 8.5.9 tracing-JIT
modulo median of 11.234999 ms makes the new RPHP path 2.33x faster. x86-64
passes 167 library tests, all fourteen JIT integration tests, the four-test
corpus and `cargo check --all-features`.

The next x86 optimization boundary is constant-bound loop register
specialization followed by structured basic-block placement/alignment. It
must free or repurpose the bound register rather than merely replace the
three-byte backedge `CMP` with a longer immediate form.

The constant-bound register checkpoint specializes only signed `imm8`/`imm32`
bounds for which `RCX` is actually allocated to a fourth resident value.
Entry and backedge comparisons then embed the bound, while `RCX` can retain a
deferred publication or ranked invariant alongside `R13-R15`. If no fourth
value is profitable, the backend deliberately keeps the constant in `RCX` and
the shorter register-register backedge; dynamic and wider constant bounds are
unchanged. This makes the specialization a register-capacity decision rather
than an instruction-count heuristic.

A permanent previously unseen PHP benchmark combines three independent
recurrences, a literal 10M loop bound and one shared runtime invariant. In 201
order-alternated pinned-CPU A/B pairs against `0804427`, its median falls from
3.927946 ms (3.896952/3.967047 ms p10/p90) to 3.864765 ms
(3.836393/3.908873 ms), a 1.61 percent reduction. Generated-code tests require
one `RCX` invariant load, two immediate induction comparisons and direct
consumption of `RCX` by all three fixed-register recurrences. A real-PHP test
also requires one range-proof evaluation, one native entry/call and no side
exit.

The same 201-pair control matrix keeps modulo unchanged, improves conditional
composed recurrence by 0.02 percent and regresses scalar expression chain by
0.02 percent. Order corpus improves 0.75 percent, routing improves 0.04 percent
and ledger regresses 0.14 percent with overlapping p10/p90 ranges. x86-64
passes 169 library tests,
fifteen JIT integration tests, the four-test corpus and
`cargo check --all-features`. The next x86 optimization boundary is structured
basic-block placement/alignment.

The guarded simple-accumulate parity checkpoint removes a front-end length
heuristic that admitted `$sum += $i` and invariant-addend terms only when the
loop body ended immediately after the assignment. Detection is now structural:
the same accumulate region may end in a strict comparison whose unlikely edge
contains arbitrary canonical PHP. A target-neutral straight kernel executes
the term, checked sum and trace guard on both ARM64 and x86-64. Guard failure
publishes every completed value and resumes the original comparison before the
cold block; the older ARM-only accumulator explicitly rejects guarded plans.

Complete-range analysis now proves `Equal`, `NotEqual`, `LessThan` and
`LessThanOrEqual` guards when operand intervals force the expected edge. A
proven cold guard therefore retains loop-carried values and safepoint polling
inside one native call; an overlapping interval keeps the checked exact
side-exit. ARM's accumulate cache now exposes the same straight and
range-proven program contract already used by x86 instead of labeling every
general program as a call kernel.

The permanent 10M guarded-accumulate benchmark preserves
`10000000:49999995000000`. Across 101 alternating native-CPU `max-perf` runs,
ARM64 falls from 8.278375 to 4.529959 ms (-45.28 percent) and x86-64 from
8.989553 to 5.276967 ms (-41.30 percent). An additional same-host ARM64 matrix
records 5.289583 ms for RPHP, 51.840375 ms for PHP 8.5.9 tracing JIT and
80.354250 ms for PHP without JIT. ARM64 passes 152 library tests, 86 JIT
integration tests and the four-test corpus; x86-64 passes 170 library tests,
18 JIT integration tests, the corpus and `cargo check --all-features`.

The x86 structured-loop alignment checkpoint isolates instruction placement
from PHP and IR semantics. Range-proven bodies with forward control flow now
start on a 64-byte L1-I cache-line boundary; linear loops remain unpadded. The
one-time entry guard and padding execute once, while the aligned body is fetched
on every native iteration. A generated-code assertion derives the loop start
after the empty-range guard and requires the backend alignment constant rather
than a benchmark-specific address.

The boundary was selected experimentally rather than assumed. Across 201
alternating fixed-CPU pairs, reducing the previous 32-byte alignment to 16
bytes regresses the half-range branch by 41.76 percent and conditional composed
recurrence by 20.10 percent, so 16 bytes is rejected. The 64-byte candidate is
then repeated for 301 pairs: conditional composed recurrence falls from
4.030228 to 3.883839 ms (-3.63 percent) and the carried-condition recurrence
from 4.788876 to 3.489971 ms (-27.12 percent), with separated p10/p90 ranges.
Linear controls remain within +0.23 and -0.08 percent; order, ledger and routing
corpora remain within +0.01, -0.17 and -0.20 percent. x86-64 continues to pass
170 library tests, 18 JIT integration tests, the four-test corpus and
`cargo check --all-features`. The next placement slice should remove redundant
hot jumps and coalesce cold side-exit stubs without changing this loop-header
boundary.

The x86 structured-control-flow compaction checkpoint completes that slice.
A conditional branch is omitted when its false edge is already the immediate
physical successor, because the true edge falls through to the same operation
and the predicate has no materialized PHP value. An unconditional jump to the
immediately following operation is omitted for the same reason. These are CFG
identities in the target-neutral operation stream rather than source-pattern
or benchmark rules.

Checked arithmetic exits are now grouped by failed operation. Multiple native
failure conditions belonging to one operation, such as the zero-divisor and
signed-division-overflow checks, target one status stub. Regions with multiple
checked operations retain one small status selector per operation and share
the induction publication, resident-value publication, register restoration
and return epilogue. The exact failed-operation index is therefore preserved
while cold machine code is no longer duplicated. Execution tests cover both a
divide-by-zero exit from operation zero and an overflow exit from operation
one; generated-code tests require one selector per operation and no emitted
control transfer for the immediate-successor cases.

Across 201 order-alternated, fixed-CPU `max-perf` A/B pairs against `09999df`,
branch loop, branch expression, modulo branch, carried condition and order
corpus improve between 0.03 and 0.12 percent; conditional composed recurrence
is within a 0.03 percent regression. Ledger improves 0.29 percent and the
routing holdout improves 1.47 percent. This checkpoint is retained primarily
for its smaller hot/cold footprint and canonical control flow rather than a
claimed broad timing win. x86-64 passes 172 library tests, 18 JIT integration
tests, the four-test corpus and `cargo check --all-features`; ARM64 passes 152
library tests, the corpus and `cargo check --all-features`. The next placement
candidate is measured short-branch relaxation and physical ordering of small
structured diamonds, without moving the proven 64-byte loop-header boundary.

The x86 short-branch checkpoint adds a final relocation-aware relaxation pass
to the handwritten assembler. Every emitted near branch records its original
instruction, displacement and logical target. Only forward structured body
edges are currently eligible for `rel8`; the pass repeatedly admits candidates
until shortening an inner edge can no longer make an enclosing edge fit, then
rebuilds the byte stream and repatches both shortened and remaining `rel32`
branches to their compacted offsets. Longer regions therefore retain their
near branches without a separate emitter or a guessed source-size limit.

Candidates start after the structured loop header, so compaction cannot move
the established 64-byte body boundary. Entry offsets for later ABI variants
are computed from the already compacted preceding variant, and their polling
bodies are aligned from those final offsets. The representative diamond emits
one short conditional edge and one short join edge in each of its five ABI
entries, removing 35 generated bytes in total. An encoder test also covers a
boundary cascade where shortening an inner jump makes an earlier conditional
jump fit, while a following backward `rel32` is retained and repatched.

Across 201 order-alternated, CPU-pinned `max-perf --all-features` A/B pairs
against `e018940`, five branch-heavy microbenchmarks range from a 0.03 percent
improvement to a 0.10 percent regression. Order corpus improves 0.85 percent,
routing holdout improves 1.39 percent and ledger regresses 0.40 percent with
overlapping p10/p90 ranges. The checkpoint is therefore a general code-density
and relocation capability, not a claimed scalar-throughput win. x86-64 passes
173 library tests, 18 JIT integration tests, the four-test corpus and
`cargo check --all-features`; ARM64 passes 152 library tests, the corpus and
the same check. Physical diamond ordering is measured separately below.

The first physical-diamond ordering candidate is deliberately rejected. A
range-derived layout hint identified simple induction comparisons whose true
edge covered at least 75 percent of the exact remaining iterations. For a
previously unseen 90/10 `if/else`, the candidate moved the false arm before the
true arm and inverted the conditional transfer, reducing the likely path from
a not-taken conditional plus predicted direct jump to one taken conditional.
Semantics, exact publications and the five ABI entries all passed, but 201
order-alternated fixed-CPU pairs regressed from 5.887508 to 7.030725 ms
(+19.42 percent), with fully separated p10/p90 ranges.

The existing 50/50 branch controls remained within -0.08 to +0.05 percent;
order and ledger corpus moved +0.32 and +0.01 percent, while routing improved
0.23 percent. This isolates the loss to the changed hot branch shape rather
than a general runtime regression. The likely explanation is x86 front-end
redirection cost: on this host, the original macro-fused not-taken condition
followed by a highly predictable direct jump is materially cheaper than the
taken conditional into the relocated arm. All candidate code and its layout
hint were removed. Source-order diamonds remain authoritative; a future retry
requires hardware branch-profile evidence and must test branchless selection
or duplication separately instead of assuming fewer dynamic branches wins.

The second physical-diamond experiment is also deliberately rejected after
testing the non-inverted form. An exact remaining-range proof marked true
edges covering at least 75 percent of the induction range. Post-emission
relocation then preserved the hot true arm as the not-taken fallthrough,
moved the cold false arm out of line and repatched every affected `rel32` and
eligible `rel8`. Both control-flow edges, backward cold joins, polling returns
and a checked overflow side exit retained exact state in native execution.

Moving the cold arm after all return stubs nevertheless changed an unseen
90/10 diamond from 5.876541 to 6.826162 ms (-16.16 percent). Warming the same
function before measurement made the steady-state loss clearer: 5.888700 to
7.119656 ms (-20.90 percent). A third placement kept the false target close by
putting it after every polling backedge but before the epilogues; `CMP/JGE`
remained the original short, macro-fusible condition and normal iterations
could not fall into the cold arm. It produced the same regression, disproving
the long-conditional hypothesis.

An in-process native-only A/B isolated the actual boundary. With production
CV publication, source order took 5.809603 ms and outlining 7.128304 ms
(+22.70 percent). When internal temporary outputs were artificially included
in the publication mask, the same transformation improved 6.188633 to
5.872635 ms (-5.11 percent). Disassembly showed that real deferred publication
keeps selected values in fixed R13/R14 registers and the source-order join
jump separates a dependent `ADD -> IMUL` chain. On the Ryzen 9 7950X, the
narrow real holdout retired about 8.0 million fewer instructions and 8.1
million fewer branches after outlining, but cycles rose from 37.78 to 44.99
million because IPC fell from 4.39 to 3.51. Branch misses moved only from 0.29
to 0.43 percent and cannot explain the loss.

A final 32-byte op-cache-phase gate rejected the known 19-byte counterexample
and restored 5.896807 versus 5.898952 ms (-0.04 percent). It did not generalize:
an admitted 36-byte cold span still regressed from 6.589651 to 7.344246 ms
(-11.45 percent), again retiring roughly 8.1 million fewer instructions and
branches while IPC fell from 3.95 to 3.57. The phase gate, range hint,
relocation pass and backend plumbing were therefore removed rather than
leaving dormant complexity in the core. The three warmed/skewed workloads are
retained as holdouts. A future retry must be dependency- and register-bank
aware, or driven by measured hardware profiles; the nearer opportunity is
instruction scheduling around deferred fixed-register publications rather
than physical cold-arm outlining by itself.

The follow-up x86 instruction-scheduling checkpoint retains source-order
diamonds and fills the exposed fixed-register dependency gap instead. A shared
CFG/liveness query finds the earliest operation that dominates every normal
body exit. The suffix from that point must contain only pure scalar operations,
must not read the induction slot and must not materialize a post-increment
result. Only the unchecked range-proven polling entry currently consumes this
answer: it moves the existing induction increment to the selected join without
adding or removing an instruction. Checked, budgeted and post-result entries,
plus bodies without a pure dominating suffix, retain their canonical tail
increment and exact side-exit state.

Generated-code coverage requires the x86 `ADD R11, 1` immediately before the
common fixed-register suffix, while execution coverage interrupts at iteration
1,024, resumes, takes both diamond arms and verifies the exact induction,
carried and result publications. Across fixed-CPU, order-alternated A/B pairs,
the warmed narrow 90/10 holdout remains flat at 5.877018 versus 5.876780 ms,
while the wider 90/10 holdout falls from 6.571531 to 5.837917 ms (-11.16
percent). The unwarmed variant improves from 5.867720 to 5.715370 ms (-2.60
percent), the balanced branch expression from 6.469250 to 5.737066 ms (-11.32
percent), conditional recurrence from 3.803968 to 3.379107 ms (-11.17 percent)
and carried-condition recurrence from 3.460884 to 3.368378 ms (-2.67 percent).

Simple branch, conditional-composed, dependent, reverse-dependent, expression
chain and modulo controls stay between a 0.09 percent improvement and a 0.03
percent regression. In 101-pair application checks, order, typed order, ledger,
typed ledger and routing regress by 0.17, 0.27, 0.08, 0.02 and 0.33 percent
respectively, all with overlapping p10/p90 ranges. Supplementary wide-holdout
hardware counters keep retired instructions effectively unchanged
(171.151/171.185 million) while sampled IPC moves from 4.23 to 4.30; the timing
win therefore comes from scheduling independent work across a dependency
chain, not from benchmark-specific work elimination. x86-64 passes 175 library
tests, 18 JIT integration tests, the four-test corpus and
`cargo check --all-features`; ARM64 passes 153 library tests and the same
check. The target-neutral query is shared, but each target separately decides
whether its physical schedule is profitable.

The ARM64 follow-up consumes the same proof only for structured scalar polling
bodies. The first symmetric candidate also scheduled linear bodies; although
branch joins improved, the scalar expression-chain control regressed from
3.208160 to 3.252029 ms (+1.37 percent). ARM therefore retains canonical tail
placement for linear code instead of inheriting x86 policy. With that
target-specific gate, 201 order-alternated native-CPU pairs improve the narrow
90/10 holdout from 3.672838 to 3.545046 ms (-3.48 percent), the wider holdout
from 3.695011 to 3.556013 ms (-3.76 percent), balanced branch expression from
4.110813 to 4.036903 ms (-1.80 percent), and conditional recurrence from
3.593206 to 3.560066 ms (-0.92 percent). The linear expression-chain control
returns to 3.242970/3.239870 ms (-0.10 percent); dependent, reverse-dependent,
simple-branch and modulo controls remain between a 0.38 percent improvement and
a 0.21 percent regression.

Longer ARM corpus batches avoid the unstable performance/efficiency-core split
seen in short macOS process samples. Order, typed order, ledger, typed ledger
and routing aggregate medians remain between a 0.18 percent regression and a
0.15 percent improvement, with paired medians between a 0.13 percent regression
and a 0.24 percent improvement.

The ARM generated-code test requires `ADD x3, x3, #1` immediately before the
common fixed-register multiply, while its existing interrupt/resume assertions
continue to validate both arms and exact publications. ARM64 passes 153 library
tests, 86 JIT integration tests, the four-test corpus and
`cargo check --all-features`; the unchanged x86 scheduling test and build also
remain green.

A follow-up attempt to move the ARM64 polling `CMP induction, chunk_end` next
to the early induction increment is deliberately rejected. The selected pure
scalar suffix preserves condition flags, so the transformation was
semantically valid and added no instruction. It nevertheless separated the
compare from its consuming `B.NE`; on the wide skewed holdout the median moved
from 3.574848 to 3.844023 ms (+7.53 percent), with non-overlapping p10/p90
ranges and a 7.49 percent paired regression. This is consistent with losing a
profitable compare/branch front-end pairing on the Apple ARM64 host. All code
for the candidate was removed. x86 was not exposed to the experiment because
its common `ADD`/`IMUL` suffixes overwrite flags, so the same schedule is not
semantically available there without extra instructions.

The next x86-64 instruction-selection checkpoint fuses an adjacent immediate
`source * scale + bias` scalar pair into one SIB-addressing `LEA` when `scale`
is 3, 5 or 9 and the signed bias fits `i32`. This is admitted only in the
unchecked range-proven polling entry: checked entries retain two operations
and therefore their exact per-operation overflow side exits. The intermediate
must be dead after its consumer, must not require a shadow store or deferred
publication, and the consumer must not be a separately reachable structured
block start. The fused result still uses the ordinary direct-result,
fixed-register, shadow-publication and safepoint machinery. A negative
generated-code test publishes the multiply intermediate and proves that this
observable case remains unfused; the existing structured-phi test now requires
all three admitted affine expressions to be generated directly in their fixed
publication registers.

On the Ryzen 9 7950X, 201 fixed-CPU order-alternated pairs reduce the balanced
branch-expression median from 5.769253 to 3.876448 ms (-32.81 percent), the
wide 90/10 holdout from 5.812645 to 4.808903 ms (-17.27 percent), and the narrow
90/10 holdout from 5.873919 to 4.647970 ms (-20.87 percent). Seven unrelated
scalar controls stay within 0.11 percent of baseline. The noisier scalar-method
control moves by -1.27 percent, but its old and new p10/p90 ranges overlap;
this shape has no admitted immediate affine pair. Order, typed order, ledger,
typed ledger and routing corpus medians improve by 0.28 to 0.39 percent. x86-64
passes 177 library tests, 18 JIT integration tests, the four-test corpus and
`cargo check --all-features`; ARM64 passes 153 library tests and the same
check.

The mathematical pattern is architecture-neutral, but that constant-scale
lowering is deliberately x86-specific. ARM64 already emits a shifted `ADD`
followed by an immediate `ADD`/`SUB`; it has no one-instruction equivalent of
this `LEA`. Replacing that pair with `MADD` would first have to materialize the
scale or bias and would not reduce the instruction count.

The follow-up therefore starts from a genuinely variable multiply/consumer
shape. A shared affine analysis now recognizes an adjacent multiply followed
by product-plus-operand, product-minus-operand or operand-minus-product. It
rejects a self-consuming combination and any multiply intermediate read after
the consumer. The x86 SIB recognizer consumes this same proof and applies its
existing constant scale/bias filter. The ARM64 profitability module separately
admits only three-slot `product + addend` and `minuend - product` forms, which
one `MADD` or `MSUB` replaces without materializing a constant. x86-64 has no
equivalent general integer instruction and deliberately retains `IMUL + ADD`.

As with the x86 fusion, only the unchecked range-proven polling entry can erase
the operation boundary. Checked code retains independent overflow side exits;
a separately reachable consumer, shadow-stored intermediate or deferred
intermediate publication rejects the fusion. Moving the producer to its
consumer must also not cross a scheduled induction increment when a fused
operand reads the induction value; dedicated ARM64 and x86-64 regressions prove
that this case retains the old value and remains unfused. Exact encoder tests
cover `MADD` and `MSUB`; execution tests interrupt/resume the fused add, execute
the fused reverse subtract, and prove that publishing the multiply intermediate
restores the separate instructions. Pattern discovery and both backend
profitability filters live in small dedicated modules rather than expanding
the main encoder files.

The ARM measurements use 100-million-iteration holdouts to suppress the short
macOS performance/efficiency-core split, with a baseline rebuilt directly from
git commit `acc23b9`. One variable multiply-add feeding a recurrence moves from
28.104067 to 28.005123 ms (-0.35 percent); one independent result moves from
27.897835 to 27.788877 ms (-0.39 percent). Two independent affine results make
the instruction-density benefit visible: the final source build moves
32.276869 to 31.621933 ms (-2.03 percent), and paired p10/median/p90 reductions
are 0.19/2.06/3.68 percent. Constant branch-expression, wide-skewed and
expression-chain controls
stay between a 0.27 percent regression and a 0.33 percent improvement with
overlapping ranges. The five application corpus medians remain between a 0.40
percent regression and a 0.05 percent improvement, also with overlapping
ranges.

On the Ryzen 9 7950X the three variable holdouts remain neutral within 0.01 to
0.02 percent, confirming that shared discovery does not imply shared lowering;
the prior x86 generated-`LEA` assertion remains green. ARM64 passes 160 library
tests, 86 JIT integration tests, the four-test corpus and
`cargo check --all-features`; x86-64 passes 180 library tests, 18 JIT
integration tests, the four-test corpus and the same check.

The next ARM64 publication checkpoint removes the remaining linear
`result in x8 -> MOV into fixed register` sequence when an operation already
owns a non-carried publication register or one of several independent carried
registers. ARM64's three-address scalar instructions can safely generate the
value in that fixed destination; the resident-value bookkeeping now records
the actual result register instead of assuming that every operation refreshed
x8. Exact generated-code tests cover a non-carried direct `ADD`, four visible
fixed results and reverse-dependent carried state, while execution tests cover
interrupt, resume and final publication.

The first unrestricted candidate exposed a target-specific counterexample. A
single loop-carried read-modify-write chain slowed by about 1.7 percent even
though one instruction disappeared: the Apple ARM core profits from computing
through x8 and treating the following `MOV` as a register rename. A physical
profitability guard therefore retains that sequence when exactly one carried
group is both read and rewritten. It is based on carried dependencies and
register ownership, not source or benchmark identity. On 301 order-alternated
100-million-iteration pairs, the composed-recurrence control returns to
28.759003 versus 28.721094 ms (-0.13 percent; paired -0.10 percent).

The admitted shapes keep the intended benefit. In 201 order-alternated long
pairs against a clean `595a8ef` build, three independent recurrences improve
from 25.866032 to 25.677919 ms (-0.73 percent), overlapping scalar lifetimes
from 34.830093 to 34.161806 ms (-1.92 percent), and two independent variable
multiply-add results from 30.996084 to 30.798912 ms (-0.64 percent). The latter
two paired p10/p90 ranges are respectively -2.88/-0.89 percent and
-6.23/+0.70 percent; the shorter instruction stream is strongest under
overlapping live values and remains modest for the affine pair. x86-64 is
unchanged: its two-address lowering cannot inherit this ARM destination choice
without a separate target-specific allocation proof. The five ARM application
corpus medians remain between a 0.81 percent improvement and a 0.57 percent
regression in 101 short order-alternated pairs. ARM64 passes 163 library tests,
86 JIT integration tests, the four-test corpus and `cargo check --all-features`;
the synchronized x86-64 tree passes 180 library tests, 18 JIT integration
tests, the four-test corpus and the same check.

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

### Exact Double scalar leaf vertical slice

The first floating-point JIT slice is target-neutral above instruction
selection. The compiler recognizes straight leaf functions and methods with up
to eight raw Double inputs and eight `+`, `-`, `*`, or `/` operations. Both the
direct adjacent-Send boundary and the compact deferred-call boundary guard
every argument as an exact non-reference Double. Weakly accepted Long values
therefore replay the canonical frame and retain normal `int -> float` coercion,
strictness and diagnostics.

ARM64 keeps all intermediates in D16-D23 and emits `FADD`, `FSUB`, `FMUL` and
`FDIV` directly. x86-64 uses mandatory SSE2 and keeps the same eight values in
XMM2-XMM9. Both ABIs publish one result transactionally. Division compares the
raw divisor against positive and negative zero before publication; NaN remains
an ordinary IEEE-754 input. A zero divisor side-exits and the unchanged PHP
operation raises the canonical `Division by zero` error. One-operation leaves
remain in the Rust adapter because isolated measurements show no native-entry
benefit; two or more operations compile lazily after 64 calls.

The same path is used by ordinary functions and monomorphic methods. Method
execution is admitted only after the receiver class and method inline cache
match; the plan builder cannot describe `$this`-dependent bodies, so eliding
the body frame does not erase receiver semantics. Architecture-specific tests
cover exact instruction bytes, native execution, NaN, `-0.0`, transactional
side exit and cache profitability. Real PHP tests prove native entry, Long
fallback, method-cache dispatch and canonical zero-division replay on both
targets.

The permanent five-million-call `bench_typed_float_leaf.php` workload contains
a general five-operation typed expression. Nine interleaved `max-perf` runs on
ARM64 measure 138.778 ms in the frame-free Rust adapter and 119.153 ms with the
Double JIT, a 14.1 percent reduction. PHP 8.5.9 records 121.202 ms without JIT
and 25.699 ms with CLI tracing JIT. On the pinned Ryzen x86-64 host, RPHP moves
from 214.214 to 180.383 ms, a 15.8 percent reduction; the currently installed
PHP 8.4.24 records 149.821 and 27.054 ms respectively. Outputs are identical.

An ARM64 profitability matrix makes the remaining boundary visible: one
operation is flat at 89.48 ms, two improve from 99.82 to 96.71 ms, three from
104.29 to 98.51 ms and five from 138.78 to 119.15 ms. The largest remaining
gap is therefore not scalar Double instruction selection. PHP tracing JIT
inlines the leaf into its caller loop, while RPHP still performs one VM/native
transition per call. The next Double milestone is a shared composed-call
lowering that imports a proven `ScalarDoubleProgram` into a surrounding typed
region, followed by scalar argument-expression composition. It must reuse the
same IR and side-exit contract rather than add a benchmark-shaped loop kernel.
The completed checkpoint passes 167 ARM64 and 184 x86-64 library tests, 89
ARM64 and 21 x86-64 JIT integration tests, plus the four application corpus
tests on each target.

### Composed exact Double call-loop checkpoint

The first caller/callee composition slice is complete on ARM64 and x86-64
(2026-08-05). The compiler recognizes a Long-controlled loop whose body calls
one proven `ScalarDoubleProgram` with invariant exact-Double CV or constant
arguments and accumulates its result into an exact-Double CV. A target-neutral
`QuickDoubleCallAccumulateLoop` owns the function-cache guard, argument
bindings, typed state and exact baseline resume points. The no-JIT runtime
executes the same plan frame-free in Rust, so semantic coverage does not depend
on native code generation.

Both native backends import the existing scalar callee IR into one surrounding
loop. Their shared ABI contains only Long induction/bound state, Double
accumulator/last-term state, an exact-Double input array and an interrupt flag.
The hot region therefore performs no PHP frame construction and no per-call
VM/native transition. It polls every 1,024 completed iterations. Completion,
interrupt and division-by-zero side exit publish identical state on both
architectures; a zero divisor commits none of the failing iteration and resumes
the canonical `InitFcall`.

Seven-run medians on the permanent five-million-call workload are:

| Host | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| ARM64, PHP 8.5.9 | 3.977 ms | 34.656 ms | 25.791 ms | 122.081 ms |
| Ryzen x86-64, PHP 8.4.24 | 5.211 ms | 39.805 ms | 26.908 ms | 143.421 ms |

Thus the composed native region is approximately 6.5x faster than PHP tracing
JIT on ARM64 and 5.2x on x86-64 for this shape. More importantly, a real-PHP
integration test proves the loop cache compiles once while the standalone leaf
cache remains cold: the result comes from region composition rather than a
special benchmark entry point. The checkpoint passes 170 ARM64 and 187 x86-64
library tests, 90 ARM64 and 22 x86-64 JIT integration tests, and all 42 loop
end-to-end tests on ARM64. All four application corpus tests also pass on both
targets.

The next Double work should generalize the caller bindings to scalar argument
expressions, then admit composed scalar callees and monomorphic methods under
receiver-class plus method-cache guards. These extensions must reuse the same
loop ABI and side-exit protocol; unsupported bodies, mutable arguments and
polymorphic targets remain canonical.

### Composed Double argument-expression checkpoint

The caller binding layer now accepts up to eight target-neutral scalar Double
operations (`+`, `-`, `*`, `/`) before a proven typed-Double leaf
(2026-08-05). Sources may be exact-Double caller CVs, Double or Long constants,
the Long loop induction value converted to Double, or prior argument
temporaries. At least one operand of each admitted operation must already be
proven Double, so ordinary Long arithmetic and its overflow semantics remain
canonical. Direct Long arguments are likewise still handled by the regular PHP
coercion path.

`QuickDoubleArgumentProgram` is shared by the detector, frame-free Rust tier,
ARM64 and x86-64. It records compact guarded Double inputs independently from
the public callee arguments. Its dependency analysis divides work into an
invariant preheader and an induction-dependent loop phase, including transitive
dependencies through temporaries. Thus one dynamic argument does not force
constants or unrelated invariant expressions back into every iteration. ARM64
uses `SCVTF`; x86-64 uses `CVTSI2SD`. Both backends write the resulting public
arguments through the same bounded working ABI before executing the existing
callee IR.

The side-exit contract remains transactional. Argument division by positive or
negative zero commits none of the failing iteration and resumes the canonical
`InitFcall`; an empty loop evaluates no arguments at all. Permanent tests cover
the Rust and native paths, invariant dependencies feeding dynamic expressions,
interrupt/completion state, the canonical division error and empty-loop
behavior. The final matrix passes 173 ARM64 and 190 x86-64 library tests, 91
ARM64 and 23 x86-64 JIT integration tests, all 45 loop end-to-end tests, and all
four application corpus tests on both targets.

Nine interleaved `max-perf`, native-CPU runs of the new five-million-iteration
`bench_typed_float_argument_expr.php` workload produce identical
`6250023750000` output and these medians:

| Host | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| ARM64, PHP 8.5.9 | 3.392 ms | 58.090 ms | 35.165 ms | 126.976 ms |
| Ryzen x86-64, PHP 8.4.24 | 16.835 ms | 67.947 ms | 37.464 ms | 142.154 ms |

RPHP is therefore about 10.4x/2.2x faster than PHP JIT/no-JIT on ARM64 and
2.2x/2.1x on x86-64 for this shape. The unchanged invariant-argument control
remains at approximately 3.181 ms JIT and 32.050 ms no-JIT on ARM64, and 5.084
ms JIT and roughly 41 ms no-JIT on x86-64. The native ARM64 expression cost is
almost completely hidden, while x86-64 still spends most of its new delta
storing a dynamic public argument to the working buffer and immediately
loading it into the leaf program.

The next structural step is therefore direct argument/leaf IR composition with
liveness-aware source remapping. It should forward dynamic argument values in
registers when their leaf uses permit it, while retaining the working buffer as
the general fallback. That same composition machinery should then admit nested
scalar callees, followed by monomorphic scalar methods guarded by receiver
class and method cache. It must not introduce a workload-specific float loop.

### Liveness-aware Double argument/leaf composition checkpoint

Dynamic Double arguments can now flow directly from the caller argument IR
into the leaf IR without a memory round trip (2026-08-05). The target-neutral
`QuickDoubleArgumentProgram` computes a forwarding mask from the actual leaf
uses. Only induction-dependent values backed by an argument temporary are
eligible, and only while the leaf has not overwritten the shared physical
register assigned to that temporary.

The proof deliberately models the stricter two-operand x86-64 instruction
order. A forwarded argument may be the LHS of the leaf operation that reuses
its register, because the destination move is then a no-op. A RHS-only use at
that operation is rejected because x86-64 would overwrite it before the
arithmetic instruction consumes it. Uses after the overwrite and leaf outputs
observed after the overwrite are rejected as well. This conservative shared
rule keeps ARM64 and x86-64 behavior identical even though ARM64's
three-operand instructions could admit a few additional cases.

Both native emitters use the mask to omit only proven-redundant dynamic
argument stores and remap matching leaf inputs to the resident argument
temporary. Invariant arguments, unsafe live ranges and all unsupported shapes
continue through the existing bounded working buffer. No new runtime guard or
side-exit state is required: the proof is derived from the already guarded,
immutable argument and leaf IR. Permanent tests cover successful forwarding,
the x86-64 RHS overwrite conflict, native buffer fallback, composed invariant
dependencies, division side exits and complete loop state.

Nine native-CPU `max-perf --all-features` runs preserve the exact benchmark
outputs and produce these medians:

| Host | Dynamic expression before | Register composition | Invariant leaf control |
|---|---:|---:|---:|
| ARM64 | 3.392 ms | 3.434 ms | 3.209 ms |
| Ryzen x86-64 | 16.835 ms | 8.371 ms | 5.164 ms |

ARM64 remains unchanged within run-to-run noise because its load/store path was
already almost completely hidden. On x86-64, direct composition removes
`8.464 ms`, or `50.3%`, from the complete expression workload and eliminates
about `72.7%` of the previous gap to the invariant control. The remaining
roughly `3.2 ms` is dominated by useful per-iteration work: signed Long-to-
Double conversion and the dynamic multiply.

The complete final matrix passes 176 ARM64 and 193 x86-64 library tests, 91
ARM64 and 23 x86-64 JIT integration tests, all 45 loop end-to-end tests, and
all four application corpus tests on both targets. The next Double composition
step is nested target-neutral scalar callees. It should reuse this same source
remapping and liveness proof before extending dispatch to monomorphic scalar
methods guarded by receiver class and method cache.

### Guarded nested Double leaf checkpoint

Straight-line typed-Double functions may now contain direct calls to other
proven typed-Double leaves (2026-08-05). The compiler records arithmetic and
call nodes in a new target-neutral `ComposedScalarDoubleFunctionPlan`; it does
not bind or inline a callee from source names alone. Each call retains the
canonical owner-op-array inline-cache position and its source-remapped Double
arguments.

At execution time, RPHP resolves an empty function cache through the normal
function table, validates the cached target's identity, arity, compact exact-
Double ABI and pure `ScalarDoubleFunctionPlan`, then flattens the call leaf and
outer arithmetic into the established `ScalarDoubleProgram`. Composed
operation results are remapped to the flattened SSA temporaries, while leaf
inputs become the outer call's actual Double sources. The complete flattened
body must fit the shared eight-operation/register capacity. Long literals at a
nested float boundary, named/by-reference calls, unsupported targets and
larger bodies are rejected before speculative execution.

The same flattener serves three execution modes:

- ordinary contiguous exact-Double calls outside loops use the frame-free Rust
  direct-call path;
- quick loops without JIT evaluate the flattened Rust scalar program;
- ARM64 and x86-64 quick-loop JITs compile that identical flat program with no
  new backend opcode.

Weak calls containing raw Long values deliberately miss the direct guard and
retain canonical float coercion. Native division side exits remain
transactional and resume the outer root `InitFcall`, so the ordinary nested
frames reproduce PHP's precise division error. Root and nested targets are all
recorded in call/hotness statistics. The native cache now stores the complete
ordered root-plus-leaf identity tuple; a changed nested target cannot reuse
machine code compiled for an earlier composition.

The new unseen five-million-iteration
`bench_typed_float_nested_leaf.php` holdout returns the identical
`6250011250000` value. Nine interleaved native-CPU `max-perf` comparison runs,
with a final 21-run ARM64 RPHP confirmation, give:

| Host | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| ARM64, PHP 8.5.9 | 3.444 ms | 44.920 ms | 42.006 ms | 160.914 ms |
| Ryzen x86-64, PHP 8.4.24 | 8.963 ms | 51.824 ms | 45.291 ms | 150.608 ms |

RPHP is therefore about 12.2x/3.6x faster than PHP JIT/no-JIT on ARM64 and
5.1x/2.9x on x86-64 for this nested call shape. The final all-feature matrix
passes 178 ARM64 and 195 x86-64 library tests, 92 ARM64 and 24 x86-64 JIT
integration tests, all 47 loop end-to-end tests, and all four application
corpus tests on both targets.

This checkpoint initially admitted one composed layer whose call targets were
flat Double leaves. The bounded recursive extension described below now closes
that structural limitation. Direct monomorphic Double methods remain the next
step and should reuse the existing receiver-class plus method-cache guard
contract rather than adding a separate object JIT.

### Bounded recursive Double composition checkpoint

Guarded Double callees may themselves now contain guarded composed Double
calls (2026-08-05). Runtime resolution walks the actual inline-cache target at
each level. A flat target contributes its original `ScalarDoubleProgram`; a
composed target is first recursively flattened and its resulting program is
then source-remapped into its caller. The target-neutral composer consumes the
same borrowed program view in both cases, so neither ARM64 nor x86-64 required
a new machine instruction or architecture-specific recursive path.

The recursion is deliberately bounded by three independent budgets: at most
four recursively composed callees below the root, eight nested target identities
and eight final arithmetic operations. An active-target chain rejects direct
and mutual cycles. Arity,
exact raw-Double arguments, compact ABI eligibility, function-cache identity
and division side-exit behavior are revalidated at every level. Exceeding any
budget abandons the complete speculative tree before publishing a result and
replays the ordinary root call. Sibling calls may legally repeat a target, and
each occurrence remains represented in both the identity tuple and call-count
accounting.

The same recursive resolver serves ordinary frame-free direct calls, the Rust
quick executor without JIT, and the ARM64/x86-64 native quick-loop dispatch.
A new three-function test (`calculateOuter` -> `calculateNested` ->
`scaleAndShift`) enters one native region with no side exit, while all three
functions retain exactly 100,000 recorded calls. Exact direct calls return the
flattened result; raw Long arguments still miss the guard and pass through
PHP's normal weak-float conversion.

The new five-million-iteration
`bench_typed_float_composed_tree.php` holdout returns the identical
`6250026250000` value. Nine native-CPU `max-perf` measurements give:

| Host | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| ARM64, PHP 8.5.9 | 4.092 ms | 47.969 ms | 60.923 ms | 228.224 ms |
| Ryzen x86-64, PHP 8.4.24 | 10.329 ms | 58.916 ms | 65.742 ms | 210.198 ms |

For this deeper call tree, RPHP JIT is about 14.9x faster than PHP tracing JIT
on ARM64 and 6.4x faster on x86-64. Without JIT, RPHP is about 4.8x and 3.6x
faster than the corresponding PHP no-JIT mode, and even remains ahead of PHP
tracing JIT on both hosts. The complete all-feature matrix passes 178 ARM64
and 195 x86-64 library tests, 93 ARM64 and 25 x86-64 native JIT integration
tests, all 27 function and 48 loop end-to-end tests, and all four corpus tests.

The next structural Double step is monomorphic method composition guarded by
receiver class plus method-cache identity. It should feed this same bounded
program composer and target tuple rather than creating a parallel object-only
JIT representation.

### x86-64 destructive Double register reuse checkpoint

The x86-64 SSE2 backend now assigns physical XMM registers from target-neutral
temporary liveness instead of requiring every IR result to occupy its operation
index forever (2026-08-05). When a temporary is the left operand at its final
use and is not retained as the program output, the destructive SSE2 result
reuses that same physical register. All other results keep their original
one-slot-per-operation assignment. This conservative rule removes the copy
chain from linear arithmetic while preserving branched values, final outputs
and the existing argument/leaf forwarding proof.

Standalone typed-Double leaves and composed quick loops use the same register
map. Argument-program temporaries deliberately retain their identity mapping,
so a forwarded dynamic argument still has the exact register proven by
`register_forwardable_output_mask`. Division checks inspect the RHS before its
destructive operation and retain the unchanged transactional side exit.

Register-to-register scalar copies now use full-width SSE2 `MOVAPD` rather than
legacy `MOVSD`. The authoritative lower Double bits are identical, but writing
all 128 destination bits breaks the false upper-lane dependency that otherwise
serializes a reused register across loop iterations. An experimental version
that collapsed the registers while keeping `MOVSD` regressed the recursive
tree from roughly 10 ms to 18 ms; it was rejected before the checkpoint. A
separate `PXOR` before legacy `CVTSI2SD` was also measured and rejected because
its 8.807 ms median was slightly worse than the 8.733 ms version without it.

Permanent x86 tests verify exact full-width move bytes, one remaining initial
copy in a three-operation linear program, a branched live-value map and native
result, forwarded/buffered argument cases, polling and transactional division
side exits. ARM64 is unchanged because its native `FADD`/`FMUL` forms already
have independent destination registers.

Twenty-one order-alternated pairs pinned to Ryzen CPU 2 returned identical PHP
results and these native `max-perf` medians:

| Workload | Previous x86 JIT | XMM reuse | Change |
|---|---:|---:|---:|
| Typed Double leaf | 5.167 ms | 2.877 ms | -44.3% |
| Double argument expression | 8.449 ms | 4.675 ms | -44.7% |
| Nested Double leaf | 8.841 ms | 7.366 ms | -16.7% |
| Recursive Double tree | 10.311 ms | 8.793 ms | -14.7% |

The recursive-tree x86/ARM ratio therefore narrows from approximately 2.52x
to 2.15x, and RPHP's x86 JIT lead over the measured PHP tracing JIT grows from
about 6.4x to 7.5x. The complete matrix passes 178 ARM64 and 199 x86-64 library
tests, 93 ARM64 and 25 x86-64 native JIT integration tests, all 27 function and
48 loop end-to-end tests, and all four corpus tests. The remaining dynamic-
expression gap is no longer dominated by redundant leaf copies. A future AVX
three-operand fast path may remove the initial copy too, but should be attempted
only behind a runtime CPU/OS feature guard with the now-optimized SSE2 path
retained as the universal fallback.

### Runtime-guarded x86 AVX Double lowering checkpoint

The x86 Double leaf and composed-loop emitters now select a complete AVX scalar
lowering when the runtime CPU and operating-system state support AVX
(2026-08-05). The standard Rust feature probe includes the OSXSAVE/XCR0 safety
check; hosts without usable AVX retain the preceding liveness-aware SSE2 path,
which remains the universal x86-64 implementation. This is a lowering choice
for the same target-neutral typed IR, not a separate source recognizer.

The AVX path uses three-operand `VADDSD`, `VSUBSD`, `VMULSD` and `VDIVSD`, VEX
loads/stores and bit moves, and `VCVTSI2SD` with an explicitly zeroed upper
source. It therefore removes the initial destructive LHS copy and avoids
mixing legacy SSE with VEX instructions inside the native region. Every
successful, interrupted and transactional side-exit return executes
`VZEROUPPER` before returning to Rust. Permanent encoder tests compare exact
bytes for both two- and three-byte VEX forms, including high XMM registers;
forced SSE2/AVX execution tests preserve results and signed-zero division side
exits, while an automatic-selection test verifies that production code emits
the AVX return marker only on a supported host.

One hundred and one order-alternated pairs pinned to Ryzen CPU 2 compared the
new native-CPU `max-perf` build with the preceding XMM-reuse SSE2 binary. All
PHP results remained identical:

| Workload | SSE2/XMM reuse | AVX | Change |
|---|---:|---:|---:|
| Typed Double leaf | 2.891 ms | 2.889 ms | -0.07% |
| Double argument expression | 4.674 ms | 4.257 ms | -8.93% |
| Nested Double leaf | 7.380 ms | 7.083 ms | -4.03% |
| Recursive Double tree | 8.964 ms | 8.488 ms | -5.31% |

The identity-argument leaf is deliberately treated as neutral rather than a
benchmark win. The retained benefit appears in the general composed dataflow:
dynamic conversion and longer operation chains no longer need the same
destructive-copy scheduling. ARM64 is unchanged because its scalar floating
instructions already have independent destination operands. The complete
all-feature matrix passes 178 ARM64 and 203 x86-64 library tests, 93 ARM64 and
25 x86-64 native-JIT integration tests, all 27 function and 48 loop end-to-end
tests, and all four corpus tests.

### Guarded monomorphic Double method composition checkpoint

The target-neutral Double call/accumulate detector now accepts a direct
`InitMethodCall` in addition to `InitFcall` (2026-08-05). It records the
existing `MethodCache` guard with the receiver CV and normalizes the hidden
`$this` argument offset before building the same `QuickDoubleArgumentProgram`.
Constants, exact-Double CVs, induction-dependent expressions, forwarding,
recursive function callees and the established eight-operation budgets are
therefore unchanged. No ARM64 or x86-64 instruction emitter was added for
methods.

At region entry, a dedicated exact-Double resolver validates that the receiver
is a non-reference Object, its current class id matches the canonical method
inline cache, the cached function identity and public arity still match, and
the selected user method has the compact Double ABI plus a proven flat or
composed Double plan. The receiver and dispatch guard do not execute inside
the native loop. The native cache remains bound to the resolved target tuple;
a different receiver class at the same bytecode call site cannot reuse the
compiled method body and falls back to the target-neutral Rust plan or
canonical bytecode. Methods that depend on `$this`, accept references, use
named arguments or lack a pure Double plan remain unsupported.

Completion, interrupt and division side exits retain the existing
transactional state contract. A signed-zero divisor resumes at the original
`InitMethodCall`, so normal method dispatch raises the canonical PHP error.
Permanent tests cover plan selection and hidden-argument normalization, a
single native region with the standalone method leaf cache still cold, a
changed receiver class at the same call site, and division-by-zero replay.
The preceding standalone method test now uses one stable method site outside a
recognizable loop, preserving independent coverage of the leaf JIT.

The new five-million-iteration
`bench_typed_float_method_accumulate.php` holdout returns the identical
`30000000` value. One hundred and one order-alternated native-CPU A/B runs
(x86 pinned to Ryzen CPU 2) compare the preceding per-call build with method
composition; 21 interleaved runs provide the PHP and no-JIT controls:

| Host | Previous RPHP JIT | Composed RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|---:|
| ARM64, PHP 8.5.9 | 141.550 ms | 3.280 ms | 36.241 ms | 32.060 ms | 130.260 ms |
| Ryzen x86-64, PHP 8.4.24 | 199.692 ms | 2.886 ms | 41.016 ms | 27.678 ms | 157.102 ms |

Removing five million method boundaries makes the native region about 43.2x
faster than the preceding ARM64 path and 69.2x faster on x86-64. It is also
about 9.7x/9.6x faster than PHP tracing JIT. Because method dispatch is a
target-neutral plan feature, RPHP without JIT is approximately 3.6x/3.8x
faster than PHP without JIT on the same holdout. The complete all-feature
matrix passes 179 ARM64 and 204 x86-64 library tests, 94 ARM64 and 26 x86-64
native-JIT integration tests, all 27 function and 51 loop end-to-end tests,
and all four corpus tests.

The next object/Double extension should be nested monomorphic method calls
only after receiver-source mapping and every inner class/cache identity can be
represented in the same bounded composer. General conditional Double regions
are an independent alternative; neither direction should introduce an
object-only backend kernel.

### Same-receiver nested Double method checkpoint

The bounded Double composer now accepts monomorphic method calls on the
owner's hidden `CV0 == $this` receiver (2026-08-05). This is deliberately not
a general Object value in the public Double ABI. The compiler admits
`InitMethodCall` only for an instance method with `this_offset == 1`, an exact
CV0 receiver, positional exact-Double arguments and the established operation,
arity and recursion budgets. It records the canonical `MethodCache` guard and
normalizes the hidden parameter offset exactly as the ordinary method caller
does.

Runtime flattening carries the already-guarded owner receiver through a nested
method tree. Every method node validates the current receiver Object and class
id against its own canonical inline cache, validates the cached target's
instance-method Double ABI and includes that target in the native cache
identity tuple. A cold inner method cache simply declines speculation; the
unchanged bytecode call resolves and warms it before a later attempt. Direct
frame-free composed calls and quick call/accumulate loops share this resolver.
Functions inside a method tree receive no synthetic receiver, and a function
body cannot smuggle a method node into the compact Double ABI.

No architecture-specific emitter changed. ARM64 and x86-64 still receive the
same flattened `ScalarDoubleProgram`; this slice removes nested dispatch and
frame construction before either native backend is selected. Permanent tests
cover the compiled method guard, a single native region, an inherited outer
method whose inner target is overridden by a child class, and a divisor that
becomes zero in the middle of native iteration and replays the canonical PHP
error transactionally.

The new five-million-iteration
`bench_typed_float_nested_method.php` holdout returns the identical
`6250011250000` value. The stable Ryzen CPU-2 measurements and the current
thermally affected ARM64 interleaved run were:

| Host | Previous RPHP JIT | Nested RPHP JIT | Nested RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|---:|
| ARM64, PHP 8.5.9 | 1196.301 ms | 6.858 ms | 81.677 ms | 64.526 ms | 161.892 ms |
| Ryzen x86-64, PHP 8.4.24 | 872.418 ms | 7.087 ms | 49.735 ms | 48.508 ms | 169.083 ms |

The ARM notebook also produced cold single-run values around 0.69--0.75 s
before the change and 3.60 ms after it, so the exact multiplier is sensitive
to passive cooling; every observed comparison remains comfortably above 100x.
The pinned x86 result is 123.1x with 99.19% of the preceding runtime removed.
RPHP JIT is about 9.4x/6.8x faster than the measured PHP tracing JIT. The same
target-neutral change improves RPHP without JIT by about 14.8x/16.7x against
the preceding build and leaves it about 2.0x/3.4x faster than PHP without JIT.

The complete all-feature matrix passes 179 ARM64 and 204 x86-64 library tests,
96 ARM64 and 28 x86-64 native-JIT integration tests, all 27 function, 51 loop
and 108 quick-loop end-to-end tests, and all four corpus tests. A later Object
ABI extension may represent a typed object argument as a nested method
receiver. That should remain separate from this zero-object-ABI `$this`
vertical slice and must prove equivalent class/cache identity and aliasing
guards before admission.

### Conditional exact-Double region checkpoint

The exact-Double function plan now represents one pure `if`/guard-clause with
two exact-Double return edges (2026-08-05). A target-neutral
`ScalarDoubleSelect` stores the predicate, a shared arithmetic prefix and two
disjoint operation ranges. The first bounded slice accepts `==`, `!=`, `<`,
`<=` and direct Double truthiness, local scalar assignments and up to eight
public arguments/eight arithmetic operations. Calls with non-Double runtime
tags, side effects, references, unsupported control flow or an edge without an
exact-Double return retain canonical execution.

The Rust evaluator, standalone leaf JIT and composed call/accumulate loop all
consume the same select representation. ARM64 lowers comparisons with `FCMP`
and condition-code branches; x86-64 uses `UCOMISD`/`VUCOMISD` and explicit
parity branches so unordered `NaN` results preserve PHP behavior. The x86
implementation is covered under both forced SSE2 and AVX. Only the selected
operation range executes: a zero divisor on the inactive edge cannot side
exit, while a zero divisor on the active edge publishes no partial iteration
and replays the canonical PHP error. Conditional leaves nested inside a
composed Double call tree remain deliberately excluded until the composer can
represent control-flow remapping instead of flattening both edges linearly.

The new five-million-iteration
`bench_typed_float_conditional.php` holdout returns the identical
`4687501250000` value and keeps both arithmetic edges hot. Seven serial
`max-perf` runs per mode produced these medians; the PHP JIT lane was verified
as enabled tracing JIT (`kind=5`, `opt_level=4`):

| Host | Previous RPHP | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|---:|
| ARM64, PHP 8.5.9 | 221.877 ms | 3.849 ms | 61.214 ms | 29.527 ms | 85.646 ms |
| Ryzen x86-64, PHP 8.4.24 | 334.352 ms | 5.930 ms | 63.165 ms | 31.762 ms | 85.844 ms |

The shared target-neutral change removes about 98.3%/98.2% of the previous
RPHP time on ARM64/x86-64. RPHP JIT is about 7.7x/5.4x faster than PHP tracing
JIT, while RPHP without JIT is about 1.40x/1.36x faster than PHP without JIT.
Permanent tests cover all four relations, signed zero, unordered `NaN`, both
arithmetic edges, inactive/active division side exits, no-JIT evaluation and a
real PHP loop entering one native region. The full local all-target/all-feature
matrix passes with the test harness stack enlarged for the pre-existing
Ackermann debug test; no product stack setting changed.

The next structural Double step is to let the bounded composed-call IR contain
a conditional callee without duplicating or linearly executing its arms. That
requires explicit block/edge remapping plus the existing target identity and
method-class guards, but should not require a new architecture-specific
recognizer or a workload-specific loop kernel.

### Conditional composed exact-Double merge checkpoint

The bounded composed-call IR now admits one conditional exact-Double callee
inside an otherwise straight function or same-receiver method tree
(2026-08-05). Both branch outputs publish to one target-neutral `Selection`
value; only then does the common arithmetic suffix execute. The composer keeps
the established eight-operation budget and target/cache identity guards. It
does not duplicate either arm, and a second conditional callee rejects the
flattened region and returns to canonical execution.

The Rust evaluator, standalone scalar JIT and call/accumulate loop use the
same merged plan. ARM64 reserves `D24` for the selected value. Linux x86-64
reserves `XMM13`, covered under forced SSE2 and AVX. Branch-only temporaries
cannot escape into the common suffix, inactive divisions remain unexecuted,
and a native side exit publishes no partial iteration. Permanent tests cover
IR remapping, both branch results plus a shared suffix, no-JIT execution,
function and monomorphic method guards, recursive composition, one native
region and safe fallback for two conditionals.

The five-million-iteration
`bench_typed_float_composed_conditional.php` holdout returns the identical
`5859391562500` value. Serial `max-perf` measurements produced these medians:

| Host | Previous RPHP JIT | Merged RPHP JIT | Merged RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|---:|
| ARM64, PHP 8.5.9 | 279.424 ms | 4.530 ms | 78.517 ms | 55.070 ms | 158.498 ms |
| Ryzen x86-64, PHP 8.4.24 | 426.105 ms | 8.795 ms | 108.357 ms | 63.428 ms | 156.816 ms |

The shared merge removes about 98.4%/97.9% of the previous RPHP JIT time on
ARM64/x86-64. Merged RPHP JIT is about 12.2x/7.2x faster than the measured PHP
tracing JIT. RPHP without JIT is about 2.0x/1.45x faster than PHP without JIT,
which independently confirms that the flattened call/frame work benefits the
runtime even without native code. The complete local all-feature matrix
passes with a larger test-harness stack for the existing Ackermann debug
test; the product runtime and stack configuration are unchanged.

The next Double control-flow extension should be chosen from corpus evidence.
Likely candidates are two independent conditional callees or a small nested
conditional, but either requires a bounded multi-merge representation and
register-pressure proof rather than silently expanding this single-selection
contract.

### Guarded invariant JSON projection checkpoint

The first typed-library pipeline slice now handles one loop-invariant
`json_decode($json, true)` followed only by fixed string/integer paths ending
in exact Long leaves (2026-08-05). The two-argument global call first lowers to
the frame-free direct internal ABI; both supported arities also expose the
same borrowed handler to callback consumers. At hot-region entry a
target-neutral JSON prelude decodes once, validates every requested path and
Long tag without
mutating the frame, then atomically publishes the complete decoded PHP value
and the projected scalar slots. The ordinary quick executor and the existing
ARM64/x86-64 Long JIT consume those slots; no JSON-specific machine-code
emitter or workload-specific loop kernel was added.

Admission requires an immutable string CV or string literal, literal
`associative=true`, fixed paths no deeper than eight elements, and a decoded
array that is read-only and does not escape the region. A changing input,
object-mode decode, missing path, non-Long leaf, reference, mutation or any
unsupported consumer retains canonical execution. Guard failure is
transactional: it publishes neither the decoded array nor a partial set of
leaf slots. The complete decoded result is materialized once so reads after
the loop remain correct. This contract matches the currently supported RPHP
JSON semantics; future `json_last_error`, throwing flags and wider PHP decode
options must become explicit effects/guards before this optimization is
extended to them.

The permanent benchmark includes both the supported invariant form and a
changing-input control. Seven order-rotated native-CPU `max-perf` runs on
ARM64 with PHP 8.5.9 produced these internal-time medians; the PHP lane was
verified as active tracing JIT (`kind=5`, `opt_level=4`):

| Workload | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| Invariant fixed Long projections, 2M | 5.874 ms | 52.131 ms | 820.336 ms | 871.693 ms |
| Changing-input canonical control, 200k | 37.326 ms | 38.783 ms | 18.335 ms | 20.702 ms |

For the admitted shape RPHP JIT is 139.7x faster than PHP tracing JIT and RPHP
without native JIT is 16.7x faster than PHP without JIT. Native lowering adds
another 8.9x over the shared quick executor. VM telemetry records one admitted
and executed `typed_ops_loop`, one native execution, zero side exits and only
the 33 canonical warm-up decodes before the remaining 1,999,967 iterations
enter the optimized region. The negative control is intentionally not hoisted;
its roughly 2x deficit against PHP identifies per-document parsing, recursive
`serde_json::Value` conversion and PHP-array materialization as a separate
runtime bottleneck rather than a reason to weaken the invariant guard.

The next typed-library step should lift this one Long-specific prelude into a
shared typed invariant-source representation, then add exact Double and String
projections with the same transactional contract. Only after those leaf types
share one planner should `json_encode`, chained array functions and callback
pipelines be fused into it. In parallel, the changing-input control should be
used to measure improvements to the canonical JSON parser/materializer, which
benefits real non-invariant code and must remain distinct from JIT coverage.

### Shared typed invariant-source checkpoint

The JSON prelude is now a producer-neutral typed invariant source rather than
metadata owned by the Long loop (2026-08-05). One guarded producer describes
its immutable input and materialized destination; fixed projections carry an
exact `Long`, `Double`, `String`, or derived String-length contract. The
runtime parses once, resolves and type-checks every path into temporary values,
and only then publishes the decoded array and all leaves. No partially valid
projection can mutate the canonical frame. Numeric-string keys continue to use
the ordinary PHP array-key normalization.

The existing general Long region consumes exact Long leaves unchanged. An
invariant exact String leaf may now feed `strlen`; its byte length is derived
once as a Long projection, while the real String TMP and decoded array are
still materialized for correct post-loop observation. This deliberately avoids
a String token ABI or architecture-specific heap dereference in native code.
The exact-Double call/accumulate planner accepts the same producer before its
ordinary function initializer, maps fixed Double leaves into the existing
`QuickDoubleArgumentProgram`, and retains all function/method-cache and callee
plan guards. ARM64 and x86-64 therefore reuse their established Double JIT
lowering; there is no JSON opcode in machine code and no JSON-specific loop
kernel.

Admission remains bounded to one dominant associative decode, immutable
literal/String-CV input, fixed paths of depth at most eight and non-escaping,
read-only output. Exact tags are guards, not coercions: an integer where a
Double is requested or a non-String passed to `strlen` falls back to canonical
PHP execution. Changing input, missing paths, mutation and unsupported uses are
also rejected. Permanent unit and end-to-end tests cover planner selection,
post-loop materialization, canonical numeric keys and positive/negative Long,
Double and String cases.

Seven order-rotated native-CPU `max-perf` runs on ARM64 with PHP 8.5.9 produced
these internal-time medians. The PHP JIT lane was verified as active tracing
JIT:

| Workload | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| Invariant fixed Long projections, 2M | 6.191 ms | 54.119 ms | 879.392 ms | 928.467 ms |
| Invariant exact Double call projection, 2M | 1.384 ms | 7.758 ms | 184.570 ms | 228.255 ms |
| Invariant String-to-`strlen` projection, 2M | 0.561 ms | 13.254 ms | 220.360 ms | 232.809 ms |
| Changing-input Long control, 200k | 39.617 ms | 38.352 ms | 18.284 ms | 21.151 ms |
| Changing-input typed control, 200k | 73.205 ms | 71.164 ms | 32.088 ms | 38.542 ms |

For the new admitted shapes RPHP JIT is about 133x/393x faster than PHP
tracing JIT for Double/String respectively. RPHP without native JIT is about
29x/18x faster than PHP without JIT, confirming that the shared producer and
typed executor carry most of the architectural value independently of native
emission. Telemetry records one `double_call_accumulate` or `typed_ops_loop`
admission and native execution, zero side exits, and only the 33 canonical
warm-up decodes before 1,999,967 optimized iterations.

Both changing-input controls remain roughly twice as slow as PHP. This is the
intentional boundary between invariant pipeline fusion and the next canonical
runtime task: profile and improve per-document JSON parsing, recursive value
conversion and PHP-array materialization without relying on hoisting. The next
pipeline extension should reuse this source for `json_encode`, chained array
functions and callback consumers; arbitrary String computation and Double
expression leaves should be admitted only through the same typed projection
and guard model.

### Canonical streaming JSON decode checkpoint

The ordinary changing-input `json_decode` path now streams parser events
directly into canonical RPHP values (2026-08-05). A shared Serde seed/visitor
constructs packed arrays, associative PHP arrays and dynamic objects while the
document is parsed. It no longer allocates a recursive `serde_json::Value`
tree, stores every object in an intermediate B-tree, walks that tree a second
time and immediately destroys it. This is a canonical runtime improvement:
it applies to every supported dynamic decode, independently of warm-up,
profiles, invariant-source admission or native JIT availability.

Parsed object keys move their existing String allocation into associative
array storage. Canonical decimal array-key recognition is now one shared,
syntax-first helper used by compilation, runtime array access and JSON
materialization; canonical numeric keys become integers while leading-zero,
out-of-range and otherwise non-canonical strings remain strings. Direct
materialization also retains JSON input order and updates a duplicate key in
its first position, matching PHP array behavior. Invalid or trailing input
still returns the canonical null result, and associative/object mode is
selected recursively as before.

Sampling the changing document workload identified JSON tree construction,
recursive conversion, B-tree insertion and their malloc/free traffic as the
dominant decode cost. The direct visitor removes that complete intermediate
phase. Dedicated tests cover scalar tags, integer overflow to Double, escaped
strings and surrogate pairs, nested associative/object modes, duplicate-key
order, numeric-key normalization, invalid/trailing documents and surrounding
whitespace.

Nine order-rotated native-CPU `max-perf` runs on ARM64 with PHP 8.5.9 produced
these internal-time medians; PHP tracing JIT was verified active:

| Workload | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| Invariant fixed Long projections, 2M | 6.184 ms | 55.235 ms | 882.232 ms | 920.617 ms |
| Invariant exact Double call projection, 2M | 1.399 ms | 7.701 ms | 182.353 ms | 218.802 ms |
| Invariant String-to-`strlen` projection, 2M | 0.567 ms | 15.626 ms | 225.564 ms | 237.134 ms |
| Changing-input Long control, 200k | 26.016 ms | 25.228 ms | 19.078 ms | 21.406 ms |
| Changing-input typed control, 200k | 53.579 ms | 53.261 ms | 32.746 ms | 38.670 ms |

Against the preceding checkpoint, RPHP no-JIT improves by 34.2% on the
changing Long control (38.352 -> 25.228 ms) and by 25.2% on the changing typed
control (71.164 -> 53.261 ms). The no-JIT gap to PHP narrows from 1.81x to
1.18x and from 1.85x to 1.38x respectively. Invariant projections retain their
separate parse-once architecture; this change deliberately does not introduce
schema caching or a JSON-specific machine-code kernel.

The remaining canonical cost is dominated by unavoidable result ownership and
lifecycle work: String/Rc allocation, PHP array/object insertion, Value
clone/drop, parser string/number handling and allocator traffic. Further work
should therefore be justified by real corpus shapes and allocation telemetry.
The planned fused `json_encode`, collection and callback pipeline remains the
next architectural extension rather than weakening canonical PHP values to
win a decode-only benchmark.

### Shared decoded-stdClass metadata checkpoint

The canonical object-mode decoder now shares the immutable `stdClass` name and
empty declared-property layout within each runtime thread (2026-08-05).
Previously every decoded object allocated identical copies of both structures
before allocating its real dynamic-property map and object container. Each
object still owns independent properties, preserves the ordinary dynamic
`stdClass` behavior and uses the same canonical `Value::Object`; only metadata
that cannot vary is reference-counted once.

A dedicated 500,000-document changing-input benchmark improved from a
228.327 ms no-JIT median to 191.994 ms, or 15.9%. The identical associative
control moved from 122.893 to 123.482 ms (0.5%, within run-to-run variation),
confirming that parser and array paths were not specialized. A permanent
object-mode row now accompanies the projection controls. Nine order-rotated
native-CPU `max-perf` runs with PHP 8.5.9 produced:

| Workload | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| Changing object control, 200k | 77.041 ms | 78.255 ms | 36.832 ms | 38.753 ms |

The remaining object gap is not decoder-only. Sampling attributes a comparable
share to ordinary `$object->property` execution: dynamic `stdClass` reads still
pass through general property-name resolution, visibility lookup and a string
HashMap lookup. That should be treated as a general dynamic-object dispatch
task. The next decode-materialization checkpoint instead targets the measured
fourth-key cliff in associative arrays: a compact ordered representation for
roughly four to eight entries should avoid building and immediately destroying
a full string hash index while retaining canonical mutation and COW behavior.

### Adaptive streamed associative-array storage checkpoint

Associative materializers with an unknown final width can now defer their full
split indexes (2026-08-05). Zero to three entries remain in the existing inline
`SmallHash`; four to eight entries use one ordered vector; a ninth unique key
promotes that same vector to the general integer/string indexes. Known-width
array literals and ordinary general arrays retain their existing indexed
policy, so the optimization is a reusable storage capability selected by
streaming producers rather than a JSON-only value representation.

The bounded representation begins with linear lookup. After four repeated
string reads it lazily creates a string index, so a short-lived decoded row
does not pay for an index it never amortizes while a retained and repeatedly
queried row does not remain linear indefinitely. Structural mutation clears
the derived index; promotion, overwrite, removal, shift, pop, iteration,
copy-on-write cloning and mixed integer/string keys all preserve the canonical
ordered PHP-array semantics. `json_decode(..., true)` is the first producer to
select this policy and continues to move parsed key allocations directly into
the array.

Seven paired native-CPU `max-perf` A/B runs against the preceding streaming
decoder produced these no-JIT internal-time medians for 200,000 changing
documents:

| Associative width | Previous indexed storage | Adaptive storage | Change |
|---|---:|---:|---:|
| 4 keys | 82.322 ms | 69.156 ms | -16.0% |
| 8 keys | 161.280 ms | 121.017 ms | -25.0% |
| 12 keys, promotion control | 225.994 ms | 223.212 ms | -1.2% |

The wider control is important: promoting immediately before the ninth unique
key removes the earlier experimental regression from scanning a full bounded
vector during general-map construction. Repeated dynamic reads of a retained
eight-key decoded row also showed no regression once the lazy index was built.

Nine order-rotated four-mode runs with PHP 8.5.9, with tracing JIT verified
active, produced the following absolute medians. These changing-input cases do
not admit invariant JSON projection or native loop fusion:

| Workload | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| Assoc decode, 4 keys | 81.120 ms | 83.918 ms | 48.007 ms | 50.427 ms |
| Assoc decode, 8 keys | 148.094 ms | 146.677 ms | 83.141 ms | 87.031 ms |
| Assoc decode, 12 keys | 277.109 ms | 272.568 ms | 126.903 ms | 130.413 ms |

The remaining 1.7x-2.1x gap is therefore no longer evidence for building full
indexes earlier. It lies in per-document parser work, key/String ownership,
`Value`/array allocation and destruction, and general dynamic access. The next
canonical checkpoints should separately profile dynamic object-property reads
and parsed-string ownership; neither should weaken PHP array semantics or add
a JSON-specific execution kernel.

### Guarded dynamic-stdClass property cache checkpoint

Ordinary `FetchObjR` sites now cache an exact dynamic-`stdClass` receiver shape
(2026-08-05). A cache hit still validates the current receiver and uses the
current property-name operand, then performs the canonical lookup in that
object's own dynamic-property map. It caches no object-local pointer or map
position. The marker only bypasses repeated caller-scope, inheritance,
visibility and declared-key resolution that cannot affect canonical
`stdClass`. A different class, missing key or unsupported receiver returns to
the complete slow path; declared-property slot caches and magic-property
behavior remain independent.

An isolated benchmark retains one decoded object and performs five million
pairs of `$row->value` and `$row->name` reads. Seven paired native-CPU
`max-perf` no-JIT runs improved from 662.583 ms to 263.676 ms, or 60.2% (2.51x).
The equivalent associative-array control changed from 189.173 to 188.223 ms in
eleven order-rotated runs (-0.5%, within variation). This identifies property
resolution rather than the surrounding loop, `strlen`, or result accumulation
as the removed cost.

The existing changing-input object workload, which also parses and destroys
200,000 distinct documents, improved from 79.685 to 62.202 ms, or 21.9%. The
smaller end-to-end percentage is expected because parsing, String/property-map
allocation and object destruction remain canonical work. Permanent tests reuse
one read site across multiple decoded objects, a declared class, mutation and a
missing property, proving both cache hits and guarded fallback.

Nine order-rotated native-CPU `max-perf` runs of the isolated permanent
benchmark produced the following absolute medians with PHP 8.5.9; the PHP
tracing JIT lane was verified active:

| Workload | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| Retained `stdClass` reads, 5M pairs | 254.977 ms | 250.565 ms | 61.328 ms | 64.311 ms |

The RPHP native-JIT lane is intentionally level with the no-JIT lane because a
dynamic `FetchObjR` is not yet admitted to a native typed region. The remaining
roughly 3.9x no-JIT deficit is now the cost of VM dispatch, receiver/operand
guards, `RefCell` object access, dynamic-map hashing and heap-value cloning,
rather than repeated visibility resolution.

A follow-up experiment deliberately tried to reuse `PhpArray` as the dynamic
property container. Retained reads improved another 18.3% and the two-property
changing decode improved 1.8%, but construction regressed about 13%/12%/54%
for four/eight/twelve-property objects. The experiment was reverted. Array
storage shares keys through `Rc` so ordered entries and `HashMap` indexes can
own one allocation; a plain object map already owns each `String` directly, so
that reuse adds the wrong ownership cost. A future compact dynamic-property
container should therefore be object-specific and own each String once. It
must pass the same width-promotion controls before admission.

### Inline dynamic-property storage checkpoint

The accepted object-specific container keeps up to three dynamic properties
inline in insertion order and promotes a fourth unique name directly into the
existing randomized `HashMap<String, Value>` (2026-08-06). Small objects avoid
both a bucket allocation and hashing while retaining the parser-owned String
without a copy or `Rc`. Wider objects immediately return to the established
secure general map, so attacker-controlled names do not enter a deterministic
hasher and the `PhpObject` field remains one pointer wide. Duplicate names
overwrite their original position; cloning, mutation, direct cached reads and
property iteration all use the shared container API.

Two more ambitious designs were rejected before admission. A position-only
open-addressed index at property nine made width 12 about 22% slower; moving
the threshold to 17 made width 20 about 33% slower. Keeping those objects
linear reduced but did not eliminate the construction regression. A hybrid
ordered-vector-to-HashMap path still paid an extra vector allocation. The final
inline-three design removes both the unused index and intermediate vector: the
fourth insert moves three owned entries straight into the standard map. An
inline-four candidate improved its exact-width workload by 25%, but retained a
repeatable 1.6% width-20 regression; inline three was selected because the
wide control stays neutral.

Nine paired native-CPU `max-perf` no-JIT A/B runs against the preceding
property-cache checkpoint produced these medians. The width-20 control used an
additional eleven isolated order-rotated runs because mixed batches showed
thermal outliers:

| Workload | Previous map | Inline-three map | Change |
|---|---:|---:|---:|
| Retained `stdClass` reads, 5M pairs | 263.839 ms | 193.939 ms | -26.5% |
| Changing object decode, 2 properties | 42.414 ms | 35.642 ms | -16.0% |
| Changing object decode, 4 properties | 68.769 ms | 67.579 ms | -1.7% |
| Changing object decode, 8 properties | 123.124 ms | 122.493 ms | -0.5% |
| Changing object decode, 12 properties | 155.900 ms | 152.720 ms | -2.0% |
| Changing object decode, 20 properties | 248.288 ms | 247.090 ms | -0.5% |

The existing two-property parse plus two-property-read control improved from
59.876 to 47.128 ms (-21.3%). Moving object materialization into a separate
non-inlined helper keeps its evolving code layout out of the generic
associative branch; the unrelated associative-array control remained within
1.0%. Four-mode results below are nine order-rotated internal-time
medians with PHP 8.5.9 and a verified active PHP tracing JIT:

| Workload | RPHP JIT | RPHP no JIT | PHP tracing JIT | PHP no JIT |
|---|---:|---:|---:|---:|
| Changing object control | 46.835 ms | 47.528 ms | 36.269 ms | 39.213 ms |
| Retained `stdClass` reads, 5M pairs | 194.809 ms | 195.318 ms | 63.122 ms | 67.475 ms |
| Object decode, 2 properties | 34.278 ms | 34.249 ms | 29.581 ms | 31.753 ms |
| Object decode, 4 properties | 65.797 ms | 66.166 ms | 48.033 ms | 50.884 ms |
| Object decode, 8 properties | 119.739 ms | 117.679 ms | 83.156 ms | 85.315 ms |
| Object decode, 12 properties | 154.123 ms | 152.495 ms | 123.585 ms | 126.073 ms |
| Object decode, 20 properties | 248.670 ms | 251.599 ms | 204.904 ms | 205.663 ms |

Canonical changing object decode is now only 1.21x slower than PHP without JIT,
and the two-property decode is within 7.9%. The remaining retained-read gap is
about 2.9x and does not shrink under the current native-JIT feature, identifying
VM dispatch, receiver/operand guards, `RefCell` access and heap-Value cloning as
the next independent object-access problem.

### Guarded invariant object-property region checkpoint

The retained-read gap is now handled as a general object optimization rather
than a JSON kernel (2026-08-06). A dynamic-property cache records the shared
canonical `stdClass` `ObjectLayout` pointer in the cache entry's otherwise idle
function-pointer field. A hit compares that existing pointer rather than
rechecking the class-name String and empty declared layout. Small dynamic maps
also contribute a position hint, but every hit validates the current key and
falls back to canonical name lookup when another object uses a different
insertion order. No object-local pointer is retained by the bytecode cache and
`PhpObject` remains its preceding size with no new construction-time store.

The baseline dispatcher also consumes adjacent `FetchObjR -> strlen` directly
from the borrowed String property. It writes only the Long length and skips the
intermediate heap-Value clone, bitmap transition and second opcode dispatch.
Non-String values, missing properties, changed receiver layouts and
magic-property cases retain their canonical paths. A declared receiver arriving
at a dynamic-cached site first performs full resolution, then its ordinary
declared-slot cache can use the same borrowed consumer fusion.

Closed typed loops gained two architecture-independent operations:
`ObjectPropertyLong` and `ObjectPropertyStringLength`. Planning requires an
invariant receiver CV and an exact literal property name. At each region entry,
runtime revalidates the warmed declared/dynamic inline cache, receiver layout,
property name, current value type and reference state, then binds the current
property pointer. Quick execution rereads that pointer per operation. Native
execution hoists the immutable value into an existing Long shadow slot and the
shared straight-loop IR feeds both ARM64 and x86-64 assemblers. `AddAssign` and
`AddAddAssign` were added to the same mixed native lowering, so this extension
also applies to non-object typed regions.

Native hoisting is deliberately rejected when the same region contains an
object method or virtual object pipeline: such a call may mutate the property.
The quick executor remains correct because it rereads the bound pointer. Native
admission can later recover those combinations only after the compiler proves
disjoint property read/write sets. Replacement receivers, different property
orders, declared receivers and a mutating-method control are permanent tests;
all failures resume at the exact original `FetchObjR` without replaying a
committed operation.

Eleven order-rotated native-CPU `max-perf` runs on ARM64 produced these internal
time medians. "Previous" is commit `cb39f90`; PHP 8.5.9 tracing JIT was verified
active:

| Workload | Previous RPHP | RPHP no JIT | RPHP JIT | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|---:|
| `stdClass` Long property, 10M reads | 201.490 ms | 56.609 ms | 6.287 ms | 74.226 ms | 68.422 ms |
| `stdClass` String property + `strlen`, 10M reads | 263.653 ms | 55.232 ms | 5.751 ms | 76.967 ms | 73.454 ms |
| `stdClass` mixed retained reads, 5M pairs | 201.536 ms | 36.552 ms | 5.518 ms | 68.659 ms | 64.596 ms |
| Declared-object mixed reads, 5M pairs | 145.103 ms | 35.917 ms | 5.445 ms | 34.164 ms | 11.221 ms |

The canonical retained `stdClass` mix is now 1.88x faster than PHP without JIT
and 11.71x faster than PHP tracing JIT. The declared-object no-JIT lane is still
5.1% slower than PHP on this ARM64 host, while the native lane is 2.06x faster
than PHP tracing JIT. VM statistics record one admitted/executed native
`typed_ops_loop`, 4,999,967 optimized iterations and zero side exits.

Twenty-one paired controls protect construction and unrelated storage. Changing
object decode moved from 48.620 to 48.306 ms (-0.6%), width two from 35.613 to
34.524 ms (-3.1%), width twenty from 251.691 to 253.736 ms (+0.8%), and the
associative width-four control from 70.694 to 69.599 ms (-1.5%). The rejected
per-object shape-ID variant exceeded the width-20 budget; using the already
shared layout pointer brought the final result below the 1% admission limit.

The same source was synchronized into an isolated x86-64 Linux build and both
feature configurations compiled successfully. Nine-run medians with an active
PHP 8.4.24 tracing JIT were:

| x86-64 workload | RPHP no JIT | RPHP JIT | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|
| `stdClass` mixed retained reads | 58.849 ms | 5.466 ms | 65.759 ms | 50.036 ms |
| Declared-object mixed reads | 58.943 ms | 5.512 ms | 43.485 ms | 12.329 ms |

The remaining object-access work was therefore narrowed to no-JIT
declared-property dispatch on x86-64, varying receivers that cannot bind once
per region, and precise property read/write analysis that can safely combine
native reads with mutating methods.

### Dense no-JIT invariant-property accumulation checkpoint

The x86-64 no-JIT gap was profiled before adding another property-storage
special case (2026-08-06). One five-million-iteration declared-property region
executed 1.222 billion instructions, 311.8 million cycles and 205.6 million
branches, while branch and cache misses were negligible. `perf` attributed
96.7% of samples to `run_quick_long_ops_loop`. VM statistics independently
confirmed one `typed_ops_loop`, 4,999,967 optimized iterations and zero guard
failures or side exits. The bottleneck was the general quick-operation dispatch
graph, not property lookup or memory locality.

The first architecture-independent change materializes guarded property
projections once at region entry. It applies only when the closed region has no
object method, getter, composed call or virtual object pipeline; those shapes
continue to reread the bound property pointer because a call may mutate an
aliased receiver. The same helper now seeds both the no-JIT shadow state and
the existing ARM64/x86-64 native IR. On ARM64 this reduced the declared-object
no-JIT control from 35.917 ms to about 28.9 ms. On x86-64 it removed 9.6% of
executed instructions but only about 1% of elapsed time, proving that the
remaining cost was dispatch throughput.

A compact no-JIT kernel now recognizes the general linear shape
`invariant property projection(s) -> checked accumulation -> induction`.
It accepts either one invariant Long projection or the checked sum of two
Long projections, including borrowed String-property lengths. Property names,
classes, receiver slots and accumulator slots remain arbitrary. The invariant
term is derived once after the loop-entry guard; the dense Rust loop retains
checked accumulator and induction arithmetic, exact overflow resume positions,
condition temporaries, interrupt safepoints and canonical state publication.
Native JIT remains the preferred lane when that feature is enabled.

After lowering the general dispatch graph, the x86-64 control executes 243.8
million instructions, 44.3 million cycles and 50.8 million branches: reductions
of 80.0%, 85.8% and 75.3% respectively. Nine-run native-CPU `max-perf` medians
with PHP 8.4.24 and verified tracing JIT were:

| x86-64 workload | RPHP no JIT | RPHP JIT | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|
| `stdClass` mixed reads, 5M pairs | 8.017 ms | 5.506 ms | 64.923 ms | 49.103 ms |
| Declared-object mixed reads, 5M pairs | 8.044 ms | 5.564 ms | 42.855 ms | 12.220 ms |
| `stdClass` Long property, 10M reads | 15.999 ms | 6.956 ms | 70.637 ms | 50.265 ms |

No-JIT RPHP is now 8.10x, 5.33x and 4.42x faster than PHP without JIT on
these three shapes. It also beats PHP tracing JIT by 6.13x, 1.52x and 3.14x.
Permanent tests cover dynamic and declared receivers, receiver rebinding,
different dynamic-property orders, one- and two-projection bodies, accumulator
and term overflow, and a mutating-method control that must remain on the
per-operation reread path. The next independent object targets are varying
receivers inside one region and read/write-set analysis for regions containing
calls; broader scalar-expression support should extend this compact kernel
rather than reintroduce benchmark-specific executors.

### Guarded changing-receiver foreach checkpoint

The first varying-receiver slice is now a separate target-neutral foreach plan
(2026-08-06). It recognizes a value-only `foreach` whose complete body adds one
or two scalar projections from the current receiver, including a borrowed
String-property length. Property names, receiver/accumulator CVs, array storage
and declared classes remain arbitrary; both packed and ordered hash arrays use
the existing raw positional value layout. This is therefore the same path used
by ordinary application rows, not a JSON- or benchmark-specific opcode.

After the canonical `FetchObjR` sites warm, one entry binding validates either
the declared class ID plus property slots or the canonical dynamic `stdClass`
layout plus property-position hints. Every array element then guards the current
receiver before reading its properties. A changed class/layout, missing or
referenced property, wrong scalar type, or checked-add overflow publishes the
current foreach position, receiver CV, accumulator and already produced
temporaries, then resumes the exact owning `FetchObjR` or `Add`. The value CV
is also materialized on completion, preserving PHP's observable last-foreach-
value behavior. Interrupt polling publishes header-consistent state every 32
iterations.

The executor is written once in Rust and is selected in both no-JIT and native-
JIT feature builds; neither architecture has a private implementation yet. The
planner records a distinct `foreach_object_property_accumulate` coverage kind.
On the declared ARM64 holdout, VM statistics reported one admission, 20,000
completed entries, 5,099,968 optimized receivers and zero guard failures or
deoptimizations. Permanent tests cover declared and dynamic receivers, packed
and hash arrays, one/two projections, last-value publication, changed classes,
a mid-stream Double type side exit, and exact term/accumulator overflow replay.

Fifteen native-CPU `max-perf` medians on ARM64 with PHP 8.5.9 were:

| ARM64 workload | Previous RPHP no JIT | RPHP no JIT | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|---:|
| Declared rows, 5.12M receivers | 148.815 ms | 15.557 ms | 15.772 ms | 54.543 ms | 15.216 ms |
| `stdClass` rows, 5.12M receivers | 197.529 ms | 55.751 ms | 56.249 ms | 97.009 ms | 68.111 ms |

The declared path is 9.57x faster than the preceding RPHP no-JIT executor,
3.51x faster than PHP without JIT and within 3.7% of PHP tracing JIT. The
dynamic path improves 3.54x and beats PHP without and with JIT by 1.74x and
1.21x respectively.

The same source and five focused correctness tests passed on x86-64 Linux,
followed by all 32 x86 native-JIT prototype tests. Eleven-run `max-perf`
medians with PHP 8.4.24 were:

| x86-64 workload | Previous RPHP no JIT | Previous RPHP JIT | RPHP no JIT | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|---:|---:|
| Declared rows, 5.12M receivers | 204.175 ms | 194.034 ms | 17.781 ms | 17.932 ms | 55.507 ms | 20.594 ms |
| `stdClass` rows, 5.12M receivers | 249.894 ms | 230.688 ms | 86.493 ms | 85.763 ms | 82.721 ms | 58.766 ms |

Declared rows improve 11.48x over the preceding no-JIT binary and now beat PHP
without JIT by 3.12x and PHP tracing JIT by 1.15x. Dynamic `stdClass` rows expose
the next narrow object bottleneck: RPHP remains 4.6% behind PHP without JIT and
46% behind tracing JIT on x86-64. The next price/performance experiment should
batch multiple guarded small-map property positions for one receiver, paying
the `RefCell`/storage-shape branch once per row. Native lowering of the same
plan and call-region read/write-set analysis remain later independent steps.

### Batched dynamic-property projection checkpoint

Two projections from the same changing dynamic receiver now share one guarded
object access (2026-08-06). `DynamicPropertyMap::get_pair_at_positions` branches
on inline versus hash storage once. Inline maps validate both cached positions
independently and fall back by name when receivers have a different insertion
order; hash maps perform both lookups after the shared storage dispatch. The
runtime helper combines the receiver-layout guard, dynamic-map access and both
reads while retaining an independent null result for each property, so a
missing or wrongly typed second property still resumes the exact second
`FetchObjR` after publishing the first temporary.

The first implementation also batched declared slots, but native A/B controls
showed that LLVM already merged their simple object loads and the wider runtime
shape caused a regression. The retained design therefore uses the batch only
for two dynamic projections. A constant-generic inner loop produces distinct
ordinary and batched machine-code kernels, and keeping those kernels
non-inlined prevents either architecture from paying the other variant's code-
layout cost. This measured rollback and separation is part of the design, not
a benchmark special case.

Thirty-one order-alternated native-CPU `max-perf` A/B pairs on ARM64 produced:

| ARM64 workload | Previous RPHP no JIT | RPHP no JIT | Previous RPHP JIT build | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|---:|---:|
| Declared rows, 5.12M receivers | 15.714 ms | 13.477 ms | 15.597 ms | 13.712 ms | 54.533 ms | 16.308 ms |
| Inline `stdClass`, 5.12M receivers | 56.721 ms | 35.020 ms | 57.577 ms | 36.121 ms | 95.259 ms | 68.138 ms |
| Hash `stdClass`, 5.12M receivers | 173.633 ms | 121.875 ms | 170.770 ms | 118.987 ms | 94.984 ms | 68.818 ms |

The inline dynamic path improves 38.3% without JIT and 37.3% in the JIT build;
it is now 2.72x faster than PHP without JIT and 1.89x faster than PHP tracing
JIT. The declared control also improves by 14.2% and 12.1%, confirming that the
separate ordinary kernel did not trade away its previous result. The four-
property hash workload improves about 30%, but remains 1.28x slower than PHP
without JIT and 1.73x slower than tracing JIT.

The same source passed the focused shape/order and exact-overflow tests on
x86-64, followed by all 32 x86 native-JIT tests. Thirty-one order-alternated
A/B pairs there produced:

| x86-64 workload | Previous RPHP no JIT | RPHP no JIT | Previous RPHP JIT build | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|---:|---:|
| Declared rows, 5.12M receivers | 18.229 ms | 17.089 ms | 18.107 ms | 16.483 ms | 55.069 ms | 20.693 ms |
| Inline `stdClass`, 5.12M receivers | 86.209 ms | 34.274 ms | 85.627 ms | 35.936 ms | 82.681 ms | 59.163 ms |
| Hash `stdClass`, 5.12M receivers | 173.509 ms | 123.595 ms | 173.812 ms | 123.107 ms | 82.868 ms | 58.567 ms |

The inline dynamic improvement is 60.2% without JIT and 58.0% in the JIT
build. RPHP now beats the corresponding PHP modes by 2.41x and 1.65x; declared
rows retain a 3.22x and 1.26x lead. The remaining object gap is isolated to the
general dynamic hash representation, not foreach dispatch. The next best
price/performance step is a bounded linear dynamic-property tier between the
three-entry inline map and general `HashMap`, mirroring the array storage
strategy. Four-to-eight-property objects should avoid per-read string hashing;
larger or mutation-heavy objects continue to promote to the general map.

### Bounded linear dynamic-property storage checkpoint

Dynamic objects now use three storage tiers (2026-08-06): one to three
properties remain in the allocation-inline array, four to eight live in a
compact ordered `Vec`, and the ninth promotes to the general secure `HashMap`.
The transition moves owned keys and values without cloning them. Direct
construction with a known JSON member count selects the final tier immediately;
streaming or later mutation follows `Small -> Linear -> Hash`. Cloning,
replacement, lookup, mutable lookup, length and property iteration share the
same representation contract, and both compact tiers preserve insertion order.

The linear tier returns a guarded numeric position through the existing dynamic
property inline cache. A warm normal read therefore validates one position and
does not scan or hash. The two-projection foreach batch validates both cached
positions; only a receiver with a different insertion order enters a separate
non-inlined bounded name scan. Promotion to hash makes position validation fail
and safely replays lookup by name. Permanent tests cover direct four-entry
construction, both promotions, replacement, cloning, ordered iteration,
position fallback, mixed insertion orders, mixed linear/hash receivers and a
live cache crossing `Linear -> Hash` before updating an existing property.

Native measurement exposed a second code-shape issue: the ordinary foreach
kernel still combined declared and one-projection dynamic bindings. Adding a
storage variant therefore enlarged even declared-object machine code. Binding
is now a static generic strategy with separate declared, dynamic-single and
dynamic-pair monomorphizations. On x86-64 the declared no-JIT kernel fell from
4,298 to 3,206 bytes; no architecture-specific implementation was added.

Thirty-one order-alternated native-CPU A/B pairs on ARM64, with fifteen PHP
8.5.9 comparison runs, produced:

| ARM64 workload, 5.12M receivers | Previous RPHP no JIT | RPHP no JIT | Previous RPHP JIT build | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|---:|---:|
| Declared rows | 13.184 ms | 12.394 ms | 13.522 ms | 12.599 ms | 54.106 ms | 15.092 ms |
| Two-property inline `stdClass` | 34.291 ms | 35.013 ms | 35.715 ms | 35.526 ms | 93.261 ms | 67.844 ms |
| Four-property linear `stdClass` | 100.283 ms | 35.712 ms | 101.327 ms | 36.121 ms | 94.210 ms | 68.247 ms |
| Eight-property linear `stdClass` | 109.332 ms | 35.938 ms | 109.002 ms | 36.579 ms | 92.489 ms | 67.925 ms |
| Nine-property hash `stdClass` | 110.568 ms | 111.571 ms | 109.861 ms | 108.966 ms | 93.367 ms | 67.799 ms |

The new tier reduces the four-property workload by 64.4% and the eight-property
workload by 67.1% without JIT; the JIT-build reductions are 64.4% and 66.4%.
Those workloads now beat PHP without JIT by 2.64x and 2.57x, and PHP tracing JIT
by 1.89x and 1.86x. Declared rows also improve 6.0% and 6.8%. The hash control
stays within one percent. The two-property no-JIT control regresses 2.1%
(about 0.14 ns per receiver), while its JIT build is stable. Avoiding that small
dispatch cost would require guarding a whole foreach on one dynamic storage
kind and deoptimizing valid mixed-shape inputs, which is not an acceptable
generality trade for this checkpoint.

Fifty-one order-alternated, CPU-pinned A/B pairs on x86-64, with PHP 8.4.24,
produced:

| x86-64 workload, 5.12M receivers | Previous RPHP no JIT | RPHP no JIT | Previous RPHP JIT build | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|---:|---:|
| Declared rows | 16.891 ms | 14.522 ms | 16.322 ms | 15.517 ms | 55.131 ms | 20.274 ms |
| Two-property inline `stdClass` | 34.334 ms | 35.132 ms | 35.880 ms | 35.802 ms | 83.135 ms | 58.333 ms |
| Four-property linear `stdClass` | 124.192 ms | 47.249 ms | 123.789 ms | 37.649 ms | 82.675 ms | 58.872 ms |
| Eight-property linear `stdClass` | 125.056 ms | 47.378 ms | 125.000 ms | 37.811 ms | 83.553 ms | 58.848 ms |
| Nine-property hash `stdClass` | 125.928 ms | 126.353 ms | 125.760 ms | 124.787 ms | 85.012 ms | 59.105 ms |

Four and eight properties improve about 62% without JIT and 70% in the JIT
build. RPHP beats the corresponding PHP modes by 1.75x/1.76x and
1.56x/1.56x. Declared rows improve 14.0% and 4.9%, the hash control remains
within one percent, and the small-object trade is the same 2.3% no-JIT versus a
stable JIT build.

The remaining dynamic-object gap is now sharply isolated at the ninth-property
promotion. Merely widening the linear threshold would move the benchmark
boundary while making arbitrary cold lookups unbounded. The systemic next
design should retain ordered values for wide dynamic objects and add a compact
name-to-slot index that shares each key allocation. Warm property caches can
then keep numeric slots at every width, while cold names and structural writes
retain bounded indexed behavior.

### Indexed wide dynamic-property checkpoint

The wide-object design above is implemented (2026-08-06). The important
architectural observation was that `PhpArray` already had the required general
shape: ordered compact entries plus a separate key-to-position index, with one
`Rc<String>` allocation shared by the entry and index. Dynamic properties now
converge on the same principle without coupling their simpler string-only API
to PHP array integer-key and COW semantics.

`DynamicPropertyMap` therefore has `Small`, `Linear` and `Indexed` tiers. The
indexed tier stores `(SharedStringKey, Value)` entries in insertion order and a
randomized `HashMap<SharedStringKey, usize>`. Promotion moves every existing
String and Value, wraps the owned String without copying its bytes, builds the
index once, and appends the ninth entry. New keys share one allocation between
the ordered entry and index. Replacements, mutable reads and cloning retain
positions and order. This also corrects the old streamed wide-object iteration
behavior, which inherited randomized `HashMap` order.

All dynamic-property tiers now return guarded positions to ordinary property
inline caches. A warm wide-object read validates its numeric slot and avoids
hashing. The two-projection foreach kernel validates both positions in one
storage dispatch; a receiver with a different insertion order calls a separate
non-inlined secure-index fallback. Tests cover allocation sharing, direct and
streamed construction, both promotions, replacement, mutable access, cloning,
ordered iteration, invalid positions, mixed insertion orders, and a live cache
crossing `Linear -> Indexed`. No architecture-specific implementation or
whole-loop storage-shape assumption was added.

Thirty-one order-alternated native-CPU pairs on ARM64 produced:

| ARM64 workload, 5.12M receivers | Previous no JIT | Indexed no JIT | Paired delta | Previous JIT build | Indexed JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Declared rows | 12.455 ms | 12.431 ms | -0.37% | 12.688 ms | 12.462 ms | -1.76% |
| Two-property inline `stdClass` | 34.411 ms | 34.019 ms | -0.60% | 34.504 ms | 33.922 ms | -1.40% |
| Four-property linear `stdClass` | 35.636 ms | 35.641 ms | +0.08% | 35.984 ms | 35.457 ms | -2.55% |
| Eight-property linear `stdClass` | 35.888 ms | 35.684 ms | -0.96% | 36.062 ms | 35.056 ms | -2.70% |
| Nine-property indexed `stdClass` | 110.924 ms | 39.644 ms | -64.49% | 109.599 ms | 39.042 ms | -64.47% |

Against the unchanged PHP 8.5.9 reference medians of 93.367 ms without JIT and
67.799 ms with tracing JIT, the new nine-property path is 2.36x and 1.74x
faster. The four compact controls remain stable or improve.

Fifty-one order-alternated pairs pinned to Ryzen CPU 2 used a clean `git
archive` of `f781cc2` as the baseline and produced:

| x86-64 workload, 5.12M receivers | Previous no JIT | Indexed no JIT | Paired delta | Previous JIT build | Indexed JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Declared rows | 14.495 ms | 14.530 ms | +0.14% | 14.552 ms | 15.195 ms | +4.28% |
| Two-property inline `stdClass` | 35.534 ms | 36.049 ms | +1.48% | 35.193 ms | 35.845 ms | +1.79% |
| Four-property linear `stdClass` | 47.583 ms | 43.209 ms | -9.14% | 48.233 ms | 43.172 ms | -10.55% |
| Eight-property linear `stdClass` | 47.886 ms | 43.668 ms | -8.86% | 48.454 ms | 43.490 ms | -10.25% |
| Nine-property indexed `stdClass` | 127.589 ms | 47.468 ms | -62.80% | 127.121 ms | 46.583 ms | -63.37% |

The new nine-property path beats the existing PHP 8.4.24 no-JIT/JIT references
by 1.79x and 1.27x. The declared JIT control was extended to 201 pairs because
its movement was unrelated to dynamic storage: normalized disassembly confirms
an identical instruction stream and symbol size, while global LTO placement
shifted the function and consistently changed its timing by 4.25%. This is
recorded as build-layout sensitivity rather than hidden as a storage-path
regression. The original remote JIT binary was excluded after its SHA-256 and
kernel sizes proved it was not the exact `f781cc2` source; the no-JIT binary was
byte-identical to the clean baseline.

This checkpoint also sharpens the remaining array work. Ordered indexed hash
arrays, guarded string positions, packed/hash foreach and bounded 4-8 entry
storage are already implemented. Their next general gaps are structural
mutation and COW costs, wide irregular accesses, and callback/JSON pipeline
fusion in the quick/JIT tiers—not the basic key-to-slot representation solved
here.

### Exact contiguous hash-prefix read checkpoint

The first array follow-up is implemented (2026-08-06). Profiles on both ARM64
and x86-64 showed that a materialized integer read from an array that had
changed from packed to hash storage still spent most of its time repeating the
generic key-to-position validation. This is a common real-code shape: build a
list, add one associative metadata field, and continue scanning the original
integer prefix.

The hash storage now retains one word describing a *proven* contiguous integer
prefix. It is established by the existing packed-to-hash transition, preserved
by value replacement, String append and cloning, invalidated by structural
integer removal/pop, and recomputed by `shift`, which already performs linear
reindexing. Ordinary sparse insertion deliberately does not extend or maintain
the proof. It therefore pays no new branch per append. The proof is
conservative: losing it only returns execution to the canonical indexed lookup.

At quick-region activation, an exact read-only layout can be derived from that
proof. COW keeps the backing allocation stable for the region. A target-neutral
one-add kernel then executes fetch, optional materialization and checked
accumulation together; access beyond the proven prefix uses the normal indexed
lookup. Overflow, type changes, interrupts, frame publication and every resume
IP retain their previous transactional behavior. The implementation is shared
by ARM64 and x86-64 and is active in both no-JIT and JIT-feature builds.

Thirty-one order-alternated pairs on ARM64 produced:

| ARM64 workload | Previous no JIT | Exact no JIT | Paired delta | Previous JIT build | Exact JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Direct integer-prefix read | 5.091 ms | 5.011 ms | -3.40% | 4.970 ms | 4.940 ms | -0.77% |
| Materialized integer-prefix read | 5.662 ms | 4.230 ms | -25.20% | 5.265 ms | 3.959 ms | -24.08% |
| Sparse integer transform | 6.141 ms | 6.107 ms | -1.84% | 6.036 ms | 5.894 ms | -2.23% |
| String-key materialized control | 19.347 ms | 19.347 ms | -0.01% | 20.291 ms | 20.226 ms | +0.06% |

On x86-64, thirty-one no-JIT pairs and fifty-one JIT-build pairs produced:

| x86-64 workload | Previous no JIT | Exact no JIT | Paired delta | Previous JIT build | Exact JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Direct integer-prefix read | 3.584 ms | 3.367 ms | -6.86% | 3.169 ms | 3.402 ms | +7.30% |
| Materialized integer-prefix read | 4.676 ms | 3.081 ms | -34.20% | 4.701 ms | 2.986 ms | -36.75% |
| Sparse integer build | 103.092 ms | 104.597 ms | +1.26% | 102.898 ms | 103.005 ms | +0.36% |
| Sparse integer transform | 6.554 ms | 6.583 ms | +0.65% | 6.568 ms | 6.598 ms | +0.58% |
| String-key materialized control | 14.084 ms | 14.105 ms | -0.08% | 13.529 ms | 14.356 ms | +5.74% |

The x86-64 JIT-build controls expose the already documented fat-LTO placement
sensitivity rather than extra work in those paths: normalized original-kernel
instruction streams are unchanged, and `perf stat` measured essentially equal
instructions for the String control (376,481,876 versus 376,344,287) while
cycles moved from 73,755,601 to 78,715,993. The clean no-JIT controls and the
sparse-build control are stable. The materialized target improves strongly in
all four architecture/build combinations.

RPHP no JIT now completes that materialized workload in 4.230 ms versus PHP
8.5.9 no JIT at 5.144 ms on ARM64, and in 3.081 ms versus PHP 8.4.24 no JIT at
7.520 ms on x86-64. PHP tracing JIT remains ahead at 2.639 ms and 2.660 ms;
the RPHP JIT-feature results are 3.959 ms and 2.986 ms respectively.

The permanent sparse-construction benchmark added by this checkpoint reveals
the next, larger general bottleneck. Building one million stride-seven integer
entries takes about 53.4 ms in RPHP versus 14.6 ms in PHP without JIT on ARM64,
and 104.6 ms versus 38.6 ms on x86-64. Each RPHP insertion currently grows and
updates both ordered entries and the integer index, including hashing/probing,
capacity growth and `Value` movement. The next array checkpoint should profile
that construction path and design a general reserve/growth or bulk-construction
strategy without weakening ordered PHP-array semantics.

### Adaptive regular integer-index checkpoint

The sparse-construction follow-up is implemented (2026-08-06). ARM64 sampling
confirmed that the previous path performed a `HashMap::get` followed by a
separate `insert` for every new integer key. `set_int`, hash insertion and
`reserve_rehash` dominated the array side of the profile, while the VM loop and
key/value preparation formed the other large component.

The existing verified integer-prefix word now also describes an exact ordered
arithmetic progression, not only stride one. While all integer entries form
that progression, with at most a String-only suffix, their positions are
derived from the first two stored keys and the verified length. The secondary
integer `HashMap` remains unallocated. A matching append extends the proof and
the ordered entry vector in O(1); replacement validates and updates its exact
position. Adding a String metadata field does not destroy the integer proof.

The first incompatible integer write materializes the complete index exactly
once and continues through `HashMap::entry`, which combines lookup and insert
into one hash/probe. Fully irregular arrays therefore retain the general
indexed representation. Arbitrary removal and `shift` rebuild the appropriate
proof or index; tail `pop`, cloning, COW and `next_int_key` retain their previous
semantics. Packed-to-hash conversion also leaves the redundant integer index
lazy. The exact read kernel from the preceding checkpoint still requires
stride one, so its pointer contract is unchanged. No field or byte was added to
`PhpArray`, and the implementation is target-neutral.

Thirty-one order-alternated ARM64 pairs produced:

| ARM64 workload | Previous no JIT | Adaptive no JIT | Paired delta | Previous JIT build | Adaptive JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Regular stride-seven build | 56.265 ms | 25.203 ms | -54.76% | 59.393 ms | 25.249 ms | -56.60% |
| Irregular integer build | 61.284 ms | 57.575 ms | -5.91% | 62.716 ms | 57.662 ms | -7.22% |
| Packed-to-hash transition | 15.726 ms | 6.158 ms | -60.45% | 16.749 ms | 6.570 ms | -60.72% |
| Irregular integer reads | 126.853 ms | 126.581 ms | -1.52% | 135.639 ms | 135.432 ms | -0.63% |
| Materialized contiguous reads | 4.231 ms | 2.729 ms | -32.85% | 4.384 ms | 2.815 ms | -34.37% |
| String-key read control | 19.390 ms | 19.414 ms | -0.01% | 20.617 ms | 20.630 ms | -0.39% |

The materialized-read gain is a secondary but real memory effect: the array no
longer carries a million-entry integer index that the exact read kernel never
uses.

The same source in thirty-one CPU-pinned x86-64 pairs produced:

| x86-64 workload | Previous no JIT | Adaptive no JIT | Paired delta | Previous JIT build | Adaptive JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Regular stride-seven build | 102.370 ms | 41.436 ms | -59.90% | 101.718 ms | 39.604 ms | -61.03% |
| Irregular integer build | 112.005 ms | 106.612 ms | -5.25% | 110.823 ms | 104.860 ms | -5.17% |
| Packed-to-hash transition | 54.834 ms | 24.091 ms | -56.21% | 54.715 ms | 24.230 ms | -55.97% |
| Irregular integer reads | 100.954 ms | 104.281 ms | +3.18% | 103.694 ms | 102.612 ms | +0.27% |
| Materialized contiguous reads | 3.066 ms | 2.953 ms | -3.27% | 2.980 ms | 3.004 ms | +0.79% |
| String-key read control | 13.888 ms | 13.710 ms | -0.05% | 14.885 ms | 13.247 ms | -8.60% |

The short x86 no-JIT irregular-read timer is a layout-sensitive outlier, not an
added hash operation. A ten-pass read-dominant `perf stat` control records
11.278B versus 11.194B instructions, 5.471B versus 5.493B cycles and 1.041
versus 1.044 seconds; cycle and elapsed changes are inside their run variance.
The large x86 JIT String-control movement is likewise recorded as code-layout
sensitivity rather than attributed to an integer path it cannot execute.

A separate fifteen-run four-mode rotation against PHP produced these new
absolute medians:

| Workload and host | RPHP no JIT | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|
| Regular build, ARM64 / PHP 8.5.9 | 24.803 ms | 24.621 ms | 14.668 ms | 12.326 ms |
| Packed-to-hash, ARM64 | 6.061 ms | 6.460 ms | 5.125 ms | 5.502 ms |
| Materialized reads, ARM64 | 2.330 ms | 2.477 ms | 5.015 ms | 2.668 ms |
| Irregular reads, ARM64 | 133.809 ms | 138.134 ms | 11.105 ms | 5.382 ms |
| Regular build, x86-64 / PHP 8.4.24 | 41.506 ms | 39.518 ms | 36.600 ms | 31.800 ms |
| Packed-to-hash, x86-64 | 24.210 ms | 24.145 ms | 15.359 ms | 15.212 ms |
| Materialized reads, x86-64 | 2.977 ms | 2.990 ms | 7.473 ms | 2.641 ms |
| Irregular reads, x86-64 | 103.729 ms | 103.160 ms | 14.577 ms | 8.043 ms |

The regular construction gap is now modest on x86-64 and substantially smaller
on ARM64. Fully irregular construction remains slower—58.6/106.3 ms versus PHP
without JIT at 20.5/42.7 ms—but the new holdout shows an even larger execution
gap in dynamic irregular reads. ARM64 sampling attributes 775 of 840 samples to
the general `execute_ex` loop and only 21 to `PhpArray::get_int`: the dominant
cost is repeated bytecode dispatch, key-expression materialization and generic
fetch/add handling, not the hash probe itself. The next checkpoint should
therefore extend the target-neutral typed quick/JIT region to composed integer
key expressions plus guarded indexed fetch and accumulation. Materialized-index
layout and irregular structural writes remain the following construction task;
another special key pattern is not the answer.

### Composed integer-key read checkpoint

The composed-key follow-up is implemented (2026-08-06). Bytecode inspection
confirmed that multiplication, checked addition and dynamic integer fetch were
already representable in the general typed loop, but binary `BitwiseAnd` ended
the region. The loop consequently executed every key-expression, fetch and
accumulation opcode through `execute_ex`.

`ScalarLongOpKind` now represents the complete binary integer bitwise family:
AND, OR and XOR. The compiler, no-JIT evaluator, interval/range analysis,
straight-loop IR and both handwritten ARM64 and x86-64 lowerings consume the
same variants. Exact-Long input guards retain canonical PHP coercion for other
types, checked arithmetic retains the precise failing instruction as its side
exit, and bitwise operations themselves cannot overflow.

The target-neutral array kernel now accepts a bounded straight scalar prefix
of up to eight operations before a guarded integer fetch. It normalizes the
existing `ModConst`, Add, general binary and materialized binary operations;
the canonical quick graph remains authoritative outside that bound. Prefix
results are published transactionally before a missing/non-Long fetch resumes
the original `FetchDimR`. Loops without a composed prefix retain their old,
smaller kernel and therefore pay neither a per-iteration empty-prefix branch
nor a larger copied kernel descriptor.

After this removed opcode dispatch, ARM64 sampling assigned 45 of 50 remaining
read-loop samples to `PhpArray::get_indexed_int` and only five to the composed
kernel itself. Replacing Rust's indexed map with a simple flat linear-probe
prototype was explicitly rejected: irregular read time regressed 12.3%, build
time regressed 84.8%, and the larger 16-byte buckets increased cache traffic.
The standard group-probed table remains canonical.

The retained index change is deliberately smaller. Integer hashing now uses
one odd 64-bit multiplicative mix followed by a folded high half; both stages
are bijective/invertible over the full hash word, while the fold moves high-key
entropy into bucket-index bits. Against the preceding two-multiply finalizer it
improved the primary irregular build 2.5% and a new high-bit-key control 2.2%
on ARM64; insertion-order and permuted reads, materialized integer reads and
String reads stayed within 1.3%.

The larger read gain comes from an adaptive ordered cursor over immutable hash
entries. The first canonical index probe also returns the validated insertion
position. The next dynamic key speculates on the following ordered entry and
revalidates the full integer key before using its value. Two prediction misses
disable the cursor for the rest of that region, bounding unordered-workload
overhead. Mutation continues to rebuild canonical positions; COW and the
read-only array guard keep entry storage stable while the region runs. This is
a general insertion-order traversal optimization, not an LCG or literal-key
special case.

Fifty-one order-alternated ARM64 pairs against the adaptive-index checkpoint
produced:

| ARM64 workload | Previous no JIT | Composed no JIT | Paired delta | Previous JIT build | Composed JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular insertion-order read | 135.737 ms | 9.172 ms | -93.21% | 131.216 ms | 9.190 ms | -93.03% |
| Irregular permuted-order read | 274.188 ms | 92.951 ms | -66.27% | 283.807 ms | 96.491 ms | -66.35% |
| Irregular integer build | 58.279 ms | 59.159 ms | +1.24% | 54.956 ms | 54.353 ms | -0.56% |
| High-bit irregular build | 28.293 ms | 28.634 ms | +1.30% | 27.535 ms | 28.444 ms | +2.69% |
| Regular sparse build | 24.720 ms | 24.353 ms | -1.71% | 24.190 ms | 24.169 ms | -0.62% |
| Materialized contiguous read | 2.843 ms | 2.798 ms | -1.32% | 2.721 ms | 2.662 ms | -1.31% |
| String-key materialized control | 19.399 ms | 19.316 ms | -0.49% | 20.246 ms | 20.327 ms | +0.40% |

Thirty-one x86-64 pairs produced the target/read controls below; the longer
101-pair build control was -0.13% no JIT and +2.05% in the JIT-feature build:

| x86-64 workload | Previous no JIT | Composed no JIT | Paired delta | Previous JIT build | Composed JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular insertion-order read | 103.607 ms | 8.877 ms | -91.34% | 103.616 ms | 9.077 ms | -91.25% |
| Irregular permuted-order read | 195.880 ms | 61.385 ms | -68.44% | 189.083 ms | 61.415 ms | -67.48% |
| Irregular integer build | 106.901 ms | 107.181 ms | +0.17% | 107.596 ms | 111.139 ms | +2.50% |
| High-bit irregular build | 51.888 ms | 50.834 ms | -2.35% | 51.849 ms | 53.910 ms | +0.73% |
| Regular sparse build | 41.170 ms | 38.383 ms | -6.49% | 39.779 ms | 41.600 ms | +4.51% |
| Materialized contiguous read | 2.952 ms | 3.074 ms | +4.18% | 2.997 ms | 2.960 ms | -1.01% |
| String-key materialized control | 13.771 ms | 13.901 ms | +1.46% | 13.152 ms | 14.009 ms | +6.35% |

The short x86 control deltas are fat-LTO placement sensitivity, not additional
work in those paths. Ten-run `perf stat` reports 374,704,030 versus 374,602,598
instructions for the JIT String control and 286,007,907 versus 284,848,982 for
the no-JIT materialized integer control. Cycles were 75.646M versus 75.716M and
203.645M versus 204.074M respectively, inside the measured variance.

A separate fifteen-run four-mode rotation gives the current absolute position:

| Workload and host | RPHP no JIT | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|
| Insertion-order read, ARM64 / PHP 8.5.9 | 9.162 ms | 9.151 ms | 10.354 ms | 4.963 ms |
| Permuted-order read, ARM64 | 86.450 ms | 88.510 ms | 63.321 ms | 35.460 ms |
| Insertion-order read, x86-64 / PHP 8.4.24 | 8.945 ms | 9.045 ms | 14.500 ms | 7.792 ms |
| Permuted-order read, x86-64 | 61.793 ms | 63.336 ms | 23.193 ms | 14.768 ms |

RPHP without JIT now wins the original unseen irregular-read workload on both
architectures and comes within roughly 15% of PHP tracing JIT on x86-64. The
permuted control deliberately defeats the ordered cursor and is the honest next
array-read boundary: improving it requires a more cache-efficient canonical
integer index/value layout or native guarded lookup integration, not another
source-expression recognizer.

### Compact integer-index payload checkpoint

The cache-layout follow-up is implemented (2026-08-06). A focused Rust
microbenchmark separated twenty million random operations over the canonical
integer index: probing the index alone took 133.97 ms, reading randomized
ordered entries alone took 58.96 ms, but the dependent index-then-entry chain
took 288.32 ms. The second load therefore cost substantially more when its
address depended on the hash result. This matched the ARM64 sample that placed
45 of the 50 remaining read-loop samples in the indexed lookup after composed
key execution had already been removed.

The canonical integer `HashMap` still has one bucket payload and remains the
only arbitrary-key index. Its former `usize` position is now an equally sized
tagged word. On 64-bit targets, a cacheable entry stores a 24-bit canonical
insertion position plus a signed 39-bit Long in that same word. Typed guarded
reads can consume the Long immediately after the hash probe and avoid the
dependent ordered-entry load. Non-Long values, wider Longs, and positions above
the compact bound store the ordinary canonical position and validate the
ordered entry exactly as before. No extra map, bucket, key-pattern recognizer,
or benchmark-specific path was added.

Insertion, replacement and index rebuild construct the payload from the
authoritative ordered entry. A mutable integer lookup clears its cached Long
before exposing `&mut Value`; removal and shift use the existing rebuild path,
and clone/COW copy or reconstruct exact state. Ordinary PHP-array APIs always
decode the canonical position, so insertion order and mixed-key behavior are
unchanged. Boundary and mutation tests cover negative cached values, the signed
compact limit, `i64::MAX` fallback, replacement, mutable invalidation, clone and
removal. `IntIndexValue`, `ArrayStorage`, and `PhpArray` retain their previous
machine sizes.

The exact contiguous-prefix kernel initially inherited the larger typed
fallback through `#[inline(always)]`, even though its successful prefix path
never executes that fallback. On ARM64 this changed LTO register/code placement
and moved the materialized-prefix control from roughly 2.55 to 3.04 ms. The
fallback is now isolated behind a cold, non-inlined helper. Thirty-one rotated
runs then returned the control to 2.53/2.56 ms without affecting the irregular
target. The permanent wide-Long benchmark uses values above the signed 39-bit
range and verifies the canonical position-based fallback with the same
permuted keys and exact sum on both architectures.

Thirty-one order-rotated ARM64 runs against a clean `f6129d2` build produced:

| ARM64 workload | Previous no JIT | Compact no JIT | Paired delta | Previous JIT build | Compact JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular permuted-order read | 77.100 ms | 35.718 ms | -53.55% | 76.913 ms | 34.992 ms | -54.55% |
| Wide-Long fallback read | 76.767 ms | 76.608 ms | +0.09% | 77.858 ms | 76.688 ms | -0.69% |
| Irregular insertion-order read | 8.638 ms | 8.680 ms | +0.43% | 8.663 ms | 8.837 ms | +2.21% |
| Irregular integer build | 49.974 ms | 49.492 ms | -0.44% | 54.611 ms | 54.748 ms | +0.81% |
| High-bit irregular build | 25.661 ms | 25.583 ms | -0.16% | 29.170 ms | 29.162 ms | -0.28% |
| Regular sparse build | 24.141 ms | 24.151 ms | -0.02% | 29.736 ms | 29.586 ms | -1.28% |
| Materialized contiguous read | 2.548 ms | 2.532 ms | -1.48% | 2.584 ms | 2.563 ms | -2.69% |
| String-key materialized control | 18.763 ms | 18.765 ms | -0.01% | 19.644 ms | 19.640 ms | -0.05% |

The same source passed the complete x86-64 suite and was measured in thirty-one
order-rotated pairs pinned to Ryzen CPU 2:

| x86-64 workload | Previous no JIT | Compact no JIT | Paired delta | Previous JIT build | Compact JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular permuted-order read | 61.757 ms | 41.309 ms | -33.44% | 62.349 ms | 41.623 ms | -32.88% |
| Wide-Long fallback read | 62.430 ms | 62.577 ms | +0.47% | 62.043 ms | 62.145 ms | +0.01% |
| Irregular insertion-order read | 9.128 ms | 8.929 ms | -1.79% | 9.251 ms | 8.832 ms | -4.48% |
| Irregular integer build | 106.425 ms | 107.119 ms | +0.78% | 105.494 ms | 107.163 ms | +1.49% |
| High-bit irregular build | 53.546 ms | 53.010 ms | -2.04% | 53.868 ms | 52.422 ms | +0.11% |
| Regular sparse build | 40.807 ms | 40.022 ms | -2.03% | 42.393 ms | 42.256 ms | -0.38% |
| Materialized contiguous read | 3.102 ms | 2.898 ms | -5.89% | 3.132 ms | 2.907 ms | -7.20% |
| String-key materialized control | 11.850 ms | 13.732 ms | +15.58% | 15.783 ms | 13.268 ms | -15.80% |

The opposing x86 String movements are fat-LTO placement sensitivity rather
than work introduced in a String path. Twenty-run `perf stat` measurements show
326,305,812 versus 326,250,219 instructions in the no-JIT binaries and
376,407,021 versus 376,342,150 in the JIT-feature binaries, both changes below
0.02%. Cycles move from 66.05M to 76.79M in the first pair and from 86.72M to
72.95M in the second. The source cannot execute the integer-index payload for a
String key; the unchanged instruction counts and opposite cycle shifts expose
global code placement transparently.

Twenty-one-run four-mode rotations give the current absolute target position:

| Host and PHP | RPHP no JIT | RPHP JIT build | PHP no JIT | PHP tracing JIT |
|---|---:|---:|---:|---:|
| ARM64 / PHP 8.5.9 | 35.896 ms | 35.420 ms | 49.628 ms | 28.990 ms |
| x86-64 / PHP 8.4.24 | 41.179 ms | 41.519 ms | 23.428 ms | 14.877 ms |

All four modes return the identical `549755289600` sum. RPHP no JIT now beats
PHP no JIT by about 28% on ARM64 and cuts the preceding RPHP permuted-read time
by more than half. x86-64 improves by one third but remains behind PHP, which
leaves native guarded lookup integration as the next read-side boundary.
Irregular structural writes and construction remain a separate storage task.

The final source passes no-default and all-feature checks, 207 ARM64 and 232
x86-64 library tests, 113 quick-loop tests, 100 ARM64 plus 32 x86-64 JIT
prototype tests, and every end-to-end suite. Ten explicit performance tests
remain ignored by design.

### Fixed three-operation composed-prefix dispatch checkpoint

The next composed-read follow-up is implemented (2026-08-06). A read-dominant
profile of the compact-index source assigned 62.81% of self samples to the
general composed array loop, 13.61% to `execute_ex`, and 8.41% to the indexed
fetch closure. Disassembly exposed a remaining front-end cost: one indirect
jump-table site evaluated every scalar-prefix operation, so the branch target
alternated among multiply, bitwise AND and checked addition on every loop
iteration even though the prefix shape was already immutable.

The indexed Hash/Long one-add kernel now has one deliberately narrow fast
case. An exactly three-operation scalar prefix is converted to a fixed array,
and the target-neutral evaluator gives every prefix position its own stable
inlined operation site. Other prefix lengths and body shapes continue through
the bounded general evaluator. The adaptive ordered cursor, canonical indexed
fallback, exact-Long guards, checked-overflow exits, resume IPs, interrupt
handling and transactional slot publication are unchanged.

The common loop mechanics are not duplicated. A single macro body owns
deoptimization, loop control, interrupt polling and publication, while the
general and fixed-prefix functions supply only their prefix, fetch and body
operations. This preserves direct inlining under their different placement
attributes without adding a runtime callback layer. Specializing prefix
lengths one through eight was rejected after it added about 33 KiB of ARM64
text; only the measured three-operation shape remains.

Thirty-one order-rotated ARM64 pairs against the compact-index checkpoint,
with 101 pairs for the two longer controls, produced:

| ARM64 workload | Previous no JIT | Fixed-prefix no JIT | Paired delta | Previous JIT build | Fixed-prefix JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular insertion-order read | 8.872 ms | 7.330 ms | -18.21% | 8.689 ms | 7.225 ms | -17.48% |
| Irregular permuted-order read | 36.058 ms | 35.626 ms | -0.64% | 35.658 ms | 35.864 ms | +0.08% |
| Wide-Long fallback read | 77.372 ms | 77.571 ms | +0.13% | 77.791 ms | 77.422 ms | -0.51% |
| Irregular integer build | 51.556 ms | 49.651 ms | -3.54% | 51.711 ms | 49.945 ms | -3.20% |
| High-bit irregular build | 26.605 ms | 25.743 ms | -3.46% | 26.617 ms | 25.642 ms | -3.68% |
| Regular sparse build | 24.034 ms | 24.003 ms | -0.00% | 23.817 ms | 24.013 ms | +0.27% |
| Materialized contiguous read | 2.562 ms | 2.513 ms | -1.92% | 2.531 ms | 2.511 ms | -1.82% |
| String-key materialized control | 18.771 ms | 18.821 ms | +0.27% | 19.648 ms | 19.795 ms | +0.74% |

The same source in thirty-one CPU-pinned x86-64 pairs, with 101 pairs for the
high-bit and materialized controls, produced:

| x86-64 workload | Previous no JIT | Fixed-prefix no JIT | Paired delta | Previous JIT build | Fixed-prefix JIT build | Paired delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular insertion-order read | 8.943 ms | 7.332 ms | -18.17% | 9.395 ms | 7.018 ms | -25.15% |
| Irregular permuted-order read | 39.533 ms | 39.398 ms | -1.04% | 40.499 ms | 40.250 ms | -1.44% |
| Wide-Long fallback read | 61.212 ms | 60.389 ms | -1.20% | 61.772 ms | 60.988 ms | -0.42% |
| Irregular integer build | 106.652 ms | 106.342 ms | -0.27% | 107.170 ms | 106.913 ms | -0.27% |
| High-bit irregular build | 53.738 ms | 54.654 ms | +1.79% | 53.926 ms | 53.593 ms | -0.41% |
| Regular sparse build | 40.403 ms | 40.302 ms | -0.66% | 39.739 ms | 39.921 ms | +0.20% |
| Materialized contiguous read | 2.903 ms | 2.915 ms | +0.16% | 2.894 ms | 2.904 ms | +0.34% |
| String-key materialized control | 13.693 ms | 11.889 ms | -13.09% | 14.120 ms | 13.237 ms | -5.80% |

The x86 no-JIT high-bit movement is transparently retained as global fat-LTO
layout sensitivity. The change moves `execute_ex` from a 64-byte boundary to
offset 48. Twenty-run `perf stat` measurements record fewer instructions
(594.716M to 594.462M), branches (108.818M to 108.768M) and branch misses
(1.802M to 1.797M), while cycles move from 265.13M to 273.14M. The benchmark
cannot execute the composed-read kernel. Conversely, the eight-pass permuted
read-dominant holdout records 2.07B versus 2.10B no-JIT cycles and 785.4M versus
802.2M branches despite timer noise. Its exact `4398042316800` result matches
in all four modes on both architectures.

The ARM64 `__text` section moves from 1,780,708 to 1,765,296 bytes without JIT
and from 1,954,340 to 1,939,748 bytes in the JIT-feature build because the
shared source body lets LLVM coalesce more generic loop code. x86-64 moves from
2,127,818 to 2,132,970 bytes and from 2,340,314 to 2,345,706 bytes respectively.
The next read-side boundary remains native guarded lookup integration for
unordered keys; fixed-prefix specialization must not expand without a new
profiled common shape and an explicit code-size budget.

The final source passes formatting enforcement, no-default all-target checks,
207 ARM64 and 232 x86-64 library tests, 115 quick-loop tests, 100 ARM64 plus 32
x86-64 JIT prototype tests, and every end-to-end suite. The two new exact-deopt
tests also pass with quick loops disabled; ten explicit performance tests
remain ignored by design.

### Native guarded integer-index lookup checkpoint

The unordered-read JIT boundary is implemented (2026-08-06). A guarded
read-only `PhpArray` can expose a compact native context containing pointers to
its existing canonical integer index and ordered entries. It is available only
when the general hash index is materialized; arithmetic-prefix arrays continue
through their established position-based path. The context does not allocate,
copy keys, or introduce a second map, and the source `Value` plus the region's
read-only/COW proof keep both allocations stable for the activation.

One non-inlined C-ABI helper probes the canonical index, consumes the compact
Long payload when present, and falls back to the authoritative ordered entry
for wide Longs. It writes the destination only after an exact Long hit and
returns zero for a missing key, wrong value type or invalid context. The new
target-neutral `IndexedLongLoad` operation maps that zero to the existing
per-operation resume IP, so the baseline `FetchDimR` replays without observing
a partial native result. Unit tests cover cached and wide Long hits, missing
and non-Long failures, unchanged failed outputs, and rejection of
progression-only hashes. Shared ARM64/x86-64 integration tests prove both a
completed region and one exact type-miss side exit.

Both handwritten backends call the same helper. ARM64 uses `BLR`, preserves
the formerly leaf link register and all live loop/budget state with aligned
`STP`/`LDP` pairs, and passes the shadow result address directly. x86-64 uses
the SysV argument ABI, preserves induction, bound, safepoint budget and input
pointers around an aligned indirect call, and retains its contextual table in
callee-saved `R12`. The first isolated x86 run exposed that the compact disp32
encoder cannot address `R12` without a SIB byte; lowering now rebases through
`RDI`, as the older String-hash context path already did. The permanent shared
integration test failed on the bad encoding and passes on the corrected one.

A manual ARM64 probe over 16.384 million lookups measured 50.02 ms through the
ordinary matched `PhpArray` method and 34.47 ms through the stable helper ABI,
showing that the call boundary itself remained profitable before backend work
was accepted. In nine order-alternated release runs of the eight-pass
permuted-read-dominant holdout, the exact preceding `398591a` ARM64 binary had
a 279.44 ms median and the native-indexed binary 135.50 ms, a 51.5% reduction.
On x86-64, the native binary measured 144.49 ms versus 276.08 ms for the same
source and storage implementation without JIT, a 47.7% reduction. Every mode
returned `4398042316800`; VM statistics recorded eight native typed-region
entries and zero side exits on each architecture.

The final matrix passes formatting and all-target checks, 209 ARM64 and 234
x86-64 all-feature library tests, 115 quick-loop tests, 100 ARM64 and 32 x86-64
backend-specific JIT tests, the two shared native indexed-array tests, and every
end-to-end suite. Both architectures also pass the complete no-default-feature
matrix. With arbitrary-key reads now executing in native scalar regions, the
next array-storage experiment should target fully irregular structural writes
and construction cost. Another read-only key-pattern specialization requires a
new independent profile and code-size budget.

### Guarded structural integer-write checkpoint

The first fully irregular construction checkpoint is implemented (2026-08-06).
Fresh repeated-build profiles on both hosts showed that the canonical integer
index itself was not the only remaining cost. Before this change, ARM64 assigned
about 43% of samples to baseline `execute_ex`, 30% to `PhpArray::set_int` and
19% to `hashbrown` rehashing. x86-64 assigned 44.5% to `execute_ex`, 19.9% to
`set_int` and 9.4% to rehashing. Replacing the standard group-probed table was
therefore still the wrong boundary; the profitable general step was to remove
per-iteration opcode dispatch, key conversion, value cloning and repeated COW
resolution around the existing canonical mutation.

The target-neutral typed loop now has `SetArrayLong`. It admits a Long key and
Long value only when the destination is a unique COW array and the same array
has no borrowed read view in the region. Runtime resolves the mutable
`PhpArray` once at entry, then every iteration calls the authoritative
`set_int`, preserving insertion order, replacement, `next_int_key`, compact
cached-Long index payloads and all storage-tier transitions. String keys,
references, shared arrays and mixed read/structural-write regions keep their
canonical fallback. The older proven-existing `StoreArrayLong` remains a
separate non-structural operation and retains its native read/update fusions.

High-bit construction exposed one independent typed-IR gap: PHP `<<` and `>>`
were still forcing the entire loop back to baseline. A target-neutral `Shift`
operation now uses the same wrapping-count behavior as the canonical bytecode.
It accepts guarded Long operands and handles both directions; no overflow side
exit is introduced. The high-bit workload consequently executes 499,967 typed
iterations after warmup with one entry, one completion and zero deoptimizations.

The structural audit also found a stale-pointer risk in an older packed
read-plus-append shape. A new structural-output mask rejects that activation
when its borrowed view is a movable packed element buffer. Hash views remain
eligible because they resolve through the stable `PhpArray` object, while
general keyed sets are rejected at planning time if the same array is read.
Permanent tests cover planner admission, read/set exclusion, replacement and
insertion order, COW isolation, high-bit and wrapping-shift semantics, and the
packed read/append fallback.

Fresh absolute medians compare the preceding native-indexed checkpoint with
the final source:

| Workload | ARM64 before | ARM64 final | Delta | x86-64 before | x86-64 final | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular integer build 1M | 48.727 ms | 39.122 ms | -19.72% | 100.009 ms | 78.076 ms | -21.93% |
| High-bit irregular build 500K | 26.064 ms | 19.872 ms | -23.76% | 49.596 ms | 38.796 ms | -21.78% |
| Regular sparse build 1M | 24.719 ms | 10.037 ms | -59.40% | 39.694 ms | 24.931 ms | -37.19% |

All workloads retain their exact output. A same-binary temporary A/B switch,
removed from the final source, isolated `SetArrayLong` from fat-LTO placement:
the primary irregular build measured 37.382 versus 55.365 ms on ARM64
(-32.48%) and 82.211 versus 107.468 ms on x86-64 (-23.50%). The regular sparse
control measured -67.24% and -43.98% respectively. This also confirmed that
the unrelated pre-shift high-bit fallback executed identical opcode counts in
both A/B modes; its retained gain comes from typed shift admission rather than
global layout.

After the change, ARM64 sampling assigns about 40.5% to canonical `set_int`,
34.1% to the general quick dispatcher and 23.6% to table rehashing. x86-64
assigns 44.7% to the quick dispatcher, 13.0% to rehashing and 12.1% directly to
`set_int`. The next experiment should therefore lower `SetArrayLong` through a
stable mutable `PhpArray` helper in a native mixed region, preserving the same
COW and safepoint contract. A separate one-time capacity hint derived from a
proven remaining trip count is worth measuring against the rehash share, but
must retain bounded memory behavior for early exits and must not reintroduce a
custom integer table.

The final source passes formatting, 212 ARM64 and 237 x86-64 all-feature
library tests, 118 quick-loop tests, 100 ARM64 and 32 x86-64 backend-specific
JIT tests, the two shared native indexed-array tests and every end-to-end suite.
Both architectures also pass the complete no-default-feature matrix.

### Native structural integer-write checkpoint

The native follow-up is implemented (2026-08-06). Target-neutral straight IR
now includes `ArrayLongSet`, which consumes guarded Long key/value operands and
one stable mutable-array context. The mixed-region runtime obtains that
`PhpArray` pointer from the existing unique-COW guard once per activation. Both
handwritten backends then call one C-ABI helper per iteration; the helper still
delegates storage promotion, replacement, growth, insertion order,
`next_int_key` and compact-index maintenance to the authoritative `set_int`.
It returns zero for an invalid context, which maps to the operation's exact
resume IP before any write.

The context table was refactored from an ambiguous indexed/non-indexed Boolean
to three explicit kinds: a prevalidated entry pointer, a read-only indexed
lookup context and a mutable `PhpArray`. This keeps dispatch-error snapshots
limited to scalar entry pointers and prevents a read context or whole-array
pointer from being dereferenced as a Long payload. Structural mutations remain
committed across later exact side exits, matching bytecode execution, while the
existing chunk boundary retains interrupt polling and bounded safepoint
latency. ARM64 preserves the link register and live loop/control state around
`BLR`; x86-64 keeps the SysV stack aligned and preserves its live caller-saved
state around the indirect helper call.

Permanent coverage now calls the helper directly for promotion, irregular
insertion, replacement, wide Long and null-context behavior. The shared native
array suite proves that a standalone structural-write loop enters native code,
grows the canonical index, preserves a preexisting COW alias and completes
without a side exit on both architectures. The existing read and exact
type-miss tests now also observe the newly native construction loop, ensuring
that read and write contexts coexist across independent regions.

Warm release measurements discard two warmups and use 31 measured runs for the
two native targets and 51 for the non-target high-bit control. They compare the
preceding guarded structural-write checkpoint with the final source:

| Workload | ARM64 before | ARM64 native | Delta | x86-64 before | x86-64 native | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular integer build 1M | 39.122 ms | 26.158 ms | -33.13% | 78.076 ms | 65.127 ms | -16.59% |
| Regular sparse build 1M | 10.037 ms | 6.153 ms | -38.70% | 24.931 ms | 18.638 ms | -25.24% |
| High-bit irregular build 500K | 19.872 ms | 18.727 ms | -5.76% | 38.796 ms | 37.615 ms | -3.04% |

Every workload retains its exact output. VM statistics for the primary build
record one native typed-region entry, 999,967 native iterations, one completion
and zero deoptimizations on both hosts. The high-bit control cannot execute the
new operation because native lowering still stops at its preceding `Shift`;
its stable-to-better result therefore rules out a general quick-dispatch
regression rather than being credited to native array writes.

The final matrix passes formatting and all-target checks, 213 ARM64 and 238
x86-64 all-feature library tests, 118 quick-loop tests, 100 ARM64 and 32 x86-64
backend-specific JIT tests, all three shared native indexed-array tests and
every end-to-end suite. Both architectures also pass the complete
no-default-feature matrix; the recursive Ackermann test uses the established
16 MiB test-thread stack on both hosts.

### Bounded native structural-write capacity checkpoint

The capacity follow-up is implemented (2026-08-07). A fresh 256-round profile
of the newly native irregular build first confirmed the remaining target:
`hashbrown::reserve_rehash` owned 1,139 of 3,756 steady ARM64 samples (30.3%)
and 11.61% of the corresponding isolated x86-64 profile. The runtime now
derives one reservation hint from the guarded induction and bound, but only
when at least 2^19 writes remain. All unique destination arrays share a 2^20
entry ceiling, so neither an extreme bound nor multiple outputs can multiply
speculative memory without limit.

Reservation never changes an array's storage tier. An existing general Hash
reserves its ordered entries; its integer index reserves buckets only when an
irregular index has already materialized. Packed, SmallHash and LinearHash
remain authoritative until canonical `set_int` promotion. This also preserves
the progression-only Hash representation: it may reserve ordered entries, but
does not eagerly create the integer index that its verified arithmetic prefix
does not need. Direct allocation happens only after every activation guard and
native-program preparation succeeds.

One compiled plan retains two bounded straight-program identities. The common
variant passes the `PhpArray` directly to the original write helper and pays no
state check per iteration. A second variant is selected only when a hot plan is
reused with a fresh array below Hash storage. Its stack context retries the
reservation for at most eight canonical writes, long enough to observe normal
SmallHash-to-Hash promotion, and then becomes inert. Multiple operations for
the same destination share one hint. This avoids both repeated growth on fresh
function/loop activations and a tag branch in the primary million-write path.

The repeated-fresh-array profile now observes one `reserve_rehash` sample out
of 1,526 (<0.1%) with a 66.4 MiB ARM64 peak footprint. Permanent tests cover
the shared cap and minimum threshold, preservation of Packed/progression
tiers, lazy reservation after SmallHash promotion, direct then deferred cache
reuse across fresh arrays, COW, exact growth, and a structural mutation that
must remain committed before a later arithmetic-overflow side exit.

Alternating same-host A/B runs compare separately built exact commit `9560d04`
and the final source under the same release flags. The primary target uses 103
measured pairs after five warmups; controls use 51 pairs. Times are medians of
the PHP-internal timed region:

| Workload | ARM64 before | ARM64 capacity | Delta | x86-64 before | x86-64 capacity | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Irregular integer build 1M | 27.093 ms | 14.422 ms | -46.77% | 65.266 ms | 55.407 ms | -15.11% |
| Regular sparse build 1M | 6.382 ms | 6.263 ms | -1.87% | 17.476 ms | 17.371 ms | -0.60% |
| High-bit irregular build 500K | 19.209 ms | 20.021 ms | +4.23% | 37.056 ms | 36.914 ms | -0.38% |
| Packed build plus read 500K | 4.349 ms | 4.416 ms | +1.54% | 7.170 ms | 7.190 ms | +0.28% |

The 500K controls are below the reservation threshold. The small ARM64
high-bit movement is therefore not attributed to the optimization and remains
a code-layout holdout; x86-64 and the packed controls are neutral. The primary
workload retains one native entry, 999,967 native iterations, exact output and
zero deoptimizations on both hosts.

The final matrix passes formatting and no-default/all-feature checks, 216
ARM64 and 241 x86-64 all-feature library tests, 118 quick-loop tests, 100 ARM64
and 32 x86-64 backend-specific JIT tests, all five shared native indexed-array
tests and every end-to-end suite. The recursive Ackermann test again uses the
established 16 MiB test-thread stack on both hosts.

### Guarded scalar callback ABI checkpoint

The first callback-pipeline checkpoint is implemented (2026-08-07). Resolved
plain user callbacks can now evaluate a compiler-proven pure
`ScalarLongFunctionPlan` without allocating, initializing and cleaning one VM
frame per array member. The shared boundary verifies the exact user function,
fixed public arity, scalar-plan-compatible signature, raw non-reference Long
arguments and checked arithmetic before publishing a result. Any callable,
type or arithmetic mismatch uses the existing canonical callback path. Plan
evaluation is side-effect free, so an overflow guard may safely replay the
original callback and preserve PHP's ordinary result or exception.

`array_map`, `array_filter` and packed `call_user_func_array` calls share this
invocation boundary; `array_reduce` feeds its already-owned carry and item into
the same ABI without cloning either value on an admitted iteration. Direct
internal handlers keep their existing slice ABI. Closures with captures,
instance and static methods, inherited methods, invokable objects, reference
arguments, impure bodies and unsupported return types retain the canonical
receiver/capture-aware frame path.

As part of the same cleanup, `array_map` and `array_filter` now use the common
callback resolver rather than their former string-only lookup. That adds the
callable forms above while retaining the monomorphic string cache, visibility
checks, key order, exception propagation and partial-progress behavior.
`array_map` also creates its result with the input's known packed or associative
capacity; filtering remains streaming because its result cardinality is not
known. Permanent isolated map/filter/reduce benchmarks and one three-stage
pipeline benchmark cover the new boundary.

Alternating same-host A/B runs compare exact commit `e9b2665` with the final
source under identical `--release --features jit-prototype` builds. Each result
is the median of 103 measured pairs after five warmups and times only the PHP
callback operation over 500,000 input values:

| Workload | ARM64 before | ARM64 callback ABI | Delta | x86-64 before | x86-64 callback ABI | Delta |
|---|---:|---:|---:|---:|---:|---:|
| `array_map` Long transform | 8.491 ms | 4.108 ms | -51.62% | 16.539 ms | 8.588 ms | -48.07% |
| `array_filter` Long predicate | 9.011 ms | 5.958 ms | -33.88% | 15.421 ms | 9.266 ms | -39.91% |
| `array_reduce` Long sum | 8.583 ms | 4.620 ms | -46.17% | 17.075 ms | 9.722 ms | -43.06% |
| map/filter/reduce pipeline | 21.973 ms | 12.320 ms | -43.93% | 40.447 ms | 22.342 ms | -44.76% |

Every workload retains its exact value and element count. The complete ARM64
all-feature suite passes 216 library tests, all end-to-end suites, 118
quick-loop tests, 100 backend JIT tests and all five shared native indexed-array
tests. Focused x86-64 validation passes 241 library tests, 33 callback-array
tests, 54 general callable tests, 118 quick-loop tests, all 32 backend JIT tests
and the same five shared indexed-array tests. Both hosts also pass the
no-default-feature build; the established 16 MiB test-thread stack is used for
the complete ARM64 run.

### Transactional scalar callback-pipeline fusion checkpoint

The first multi-stage callback fusion is implemented (2026-08-07). The
compiler recognizes the exact nested bytecode span
`array_reduce(array_filter(array_map(callback, source), callback), callback,
initial)` when all three callback operands are string literals, the source is
a directly readable CV or constant and the initial carry is a Long literal.
Materialized stages, dynamic callbacks, namespaces with fallback resolution,
argument expressions and every other arrangement retain their original
bytecode unchanged.

Runtime resolves all three exact callback identities and requires pure
`ScalarLongFunctionPlan` bodies with arities one, one and two. The input array,
initial carry and every member must be non-reference Long values. Admitted
execution maps, tests and reduces each member in one streaming pass, without
constructing either intermediate PHP array or any callback frame. Since the
plans cannot observe globals or produce side effects, their otherwise
different inter-stage ordering is unobservable. Impure callbacks use the
canonical three-stage order.

The fused pass is transactional: type, arithmetic and callback-plan failures
publish no result and resume the untouched outer `InitFcall`, allowing the
ordinary nested calls to produce PHP's Double overflow result, error or other
fallback. Callback call counters are committed in bulk only after complete
success. Interrupts remain polled every 256 members. Permanent coverage proves
the exact detector, rejects dynamic and materialized-stage shapes, checks the
fused result, exercises a later Double input at the same call site, verifies
canonical side-effect order for impure callbacks and replays Long overflow to
a Double result.

Alternating same-host A/B runs compare exact commit `212e416` with the final
fusion source under identical `--release --features jit-prototype` builds. The
pipeline uses 103 measured pairs after five warmups; isolated controls use 103
pairs on ARM64 and 51 on x86-64. Times are medians of the PHP-internal region
over 500,000 values:

| Workload | ARM64 before | ARM64 fusion | Delta | x86-64 before | x86-64 fusion | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Nested map/filter/reduce | 13.263 ms | 5.080 ms | -61.70% | 22.460 ms | 6.009 ms | -73.24% |
| Isolated `array_map` control | 4.402 ms | 4.438 ms | +0.82% | 8.630 ms | 8.602 ms | -0.32% |
| Isolated `array_filter` control | 6.230 ms | 6.137 ms | -1.49% | 9.239 ms | 9.215 ms | -0.26% |
| Isolated `array_reduce` control | 4.794 ms | 4.661 ms | -2.77% | 9.652 ms | 9.655 ms | +0.03% |

All outputs remain exact. An all-feature VM-statistics run records three
`InitFcall` and two `DoFcall` opcodes for the complete benchmark: two call
pairs belong to `microtime`, leaving only the fused outer pipeline initializer
and no materialized pipeline `DoFcall`. It also records 1,250,002 logical fast
callback calls through bulk accounting and only three physical frames (main
plus the two timers).

The focused final matrix passes formatting and all-feature/no-default checks,
218 ARM64 and 243 x86-64 library tests, 37 callback-array tests under both
feature sets, 54 general callable tests, 118 quick-loop tests, 100 ARM64 and 32
x86-64 backend JIT tests, and all five shared native indexed-array tests.

### Dead-staged scalar callback-pipeline fusion checkpoint

The callback fusion now also covers the common staged spelling (2026-08-07):

```php
$mapped = array_map("mapValue", $values);
$filtered = array_filter($mapped, "keepValue");
$sum = array_reduce($filtered, "sumValue", 0);
```

The compiler admits only the exact consecutive opcode span inside a function,
with distinct `$mapped` and `$filtered` CVs whose only syntactic mentions are
their defining assignment and the immediately following stage's source send.
Any earlier or later read, reassignment, alias operation, global/static bind,
different call shape or main-scope pipeline retains the canonical bytecode.
This conservative whole-function escape proof makes both intermediate arrays
unobservable and allows the existing streaming evaluator to begin at the map
initializer and publish only the final reduce temporary.

A runtime guard additionally requires both discarded destinations to still be
undefined. This covers parameters, including reference parameters, which enter
the frame initialized without a defining opcode: their canonical assignments
and cleanup are never skipped. Callback identity, purity, arity, source,
initial-carry, member-type, reference, arithmetic and interrupt guards remain
the same as for nested fusion. Every failure happens before externally visible
state is published and replays the untouched staged calls and assignments.
Nested and staged spans use separate compiler markers and runtime entries. The
original nested evaluator remains unchanged; staged-only escape guards and
future shape extensions therefore cannot enlarge or branch its tuned hot path.

Permanent coverage detects the dead staged span, rejects an escaping result,
checks exact fused output, verifies materialized array counts when either stage
escapes and proves that an initialized by-reference destination receives its
canonical array assignment. A separate 500,000-value staged benchmark keeps
the dead-local source spelling stable.

Alternating same-host A/B runs compare exact commit `a897b64` with this source
under identical `--release --features jit-prototype` builds. The staged target
and the already-fused nested control each use 103 measured pairs after five
warmups. Times are medians of the PHP-internal timed region:

| Workload | ARM64 before | ARM64 staged fusion | Delta | x86-64 before | x86-64 staged fusion | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Dead-staged map/filter/reduce | 12.295 ms | 4.934 ms | -60.14% | 22.505 ms | 6.346 ms | -71.77% |
| Nested fusion control | 4.909 ms | 4.895 ms | -0.75% | 6.029 ms | 6.005 ms | -0.04% |

All outputs remain exact. The focused final matrix passes formatting,
all-feature and no-default checks, 219 ARM64 and 244 x86-64 all-feature library
tests, 40 callback-array tests under both feature configurations, 54 callable
tests, 118 quick-loop tests, 100 ARM64 and 32 x86-64 backend JIT tests, and all
five shared native indexed-array tests.

### Filter-first scalar callback-pipeline fusion checkpoint

The opposite canonical stage order is now covered as a separate specialization
(2026-08-07):

```php
$filtered = array_filter($values, "keepValue");
$mapped = array_map("mapValue", $filtered);
$sum = array_reduce($mapped, "sumValue", 0);
```

Both this dead-staged form and the equivalent nested expression are recognized.
The staged detector retains the previous conservative contract: it runs only
inside a function, requires distinct intermediate CVs, proves that each CV is
mentioned only by its defining assignment and immediate consumer, and checks
at runtime that both raw destinations are still undefined and are not
references. Escaping arrays, initialized parameters, by-reference aliases,
main-scope variables and non-consecutive calls therefore materialize through
the canonical bytecode.

The streaming evaluator preserves the observable stage order. It tests every
source Long with the filter plan, maps only retained members, and reduces those
mapped values. Exact callback identity, purity, arity, non-reference Long
members, checked arithmetic and initial-carry guards remain transactional. A
Double or overflow encountered after earlier pure evaluations publishes no
state and replays the untouched calls; impure callbacks never enter fusion and
retain complete filter-then-map-then-reduce side-effect order. Logical callback
counters are published only after successful completion.

This work also exposed a baseline VM-stack reuse bug independent of fusion.
Internal handlers wrote results through raw pointers, while both DoFcall paths
first dropped the destination even when its TMP bitmap bit was clear. Reused
stack bytes could contain a call-frame header rather than a valid `Value`.
Return writes now use one caller-frame slot API, raw internal-handler writes are
bracketed by bitmap-aware prepare/finish helpers, and frames larger than the
64-slot bitmap initialize their TMP area. The same API replaces blind scalar
return drops in baseline, hot and planned-block execution. A canonical-path
regression test deliberately rejects fusion before reproducing the two-frame
reuse shape.

Alternating same-host A/B runs compare exact commit `1c91b95` with this source
under identical `--release --features jit-prototype` builds. Each workload uses
103 measured pairs after five warmups. Values below are medians of the
PHP-internal timed region over 500,000 source values:

| Workload | ARM64 before | ARM64 filter-first | Delta | x86-64 before | x86-64 filter-first | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Filter/map/reduce target | 10.882 ms | 4.107 ms | -62.26% | 19.972 ms | 5.045 ms | -74.74% |
| Existing nested map/filter control | 4.927 ms | 5.061 ms | +2.72% | 5.977 ms | 5.957 ms | -0.34% |
| Existing staged map/filter control | 4.951 ms | 4.992 ms | +0.83% | 6.350 ms | 6.373 ms | +0.37% |
| Call-heavy frame control (5M iterations) | 97.331 ms | 97.632 ms | +0.31% | 112.763 ms | 113.200 ms | +0.39% |

All outputs remain exact. The small ARM64 nested movement has no executed-path
source change, is absent on x86-64 and is retained as a code-layout holdout;
the other controls are neutral. The final matrix passes
formatting, 221 ARM64 and 246 x86-64 all-feature library tests, 44 callback-array
tests, 25 frame-cleanup tests, 54 callable tests, 118 quick-loop tests, 100
ARM64 and 32 x86-64 backend JIT tests, and all five shared native indexed-array
tests. Focused callback and frame-reuse tests also pass without default
features. A full no-default run reaches the pre-existing Ackermann test-thread
stack overflow; an archived exact `1c91b95` source reproduces the same failure.

The following checkpoint replaces the three shape-specific evaluators with one
small target-neutral collection pipeline program while retaining separate
detectors and the measured tight loops.

### Normalized scalar callback collection program checkpoint

Nested map/filter, dead-staged map/filter and nested or staged filter/map now
lower to one `CallbackArrayPipelineProgram` (2026-08-07). The normalized
description carries the bytecode span, one `MapFilter | FilterMap` order and an
optional pair of discarded CVs. Bytecode detectors and compiler markers remain
separate, so extending one syntactic proof cannot widen another admission path.

Runtime shares destination guards, callback identity and scalar-plan
resolution, source/carry validation, exact final resume construction, interrupt
handling and transactional bulk accounting. Stage order is dispatched once
before member iteration into one of two const-specialized evaluators; the
compiled member loops contain no enum match or indirect stage dispatch. This
reduces `callback_array_pipeline.rs` from 367 to 288 lines and the combined
detector/runtime source from 945 to 884 lines.

Alternating same-host A/B runs compare exact commit `7ba56dd` with the normalized
program under identical `--release --features jit-prototype` builds. All three
workloads use 103 measured pairs after five warmups. Values are medians of the
same 500,000-member PHP-internal timed regions:

| Workload | ARM64 before | ARM64 normalized | Delta | x86-64 before | x86-64 normalized | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Nested map/filter/reduce | 4.897 ms | 4.541 ms | -7.26% | 5.982 ms | 5.789 ms | -3.23% |
| Dead-staged map/filter/reduce | 4.819 ms | 4.563 ms | -5.32% | 6.388 ms | 5.821 ms | -8.88% |
| Filter/map/reduce | 4.037 ms | 3.957 ms | -1.98% | 5.023 ms | 5.046 ms | +0.47% |

Every result remains exact. The small x86-64 filter/map movement is neutral;
the two established map/filter forms improve on both targets as duplicated
guard and loop code leaves the instruction working set. Formatting, 221 ARM64
and 246 x86-64 all-feature library tests, and all 44 callback-array tests under
default and no-default features pass on both hosts.

The following checkpoint consumes this normalized representation through a
guarded scalar sink. Wider array or object encoding remains canonical until
its layout, ownership and error-state contracts are explicit.

### Direct Long callback-pipeline JSON sink checkpoint

An exact one-argument `json_encode` wrapper can now consume the final Long from
any admitted callback collection program without materializing the inner
reduce temporary or entering the outer internal-call frame (2026-08-07). This
covers nested and dead-staged map/filter and filter/map forms. The staged
detector explicitly admits the compiler's outer JSON initializer between the
second assigned collection stage and reduce while retaining both dead-CV
proofs.

Compiler metadata records stage order and staged status at the entry
instruction. Release execution decodes the already-proven immutable layout
directly instead of rescanning 15--18 instructions on every call. Runtime still
guards the cached `json_encode` target as the exact one-argument internal
function, both discarded destinations, callback identity and purity, source
and initial-carry representation, every member and checked scalar operation.
Any failed guard leaves the original bytecode untouched and canonical.

This slice is deliberately limited to Long encoding under RPHP's current
one-argument `json_encode` signature. Double results, impure callbacks,
overflow, references, escaping staged arrays, dynamic callables and namespaced
function shadows all replay normally. Array/object encoding, options, depth and
JSON error-state behavior are not claimed by this optimization.

Alternating same-host A/B runs compare exact commit `df26cfd` with this source
under identical `--release --features jit-prototype` builds. Each workload uses
103 measured pairs after five warmups; x86-64 processes are pinned to CPU 2.
Values are medians of the PHP-internal timed region:

| Workload | ARM64 before | ARM64 JSON sink | Delta | x86-64 before | x86-64 JSON sink | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Repeated small callback pipeline + `json_encode` | 30.870 ms | 27.751 ms | -10.10% | 36.631 ms | 32.614 ms | -10.97% |
| Nested map/filter/reduce control | 4.704 ms | 4.677 ms | -0.57% | 5.777 ms | 5.789 ms | +0.20% |
| Dead-staged map/filter/reduce control | 4.749 ms | 4.715 ms | -0.72% | 5.797 ms | 5.803 ms | +0.11% |
| Filter/map/reduce control | 4.157 ms | 4.081 ms | -1.83% | 5.001 ms | 5.000 ms | -0.03% |

All checksums remain exact. Permanent tests cover all four admitted shapes,
Double and impure fallback, canonical callback order, escaping intermediate
materialization and namespaced `json_encode` shadowing. The focused matrix
passes 218 ARM64 and 243 x86-64 JIT-prototype library tests, 222 and 247
all-feature library tests respectively, plus all 48 callback-array tests under
both default and no-default feature configurations.

### Adaptive callback-pipeline metadata cache checkpoint

Ordinary nested and dead-staged callback pipelines now stop rescanning their
immutable instruction span after the first successful fused execution
(2026-08-07). The first pass deliberately retains the complete structural
detector and all existing runtime guards. Only a successful scalar Long result
arms one otherwise-unused bit in the entry instruction's inline-cache side
table; subsequent executions reconstruct the normalized collection program
from compiler-proven layout metadata.

The four map/filter order and staged/non-staged layouts remain separate,
fixed compiler admissions. Staged filter/map sites carry one explicit metadata
bit; the previously JSON-specific order and staged bits are generalized and
shared with the ordinary pipeline decoder. The global function cache pointer
and method class guard are unchanged. Runtime destination, callback identity,
purity, arity, source, carry, member-type and checked-arithmetic guards remain
authoritative on every execution. A site that has never completed fusion is
not armed, while a later mismatch at an armed site still replays untouched
canonical bytecode.

Alternating same-host A/B runs compare exact commit `0817026` with this source
under identical release builds. Each workload uses 103 measured pairs after
five warmups; x86-64 processes are pinned to CPU 2. Values are medians of the
PHP-internal timed region:

| Workload | ARM64 before | ARM64 metadata cache | Delta | x86-64 before | x86-64 metadata cache | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Repeated small map/filter/reduce | 22.724 ms | 19.528 ms | -14.06% | 29.500 ms | 24.739 ms | -16.14% |
| Nested map/filter/reduce control | 4.729 ms | 4.133 ms | -12.60% | 5.761 ms | 5.102 ms | -11.43% |
| Dead-staged map/filter/reduce control | 4.833 ms | 4.187 ms | -13.37% | 5.769 ms | 5.141 ms | -10.90% |
| Filter/map/reduce control | 4.103 ms | 3.260 ms | -20.54% | 4.985 ms | 3.751 ms | -24.75% |
| Repeated JSON-sink control | 27.119 ms | 26.255 ms | -3.19% | 32.552 ms | 31.231 ms | -4.06% |

All checksums remain exact. The permanent repeated-small benchmark performs
100,000 calls over a six-member packed array. Existing regression coverage
already exercises a successful Long call followed by a Double fallback at the
same call site, plus overflow, impure ordering, escaping destinations and all
four admitted layouts. Formatting and all-target compilation pass; both hosts
pass all 48 callback-array tests under default and no-default features. The
JIT-prototype and all-feature library matrices remain green at 218/222 tests on
ARM64 and 243/247 tests on x86-64.

The next callback checkpoint should profile repeated callback identity and
scalar-plan resolution. Any cache there must be request-stable, validate the
actual function targets and preserve late definition or replacement behavior;
the bytecode and runtime type guards remain the fallback contract.

### Shared callback-target cache checkpoint

The repeated callback identity lookup identified above is removed
(2026-08-07). A five-second ARM64 sample of the six-member repeated pipeline
placed roughly one third of 4,233 samples in `find_function`, SipHash and name
comparison, while the complete scalar member evaluator accounted for about
31%. Each fused call was hashing the same three compiler-proven callback
literals even though the canonical collection builtins already own a
monomorphic String-callback cache at their `DoFcall` instructions.

Pipeline spans now retain the map and filter `DoFcall` positions in addition
to the existing reduce completion position. Fused preparation resolves all
three literal callbacks through those ordinary cache entries. The repeated
path compares the retained String identity and reads the stable function
pointer; first resolution and a defensive mismatched-key path stay in a
separate non-inlined helper. Scalar-plan support, public arity and the actual
plan pointer are still validated on every fused call.

This reuses rather than duplicates ownership and fallback behavior. The cache
retains and releases its String key through the existing `OpArray` lifecycle,
an unresolved name is not cached, and the function table rejects replacement
of an existing definition. If a later source/member/overflow guard fails, the
canonical `array_map`, `array_filter` or `array_reduce` call consumes the same
already-warmed callback cache.

Alternating same-host A/B runs compare exact commit `8ffa48b` with this source
under identical release builds. Each workload uses 103 measured pairs after
five warmups; x86-64 processes are pinned to CPU 2. Values are medians of the
PHP-internal timed region:

| Workload | ARM64 before | ARM64 target cache | Delta | x86-64 before | x86-64 target cache | Delta |
|---|---:|---:|---:|---:|---:|---:|
| Repeated small map/filter/reduce | 19.771 ms | 15.902 ms | -19.57% | 24.760 ms | 20.000 ms | -19.22% |
| Nested map/filter/reduce control | 4.109 ms | 4.106 ms | -0.07% | 5.122 ms | 5.111 ms | -0.20% |
| Dead-staged map/filter/reduce control | 4.043 ms | 4.032 ms | -0.27% | 5.177 ms | 5.149 ms | -0.53% |
| Filter/map/reduce control | 3.217 ms | 3.225 ms | +0.25% | 3.759 ms | 3.763 ms | +0.10% |
| Repeated JSON-sink control | 26.225 ms | 23.027 ms | -12.19% | 31.312 ms | 27.069 ms | -13.55% |

Every checksum remains exact; the one-shot filter/map movements are neutral.
Formatting and all-target compilation pass. Both hosts pass all 48
callback-array tests under default and no-default features, including a warmed
Long site that later replays Double input canonically. The JIT-prototype and
all-feature library matrices remain green at 218/222 tests on ARM64 and
243/247 tests on x86-64.

The next decision should come from a fresh profile with callback hashes
removed. Likely remaining costs are the ordinary caller loop/frame boundary,
per-call scalar-plan validation and the actual tiny member evaluator; none
should be cached or fused further without measured share and a general
invalidation contract.

### Frame-free pure Long `array_walk` checkpoint

The shared scalar callback ABI now covers the last explicitly planned core
collection primitive, `array_walk` (2026-08-07). Its mutation contract makes a
general fusion unsafe, so the fast path is deliberately narrower: a packed
array of non-reference Long values, implicit integer keys and one exact pure
two-argument by-value user callback without receiver or captures.

The handler resolves the callable through the ordinary `DoFcall` cache and
prepares one `ScalarLongCallback` before iteration. It passes raw `(value,
key)` pairs directly from the packed backing slice, discards the unobservable
callback return and bulk-records completed calls only after the entire walk
succeeds. The helper is kept out of line so the existing by-reference mutation
loop retains its code layout.

Any reference, non-Long member, non-packed key, arity mismatch, receiver,
capture, impure body or checked-arithmetic failure leaves the original array
untouched and enters the existing canonical snapshot/frame path. Because the
speculative callbacks are proven pure and their counters are not published
before success, replay cannot duplicate a PHP-visible effect. By-reference
callbacks, partial mutation behavior and general callable forms are unchanged.

Alternating same-host A/B runs compare exact commit `b886877` with this source
under identical release builds. Each workload uses 103 measured pairs after
five warmups; x86-64 processes are pinned to CPU 2. Values are medians of the
PHP-internal timed region:

| Workload | ARM64 before | ARM64 scalar walk | Delta | x86-64 before | x86-64 scalar walk | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 500,000-member pure Long `array_walk` | 16.081 ms | 2.262 ms | -85.93% | 27.924 ms | 2.194 ms | -92.14% |
| 100,000-member by-reference mutation control | 4.490 ms | 4.501 ms | +0.25% | 8.605 ms | 8.592 ms | -0.16% |

Checksums remain exact and the mutating control is neutral. A permanent test
warms one call site with Long values, replays Double values canonically and
checks impure callback order; the existing by-reference method test remains
green. Both hosts pass all 55 callable tests under default and no-default
features. The JIT-prototype and all-feature library matrices remain green at
218/222 tests on ARM64 and 243/247 tests on x86-64; formatting and all-target
compilation also pass.

Remaining callback-taking standard functions should now be selected from
corpus frequency and semantic headroom. `usort` is mutation- and
comparison-order-sensitive, while regex callbacks have String/match-array
ABIs; neither should inherit this scalar walk path without its own measured,
exact fallback contract.

### Proof-bounded scalar `usort` checkpoint

The next implemented callback-taking array primitive, `usort`, now reuses the
shared pure Long callback ABI (2026-08-07). The general eligible case keeps the
existing insertion-sort comparison order but evaluates exact two-argument
Long callbacks without PHP frames. References, non-Long members, receivers,
captures, unsupported signatures and impure bodies continue through the
canonical callback loop.

One narrower compiler proof also removes the algorithmic ceiling. A scalar
plan consisting exactly of `return $left - $right` or its reversed form is a
total ascending or descending ordering for guarded Long inputs. Its sign is
unchanged when canonical PHP widens an overflowing subtraction to Double, so
the handler can use Rust's stable O(n log n) slice sort without executing the
subtraction. Arbitrary scalar expressions do not receive this substitution.

The input values are cloned before resolution and the source PHP array is not
replaced until sorting completes. The fast path first validates every member,
then bulk-records only the comparisons completed by the accepted algorithm.
If a non-ordering scalar callback encounters checked-arithmetic failure after
moving clone members, the handler reloads a fresh snapshot from the untouched
array and starts the canonical insertion path. No failed speculation can
publish a reordered array, callback counter or duplicated PHP-visible effect.

Alternating same-host A/B runs compare exact commit `8961b62` with this source
under identical release builds. Each target workload uses 103 measured pairs
after five warmups. The control uses 303 pairs after ten warmups; x86-64
processes are pinned to CPU 2. Values are medians of the PHP-internal timed
region:

| Workload | ARM64 before | ARM64 scalar sort | Delta | x86-64 before | x86-64 scalar sort | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 4,096-member permuted exact Long comparator | 79.438 ms | 0.108 ms | -99.86% | 137.476 ms | 0.148 ms | -99.89% |
| 500-member impure mutation control | 11.289 ms | 11.051 ms | -2.11% | 13.865 ms | 13.505 ms | -2.59% |

The candidate also beats native PHP on the permuted workload: 1.016 ms to
0.108 ms on ARM64 (-89.37%, 9.4x) and 1.015 ms to 0.148 ms on x86-64
(-85.44%, 6.9x). Checksums remain exact.

A permanent test covers ascending and descending exact plans, warmed Double
fallback, overflowing Long subtraction replay and the original observable
comparison order for an impure callback. Both hosts pass all 56 callable tests
under default and no-default features. The JIT-prototype and all-feature
library matrices remain green at 218/222 tests on ARM64 and 243/247 tests on
x86-64; formatting and all-target compilation also pass.

### Target-neutral Long three-way-compare checkpoint

The common modern comparator body `$left <=> $right` now reaches the same
proof-bounded sort path without a `usort`-specific opcode detector
(2026-08-07). `ScalarLongOpKind::Compare` represents the operation in the
shared function IR, and the scalar evaluator returns exact `-1`, `0` or `1`
from guarded raw Long inputs. The compiler admits `Spaceship` through the
function-plan builder; Double and other runtime types still execute the
canonical opcode.

The new variant is appended after every established arithmetic kind so their
discriminants and hot code layout remain unchanged. An initial mid-enum
placement produced a repeatable 2.09% ARM64 regression in the impure `usort`
control despite identical executed source. Moving it to the end restored the
1003-pair result to +0.48%. This layout rule is now documented at the enum.

An exact one-operation Compare plan over public inputs zero and one (or the
reverse) is exposed by the existing callback object as ascending (or
descending) total ordering. The stable O(n log n) Long sort can therefore use
it directly. More general scalar functions evaluate Compare in the shared Rust
executor. Straight-loop/native plan validators on ARM64 and x86-64 reject it
explicitly for now, and range analysis still assigns its exact `[-1, 1]`
interval. This preserves one target-neutral semantic representation without
claiming an unimplemented native lowering.

Alternating same-host A/B runs compare exact commit `ceb45c6` with this source.
The target and subtraction control use 103 measured pairs after five warmups;
the impure control uses 1003 pairs after twenty warmups. x86-64 is pinned to
CPU 2:

| Workload | ARM64 before | ARM64 Compare IR | Delta | x86-64 before | x86-64 Compare IR | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 4,096-member permuted `<=>` comparator | 80.116 ms | 0.106 ms | -99.87% | 134.685 ms | 0.149 ms | -99.89% |
| Existing subtraction-order fast path | 0.108 ms | 0.107 ms | -0.88% | 0.144 ms | 0.144 ms | -0.50% |
| 500-member impure callback control | 11.007 ms | 11.060 ms | +0.48% | 13.584 ms | 13.532 ms | -0.38% |

All checksums remain exact. Compiler tests assert that a spaceship function
owns a one-operation Compare plan; direct execution covers `-1/0/1` and Double
fallback; callable tests exercise the `usort` integration. Both hosts pass 56
callable and 30 function E2E tests under default and no-default features plus
all 70 hot-tier tests. ARM64 passes all 100 native prototype tests and the
218/222 library matrices; x86-64 passes 32 native prototype tests and the
243/247 matrices. Formatting and all-feature/all-target compilation pass on
both architectures.

Native Compare lowering is not the next priority unless a profile finds
multi-operation scalar functions spending material time in the Rust adapter.
For callback standard-library coverage, the next distinct ABI is regex
replacement: String subjects plus a match array and String callback result.

### Linear repeated-regex offset checkpoint

Profiling the next callback ABI showed that PHP callback frames were not the
dominant cost. A compiler-proven constant replacement callback avoided the
match array and frame but improved the 5,000-match workload by only about 2% on
both hosts, so that narrow specialization was rejected. The shared regex
iterator instead exposed an algorithmic problem: every capture converted its
character positions to UTF-8 byte offsets by summing character widths from the
start of the subject. Repeated matches therefore made offset calculation
quadratic (2026-08-07).

The matcher now prepares one character-boundary map for capture-producing
operations and performs every conversion in O(1). ASCII subjects use an
identity representation and allocate no offset table; non-ASCII subjects keep
the exact byte boundary for every character. Match context holds one pointer
to the immutable input metadata instead of widening every recursive matcher
context with another slice. `captures_iter` also moves each finished capture
vector into its result rather than cloning it.

Boolean `preg_match` needs no capture contents unless the pattern contains a
numeric or named backreference. A compile-time AST property now retains groups
only for those patterns. Ordinary capturing parentheses with no `$matches`
argument use an empty capture store, avoiding both the boundary map and capture
maintenance while preserving backreference semantics.

Alternating same-host A/B runs compare exact commit `fbb064d` with this source.
The repeated callback target uses 103 measured pairs after five warmups; the
controls use 503 pairs after twenty warmups. x86-64 processes are pinned to CPU
2. Values are medians of the PHP-internal timed region:

| Workload | ARM64 before | ARM64 linear offsets | Delta | x86-64 before | x86-64 linear offsets | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 5,000-match `preg_replace_callback` | 90.128 ms | 2.515 ms | -97.21% | 63.970 ms | 2.259 ms | -96.47% |
| 20,000 cached `preg_match` calls without groups | 9.544 ms | 9.508 ms | -0.38% | 8.060 ms | 8.058 ms | -0.02% |
| 20,000 failed matches with an unused group | 10.034 ms | 9.496 ms | -5.36% | 8.001 ms | 7.917 ms | -1.05% |

Checksums remain exact. UTF-8 unit tests cover the identity and mapped offset
representations, named captures, repeated matches and zero-width advancement.
End-to-end tests cover the same contracts through `preg_match_all` and
`preg_replace_callback`, including named groups and zero-width UTF-8 matches.
Both hosts pass 154 default-feature and 147 no-default-feature library tests,
31 regex E2E tests under both configurations and all 70 hot-tier tests. The
all-feature library matrices pass 227 tests on ARM64 and 252 on x86-64;
formatting and all-feature/all-target compilation also pass on both hosts.
Native PHP 8.5 remains about 9x faster on the ARM64 repeated-callback workload
(2.535 ms RPHP versus 0.282 ms PHP), so the next regex decision should come
from a fresh profile of the now-linear matcher and output materialization, not
from restoring the rejected callback-only special case.

### Forward regex-callback output checkpoint

The first profile after linearizing capture offsets placed 53% of sampled time
in `String::replace_range`, not in matching or PHP callback execution
(2026-08-07). `preg_replace_callback` collected every replacement and then
applied them in reverse to a clone of the subject. Each call moved the
already-built suffix again, making output construction quadratic in the number
and position of matches.

The handler still freezes the complete ordered capture set before invoking any
callback, resolves the callback once and preserves exact callback/exception
order. It now appends each untouched subject span and callback result to one
forward output String, then appends the final tail. No partially assembled
String is published if a callback raises an exception. Variable-length, empty,
named, zero-width and UTF-8 replacements retain their canonical behavior.

Shrinking the standard-library handler changed x86 code placement enough to
produce an initial +10.46% result in a `preg_match` control even though that
function's machine code was byte-for-byte the same size. `Regex::is_match` has
only one production caller, so it now carries an ordinary inline hint; the
compiler folds the wrapper into `fn_preg_match` and removes that separate
layout dependency. The final long control is -1.17%, while the group-free
control is +0.99%.

Alternating same-host A/B runs compare exact commit `5542f51` with this source.
The 5,000-match target uses 103 measured pairs after five warmups and the
20,000-match scaling target uses 51 pairs. Controls use 503-2,003 pairs after
twenty or thirty warmups; x86-64 remains pinned to CPU 2:

| Workload | ARM64 linear offsets | ARM64 forward output | Delta | x86-64 linear offsets | x86-64 forward output | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 5,000-match `preg_replace_callback` | 2.410 ms | 1.050 ms | -56.43% | 2.265 ms | 1.045 ms | -53.87% |
| 20,000-match scaling target | 33.276 ms | 4.443 ms | -86.65% | 22.549 ms | 4.183 ms | -81.45% |
| 20,000 cached `preg_match` calls without groups | 9.477 ms | 9.435 ms | -0.44% | 8.015 ms | 8.094 ms | +0.99% |
| 20,000 failed matches with an unused group | 9.270 ms | 9.238 ms | -0.35% | 7.861 ms | 7.769 ms | -1.17% |

Against the original `fbb064d` regex baseline, the combined 5,000-match result
is 88.144 to 1.079 ms on ARM64 (-98.78%) and 63.804 to 1.038 ms on x86-64
(-98.37%). The 20,000 result scales at almost exactly four times the final
5,000 cost instead of exposing either former quadratic component. Native PHP
8.5 is still about 4.0x faster on ARM64 (1.107 ms RPHP versus 0.279 ms PHP), so
the next profile should distinguish callback/match-array allocation from the
remaining backtracking matcher cost.

Both hosts pass all 33 regex E2E tests under default and no-default features,
154/147 corresponding library tests and all 70 hot-tier tests. The all-feature
library matrices remain green at 227 ARM64 and 252 x86-64 tests; formatting and
all-feature/all-target compilation also pass.

### Streaming regex-capture consumer checkpoint

The remaining capture and match-array profile is now addressed through one
shared lending visitor (2026-08-07). `CaptureView` borrows the current capture
slots and named-group map only for the duration of a visitor call;
`try_visit_captures` then clears and reuses the same slot vector for the next
candidate. Consumers may stop deliberately or propagate an error without
scanning later matches. The owned `captures_iter` compatibility API is now a
thin collector over this visitor rather than a second matching loop.

`preg_replace_callback` consumes each capture immediately, resolves its
callback and allocates its output lazily on the first match, and stops the
visitor as soon as the callback raises an exception. The original owned subject
and compiled regex remain fixed across callbacks, callback order is unchanged,
and no partial output is published. This removes the complete intermediate
`Vec<Captures>`, its per-position group allocations and per-match named-map
clones.

`preg_match_all` uses the same visitor. Its count-only form no longer stores any
owned capture collection; `PREG_PATTERN_ORDER` output is appended directly to
the numeric and named result arrays. The established no-match result shape is
preserved. A required start literal is now located with slice `position`
scans, skipping non-candidate spans without running the outer match loop at
every character. Besides being independently useful, this removes the x86
layout sensitivity exposed by the non-participating `preg_match` controls.

Alternating same-host A/B runs compare exact commit `c151b4b` with this source.
Targets use 103 measured pairs after five warmups; controls use 503 pairs after
twenty warmups and x86-64 remains pinned to CPU 2:

| Workload | ARM64 forward output | ARM64 streaming | Delta | x86-64 forward output | x86-64 streaming | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 5,000-match `preg_replace_callback` | 1.146 ms | 0.922 ms | -19.53% | 1.031 ms | 0.759 ms | -26.35% |
| 5,000-match `preg_match_all` with numeric/named output | 4.625 ms | 4.094 ms | -11.48% | 3.579 ms | 2.779 ms | -22.34% |
| 5,000-match count-only `preg_match_all` | 0.500 ms | 0.265 ms | -46.97% | 0.593 ms | 0.324 ms | -45.48% |
| 20,000 cached `preg_match` calls without groups | 9.517 ms | 7.722 ms | -18.86% | 8.115 ms | 6.718 ms | -17.21% |
| 20,000 failed matches with an unused group | 9.258 ms | 7.412 ms | -19.94% | 7.802 ms | 6.441 ms | -17.45% |

On the 20,000-match callback workload, observed ARM64 maximum RSS falls from
8.14 MB to 5.00 MB (-38.6%); macOS peak footprint falls from 6.31 MB to 3.16 MB
(-49.9%). A fresh combined run against the original `fbb064d` baseline measures
110.241 to 1.174 ms on ARM64 (-98.94%) and 63.718 to 0.755 ms on x86-64
(-98.81%). Native PHP 8.5 remains about 3.3x faster on ARM64 (1.150 ms RPHP
versus 0.349 ms PHP), with the current profile divided primarily between the
backtracking matcher and general PHP callback/match-array execution.

Unit tests cover named UTF-8 streaming, deliberate early stop, propagated
errors, later required-literal candidates, anchors and end-position matches.
End-to-end coverage checks no-match `PREG_PATTERN_ORDER` output and verifies
that a callback exception leaves the assignment target unchanged. Both hosts
pass 159 default-feature, 152 no-default-feature and 35 regex E2E tests under
both configurations, plus all 70 hot-tier tests. All-feature library matrices
pass 232 tests on ARM64 and 257 on x86-64; formatting and all-feature/all-target
compilation pass on both hosts.

### Required-literal streaming scan checkpoint

A fresh repeated-callback profile after capture streaming still placed 516 of
2,170 ARM64 samples at the top of the stack in `match_seq_from`. The streaming
visitor was invoking the full AST at every subject position even when regex
compilation had already proved a literal that every match must consume first.
`try_visit_captures` now caches that invariant `start_literal` before its hot
loop and uses a case-aware slice `position` scan to advance directly to viable
candidates. Patterns without a proven literal retain the canonical per-position
matcher and pay only one loop-invariant `Option` branch.

The proof remains the existing target-neutral AST analysis: literals after
zero-width anchors, boundaries and lookarounds are admissible; alternatives
must agree; optional prefixes are rejected. New tests cover later
case-insensitive candidates and prove that anchored and multiline matching keep
their previous semantics.

Alternating runs compare the exact `182a3a7` code baseline with this source.
Targets use 103 measured pairs after five warmups; the no-literal control uses
503 pairs after twenty warmups, and x86-64 remains pinned to CPU 2:

| Workload | ARM64 streaming | ARM64 literal scan | Delta | x86-64 streaming | x86-64 literal scan | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 5,000-match `preg_replace_callback` | 0.906 ms | 0.862 ms | -4.87% | 0.751 ms | 0.734 ms | -2.29% |
| 5,000-match `preg_match_all` with output | 3.711 ms | 3.560 ms | -4.07% | 2.771 ms | 2.632 ms | -5.02% |
| 5,000-match count-only `preg_match_all` | 0.257 ms | 0.218 ms | -15.21% | 0.325 ms | 0.296 ms | -9.08% |
| Count-only `[0-9]+` without a start literal | 0.359 ms | 0.361 ms | +0.53% | 0.434 ms | 0.436 ms | +0.38% |

An independent attempt to store tiny callback match arrays in the existing
inline associative tier was rejected: it improved the ARM64 callback by 12.21%
but regressed the same x86-64 target by 1.38% and full `preg_match_all` by
1.18%. Both source and remote binaries were restored before this checkpoint.

Both hosts pass 161 default-feature, 154 no-default-feature and 35 regex E2E
tests under both configurations, plus all 70 hot-tier tests. All-feature
library matrices pass 234 tests on ARM64 and 259 on x86-64; formatting and
all-feature/all-target compilation pass on both hosts.

### Capture-free linear regex matcher checkpoint

The post-literal-scan count-only profile placed 1,097 of 1,293 ARM64 samples at
the top of the stack in recursive `match_seq_from`. A bounded iterative matcher
now handles capture-free ASTs whose sequence consists only of deterministic
single atoms and at most one terminal quantifier (2026-08-07). A terminal
quantifier cannot need continuation backtracking; fixed literals, dot, anchors,
boundaries, character classes and shorthands each have exactly one outcome.
Greedy, lazy and bounded tails retain their canonical repetition semantics.

The proof rejects captures, alternation, lookarounds, backreferences, nested
quantifiers and any quantified middle node. It runs once when a streaming
consumer enters and chooses the iterative or canonical loop before scanning;
no matcher bit is added to `Regex` and no dispatch branch remains inside the
fallback loop. The planner/executor lives in `regex/linear.rs`, isolating its
monomorphized visitor loop from canonical matcher codegen. Earlier versions
that added a field or duplicated both loops in `regex.rs` produced unrelated
x86 code-layout movements and were rejected before this checkpoint.

Alternating runs compare exact `0bf8e90` binaries with the isolated-module
source. Targets use 103 measured pairs after five warmups, grouped fallback
uses 303 pairs after twenty warmups, and x86-64 remains pinned to CPU 2:

| Workload | ARM64 literal scan | ARM64 linear matcher | Delta | x86-64 literal scan | x86-64 linear matcher | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 5,000-match count-only `user[0-9]+` | 0.212 ms | 0.172 ms | -18.79% | 0.285 ms | 0.256 ms | -10.19% |
| 5,000-match count-only `[0-9]+` | 0.357 ms | 0.281 ms | -21.31% | 0.437 ms | 0.297 ms | -32.02% |
| 5,000-match `preg_replace_callback` | 0.855 ms | 0.844 ms | -1.28% | 0.725 ms | 0.684 ms | -5.63% |
| Grouped/named `preg_match_all` fallback | 3.543 ms | 3.543 ms | 0.00% | 2.635 ms | 2.625 ms | -0.37% |

The isolated follow-up profile contains no recursive `match_seq_from` sample.
Its remaining work is the iterative atom/class executor and subject-to-`char`
materialization. Both non-participating `preg_match` controls remain within
0.89% of baseline on both architectures.

Tests explicitly admit fixed-prefix/terminal-class shapes, verify greedy,
lazy and bounded results, and reject mid-sequence quantifiers, captures and
alternation. Both hosts pass 164 default-feature, 157 no-default-feature and 35
regex E2E tests under both configurations, plus all 70 hot-tier tests.
All-feature library matrices pass 237 tests on ARM64 and 262 on x86-64;
formatting and all-feature/all-target compilation pass on both hosts.

### ASCII byte linear regex checkpoint

The next profile left subject-to-`char` materialization as the avoidable cost
inside the capture-free linear matcher. An admitted ASCII subject can now use
its bytes directly: every character index is already the exact capture byte
offset, group zero is one stack slot, and no `Vec<char>` or UTF-8 offset table
is built (2026-08-08).

The byte executor remains deliberately bounded. One stack-resident plan holds
at most 32 case-sensitive ASCII prefix bytes and an optional terminal
quantifier. A second plan handles a root terminal character-class quantifier,
choosing that class shape once instead of interpreting the AST again for every
candidate byte. Case-insensitive patterns, non-ASCII subjects, longer prefixes
and all other shapes return to the unchanged character executor. Planning adds
no field to `Regex` and allocates no heap storage.

The dispatcher, byte scan, character scan and canonical backtracking scan have
separate non-inlined boundaries. This was necessary because fat LTO otherwise
moved the existing character loop enough to regress no-prefix controls by up
to 27% on ARM64. The accepted layout keeps both ordinary `preg_match` controls
inside 0.62% on ARM64 and improves them by more than 13% on x86-64. The grouped
fallback is +1.02%/+1.33%; its x86 `fn_preg_match_all` symbol remains exactly
the baseline size (`0xa52` bytes), so the residual difference is instruction
placement rather than a changed matching algorithm.

Alternating same-host A/B runs compare exact commit `2680bb0` with this source.
Targets and ordinary controls use 1,003 measured pairs after fifty warmups;
grouped fallback uses 2,003 pairs after one hundred warmups. x86-64 remains
pinned to CPU 2:

| Workload | ARM64 linear chars | ARM64 ASCII bytes | Delta | x86-64 linear chars | x86-64 ASCII bytes | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 5,000-match count-only `user[0-9]+` | 0.177 ms | 0.080 ms | -54.78% | 0.260 ms | 0.059 ms | -77.43% |
| 5,000-match count-only `[0-9]+` | 0.280 ms | 0.115 ms | -58.94% | 0.300 ms | 0.097 ms | -67.70% |
| 5,000-match `preg_replace_callback` | 0.850 ms | 0.717 ms | -15.65% | 0.687 ms | 0.513 ms | -25.42% |
| UTF-8 `uživatel[0-9]+` control | 0.322 ms | 0.320 ms | -0.59% | 0.553 ms | 0.520 ms | -5.95% |
| Grouped/named `preg_match_all` fallback | 3.527 ms | 3.563 ms | +1.02% | 2.606 ms | 2.640 ms | +1.33% |
| 20,000 cached `preg_match` calls without groups | 7.693 ms | 7.737 ms | +0.57% | 7.907 ms | 6.829 ms | -13.64% |
| 20,000 failed matches with an unused group | 7.386 ms | 7.425 ms | +0.53% | 7.636 ms | 6.547 ms | -14.27% |

Tests cover UTF-8 byte offsets, the bounded-prefix fallback and direct planner
admission for fixed prefixes and terminal classes. Both hosts pass 167
default-feature, 160 no-default-feature and 35 regex E2E tests under both
configurations, plus all 70 hot-tier tests. All-feature library matrices pass
240 tests on ARM64 and 265 on x86-64; formatting and all-feature/all-target
compilation pass on both hosts.

### Capture-free callback match-array reuse checkpoint

An ARM64 sample of the post-ASCII `preg_replace_callback` workload left 4,218
samples dominated by allocation/free work and `call_function_owned_iter`.
Capture-free callbacks now move their one-element `$matches` array into the
callback frame and read the first public argument back after execution
(2026-08-08). If that value is still the uniquely owned array, the next match
replaces element zero in place. If callback code retained, returned, replaced
or otherwise shared the value, the ordinary COW test fails and the next match
gets a fresh array. Methods remain correct because readback uses
`param_cv_index(0)`, which skips their hidden `$this` slot.

The specialized streaming consumer lives in `stdlib/regex_callback.rs`, away
from the general stdlib codegen unit. Grouped and named patterns keep the
canonical array builder and callback path. Callback resolution and output
allocation remain lazy, exceptions still discard partial output, and two E2E
tests cover both escaped arrays and callback-local mutation. The ASCII visitor
also stops clearing its single group-zero slot before immediately overwriting
it on every published match.

The code-layout audit exposed two non-local effects. `Regex::is_match` is now
forced inline so the separate callback monomorph does not pessimize ordinary
`preg_match` on x86-64. Count-only fixed-prefix ASCII `preg_match_all` uses a
small non-generic counter, avoiding a capture view and stabilizing its hot loop;
no-prefix, grouped and non-ASCII shapes retain the established visitor. A
version that kept another array owner alive across the callback was rejected
because callback mutation then forced COW and regressed x86-64 by 10.52%.
Variants that changed the shared callback helper, split capture-free and
grouped visitors, or cached per-match classification were also rejected after
moving unrelated x86 controls by 3--18%.

Alternating same-host A/B runs compare exact commit `253c315` with this source.
Rows use 1,003 measured pairs after fifty warmups and x86-64 is pinned to CPU 2;
the final x86 UTF-8 holdout uses 3,003 pairs after one hundred warmups:

| Workload | ARM64 baseline | ARM64 reuse | Delta | x86-64 baseline | x86-64 reuse | Delta |
|---|---:|---:|---:|---:|---:|---:|
| 5,000-match count-only `user[0-9]+` | 0.076611 ms | 0.075314 ms | -1.69% | 0.059866 ms | 0.060095 ms | +0.38% |
| Count-only `[0-9]+` without a start literal | 0.113888 ms | 0.113332 ms | -0.49% | 0.098837 ms | 0.093166 ms | -5.74% |
| UTF-8 count-only `uživatel[0-9]+` control | 0.309836 ms | 0.311245 ms | +0.45% | 0.516906 ms | 0.525540 ms | +1.67% |
| 5,000-match capture-free callback | 0.710332 ms | 0.488812 ms | -31.19% | 0.520580 ms | 0.374073 ms | -28.14% |
| Callback that mutates `$matches[0]` | 0.772371 ms | 0.571248 ms | -26.04% | 0.568192 ms | 0.442155 ms | -22.18% |
| Callback that retains the prior `$matches` | 1.528322 ms | 1.512783 ms | -1.02% | 1.698780 ms | 1.617939 ms | -4.76% |
| Grouped callback fallback | 4.072361 ms | 4.000808 ms | -1.76% | 2.321924 ms | 2.309887 ms | -0.52% |
| Grouped/named `preg_match_all` with output | 3.550468 ms | 3.510766 ms | -1.12% | 2.665763 ms | 2.635668 ms | -1.13% |
| 20,000 cached `preg_match` calls without groups | 7.681123 ms | 7.726817 ms | +0.59% | 6.902440 ms | 6.319882 ms | -8.44% |
| 20,000 failed matches with an unused group | 7.433335 ms | 7.482945 ms | +0.67% | 6.655008 ms | 6.032375 ms | -9.36% |

The x86 UTF-8 holdout is a measured 0.009 ms absolute movement; the UTF-8
matcher itself is unchanged and all participating and grouped controls are
neutral or faster. Both hosts pass 168 default-feature, 161
no-default-feature and 37 regex E2E tests under both configurations, plus all
70 hot-tier tests. All-feature library matrices pass 241 tests on ARM64 and
266 on x86-64; formatting and all-feature/all-target compilation pass on both
hosts.

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

## Phase 4.25: codegen-stability and maintainability refactor

Before another runtime optimization family, stabilize the boundaries exposed
by the regex callback work. Small source edits repeatedly changed unrelated
x86-64 `preg_match` and count-only results by 3--20% even when their algorithms
and symbol sizes were unchanged. That makes successful local optimization too
dependent on linker placement and makes the 6,000-plus-line stdlib unit harder
to evolve safely.

This is a behavior-preserving phase, not a license to mix cleanup with new fast
paths. Each checkpoint starts from exact ARM64 and x86-64 binaries, moves one
responsibility, passes the full test matrix, and runs the common paired regex
gate in `benches/compare_regex_stability.sh`. A refactor is accepted only when
participating workloads do not slow materially and unrelated controls stay
within one percent, allowing a narrowly documented exception only after a
longer paired run proves a small absolute movement.

The intended sequence is:

1. Move public regex stdlib handlers beside their domain implementations while
   retaining the canonical frame-argument and return macros.
2. Give single-match, match-all/count, replacement and callback consumers
   separate non-inlined ownership boundaries so their generic visitors cannot
   silently merge into one code-layout dependency.
3. Replace ad-hoc callback readback variants with one explicit owned-frame
   result contract, then keep COW policy in the consuming domain rather than
   the baseline executor.
4. Split remaining high-churn domains out of `stdlib.rs` only when a dependency
   audit shows that helpers and unsafe frame access are not being duplicated.
5. Record symbol sizes and paired holdouts with every structural commit; never
   add padding or benchmark-specific branches merely to recover a favorable
   address.

Exit this phase when the regex handler family has clear module ownership,
future callback-only edits do not recompile or materially move single-match and
count consumers, `stdlib.rs` is smaller without duplicated ABI helpers, and
both architectures pass the established correctness and performance gates.

The first audit checkpoint adds the shared gate without changing runtime code.
Moving only the public callback handler beside its existing implementation
passed correctness but moved the ARM64 no-literal count control by +9.55%; an
explicit 64-byte loop alignment still measured +8.91%. Both runtime edits were
reverted. This establishes the phase rule in practice: source organization is
not accepted as a refactor when the generated program materially regresses.

The second audit checkpoint (2026-08-08) starts with test ownership before
touching another production boundary. A textual extraction of the complete
regex handler family was rejected after the 1,003-pair ARM64 gate measured the
mutating and retaining callback controls at +1.42% and +1.44%. Keeping only
`preg_match` and `preg_replace` in the extracted file was also rejected when
the longer mutating-callback result moved by +3.20%. Neither code-layout
outcome was committed.

The accepted checkpoint moves 3,770 lines of in-source tests into dedicated
files without changing their module names. `src/jit/x86_64.rs` falls from
6,277 to 4,141 lines, `src/regex.rs` from 2,516 to 1,913,
`src/parser.rs` from 3,268 to 3,167, and `src/value/mod.rs` from 4,925 to
3,995. The ARM64 Mach-O `__text` remains byte-identical at SHA-256
`8ac94a60a27bbe541cab022377a4635edb4c17038c3fcf5479545caa31161bc0`;
the x86-64 ELF `.text` remains byte-identical at SHA-256
`bdad28a36aa0b5efff5fefc1c13c494f8924a3b8020472e7288b5db338dda046`.
The paired x86 gate stays between -0.53% and +0.66%. One 303-pair ARM64
mutating-callback sample reported +2.31%, but the baseline and candidate have
identical executable bytes and symbol addresses, so this is recorded as
measurement noise rather than a code change.

Both hosts pass 168 default-feature and 161 no-default-feature library tests,
37 regex E2E tests in both configurations, and 70 hot-tier tests. The
all-feature library matrix passes 241 tests on ARM64 and 266 on x86-64,
including the relocated target-specific x86 tests; formatting and all-target
all-feature compilation also pass on both hosts. The remaining largest files
are now the production-heavy `stdlib.rs`, `vm/execute.rs`, `jit/aarch64.rs`,
and compiler units. Their next splits require an explicit dependency boundary
and the same two-architecture code/performance gate rather than another broad
mechanical move.

The first accepted production split (2026-08-08) separates the parser without
changing its public paths. `src/parser.rs` is now a 19-line composition root;
the AST (405 lines), statement parser (678), expression parser (764),
declaration parser (739), and shared parser helpers (570) live in focused
included files. The associated methods are grouped into four `impl Parser`
blocks, while callers continue to use the exact `crate::parser::*` API.

Source-span changes alter executable hashes even though section sizes remain
constant. ARM64 keeps the measured hot regex symbols at their previous
addresses; x86-64 moves them uniformly by 64 bytes, so both targets were gated
rather than treating the move as cosmetic. The 1,003-pair ARM64 result ranges
from -1.98% to +0.91%; the 303-pair pinned x86-64 result ranges from -1.87% to
+0.79%. The ARM64 retention holdout is +0.91% (0.014 ms), and all other
positive ARM64 movements are at most +0.48%. Default, no-default and
all-feature library matrices pass on both hosts, as do every all-feature
integration test and all-target compilation. This establishes the parser
composition-root pattern as the preferred next boundary shape: domain files
remain independently readable while exported type and method ownership stays
stable.

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

## Phase 6: optional numerical computing and accelerator platform

After production-oriented compatibility is broad enough, use the proven typed
IR, collection fusion, CPU JIT backends and coroutine substrate as the base for
an opt-in numerical computing layer. This phase is not a prerequisite for PHP
compatibility and must not slow or change ordinary PHP arrays. Its purpose is
to make typed numerical workloads expressible in PHP while allowing RPHP to
select an efficient scalar, SIMD, parallel CPU or later GPU implementation.

The first milestone is a target-neutral contiguous-buffer contract with
explicit element types such as `Int32`, `Int64`, `Float32` and `Float64`.
`TypedBuffer`/`NDArray` values should describe dtype, shape, strides, ownership,
views and device placement without expanding every member into a general PHP
`Value`. Slicing and reshape should remain zero-copy when their layout permits
it. Aliasing, COW, references, mutation and lifetime rules must be explicit so
that a failed proof returns to a correct generic implementation rather than
silently changing PHP-visible behavior.

Build the CPU path before a GPU backend. Element-wise operations, broadcasting,
map/filter-style transforms and reductions should lower to one data-parallel
program and reuse the existing typed scalar expression IR instead of creating
one runtime call and one intermediate array per stage. Establish the scalar
ARM64/x86 implementation first, then add measured NEON and AVX lowering,
parallel reductions and memory-bandwidth-aware scheduling. Fusion is admitted
only when intermediate arrays, callback effects and mutation are unobservable.

Once this substrate is stable, keep higher numerical algorithms in ordinary
typed RPHP packages wherever possible. A first statistics package should prove
that mean, numerically stable variance, covariance, correlation, histograms,
quantiles, moving aggregates and basic regression can be written once in PHP
while executing through fused native loops. The runtime should provide the
small set of reusable primitives--typed iteration, reductions, selection,
sorting, dot products, matrix kernels, random-number state and required special
functions--rather than embedding a SciPy-sized API in the language core.

Only after the fused CPU pipeline is competitive should the same
data-parallel IR gain an optional GPU lowering. Start with explicit
`GpuBuffer`/kernel or annotated typed-function APIs; do not attempt to compile
arbitrary dynamic PHP for a GPU. Evaluate a portable WGSL/SPIR-V route and
native platform backends such as Metal separately, keeping backend choice out
of PHP semantics. Kernel compilation and pipeline caches, device loss, buffer
residency, synchronization and CPU fallback are runtime responsibilities.
End-to-end measurements must include compilation, upload, download and
synchronization costs; a GPU win that excludes transfers is not an admission
result.

The later server integration may combine this backend with Phase 4.5
coroutines. A request awaiting a GPU result should release its logical task,
and an opt-in scheduler may batch compatible kernels across requests while
preserving per-request results, cancellation and error delivery. Persistent
buffers and compiled kernels should be reused by a long-lived runtime; small
or irregular workloads remain on the CPU.

Numerical correctness is part of the gate, not a follow-up. Differential tests
must cover NaN and infinity, signed zero, integer overflow, empty dimensions,
strided and overlapping views, broadcasting, reduction axes, random-state
reproducibility and reference algorithms. Parallel and GPU reductions must
either document an explicit floating-point reproducibility contract or offer a
deterministic mode. Performance corpora must contain both fused wins and
holdouts where scalar CPU, canonical PHP or an established native library is
the correct choice.

Proceed to the GPU milestone only when the typed-buffer API is stable, the CPU
pipeline removes intermediate materialization generally rather than through
benchmark-specific detectors, and profiling shows sufficiently large
data-parallel workloads whose gains survive all transfer and scheduling costs.

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
