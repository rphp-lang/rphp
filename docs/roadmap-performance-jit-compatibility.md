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

The compiler follow-up applies the same gate more narrowly. A proposed
seven-file split of `compiler/compile.rs` was rejected after its isolated
1,003-pair x86-64 run moved the grouped callback by +2.23% and fixed-prefix
count by +1.12%. A smaller statements-plus-expressions split was also rejected
when the mutating callback remained +1.81%. The favorable 3.9--5.0% movements
in unrelated `preg_match` controls did not override either regression.

The accepted boundary moves only `Compiler::compile_stmt` into a dedicated
1,436-line file and leaves expression compilation, analysis, constants,
parameters, and emission helpers in their original lexical order.
`compiler/compile.rs` falls from 4,472 to 3,041 lines while all public paths
remain unchanged. On x86-64 the tracked regex handlers retain exactly the
same addresses and symbol sizes as the parser-only baseline, and ELF `.text`
remains 2,424,113 bytes. The 303-pair ARM64 gate ranges from -1.03% to +0.37%;
the isolated 1,003-pair x86-64 gate ranges from -3.36% to +0.99%, with
fixed-prefix count at +0.58% and the mutating callback at +0.99%.

Both hosts again pass no-default and all-feature library matrices, every
all-feature integration test, formatting, and all-target compilation. This
checkpoint also identifies expression compilation as a code-layout-sensitive
boundary: it stays in place until the affected callback code can be isolated
without moving participating runtime paths.

A subsequent `OpArray` extraction from `compiler/mod.rs` was likewise rejected
despite a neutral ARM64 gate. The pinned 303-pair x86-64 control moved retained
callbacks by +6.89%, grouped callbacks by +3.99%, fixed-prefix count by +1.34%,
and UTF-8 count by +1.52%. The experiment was fully reverted, including the
server binary, before any further cleanup was admitted.

The safe follow-up instead removes a test-maintenance bottleneck without
changing the release crate. The 4,519-line ARM64 prototype integration test is
now a 52-line shared harness plus six responsibility files covering Double
runtime paths, scalar code generation, guarded runtime paths, scalar calls,
straight-loop code generation, and straight-loop runtime behavior. Individual
files stay between 578 and 874 lines, all 100 prototype tests retain their
original root-module names, and the complete all-feature integration matrix
passes.

The same test-only boundary is applied to the 2,883-line quick-loop end-to-end
suite. Its root is now an 11-line harness and seven responsibility files cover
basic loops, scalar/object calls, conditional kernels, array reads, array
mutation, hash kernels, and foreach/Double fallback behavior. The extracted
files range from 188 to 708 lines and all 118 existing tests pass without a
release-crate change.

Type-hint coverage is split at the same test-only boundary. The former
2,404-line end-to-end file is now a 34-line harness plus six files for
parameter hints, return hints, runtime guards, scalar propagation, String
plans, and object plans. The responsibility files range from 193 to 633 lines;
the target passes 106 tests without default features and 108 with all features.

The internal quick-plan unit suite is also test-only modularized. Its
2,098-line wrapper becomes a 20-line module index plus five files for Double
plans, plan selection, scalar loops, array/hash loops, and dynamic control
flow. The files range from 299 to 521 lines and retain all 58 tests. ARM64
library matrices pass 161 tests without default features and 241 with all
features; x86-64 passes 161 and 266 respectively.

The x86-64 backend unit suite follows the same test-only structure. Its
2,120-line file becomes an 8-line index plus six files for shared context,
scalar code generation, linear polling, structured residency, structured
arithmetic, and chunk/side-exit contracts. Files range from 195 to 542 lines,
all 51 target-specific tests remain present, and the native x86-64 library
matrices pass 161/266 tests together with all-target compilation.

The 1,701-line hot-tier end-to-end suite is reduced to a 22-line harness and
four files for promotion/scalar plans, bailout/tier transitions, property
execution, and method execution. The extracted files range from 397 to 447
lines and all 70 tests pass with both no-default and all-feature builds; the
release crate is unchanged.

The 1,602-line interface/visibility end-to-end suite is now a 9-line harness
plus five files for interface basics and `Throwable`, visibility and
instantiation, contract/private-scope regressions, parameter compatibility,
and return compatibility. Files range from 232 to 428 lines and all 77 tests
pass in both feature configurations without changing release code.

The Linux x86-64 prototype integration target is likewise split without
touching release code. Its 1,362-line file becomes a 22-line harness and four
files for Double calls, Double composition/fallback, mixed corpus/runtime
paths, and scalar calls/guards. Files range from 285 to 407 lines; all 32
target-specific tests pass natively with all features, while the no-default
configuration correctly selects zero prototype tests.

The low-level VM integration suite is split at the same maintenance boundary.
Its 1,054-line file becomes a 37-line shared harness plus three files for
basic/CV instructions, function calls, and recursion/interrupt/result
contracts. Files range from 315 to 374 lines and all 14 tests pass in both
feature configurations.

The next production-boundary experiment (2026-08-08) tested a narrow extraction
of the 145-line Generator method implementation from `stdlib.rs`. The function
bodies, public paths, and lexical include position remained unchanged; all
Generator, no-default, all-feature, integration, and all-target test matrices
passed on ARM64 and x86-64. ARM64 was neutral across the 1,003-pair regex gate
(-0.84% to +0.73%) and five representative VM workloads (-0.34% to +0.09%).
The candidate was nevertheless rejected by the pinned x86-64 gate. Although
the ELF total size stayed byte-for-byte constant, 64 bytes moved from BSS to
text; a focused 3,003-pair run then measured retained callbacks at +1.65% and
UTF-8 count at +1.29%. The source and release binary were restored exactly on
both hosts. This confirms that even cold-looking `stdlib.rs` extraction is a
code-layout change, not test-only maintenance, and must wait for a boundary
that isolates participating runtime code rather than merely moving source.

The safe follow-up splits the 891-line array integration target without
touching the release crate. `tests/e2e_arrays.rs` is now a 9-line harness plus
three responsibility files for basic operations (414 lines), copy-on-write
isolation (275), and mutation/hot-path regressions (196). All 72 tests retain
their original root target and pass with both no-default and all-feature builds
on ARM64 and x86-64; the complete all-feature integration matrix and all-target
compilation pass on both hosts. The ARM64 Mach-O remains byte-identical at
SHA-256 `695b7e472ce1913a59e2fdfed105f5626c4d41ae7deea86027eac000de8dab7d`,
and the x86-64 ELF remains byte-identical at SHA-256
`5359c1d345ecd9878c5e6bc0d358a7d97f294b80dc2b93be745a3e6cc533dcb8`.

The callback-array integration target follows the same safe boundary. Its
889-line source becomes an 11-line harness plus four files for map basics (104
lines), filter and optimized pipelines (453), combined usage and callback
errors (229), and Error/Exception hierarchy regressions (96). All 48 tests
retain their original target names and pass with no-default and all-feature
builds on both architectures; complete integration and all-target checks pass
as well. The release Mach-O and ELF retain the exact hashes recorded above.

Callable coverage is modularized next. The 873-line integration target is now
a 31-line harness that keeps the shared compiler/opcode helpers, plus four
files for string callbacks and direct lowering (218 lines), callback-array
shapes (323), callable values and closures (94), and method visibility and
inheritance (212). All 56 tests preserve their original target names and pass
in both feature configurations on ARM64 and x86-64. Complete integration and
all-target checks pass on both hosts, while the release Mach-O and ELF remain
bit-identical to the preceding test-only checkpoints.

Exception-flow coverage follows the same pattern. The 710-line try/catch
target becomes an 11-line harness plus four files for basic flow (158 lines),
finally control flow (220), Error/Exception hierarchy (124), and throw
validation (201). All 35 tests retain their original target names and pass in
both feature configurations on ARM64 and x86-64. Complete integration and
all-target checks pass on both hosts, and both release binaries remain
bit-identical to the preceding test-only checkpoints.

Named-argument coverage is grouped into larger responsibilities rather than
its former sequence of tiny regression sections. The 707-line target is now an
11-line harness plus files for basic calls (222 lines), references and internal
functions (127), duplicate/keyword/variadic handling (205), and variadic errors
with recovery (146). All 45 tests retain their original target names and pass
in both feature configurations on ARM64 and x86-64. Complete integration and
all-target checks pass on both hosts, with bit-identical release binaries.

Range-proof unit coverage is also moved behind a safe test-only boundary.
`jit/straight_range.rs` drops from 1,724 to 877 production lines, while its 831
test lines move to `jit/straight_range_tests.rs` under the same module name.
Thirteen focused ARM64 tests and nine x86-64 tests pass; the complete host
library/all-target gates and x86 integration matrix remain green. The release
executables remain bit-identical at ARM64 SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`
and metadata-normalized x86-64 SHA-256
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.

A proposed 863-line production extraction from `stdlib.rs` did not pass the
same admission gate. Although code sizes and text-symbol addresses stayed
identical, pinned x86-64 callback workloads regressed by 6.95 to 7.78 percent
after constant layout changed; ARM64 had remained within -0.78 to +0.38
percent. The extraction was fully reverted. This keeps two-host measurement,
not source equivalence, as the acceptance condition for production refactors.

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

### Internal context prototype checkpoint (2026-08-09)

Milestone one is complete as the executable integration prototype in
`tests/e2e_coroutine_context.rs`. It proves pinned two-context ownership, an
executor-lifetime-bound driver, O(1) exchange of both VM stacks and all
frame-bound transient state, deterministic isolation, invalid-transition
rejection and result completion without adding a PHP API. The permanent
one-million-switch release test measures 13.78 ns per hand-off on ARM64 and
11.44 ns on pinned x86-64.

The prototype is intentionally not linked into the ordinary runtime yet.
Direct `ExecutorGlobals` storage and later opt-in sidecar placements were all
correct, but repeated x86-64 gates exposed code-layout regressions ranging from
1.54 to 4.84 percent in otherwise unrelated callback and regex controls. Those
variants were rejected rather than weakening the one-percent rule. With the
accepted standalone test boundary, ARM64 and x86-64 release binaries retain
their exact pre-milestone SHA-256 hashes (`695b7e472ce1913a59e2fdfed105f5626c4d41ae7deea86027eac000de8dab7d`
and `5359c1d345ecd9878c5e6bc0d358a7d97f294b80dc2b93be745a3e6cc533dcb8`),
so ordinary execution has provably zero allocation, layout, branch and code
size cost. Both feature configurations, the complete integration matrices and
all-target checks pass on both hosts. The next checkpoint is lazy pooled stack
segments with cleanup, exception and `finally` proofs before the substrate is
again considered for production-crate linkage.

### Lazy pooled context checkpoint (2026-08-09)

Milestone two is complete in the standalone executable prototype. Context
construction is now stack-allocation-free: paired main/pending VM stacks are
checked out lazily on first activation and returned to a driver pool only after
completion or explicit discard. Pool instrumentation proves no stack
construction during hand-off and 63 reuses across 64 sequential contexts.

Discard mirrors the canonical frame ownership boundary. It cleans heap slots
on main and pending-call frames, removes named variadics, clears exception,
generator and receiver state, pops the frame chain and then recycles storage.
Sixty-four repeated suspend/resume/discard cycles verify reference-count
release, exception isolation and the per-frame pending-finally flag. A separate
32-iteration real PHP try/catch/finally test produces exact output while using
one recycled stack pair.

Depth and live-slot independence now have a permanent ignored release
benchmark. ARM64 records 11.25 ns/switch for depth 1 with zero slots and 11.05
ns for depth 64 with 32 slots per frame; pinned x86-64 records 12.53 and 12.42
ns. Ordinary hand-off results are 11.10/13.01 ns on ARM64/x86-64. Both release
binaries remain bit-identical at the accepted hashes, and every feature,
integration and all-target gate passes on both hosts. Phase 4.5 can therefore
move to structured parent ownership and the first minimal PHP-facing API,
while production linkage remains subject to the same one-percent admission
rule.

### Structured coroutine API checkpoint (2026-08-09)

Milestone three promotes the lazy pooled substrate into the production crate
behind the non-default `coroutines` feature. One lexical `coroutine_scope`
owns every task created directly or by a running child. The minimal PHP API is
`coroutine_spawn`, `coroutine_suspend`, `coroutine_resume` and
`coroutine_join`; join returns a result or restores the child exception for
normal PHP catch/`finally` handling. Scope exit cancels unfinished children and
propagates the oldest unjoined failure deterministically.

Production code is split into API, state and scheduler modules. The scheduler
does not enlarge `ExecutorGlobals`, and its pinned boxed contexts make nested
spawn safe across task-map rehashing. Stack storage remains lazy and pooled:
64 sequential tasks construct one pair and reuse it 63 times. Seven PHP-facing
integration scenarios include nested ownership, cancellation, exception and
`finally` propagation, deterministic multiple failure selection and a
multi-frame suspend/resume chain.

The ordinary executor loop and two-argument ABI remain unchanged. Deep resume
is implemented by a feature-only wrapper that re-enters successive preserved
frames until the owned bottom frame completes. Suspension uses a feature-local
sidecar control bit and a zero-capacity existing error carrier, avoiding a new
hot enum variant, per-entry TLS lookup, added branch, copied frame or allocation.
The complete PHP API cycle measures 79.74 ns on ARM64 and 84.28 ns on pinned
x86-64 for one million iterations.

A separate callback cleanup replaces `echo_to_string()` temporaries with
`append_echo_to()` and is retained as commit `9f038eb`. Relative to that commit,
the default ARM64 binary is byte-identical at SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`.
On x86-64 only GNU build-id/symbol metadata changes; program section sizes are
identical and stripped binaries match at
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.

Against the original `3d546a2` phase baseline, all 1,003-pair regex gates and
101-pair application gates remain within the one-percent ceiling on both
architectures. ARM64 ranges are +0.54 to -17.86 percent for regex and +0.33 to
-1.98 percent for applications; pinned x86-64 ranges are +0.07 to -12.57 and
+0.47 to -10.41 percent. Complete default, no-default and all-feature release
integration matrices plus all-target checks pass on both hosts. An isolated
x86 callback comparison against the intermediate regex commit is +1.70 percent,
so future opt-in modules must continue treating ELF code layout as an explicit
gate rather than assuming source-level separation is sufficient.

### Bounded channels and timer readiness checkpoint (2026-08-09)

The first milestone-four slice adds capacity-bounded FIFO channels and a FIFO
ready queue with stable timer ordering. New feature-only PHP operations create
a channel, send or receive a `mixed` value and sleep a logical child for a
millisecond duration. Full sends and empty receives suspend without copying
the frame chain; joins drive other ready tasks and wait on the nearest timer
only after runnable work is exhausted. With no possible channel or timer
progress, join reports a deterministic deadlock.

PHP handlers were extracted to `runtime/coroutine/api.rs`, while channel state
and readiness policy are isolated below the scheduler. Delayed channel
delivery uses one feature-private frame-slot write that maintains heap cleanup
metadata for dormant strings and arrays. Integration coverage proves
backpressure, FIFO values, direct heap-value handoff, timer fairness, deadlock
and cancellation; lower-level tests prove blocked-sender promotion and stable
queue ordering.

On ARM64, one million capacity-one producer/consumer values take 166.46 ns
each, while the existing API suspend/resume control remains at 79.06 ns per
cycle. Pinned x86-64 records 151.54 and 81.63 ns respectively. The default
ARM64 release executable is still byte-identical to milestone three at
SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`;
after removing GNU build-id and symbol metadata, the x86-64 executable also
matches at
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.
### Non-blocking coroutine I/O readiness checkpoint (2026-08-09)

The second milestone-four slice adds scope-owned non-blocking Unix stream
pairs and explicit readable/writable waits. Non-blocking reads return data or
`false` for `WouldBlock`; writes return a possibly partial byte count or
`false`. The initial adapter keeps descriptor lifetime inside the coroutine
scope and deliberately does not claim generic PHP stream or TCP compatibility.

The combined progress driver polls I/O only inside the feature-only scheduler,
only blocks after runnable work is exhausted and uses the nearest logical
timer as its timeout. Stable stream and FIFO waiter ordering plus a
single-in-flight readiness guard preserve deterministic fairness. I/O,
progress-driving and scope cleanup now live in separate scheduler modules.
The platform call uses a small internal Darwin/Linux `poll(2)` binding, so no
external library or new Cargo dependency is introduced.

Four new PHP scenarios cover stream progress, ready-queue fairness, timer/I/O
interaction and scope cancellation; two unit tests cover bytes and
level-triggered waiter admission. Both complete all-feature/all-target host
matrices pass. Across five warmed release runs, the ARM64 medians are 53.72 ns
per suspend/resume cycle, 149.72 ns per bounded-channel value and 2,522.24 ns
per stream-readiness round trip. Pinned x86-64 records 78.36 ns, 149.46 ns and
4,357.96 ns respectively.

The ordinary ARM64 binary remains byte-identical at SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`.
The x86-64 text/data/BSS sizes remain 2,931,803/49,784/2,504 bytes and the
metadata-normalized executable remains byte-identical at
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.
This completes the bounded single-threaded coroutine substrate; broader
stream adapters continue in Phase 5, while multi-threaded work stealing stays
outside it.

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

### TCP listener coroutine adapter checkpoint (2026-08-09)

The first Phase 5 compatibility slice extends the feature-only coroutine I/O
substrate to real inbound TCP without adding a dependency. A coroutine scope
can bind a numeric `SocketAddr` through `coroutine_tcp_listen`, wait for the
listener to become readable, accept non-blocking connections through
`coroutine_tcp_accept`, and use the existing non-blocking byte-stream API on
each accepted socket. Both listener and accepted-stream ownership end with the
scope. Port zero returns the resolved local address; DNS names are rejected so
address resolution cannot block the cooperative scheduler.

The descriptor layer now represents Unix streams, TCP streams and TCP
listeners directly with standard-library types. It preserves deterministic
descriptor traversal, FIFO direction waiters and the single-in-flight
readiness rule. Listener/stream operation mismatches are rejected explicitly.
The related refactor moves Unix/TCP PHP handlers into a 178-line API unit and
descriptor tests into a 127-line sibling, leaving the main API, scheduler and
I/O policy files at 350, 474 and 479 lines respectively.

Real loopback tests cover full listen/accept/read/write progress,
runnable-before-network fairness, `WouldBlock`, cancellation and invalid
descriptor operations. The complete all-feature/all-target matrices pass on
both hosts, including 20 coroutine PHP scenarios and four descriptor-level
tests. Nine order-alternated pairs against the preceding checkpoint put the
existing ARM64 suspend/channel/readiness controls at paired median changes of
-0.50%, +0.42% and -0.05%; pinned x86-64 records +0.97%, -2.11% and +0.04%.

Default execution remains pay-for-use. The ARM64 executable is byte-identical
at SHA-256
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`.
The x86-64 text/data/BSS sizes remain 2,931,803/49,784/2,504 bytes and its
metadata-normalized SHA-256 remains
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.
Non-blocking outbound connect, explicit descriptor lifecycle operations and
generic PHP stream integration remain follow-up Phase 5 work.

The first explicit descriptor-close candidate was deliberately rejected. It
correctly released idle Unix streams and TCP listeners, propagated peer EOF,
allowed listener rebinding and refused descriptors with live readiness
waiters, all without a dependency. Correctness matrices passed and default
binaries retained their exact hashes. However, two order-balanced 20-pair
ARM64 gates of separately isolated cold variants moved the existing coroutine
suspend/resume control by +3.99% and +6.23%; channel and readiness remained
flat or improved, and x86-64 did not reproduce the suspend loss. The
architecture cleanup did not remove the ARM64 code-layout sensitivity, so no
candidate code remains. Explicit close should return only after feature API
placement is stabilized or as part of a larger adapter with an independently
measured payoff.

The first placement prerequisite is now accepted. Coroutine API metadata is
split into immutable core and platform descriptor slices, and registration
iterates them directly into the owned function vector. This removes the
temporary definitions vector and its startup allocation; future entries alter
static table data instead of the construction sequence. The API source remains
357 lines and uses no new dependency.

The gate also corrects the comparison protocol exposed by the rejected close
experiment: twenty pairs provide ten runs in each order, and the reported
paired result is the mean of the two order-specific medians. ARM64
suspend/channel/readiness changes are -0.73%, -0.68% and -0.16%; pinned x86-64
records +0.55%, -1.20% and +0.24%. Complete host matrices pass, and default
ARM64 plus metadata-normalized x86-64 executables remain byte-identical at
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`
and `0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`.
The x86-64 text/data/BSS sizes remain 2,931,803/49,784/2,504 bytes.

A controlled close retry on this new static baseline isolates the remaining
cause. The order-balanced ARM64 suspend/resume delta was still +3.96%, while
channel/readiness were -0.59%/+0.25%. Because registration construction was
already stable, the regression belongs to the newly linked handler/descriptor
layout rather than the removed temporary vector. The retry was rejected at
the ARM64 gate; no close source or API remains and no external dependency was
introduced.

### Internal non-blocking outbound TCP connect checkpoint

The next Phase 5 substrate is accepted below the PHP surface. A private
Darwin/Linux adapter creates numeric IPv4/IPv6 sockets with local POSIX FFI,
wraps ownership immediately in `TcpStream`, applies non-blocking and
close-on-exec semantics, and disables Darwin `SIGPIPE`. Native socket-address
layouts are compile-time checked at 16 and 28 bytes. Connect interruption,
already-connected, in-progress and already-progressing outcomes are handled
explicitly; completion uses `SO_ERROR` plus `peer_addr`. No external library
or Cargo feature was added, and DNS resolution remains outside the scheduler.

The final representation deliberately adds no pending enum variant, flag or
branch to the existing stream path. The socket occupies an ordinary TCP
descriptor while the private connect operation retains its progress outcome;
the kernel socket state is authoritative until writable readiness and finish.
Two earlier stateful layouts passed functional tests but were rejected after
order-balanced controls exceeded one percent, including +1.84%/+2.09% ARM64
suspend/resume and +1.42% x86-64 readiness holdouts. Those candidates leave no
source behind.

The state-free candidate and the static-registration baseline have identical
release machine code. ARM64 `.text` SHA-256 is
`f8fb9858de9b6f011d8e13483774a8de7a65cfd6cccde7516f5dd998da9261d7` in
both builds; x86-64 is
`f96f0ea3f953d2e0292984d16ecf77b967d8bcc57e6cceb2c8a65cbe69b4e5f1`.
Section sizes also match exactly, so timing noise cannot represent a changed
existing execution path. Loopback success and refusal tests pass on both
architectures. Full all-feature matrices now contain 252 ARM64 and 277 x86-64
library tests and keep the 20/3 coroutine E2E result; no-default matrices and
the established default release hashes remain exact. A PHP-visible numeric
connect handler, cancellation ownership and API admission remain a separate
measured checkpoint; this source does not claim DNS or generic streams.

### Numeric non-blocking TCP connect API checkpoint

Phase 5 now exposes the accepted substrate as feature-only
`coroutine_tcp_connect(address)`. Only numeric IPv4/IPv6 socket addresses are
accepted. A completed connect returns a positive scope-owned stream descriptor;
an in-progress connect suspends the child without exposing the descriptor, and
blocking DNS resolution is still deliberately excluded.

The suspension continuation is isolated in its own I/O map rather than
widening `ByteStream` or every descriptor. It retains the task, caller frame
and return slot, while `WaitReason::TcpConnect` distinguishes completion from
an ordinary writable wait. The driver validates `SO_ERROR`, writes through the
VM's heap-aware result helper and only then wakes PHP. Spurious readiness is
rearmed. Refusal removes the private socket and returns the established fatal
connect error; scope cancellation removes the continuation before frame
cleanup. No dependency or new Cargo feature was introduced.

The existing controls pass two independent order-balanced 20-pair ARM64 gates:
suspend/channel/readiness are +0.37%/-0.70%/+0.82% in the first and
-0.15%/-3.13%/-1.44% in the repeat. Pinned x86-64 records
-2.85%/-0.19%/+0.14%. Full all-feature/all-target matrices pass 253 and 278
host library tests, seven descriptor tests and 22/3 coroutine E2E scenarios;
both no-default matrices pass 161 library tests. The default ARM64 SHA-256
remains `f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`.
The metadata-normalized x86-64 SHA-256 remains
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`
with unchanged text/data/BSS sizes 2,931,803/49,784/2,504 bytes.

Implementation tests were split from the ABI source into
`io_connect_tests.rs`; both architecture-specific release test executables are
bit-for-bit identical before and after that refactor. This checkpoint claims a
numeric connect primitive only. Explicit close, timeouts, DNS and a generic PHP
stream adapter remain separate measured slices.

### Optional outbound TCP connect timeout checkpoint

The numeric API now accepts an optional second argument as
`coroutine_tcp_connect(address, timeoutMilliseconds)`. Omitting it preserves
the unbounded connect behavior. A supplied value must be a non-negative PHP
integer; the scheduler converts it to a checked `Instant` deadline before
creating the socket, so an unrepresentable duration fails without leaving a
descriptor behind. The address surface remains numeric IPv4/IPv6 only.

Connect deadlines reuse the existing readiness timer heap. Only an
in-progress connect schedules a timer. Successful writable completion removes
that task's timer before publishing the descriptor, preventing a stale future
deadline from keeping an otherwise idle scheduler alive. If the deadline is
drained while the task still has `WaitReason::TcpConnect`, the driver removes
the private continuation and descriptor and returns a fatal timed-out connect
result. Ordinary sleep timers retain their previous wake path. No crate,
Cargo feature or external resolver/event library was added.

Two earlier representations were rejected by the one-percent rule. Scanning a
separate connect-deadline map on every driver pass produced +2.13% ARM64
channel and +1.38% readiness controls. Guarding that scan behind existing I/O
work removed the channel cost but left readiness at +1.10%. Both candidates
were removed. Reusing the established timer heap leaves `next_runnable`'s
ordinary I/O branch unchanged and passes an order-balanced 20-pair ARM64 gate
at -2.47%/+0.35%/+0.20% for suspend/channel/readiness. Pinned x86-64 records
-11.21%/-3.08%/-0.79%; these are control results, not claimed speedups.

Complete all-feature/all-target matrices pass 255 ARM64 and 280 x86-64
library tests, with 23/3 coroutine E2E scenarios; both no-default matrices pass
161 library tests. The default ARM64 release remains byte-identical at
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`.
The metadata-normalized x86-64 release remains byte-identical at
`0031f562a9fefb7771d3d9d14da44d1aa7f763c2079a617898ceed0759db5b70`
with unchanged text/data/BSS sizes 2,931,803/49,784/2,504 bytes. The accepted
feature test executables have SHA-256
`fdfe498ac84fefed63c470ed136a692b5fc71334b32dce01951fe87213da4621`
on ARM64 and
`275b9d6c9031f2ab234056d9e51e8055ccdc427c8adb9dd926f28676035669d5`
on x86-64. Scheduler timeout tests live in a separate `driver_tests.rs`,
leaving the production driver at 118 lines. Explicit close, DNS and generic
PHP stream integration remain independent Phase 5 slices.

### Post-timeout close section retry (rejected)

A third explicit-close experiment tested stronger code-placement isolation
than the two earlier cold-module attempts. The handler, scheduler bridge and
idle-only descriptor removal were all marked non-inline/cold and placed in a
dedicated ARM64 Mach-O `__TEXT,__rphp_life` section. That section contained
only the three lifecycle functions and occupied `0x308` bytes. Functional
tests again proved EOF delivery, immediate listener rebinding and rejection of
queued or in-flight readiness waiters without adding a dependency.

The separate section did not stabilize the rest of the linked test image:
ordinary `__text` moved from `0x257b5c` to `0x25698c`. An order-balanced
20-pair ARM64 gate then recorded +5.84%/-0.48%/-0.23% for
suspend/channel/readiness. Pinned x86-64 was +0.97%/-0.37%/+0.10%, but the
ARM64 suspend result rejects the slice. All close API/source/test changes were
removed and both feature executables returned bit-for-bit to the accepted
timeout hashes. A future explicit lifecycle surface should therefore arrive as
part of a broader stream adapter with its own measured payoff, not as another
handler-placement retry.

### Coroutine stream-policy module checkpoint

The first accepted post-timeout production split moves pair/listener creation,
accept, readiness admission and queueing, and stream reads/writes into the
155-line internal `scheduler/io_stream.rs` module. Descriptor registration,
polling, connect continuations and lifecycle bookkeeping stay in
`scheduler/io.rs`, which falls from 489 to 366 lines. Visibility is limited to
the enclosing scheduler. No handler, public API, Cargo feature, crate or other
external library is added.

The explicit module boundary changes feature-test code layout on both hosts,
so it was measured against the accepted timeout executables. An order-balanced
20-pair gate records +0.45%/+0.00%/-0.61% for suspend/channel/readiness on
ARM64 and +0.21%/-0.15%/-0.78% on pinned x86-64. All results remain below the
one-percent regression ceiling; no speedup is inferred. Clean feature-test
SHA-256 values are
`ba47663ee205fbcae00518bc4d8b38fe10ae37796d03b5a2850b2f7da6cc5569`
on ARM64 and
`f77d83f30063f5d582302279c3dd9f23033d1cc31c184e7e227dd46eaaf55a3d`
on x86-64.

Seven descriptor/connect tests and 23/3 coroutine E2E scenarios pass on both
hosts. Complete all-feature/all-target and no-default matrices are green with
the established 16 MiB test-thread stack used only by the pre-existing
Ackermann debug test. Default ARM64 and metadata-normalized x86-64 release
hashes remain exact, and x86-64 text/data/BSS sizes stay
2,931,803/49,784/2,504 bytes. The split is accepted solely as a clearer
ownership boundary with measured neutral runtime behavior.

This checkpoint also tightens benchmark hygiene. `cargo build --release` does
not rebuild an ignored integration-test executable, so every future coroutine
gate must first run the release-test no-run build (`cargo test --release
--features coroutines --test e2e_coroutines --no-run`) in a fresh target
directory. A clean rebuild exposed one stale intermediate image during this
checkpoint; the recorded hashes and deltas above are the corrected comparison
of actual source states.

`benches/run_coroutine_gate.sh` makes the corrected protocol repeatable. Given
candidate and baseline source roots, it builds each release integration target
in a separate fresh temporary directory, alternates execution order, computes
the balanced order-specific median ratios and exits nonzero above the default
+1% ceiling. Optional Linux CPU pinning remains explicit; the script adds no
crate, Cargo feature or non-system runtime dependency.

### Cohesive stream lifecycle adapter retry (rejected)

With the corrected fresh-target protocol, one broader dependency-free adapter
tested TCP local/peer address metadata, Unix/TCP shutdown and explicit idle
descriptor close as a single Phase 5 slice. Its lifecycle contract was sound:
shutdown delivered EOF, listener close allowed immediate rebinding, and close
rejected queued, in-flight or connecting descriptors. Nine I/O tests and 25/3
coroutine E2E scenarios passed on ARM64 and x86-64.

Clean candidate feature-test SHA-256 values were
`5b105bb10a71e311e1496382d2661b533eb8b8dfea669c8f7ebdbf5ec5b543c0`
on ARM64 and
`b9ac49c1efe76d68d49b76a0b64b93d286a82248630fdc01fe6acf0983ab2dac`
on x86-64. The valid 20-pair comparison against the clean stream-policy
baseline measured +2.67%/+1.44%/+0.05% for suspend/channel/readiness on ARM64
and +10.99%/+3.99%/+0.04% on pinned x86-64. Both hosts therefore reject the
slice at the one-percent gate. All source, API and tests were removed; no
external library or Cargo change was introduced. Generic stream resources now
require a code-placement boundary that does not perturb existing coroutine
handlers, rather than another larger static-table retry.

### Coroutine context prototype test split

The standalone context-switch prototype receives a codegen-neutral test-only
cleanup. Its 1,039-line integration target is now a 451-line model/driver root
plus a 588-line `e2e_coroutine_context/tests.rs` module. The nested `tests::*`
paths and all eight names remain unchanged; no production source, Cargo entry
or dependency changes.

No-default and all-feature configurations pass six tests with two ignored
benchmarks on both hosts. Isolated ARM64 measurements are 12.36 ns per context
hand-off and 11.30/11.98 ns for depth=1/slots=0 versus depth=64/slots=32;
x86-64 records 12.77 ns and 12.89/12.73 ns. The accepted default and
coroutine feature executables retain their exact hashes on ARM64 and x86-64.

### Coroutine integration-target test split

The 787-line coroutine integration target now has an explicit test-only
ownership boundary. Its 78-line root contains the shared process and loopback
harness and directly includes 281 lines of structured-concurrency/channel
tests, 293 lines of non-blocking stream/TCP tests and 135 lines of release
benchmarks. Using `include!` rather than nested modules preserves all 26 root
test names and the exact benchmark command filters. Production source, Cargo
configuration, crates and external libraries remain unchanged.

ARM64 and x86-64 each pass 23 scenarios with three ignored benchmarks. The
permanent fresh-target runner measured 20 order-alternated pairs against an
archived one-file baseline. Suspend/channel/readiness deltas are
-0.056%/+0.242%/-0.253% on ARM64 and -0.382%/-0.162%/+0.616% on pinned x86-64.
All remain below the +1% admission limit; the measurements establish neutral
runtime behavior rather than a speedup.

### Codegen-stable coroutine core-API split

The PHP-facing coroutine API now separates its eight structured-concurrency,
channel and timer handlers into the 141-line `api/core.rs`. The 227-line root
keeps scope-root invocation, manual suspend mechanics, ABI helpers and the
complete registration tables. A private `include!` is intentional rather than
temporary: it preserves the existing `api::*` handler identities and exact
registration order while establishing physical source ownership. No external
library, crate, Cargo configuration or public API changes.

A real Rust submodule was functionally correct but failed admission. It passed
255/280 library tests and 23/3 coroutine E2E scenarios on both hosts, then the
ARM64 20-pair gate measured +5.957%/-3.813%/+0.882% for
suspend/channel/readiness. The candidate was removed before the accepted
checkpoint because the suspend path exceeds +1%.

The codegen-stable split passes the same functional matrix. Fresh-source,
order-balanced 20-pair deltas are +0.306%/-0.040%/-0.224% on ARM64 and
+0.188%/-0.515%/+0.355% on pinned x86-64. Clean feature-test hashes are
`2333dd6ee8bcc9ddb300be6e113b050e37b07d83401af43cdd399c8a15061b3d` on ARM64
and `1922d968ed64ecb069aa1a23290895644a5daf4b3472f165367d34805a7a0e42` on
x86-64. Default release hashes remain exact, so the checkpoint is accepted as
ownership cleanup without a performance claim.

### Scheduler operation include split (rejected)

The next cleanup candidate moved 154 contiguous channel, timer and I/O adapter
method lines into a macro included and expanded inside the existing
`CoroutineScheduler` implementation. This preserved inherent method names,
reduced the root from 506 to 355 lines and changed no behavior, API, state,
crate or external library. Both hosts passed their complete 255/280 library
sets and 23/3 coroutine E2E scenarios.

Fresh-source 20-pair deltas were -0.562%/+0.422%/-1.671% on ARM64 and
-0.097%/+1.470%/+1.099% on pinned x86-64 for
suspend/channel/readiness. The x86 channel and readiness regressions exceed
the +1% limit, so the entire candidate was removed and both source trees were
restored exactly to the core-API checkpoint. Do not retry an ownership-only
split of these hot methods without a stronger linker/code-placement boundary.

### Standard-library resolver wake prototype

Phase 5 now has an isolated asynchronous hostname-resolution transport
prototype without linking it into production. The 310-line integration target
uses one named `std::thread`, standard `mpsc` request/completion queues, a
non-blocking `UnixStream` wake pair, monotonic job ids and a pending-result
filter. Resolution runs through `ToSocketAddrs` only on the worker; the caller
can poll the wake descriptor through the same Darwin/Linux ABI shape already
used by coroutine I/O. No dependency, Cargo feature or runtime source changes.

Both hosts pass four correctness tests with one ignored release benchmark.
For 10,000 numeric jobs, the complete enqueue/resolve/wake/drain path measures
513.19 ns/job on ARM64 and 1,022.91 ns/job on x86-64; direct numeric resolution
controls measure 26.34 and 17.21 ns/job. Default production hashes remain exact
at the established ARM64 and x86-64 values.

The prototype deliberately makes no PHP DNS claim. Standard-library resolver
calls are not individually cancellable once inside the OS; cancelled ids only
filter late completion, and orderly shutdown joins the worker. The next slice
must define bounded worker lifetime and integrate the wake descriptor behind
an inactive-by-default scheduler resource, then pass the existing two-host
coroutine gate before changing `coroutine_tcp_connect` parsing.

### Bounded resolver-pool prototype

The resolver follow-up replaces the single unbounded worker queue with two
fixed workers and a 64-entry `sync_channel`. Submission is exclusively
`try_send`: a full queue returns `WouldBlock`, so the future scheduler path can
apply explicit backpressure without ever waiting on a resolver worker. Tests
can inject a resolver function and choose pool dimensions; production-shaped
tests still use standard `ToSocketAddrs`. The target grows to 431 lines and
adds no runtime source, Cargo setting or external library.

Two deterministic tests block one injected worker while a fast job completes
on the second, then fill a one-worker/one-slot queue and verify immediate
overflow rejection plus delivery of both admitted jobs. Together with the
original cases, ARM64 and x86-64 pass six tests and retain one ignored release
benchmark. The 10,000-job numeric transport records 524.42 ns/job on ARM64 and
827.74 ns/job on x86-64, versus the preceding 513.19/1,022.91 ns/job prototype.
Default release hashes remain exact.

This bounds queue memory and removes one-job head-of-line blocking, but it does
not make OS resolver calls cancellable. Production integration remains gated
on lazy pool ownership, scope-safe continuation cleanup and the existing
two-host coroutine performance controls.

### Lazy production coroutine DNS checkpoint

Phase 5 now promotes the bounded standard-library resolver into the
feature-only runtime. `coroutine_tcp_connect` accepts numeric IPv4/IPv6 exactly
as before and additionally accepts `hostname:port`. Hostnames are submitted by
`try_send` to one lazily initialized process-wide pool: two named workers and a
64-entry `sync_channel`. There is no crate, Cargo feature, external DNS client
or event-loop dependency.

Each scheduler that actually resolves a hostname owns a completion channel and
non-blocking `UnixStream` wake pair. Its read side is a hidden `IoSet`
descriptor, armed only while resolver jobs have live continuations. Resolver
completion therefore shares the existing `poll(2)` call and ordinary readiness
queue. Cancellation removes the waiter and disarms that descriptor without
joining a worker. The platform's blocking `ToSocketAddrs` call itself remains
non-cancellable; a result arriving after timeout or scope cancellation is
filtered by job id or discarded with the scheduler-local receiver.

The connect deadline starts before DNS and remains unchanged across every
resolved address. A failed asynchronous candidate preserves its suspended
frame and tries the next address, covering common `localhost` IPv6-to-IPv4
fallback. Success cancels the timer before waking the task. Resolver error,
queue saturation, address exhaustion, timeout and scope cleanup all leave no
live scheduler continuation or private connect descriptor.

An initial production version was functionally green but checked an optional
resolver fd in every scheduler pass. ARM64's fresh 20-pair gate rejected it at
+9.452%/+1.361%/+1.110% for suspend/channel/readiness. Moving resolver wake
ownership into the existing I/O descriptor set and keeping transition methods
in the cold resolver module restores the inactive path. Final fresh-target
20-pair deltas are -0.333%/-1.938%/-0.401% on ARM64 and
-2.797%/-3.112%/-0.644% on pinned x86-64. All are below the +1% ceiling and
are treated as neutral admission evidence rather than performance gains.

Both architectures pass 168 no-default library tests, 255 ARM64 or 280 x86-64
all-feature library tests, 24/3 coroutine E2E scenarios and complete
all-feature/all-target compilation. Clean coroutine feature-test hashes are
`52f85dd6c78789a3f5718ca095767de842bde3811ece7e1bd7776bc3478bbbb2`
and `863e991e667d667133a756c697fd7085b4c7d95dd1b6473def19135084b94a0d`.
Default ARM64 remains exact at
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`;
fresh x86-64 candidate and exact baseline default releases are identical at
`1b42d96b831bc0d717e28f46bbb49896f56891aac4385917a7dea07941f7070d`.
Generic PHP stream resources remain the next independent compatibility
boundary.

### Resolver transport ownership split (rejected)

The immediate cleanup candidate moved the fixed worker pool and transport out
of the 376-line resolver continuation into a 203-line submodule, leaving the
scheduler-facing file at 181 lines. The 73-line wake adapter also moved from
`io.rs`, reducing that hot ownership root from 435 to 380 lines. No behavior,
API, crate, Cargo feature or external library changed, and both hosts passed
168 no-default tests, 255/280 all-feature library tests, 24/3 coroutine E2E
scenarios and complete all-target compilation.

The split is nevertheless rejected by the same codegen gate used for the DNS
feature. A preliminary ten-pair ARM64 check reported
-2.500%/+0.162%/+0.969% for suspend/channel/readiness. The final 20-pair run
reported -5.228%/-0.866%/+1.244%, putting readiness above the +1% ceiling.
Pinned x86-64 remained admissible at +0.845%/-0.118%/-0.657%, which does not
override the ARM64 failure. Both source trees were restored exactly to
`4cf4abb`; none of the split modules or lifecycle changes remains. Further
ownership-only movement of this resolver path requires a stronger linker or
code-placement boundary rather than another module reshuffle.

### Non-blocking UDP coroutine adapter

The next accepted Phase 5 slice extends the existing scoped descriptor and
`poll(2)` driver to datagrams using only `std::net::UdpSocket`.
`coroutine_udp_bind` accepts a numeric address and returns `[socket,
localAddress]`; `coroutine_udp_send_to` returns a byte count or `false` on
`WouldBlock`; `coroutine_udp_recv_from` returns `[data, peerAddress]` or
`false`. Callers suspend through the unchanged readable/writable wait API and
retry, so UDP shares the same timer/readiness ordering and adds no scheduler
scan.

The descriptor registry represents datagrams explicitly. UDP receive retains
one packet boundary and its peer, while generic byte-stream and TCP-listener
operations reject that descriptor kind. Receive buffers are capped at 65,535
bytes. Binding and destinations remain numeric-only; introducing a resolver
into the non-suspending send operation is intentionally outside this slice.
Implementation ownership is split from the outset into an 85-line I/O policy,
37-line scheduler bridge and 111-line PHP adapter, with 95 lines of dedicated
descriptor tests.

Tests cover bidirectional loopback traffic, sender identity, readable and
writable poll integration, `WouldBlock`, wrong descriptor kinds, scheduler
fairness, malformed DNS-style bind input and oversized receives. ARM64 passes
257 all-feature library tests and x86-64 282; both pass 168 no-default tests,
26/3 coroutine E2E scenarios and every all-feature/all-target compile.

Fresh 20-pair admission against the accepted DNS source reports
-2.861%/-0.288%/-0.486% on ARM64 and +0.079%/-0.529%/+0.001% on pinned
x86-64 for suspend/channel/stream readiness. All controls remain below +1%.
Clean feature-test hashes are
`6d9b3278845b051844d2613ba4b7b17b96c41d4fe85752b79f478dec633e60b0`
and `7f038c8bfbe1be43af5380c2071880a45acbec68185b0dbab96c9046567108cb`.
Default ARM64/x86-64 releases remain exact at
`f0129c6de8fdf33c2b12e7ef6d738c535787cb360bc36d183bb29f93594472b3`
and `1b42d96b831bc0d717e28f46bbb49896f56891aac4385917a7dea07941f7070d`.
The checkpoint adds no crate, Cargo feature or external library.

### Generic PHP stream-resource checkpoint

Phase 5 now has a generic, request-owned PHP resource identity and its first
file/memory stream implementation without an external library or Cargo change.
The 16-byte `Value` stores only a request-local integer id; a lazy registry owns
`std::fs::File` or `Cursor<Vec<u8>>`, closes explicitly on `fclose()` and drops
everything still open with the request. Ordinary scalar clone/drop remains
unchanged. Bare paths, `file://`, `php://memory`, `r/w/a/x/c` plus update modes,
read/write/flush/seek/position/EOF and the resource inspection functions are
covered by seven E2E scenarios. Dropping the last alias without `fclose()`
still retains the backend until request shutdown and is the explicit next
lifecycle boundary, not hidden as full PHP compatibility.

Admission includes general runtime work exposed by the two-host gate. Packed
`$array[]` appends now take one direct `Vec::push` fast path. Monotonic
coroutine contexts and channels use dense private vectors instead of hashed
registries while preserving pinned context addresses and established scheduler
field offsets. Channel blocking reuses its validated task id, and resume entry
does not repeat the scheduler/executor check already made by all callers.

The permanent fresh-build coroutine gate measures suspend/channel/readiness at
-18.517%/-55.960%/-3.457% on ARM64 and
-19.220%/-39.062%/-3.990% on pinned x86-64 against `1f28a2b`. A new
standard-library-only runtime gate applies the same alternating 20-pair and
+1% protocol to five default workloads. Pinned x86-64 records
-0.197%/-0.987%/-0.144%/+0.742%/+1.115% for scalar/array/string/order/ledger;
the sole failure is not reproduced by an independent 20-pair ledger rerun at
+0.508%.
The ARM64 full run records -0.174%/-2.370%/+1.668%/-0.265%/-1.566%; the sole
String failure occurs during visible thermal drift and an independent 20-pair
rerun is +0.202%. The focused packed-array result is -2.829%.

Both architectures pass 167 no-default library tests, complete all-feature
sets (265 ARM64 / 290 x86-64), seven stream scenarios and all-feature/all-target
compilation. Clean default hashes are
`de0fec7335fa71d1cdc5720637dbe970de4be0cfde604f455971465aa638af31` and
`a0228fe3e34a6a367c96a71a9f4a2322db2d963d7f83d3f4a0b487a3bf6c23e0`;
clean coroutine integration hashes are
`813117642d8b990b574eae5e5de5f2db87fbd722deeb50912c27dd98aa606909` and
`bccc63469e4967ddf5a37cf6aa37f12399108446fac69d9a788b2e4d2c2dc796`.
The next compatibility step is a bounded stream extension such as
`php://temp` plus line-oriented reads and metadata, retaining the
standard-library-first policy.

### Bounded temporary streams and dense no-JIT loop kernels

The next Phase 5 checkpoint adds `php://temp` without a crate, Cargo change or
platform stream dependency. The default in-memory budget is 2 MiB and
`php://temp/maxmemory:N` selects an explicit non-negative budget. Writes stay
in a seekable `Cursor<Vec<u8>>` until the next write would cross that limit;
the backend then copies once into a uniquely named owner-only temporary file,
preserves the logical cursor and continues through the same `PhpStream` API.
Explicit close, registry shutdown and ordinary backend drop all remove a
spilled file. Memory-only streams never create one. The parser rejects extra
path components and malformed limits rather than silently selecting another
policy.

Admission also turns four recurring no-JIT shapes into target-neutral dense
execution rather than accepting layout regressions. An exact three-operation
String-append loop and an exact packed-array-push loop retain the existing
guard, interrupt and overflow/deoptimization boundaries while avoiding the
general operation dispatcher. An induction-plus-constant accumulator folds
one 32-iteration interrupt interval with checked arithmetic and falls back to
the canonical one-step loop for short, sign-crossing or potentially
overflowing ranges. Read-only virtual object-array pipelines resolve invariant
nested receivers, method caches, targets, plans and declaring classes once at
region entry; changing arguments remain checked per iteration. Finally,
one-to-four-operation scalar-plan ranges are explicitly unrolled after one
bounds validation, with larger and malformed public plans retaining the safe
loop/failure path.

Fresh 20-pair default-runtime gates against `178ef41` report
-95.649%/-18.367%/-57.683%/-1.716%/-1.326% on ARM64 and
-96.604%/-12.450%/-59.513%/-10.019%/-2.207% on pinned x86-64 for scalar,
packed array, String, order and ledger workloads. The independent coroutine
gate remains neutral and below its +1% ceiling at
+0.367%/-0.841%/-0.741% on ARM64 and -1.869%/-0.086%/+0.343% on x86-64 for
suspend/resume, bounded channel and readiness.

Both hosts pass 174 no-default library tests, 272 ARM64 or 297 x86-64
all-feature library tests, eight stream scenarios, four application-corpus
scenarios and complete all-feature/all-target compilation. Dedicated tests
cover spill position and deletion, exact kernel selection and rejection,
chunk overflow fallback, scalar temporary dependencies and malformed range
rejection. `Cargo.toml` and `Cargo.lock` are unchanged. Clean ARM64/x86-64
default release SHA-256 values are
`859e22b8a1fc8d52a3b495b037d68ee347ffe1c560dd5a391f722d4f08c03e2f` and
`83043325ba2545a2c6517bdaceb4d7cbc368c26a80b649dcb4e773dafb85dab4`;
the corresponding coroutine integration hashes are
`40dc251671dca21c978d25cf17978080be51d3cc8f74d6ff2c5d5a6b80a21fac` and
`8eb5a0592f9c5b1fc888d925fb26cf13717b8a3825929bac8818cd3ffe11c2c1`.
Line-oriented reads and stream metadata remain the next compatibility
boundary; dropping the final resource alias without `fclose()` still
intentionally retains the backend until request shutdown.

### Line reads, backend metadata and 32-turn dense consumers (2026-08-10)

The stream compatibility boundary now includes `fgets()` and
`stream_get_meta_data()` without a buffering crate or any Cargo change.
`fgets()` uses one fixed 8 KiB stack scratch buffer, retains a terminating
newline, treats the optional length as a `length - 1` byte ceiling and seeks
back any over-read bytes so the next operation observes the exact cursor.
Lines longer than the scratch buffer are assembled incrementally. A final
unterminated line performs the same EOF probe as PHP, while allocation failure
rewinds the chunk it could not publish.

Metadata is owned by `PhpStream` rather than reconstructed in the PHP handler.
Plain files report the original URI and mode with `plainfile`/`STDIO`; memory
streams report normalized binary modes with `PHP`/`MEMORY`; temporary streams
report `PHP`/`TEMP` and retain their identity after spilling. The backend
controls which timeout, blocking and EOF keys exist, matching the observed PHP
8.5 shapes. Stream implementation tests now live in `stream/tests.rs`, leaving
the production ownership and spill policy in one readable module. Eleven E2E
stream scenarios cover multi-buffer lines, length/cursor/EOF behavior and all
three metadata backends.

The no-JIT follow-up removes repeated work from three dense consumers. Virtual
object-array regions resolve each invariant consumer and trailing key to an
entry index once at activation, then continue to guard the exact object plan
and values on every turn. Exact String-append and packed-array-push loops can
prove one 32-iteration interrupt interval, simulate the same post/condition
state and publish through one dense batch; short tails, mutable bounds,
overflow risk and non-canonical arrays retain the one-step path. Canonical
packed Long batches append directly to their `Vec<Value>` after one checked
key advance, with an exact generic fallback for every other storage form.

The initial source expansion exposed a genuine coroutine code-placement
regression even though scheduler source was unchanged. Admission therefore
removes work from the common switch instead of padding code: empty exception,
named-variadic, generator and pending-invoke states no longer move between two
empty sides, and the one-byte suspension sidecar occupies an existing dense
registry reserve word instead of traversing a second thread-local on suspend
and resume. Non-empty side state retains the original complete exchange, and
dedicated tests cover both that path and one-time sidecar consumption.

Fresh 20-pair default-runtime gates against `bc54dfe` record
-0.966%/-24.187%/-47.041%/-16.267%/-4.400% on ARM64 and
+0.560%/-45.612%/-17.061%/-10.892%/-3.445% on pinned x86-64 for scalar,
packed array, String, object order and ledger. The matching coroutine gate
records -7.926%/-3.242%/+0.088% on ARM64 and
-7.885%/-1.509%/-2.045% on x86-64 for suspend/resume, bounded channel and
stream readiness. Every positive result remains below the +1% ceiling.

Both hosts pass 178 no-default library tests, 280 ARM64 or 305 x86-64
all-feature library tests, 11 stream scenarios, 118 quick-loop scenarios, four
application-corpus scenarios, 26 coroutine scenarios with three ignored
release benchmarks and complete all-feature/all-target compilation. Final
ARM64/x86-64 default release SHA-256 values are
`3816f11807ad36b2a251130e193c52ed2f8e60f7a1a880760ab697f8432785b1` and
`31d18d9a1af594f9c37c4f8348aa5aeb695d6510085fff93eefa2b0fa409999c`;
the corresponding coroutine integration values are
`49fd48cb26d833bd84b1a57136a592a893f4511429ab415f94bd43ee55bfd801` and
`192c6d57fa7a93a933181eed19f045bb693572f2bf5044376b4f92b97a2f0b26`.
`Cargo.toml` and `Cargo.lock` are unchanged, and the complete checkpoint uses
only the Rust standard library. Final-alias resource release remains the next
lifetime boundary; broader line parsers such as CSV should build on this
cursor contract rather than add a parallel buffered-stream abstraction.

### Streaming CSV records and cold-layout isolation (2026-08-10)

The next stream slice implements `fgetcsv()` directly on the established
seekable cursor contract, using only the Rust standard library. `CsvParser` is
an incremental byte state machine rather than a second buffered-stream layer.
It retains only the current record, continues quoted fields across arbitrary
physical lines, handles doubled enclosures and PHP's retained escape byte,
and uses fallible `Vec` growth. A positive length constrains the first physical
read exactly; an enclosure still open at that boundary continues to the end of
the logical record. Parser or allocation failure rewinds every consumed byte.

Differential probes against PHP 8.5.9 established the byte-level edge rules:
leading horizontal whitespace before an opening enclosure is discarded,
quotes in unquoted fields remain literal, bytes after a closing enclosure are
appended, a blank record is `[null]`, delimiter-only fields are empty strings,
CRLF is stripped outside quotes and retained inside them, and a newline remains
the record boundary even when selected as the separator. The enclosure wins
when enclosure and escape are equal. The first compatibility slice returns
`false` for a negative length or invalid multi-byte control argument; PHP 8.5
raises `ValueError` for those argument-validation cases, which remains a
deliberate error-surface follow-up.

Adding a cold handler initially changed placement of the large
`run_quick_long_ops_loop` function enough to move unrelated runtime timings.
Sampling still placed approximately 97 percent of the ledger workload in that
same function, identifying code generation and placement rather than new CSV
work as the cause. The accepted boundary keeps the established textual
quick-dispatch include on Linux. Apple builds compile the dispatcher as a real
child module and place CSV parser/handler code in a dedicated cold text
section; the Apple registration is append-only so existing handler order stays
stable. Comments at each target boundary record this measured constraint.

A clean 20-pair ARM64 default-runtime gate on the admitted code reports
-0.656%/+0.318%/+0.678%/-1.062%/-0.126% for scalar, packed array, String,
order and ledger, with a later focused final ledger confirmation at -0.687%.
A clean code-equivalent x86-64 gate reports
+0.497%/+0.386%/-0.071%/-0.434%/+0.533%. One final full x86-64 run under
system load measured +1.367% for ledger while its other four controls stayed
between -1.224% and +0.342%; the independent 20-pair ledger rerun measured
+0.757%, below the +1% ceiling. The final coroutine gates pass at
-0.928%/-3.618%/+0.265% on ARM64 and -2.920%/+0.316%/+0.765% on x86-64.

Both hosts pass 186 no-default library tests, 288 ARM64 or 313 x86-64
all-feature library tests, 14 stream scenarios and complete
all-feature/all-target compilation. Final ARM64/x86-64 default release
SHA-256 values are
`532ec13e40fc001c6202444695d63d3f99501c09168ef0454d3c5b6640f2517c` and
`5d2e7ba750f037414078617ec717c967cc2a38c2536df938595f9a8c1549d20a`.
`Cargo.toml` and `Cargo.lock` remain unchanged: the complete CSV path adds no
external dependency. `fputcsv()` and exact PHP argument-error construction are
the next bounded CSV compatibility slices; final-alias stream release remains
an independent lifetime boundary.

### Opt-in streaming CSV writer and default-layout admission (2026-08-10)

The complementary write slice implements `fputcsv()` in the new
`streams::csv_write` child module using only the Rust standard library. Its
incremental `CsvEncoder` follows PHP 8.5 byte rules: fields containing the
separator, enclosure, configured escape, newline, carriage return, tab or an
ASCII space are enclosed; unescaped enclosure bytes are doubled; legacy
escape-byte behavior is retained. Array insertion order, PHP scalar rendering,
custom and empty record terminators and empty records are covered directly.
The handler uses fallible record growth, reports the complete encoded length
and loops over short writes until the full record reaches the existing stream
backend. Invalid control widths, non-array fields, closed resources and
unwritable streams conservatively return `false`; exact PHP `ValueError` and
`TypeError` construction remains the shared argument-error follow-up.

The writer was first evaluated as a default function. Even after moving every
new implementation detail into a real child module and restoring
`stream.rs`, `stream/csv.rs` and the existing reader path byte-for-byte, the
additional linked code changed ARM64 placement/code generation enough to move
the unrelated ledger workload by about 4--5 percent. The same change passed on
x86-64, confirming that this was not execution of the writer. Single-codegen-
unit and whole-crate LTO experiments could stabilize selected symbols, but
caused much larger packed-array, String or scalar regressions elsewhere; custom
text-section experiments also failed the gate. None of those global profile or
layout workarounds is retained.

`fputcsv()` therefore lands behind the explicit `csv-write` feature. With the
feature disabled, the module and registration compile out. The final default
ARM64 image has the same `.text` size and the same addresses for
`run_quick_long_ops_loop` and `QuickStringSlotState::commit` as the accepted
parser-only baseline. A fresh order-balanced 20-pair default-runtime gate
reports +0.299%/-0.004%/-0.414%/-0.115%/-0.542% for scalar, packed array,
String, order and ledger, all within the +1% ceiling. With `csv-write` enabled,
18 stream scenarios and the dedicated encoder tests pass; the default 14
stream scenarios remain unchanged. The ARM64 matrices pass 195 default, 186
no-default and 291 all-feature library tests plus complete all-feature/all-
target compilation. No crate or external library was added; `Cargo.lock` is
unchanged. Resolving the compiler/layout boundary is required before promoting
the writer into the default function set.

### Opt-in exact CSV argument exceptions (2026-08-10)

The error-surface follow-up adds PHP's `ValueError` as an `Error` subclass with
the existing throwable constructor and `getMessage()` contract. A separate
checked CSV module validates the stream before later arguments, distinguishes
wrong values from closed or non-stream resources, accepts PHP-style weak
numeric length values and rejects non-numeric strings, and enforces the
single-byte separator/enclosure, empty-or-single-byte escape and nullable EOL
contracts.
`fgetcsv()` and `fputcsv()` now produce the PHP 8.5 exception classes and exact
messages covered by the differential probes, including the platform Long
range `0..=9223372036854775806`. Catching a `ValueError` through its `Error`
parent and calling its inherited method surface are tested directly.

Default-linking this entirely cold behavior changed the ARM64 quick dispatcher
again: its generated body became 276 bytes smaller, but a fresh 20-pair gate
regressed scalar by +3.867 percent and ledger by +5.094 percent. Moving helper
functions and the class registrar out of line did not change that machine
shape, so neither the smaller body nor the source-level cold designation was
treated as an optimization result. The failed default-linked version and its
custom-section experiment were rejected.

The accepted boundary adds an explicit `csv-errors` feature and makes
`csv-write` depend on it. The original default `fgetcsv()` handler remains
compiled byte-for-byte when the feature is absent; the checked handler and
`ValueError` class compile only when requested. The final default image again
has the exact parser-only `.text` size, surrounding symbols, hot-symbol
addresses and hot dispatcher size of checkpoint `6dc17b6`. The fresh full
20-pair gate records -0.198%/-0.579%/+1.961%/+0.709%/+0.160% for scalar,
packed array, String, order and ledger. The lone noisy String result was then
rerun independently at -2.450%; the other full-gate controls were already
within the +1% ceiling. Stream matrices pass 14 default scenarios, 15 with
`csv-errors` and 20 with `csv-write`. The ARM64 matrices pass 195 default, 186
no-default and 291 all-feature library tests plus complete all-feature/all-
target compilation. The implementation remains standard-library-only and
`Cargo.lock` is unchanged.
Making either CSV extension a production default still requires a compiler or
translation-unit boundary that passes the ordinary runtime gate on both
architectures.

### Opt-in final-alias resource ownership (2026-08-10)

The next resource-lifetime slice implements automatic backend release after
the last PHP resource alias disappears. With `resource-lifetime`, the 16-byte
`Value` stores a raw pointer to a standard-library `Rc<ResourceHandle>`; the
handle carries request scope, stable id and an indirect close callback.
Resource clone/drop participates in the existing owned-value path, including
small-frame bitmaps, large-frame scans, closure captures and guarded raw-copy
fast paths. Explicit `fclose()` still invalidates every alias immediately and
later handle drops cannot close the backend twice. Registry removal precedes
payload destruction, which also makes nested resource destruction re-entrant
without retaining a thread-local mutable borrow.

Linking that lifecycle into the default build retained the exact 16-byte
`Value` and total text size, but changed whole-program placement. A fresh
20-pair ARM64 gate against `f5d4e68` measured -1.081%/+0.028%/+0.462%/-0.670%
and +2.102% for scalar, packed array, String, order and ledger. The ledger
failure rejects the default-linked form. Moving the handle into an independent
module and calling the registry through an indirect callback removed the
direct ownership/registry dependency, but did not justify weakening the gate.

The accepted `resource-lifetime` feature compiles every ownership change out
of default builds. Default release returns to the exact 2,818,048-byte
`__TEXT` and the monitored `run_quick_long_ops_loop`/String commit addresses of
`f5d4e68`. The pinned x86-64 gate records
+0.106%/+0.378%/-0.274%/-0.549%/-0.191% for scalar, packed array, String,
order and ledger. Both hosts pass 195 default and 186 no-default tests;
all-feature coverage is 296 on ARM64 and 321 on x86-64. Fourteen stream
scenarios pass in both lifecycle configurations and all-feature/all-target
compilation succeeds. The slice is implemented only with `std`; no dependency
was added and `Cargo.lock` remains byte-identical.

### Opt-in bulk stream reads (2026-08-10)

Phase 5 now exposes `stream_get_contents()` behind the independent
`stream-contents` feature. It matches the probed PHP 8.5 length and offset
contract: `null` and `-1` read to the end, zero performs no read, positive
lengths are upper bounds, every negative offset retains the current cursor and
non-negative offsets seek from the start. Memory, spilled temporary and real
file backends share the same implementation. Unreadable streams return
`false`; invalid, closed and non-stream arguments produce the covered exact
`TypeError`/`ValueError` classes and messages.

The backend reads through a fixed 8 KiB stack chunk and grows the result with
fallible incremental reservations, so a user-supplied length neither creates
an equally large temporary buffer nor forces eager allocation. Checked stream
arguments and weak integer conversion moved into a private shared module used
by both CSV and bulk-read handlers. `ValueError` registration is correspondingly
owned by the internal `value-errors` feature, which `csv-errors` and
`stream-contents` imply. This refactor removes the duplicated validation path
without linking it into ordinary builds.

The default ARM64 release remains at the exact 2,818,048-byte `__TEXT` size and
the monitored hot-symbol addresses of `f5d4e68`; its lockfile hash is also
unchanged. On x86-64, text/data/bss sizes and both monitored addresses likewise
match the exact baseline. The first full pinned 20-pair x86 gate measured
-0.020%/+0.049%/-0.241%/-0.267%/+1.933% for scalar, packed array, String,
order and ledger. Because the lone ledger result contained large bidirectional
system outliers while static layout was exact, its independent 20-pair rerun
was required and passed at +0.571%.

ARM64 passes 195 default, 186 no-default and 299 all-feature library tests;
x86-64 passes 195, 186 and 324 respectively. Stream matrices pass 14 default,
17 `stream-contents`, 15 `csv-errors`, 20 `csv-write`, 14
`resource-lifetime` and 23 all-feature scenarios, including a real-file offset
read. Complete all-feature/all-target compilation passes on both hosts. The
slice uses only `std`; no crate was added and `Cargo.lock` remains
byte-identical.

### Opt-in bounded stream-to-stream copies (2026-08-10)

The next Phase 5 slice exposes `stream_copy_to_stream()` behind `stream-copy`.
Differential PHP 8.5 probes establish its distinct cursor contract: omitted,
zero and negative offsets keep the current source position, while a positive
offset seeks absolutely before copying, including for a zero length. `null`
and every negative length copy to EOF; an exact non-negative limit stops
without probing EOF. Read or write failure returns `false`, and the covered
wrong-type and closed-resource cases reproduce the exact `TypeError` classes
and messages for `$from`, `$to`, `$length` and `$offset`.

The handler owns one fixed 8 KiB stack chunk and never allocates a buffer based
on the requested length. It borrows the request resource registry only for one
source read or destination write at a time, so it neither holds two mutable
registry borrows nor requires a second resource API. Short writes are completed
before the next read; a write failure preserves PHP's already-advanced source
cursor. Using the same resource for source and destination follows the same
sequential cursor behavior. The shared checked-argument helper now accepts an
argument index and parameter name, removing a second validation path without
changing the existing CSV or bulk-read surfaces.

The default ARM64 release still has the exact 2,818,048-byte `f5d4e68`
`__TEXT`, monitored hot addresses and lockfile hash. X86-64 text/data/bss and
the same hot addresses also remain exact. Its fresh pinned 20-pair runtime gate
passes at -0.797%/-0.138%/-0.795%/+0.309%/-0.332% for scalar, packed array,
String, order and ledger.

ARM64 passes 195 default, 186 no-default and 299 all-feature library tests;
x86-64 passes 195, 186 and 324. Seventeen `stream-copy` scenarios cover
memory, spilled temporary and real files, a 20,000-byte multi-chunk transfer,
same-resource copying, offsets, exact EOF and independent read/write failures.
The combined stream matrix passes 26 scenarios and complete all-feature/all-
target compilation passes on both hosts. No crate or external buffering
library was added and `Cargo.lock` remains byte-identical.

### Opt-in bounded `file_get_contents()` bridge (2026-08-10)

The existing one-argument `std::fs::read` implementation remains the default
handler. The `file-contents` feature replaces only its registration with a
five-argument handler in a dedicated `stdlib/file_contents.rs` module. That
handler reuses `PhpStream` and the fixed-chunk `read_contents` backend without
enabling the PHP-visible `stream-contents` feature, so the file and stream
surfaces remain independently selectable.

PHP 8.5 differential probes define the expanded contract. Positive offsets
seek from the start, negative offsets seek from the end, positions beyond EOF
return an empty string and positions before the start return `false`. A null
length reads the remainder, zero returns an empty string after seeking and a
negative length raises the exact `ValueError`. Weak scalar conversions, named
arguments, `file://`, validation order and exact type errors for all five
parameters are covered. A non-context resource reproduces PHP's specific
Stream-Context `TypeError`; RPHP does not yet create valid context resources,
and `use_include_path=true` currently uses the ordinary current/absolute path
resolver because no configurable include path exists.

The shared backend grows its result incrementally behind one 8 KiB stack
chunk, rather than allocating the file's reported or requested size eagerly.
The common weak-long/error helpers now have stdlib-scoped visibility, while
stream-resource validation remains compiled only for the features that use it.
The 9k-line composition root retains the original default handler body
unchanged and moves all expanded policy into the new domain file.

Default ARM64 remains the exact 2,818,048-byte `f5d4e68` `__TEXT` image with
identical monitored addresses and lockfile. X86-64 text/data/bss and hot
addresses are also exact. Its pinned 20-pair runtime gate passes at
-0.489%/-0.414%/-0.400%/-0.084%/+0.431% for scalar, packed array, String,
order and ledger.

ARM64 passes 195 default, 186 no-default, 198 `file-contents` and 299
all-feature library tests; x86-64 passes the same first three counts and 324
all-feature tests. Filesystem E2E coverage passes one default and three
expanded scenarios, including a 20,000-byte multi-chunk file. The 26 combined
stream scenarios and all-feature/all-target compilation pass on both hosts.
No crate was added and `Cargo.lock` remains byte-identical.

### Opt-in bounded `file_put_contents()` bridge (2026-08-10)

Phase 5 now complements bounded file reads with the `file-write` feature. The
old two-argument `std::fs::write` handler remains the default; the feature
selects a four-argument handler in the dedicated
`stdlib/file_contents/write.rs` child module and exposes the standard
`FILE_USE_INCLUDE_PATH`, `LOCK_EX` and `FILE_APPEND` constants. The parent
file-content module is reduced to 204 lines of read policy and shared frame
helpers, while the write domain owns its 232-line validation and transfer
policy.

PHP 8.5.9 differential probes cover replacement, append, exclusive locking,
unknown flag bits, weak scalar flags, ordered scalar arrays, readable stream
sources, named arguments, `file://` and exact errors for filename, flags,
context and invalid/closed stream data. Typed flags and contexts validate
before the path; an invalid stream source also precedes the empty-path error.
Valid Stream-Context resources and configurable include-path search remain
future substrate. Non-stringable object/closure warning details are not yet a
complete PHP error surface and conservatively return `false` after the file is
opened.

The dependency-free transfer path never creates one combined record. ASCII
strings write directly, non-ASCII byte mapping uses an 8 KiB stack chunk,
array fields stream in insertion order and resource data advances through the
same fixed-size buffer. Every short write is completed or reported as
`false`, and byte-count overflow is checked. A source read failure preserves
already-written destination bytes and an already-advanced source cursor.

Exclusive locking uses stable `std::fs::File::lock`, not a locking crate. A
replacement opens with non-truncating create semantics, blocks for the lock,
then truncates and rewinds inside the protected interval. Append acquires the
same lock without truncation. Non-file wrappers reject `LOCK_EX`, matching the
covered PHP result. A direct backend test proves a competing descriptor cannot
take the lock until the writing stream drops.

Default ARM64 remains the exact 2,818,048-byte `f5d4e68` `__TEXT` image with
identical monitored addresses. X86-64 text/data/bss and addresses are exact,
and its fresh pinned 20-pair gate passes at
+0.001%/-0.126%/+0.446%/+0.500%/-0.618% for scalar, packed array, String,
order and ledger. ARM64 passes 195 default, 186 no-default, 196 `file-write`
and 300 all-feature library tests; x86-64 passes 195/186/196/325. Filesystem
E2E passes 2 default, 5 writer and 7 combined scenarios; 26 stream scenarios
and complete all-feature/all-target compilation pass on both hosts. The slice
adds no crate, and `Cargo.lock` remains byte-identical.

### Opt-in streaming `file()` line arrays (2026-08-10)

The next bounded file slice adds `file-lines` while preserving the old
one-argument `std::fs::read` handler in default builds. The expanded
three-argument handler is isolated in `stdlib/file_contents/lines.rs` and
reuses `PhpStream::read_line`; one retained `Vec<u8>` crosses arbitrary 8 KiB
backend chunks, and each completed line is published directly to the result
array. There is no parallel buffered-stream layer or external line-reading
crate.

Differential PHP 8.5.9 coverage defines the flag interaction precisely.
`FILE_IGNORE_NEW_LINES` removes LF and an immediately preceding CR;
`FILE_SKIP_EMPTY_LINES` tests after that removal. Therefore skip-empty by
itself retains newline-only physical records, while the combined flags omit
them without dropping space, tab or `"0"` records. Empty files return an empty
array, a final unterminated line is retained and unknown flag bits raise the
exact PHP `ValueError`.

Typed flag and context validation occurs before invalid flag values. A stream
resource is then rejected as a non-Context resource, followed by the empty-path
check. Current, absolute and `file://` paths are covered; true include-path
search and valid Stream-Context resources still require shared substrate. A
separate pre-existing VM issue with adjacent named arguments inside nested
calls surfaced during testing and was deferred to the following independent
call-dispatch checkpoint rather than being mixed into this filesystem slice.

Default ARM64 codegen remains the exact 2,818,048-byte `f5d4e68` `__TEXT` and
monitored addresses. X86-64 text/data/bss and addresses are exact, and its
fresh pinned 20-pair gate passes at
-0.095%/+0.197%/+0.005%/-1.697%/+0.404% for scalar, packed array, String,
order and ledger. ARM64 passes 195 default, 186 no-default, 195 `file-lines`
and 300 all-feature library tests; x86-64 passes 195/186/195/325. File E2E
passes 3 default, 5 line-feature and 10 combined scenarios; 26 stream scenarios
and all-feature/all-target compilation pass on both hosts. No dependency was
added and `Cargo.lock` remains byte-identical.

### Named argument frame initialization and nested `__invoke` (2026-08-10)

The deferred VM issue was stale call-frame storage, not `file()`. Positional
sends overwrite their complete prefix, so frame allocation deliberately does
not clear it. A first named send can instead leave holes and previously read
those stale values during duplicate and required-argument checks. Both ordinary
and precompiled call emission now store the source argument position in
`SendNamed.extended_value`; one outlined cold helper keeps the positional
prefix and initializes the rest of the declared public signature to `Undef`.
The common positional frame and send paths remain unchanged.

Dynamic object calls had a second coupling: one global pending `$this` could be
consumed by a nested argument call, while all named destinations already used
the method signature's hidden receiver offset and were shifted again at
`DoFcall`. The existing `Option<Value>` side-state now contains a private packed
stack of call-frame/receiver pairs without changing `ExecutorGlobals` layout.
The first named send binds `$this` and shifts only preceding positional values;
an `Undef` receiver marker keeps dynamic calls on the canonical full-call path
until completion. Error cleanup and coroutine context exchange own the same
single side-state value.

The first direct map implementation was rejected after ARM64 ledger measured
+1.195% and pinned x86 order measured +2.003%. A smaller vector revision was
also rejected. The admitted representation restores exact ARM64 `__TEXT`,
quick-loop and string-commit addresses. On Linux the named helpers, bitmap-mark
slow path, interrupt handler and post-loop object recorder occupy a dedicated
0x988-byte executable cold section; this restores the exact x86 quick-loop,
array-push, region-entry and array-kernel addresses that the first build moved
by 0x990.

The final 20-pair ARM64 gate is
-0.088%/-0.045%/-0.386%/-2.942%/-0.382%; pinned x86-64 is
+0.092%/+0.291%/-0.753%/-0.192%/-0.209% for scalar, packed array, String,
order and ledger. The named E2E target passes 49 scenarios on both hosts.
Library matrices pass 195/186/300 on ARM64 and 195/186/325 on x86-64; file,
stream, coroutine-context and all-target checks are clean. No external crate,
Cargo feature or lockfile change is involved.

### Executor call-path ownership split (2026-08-10)

The refactoring interlude reduces `src/vm/execute.rs` from 6,917 to 1,877
physical lines without introducing a second executor or a Rust module ABI
boundary. Five contiguous implementation domains now live in
`frame_runtime.rs`, `scalar_calls.rs`, `object_calls.rs`, `composed_calls.rs`
and `call_frames.rs`. Private `include!` expansion keeps their exact item
order, parent-module visibility and existing symbol identities. The original
5,045-line sequence is mechanically unchanged apart from ownership comments.

This split specifically separates frame writes and cleanup from typed scalar,
object and composed call evaluation. A future call or frame change can now be
reviewed against one ownership domain instead of crossing the quick-loop
composition root. It adds no indirection, allocation, dispatch or dependency.

Default/no-default/all-feature library matrices pass 195/186/300 on ARM64 and
195/186/325 on x86-64. Named-call coverage remains 49 scenarios, coroutine
context coverage remains six active scenarios, and all-feature/all-target
compilation passes on both hosts. ARM64 retains the exact 2,818,048-byte
`__TEXT` and monitored addresses; its 20-pair gate is
+0.302%/-0.190%/-0.454%/-1.697%/+0.156%. X86-64 keeps the exact total reported
binary section size while GNU `size` trades 192 bss bytes for 192 text bytes
and the monitored group moves uniformly by `0xc0`; the pinned gate passes at
-0.290%/+0.285%/-0.822%/-0.204%/-0.252%. Cargo files and external libraries
remain unchanged.

### Opt-in Stream Context substrate checkpoint (2026-08-10)

The next compatibility slice adds a valid Stream Context substrate behind the
opt-in `stream-context` feature. It introduces the request-owned
`stream-context` resource plus `stream_context_create()`,
`stream_context_get_options()` and `stream_context_get_params()`. Wrapper
options are normalized into nested PHP arrays; parameters retain a validated
`notification` callback. The feature-only `fopen()`, `file_get_contents()`,
`file_put_contents()` and `file()` paths accept this resource. PHP uses the
Context for the open operation but gives the resulting stream independent,
initially empty Context state.

PHP 8.5.9 differential tests lock the covered weak types, validation order,
resource-kind errors, option shape and getter results. Mutation APIs are added
in the independently measured follow-up below; wrapper-specific interpretation
of stored options and include-path search remain unclaimed. The implementation
reuses the existing callback, array, resource and stream owners and adds no
external crate; `Cargo.lock` remains unchanged.

With the feature disabled, ARM64 retains the exact 2,818,048-byte `__TEXT` and
monitored addresses. Its noisy full 20-pair run ended at
-2.007%/-2.011%/+0.273%/+0.491%/+1.610%; exact static layout and wide timing
outliers required the isolated ledger gate, which passed at +0.347%. X86-64
retains the post-refactor 3388067/51816/3048 text/data/bss, 0x988-byte
`.rphp_cold` and monitored hot addresses. Its pinned gate passes at
-0.287%/+0.217%/-0.693%/-0.694%/+0.049%. Library matrices pass 195/186/300 on
ARM64 and 195/186/325 on x86-64, with 16 feature-only stream, 28 all-feature
stream and 11 all-feature file scenarios plus all-target compilation on both
hosts.

### Mutable Stream Context state checkpoint (2026-08-10)

The follow-up implements `stream_context_set_option()`,
`stream_context_set_options()` and `stream_context_set_params()` for both
Context resources and ordinary streams. Wrapper options merge by string key;
numeric inner keys are ignored, invalid outer shapes retain PHP's exact
ValueError and weak wrapper/option strings preserve the probed null and omitted
argument distinctions. `set_params()` recognizes a validated `notification`
callback plus nested `options`; PHP's observable partial-update order is kept
when a valid callback precedes an invalid options shape.

The original 628-line implementation is divided into a 374-line create/get/open
module and a 301-line mutation child module. The state owner remains one
`StreamContext`; the split adds no indirection, runtime dependency or default
registration. The benchmark gate can now reuse explicitly supplied executables
for paired reruns, avoiding compile-induced thermal drift without changing its
statistics.

ARM64 `__text` is byte-identical to checkpoint `7ec3224` at SHA-256
`feae31bd9f8de1ce4b08aaf5da5a106c28c61c6592ee91d6d0429cd35a528a2a`,
with exact 2,818,048-byte `__TEXT` and monitored symbols. Its historical batch
was thermally unstable at -1.168%/+1.439%/+1.459%/-5.998%/-1.068%; only UUID
and five line-metadata bytes differ outside identical executable code.

X86-64 retains exact 3388067/51816/3048 text/data/bss, `.rphp_cold` and hot
addresses. `.text` matches `7ec3224` at SHA-256
`66b53177492bbcc339cf7383a7770de330eed12d40b85e1e767b18db29096204`, and the
build-free pinned gate against that checkpoint passes at
+0.078%/-0.036%/-1.033%/-0.144%/+0.691%. The separately reproduced +5.077%
String drift against historical `f5d4e68` is present in both identical
checkpoint binaries and remains explicit follow-up performance work.

Library matrices pass 195/186/300 on ARM64 and 195/186/325 on x86-64. The
feature-only/all-feature stream targets pass 17/29 scenarios, file coverage
remains 11, and all-feature/all-target compilation passes on both hosts. No
feature, crate or lockfile change is involved.

The historical String follow-up was bisected to the executor ownership split.
Five new source-location identities add 192 read-only bytes and move the
unchanged x86-64 text body by `0xc0`; a 100-pair pre/post comparison reports
+4.806% only for the short String activation workload, while long append runs
show no steady-state kernel loss. Linker anchoring, read-only-section movement
and bounded pre-reservation were all rejected because their full gates moved
packed-array or order controls beyond one percent. The admitted cleanup only
shortens the five ownership filenames; it keeps their content and include order
unchanged, restores the accepted default section totals and symbol addresses,
and adds no allocator shortcut, linker policy or dependency. Its 40-pair gate
passes at +0.051%/+0.203%/-5.654%/-1.156%/-0.073%; a separate 100-pair String
confirmation measures -5.526%.

### Opt-in configurable include-path checkpoint (2026-08-10)

Phase 5 now turns the previously validation-only include-path flags into one
request-local filesystem policy behind `include-path`. The feature composes the
already independent Stream Context and bounded file features, registers
`get_include_path()` and `set_include_path()`, and applies the same ordered
resolver to `fopen()`, `file_get_contents()`, `file_put_contents()`, `file()`
and include/require opcodes. The RPHP default is `.`; a successful setter
returns the preceding value, while an empty weak string returns `false` without
mutation and embedded NUL bytes raise PHP's exact `ValueError`.

PHP 8.4.24 differential probes define the path contract. Absolute paths,
wrapper URLs and explicit `./` or `../` paths bypass the search. Other paths
select the first existing entry, so readers and existing-file writers use the
configured directory in order; a new write falls back to the originally
requested path. Empty list entries retain current-directory behavior and a
resolved regular-file stream reports the selected URI. The resolver uses only
`std::path` and `std::fs`. Its current value lives under a private namespace in
the existing request-owned state map, avoiding an
`ExecutorGlobals` layout change or a second global/TLS registry.

Default ARM64 and x86-64 `.text` are byte-identical to `c026124` at SHA-256
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`
and `12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`.
ARM64 keeps the exact 2,818,048-byte `__TEXT` and monitored addresses; x86-64
keeps 3,388,067/51,816/3,048 text/data/bss, the 0x988-byte `.rphp_cold` and
monitored addresses. Its pinned 20-pair gate passes at
-0.012%/+0.175%/+0.364%/-0.401%/+0.088%. A thermally unstable ARM64 batch had
one noisy ledger result despite identical code; 40-pair isolated array and
ledger repeats pass at -0.003% and +0.129%.

Library matrices pass 195/186/301 on ARM64 and 195/186/326 on x86-64. Two new
E2E scenarios cover every integrated path surface, include-once lookup, URI
identity, weak values and exact errors; the existing 11 file, 29 stream and 12
include scenarios remain green, and all-feature/all-target linking passes on
x86-64. The slice adds no crate, external library or lockfile change.

### Default Stream Context API checkpoint (2026-08-10)

The next measured Phase 5 slice completes the management side of PHP's default
Stream Context behind the existing `stream-context` feature.
`stream_context_get_default()` and `stream_context_set_default()` return one
stable request-owned resource and merge options into it. Mutating any alias is
visible through every later getter; unsetting all userland aliases does not end
the singleton's lifetime. Explicit contexts and ordinary stream-local context
state remain independent.

PHP 8.4.24 differential probes lock the less obvious update order. Valid
wrapper entries are retained even if a later entry has an invalid outer key or
non-array value, while numeric inner keys are ignored. This partial-publish
contract lives in the 165-line `context/default.rs` child and does not weaken
the full-shape validation used by context creation or ordinary setters. The
resource handle is stored in the executor's existing private request-state
namespace, including under `resource-lifetime`; no new global registry or
dependency is introduced. Default-option consumption by future wrapper
transports remains an independent compatibility slice.

The feature-off binaries remain exact. ARM64 `__text` is the same 0x244700-byte
body as `c026124` at
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`,
with a 2,818,048-byte `__TEXT`. X86-64 retains
3,388,067/51,816/3,048 text/data/bss and `.text` SHA-256
`12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`.
Build-free 20-pair gates report
+0.385%/-0.144%/+0.094%/+0.044%/+0.248% on ARM64 and
+0.035%/-4.891%/-0.838%/+0.280%/+0.348% on CPU-pinned x86-64. All positive
deltas remain below one percent; the noisy negative x86 array result is treated
only as gate evidence, not as an optimization claim.

ARM64 and x86-64 library matrices remain 195/186/301 and 195/186/326.
Stream-context-only coverage passes 19 scenarios and the all-feature surface
passes 31, with both `resource-lifetime` configurations covered. Complete
all-feature/all-target compilation passes on x86-64. `Cargo.toml` dependencies
and `Cargo.lock` are unchanged.

### Canonical include-path resolution checkpoint (2026-08-10)

Phase 5 now exposes `stream_resolve_include_path()` behind the already accepted
`include-path` boundary. PHP 8.4.24 probes define a reporting contract that is
deliberately stricter than open-time search: the first existing candidate is
returned as a canonical absolute path, symlinks and parent components are
resolved, empty include-path entries are skipped, and an empty filename can
name the first non-empty include directory. Absolute and explicit relative
paths resolve directly; local, case-insensitive `file://` and
`file://localhost` URIs are accepted; missing paths and other wrappers return
`false`.

The handler shares weak string and embedded-NUL validation with
`set_include_path()` but keeps canonical reporting separate from the
operational resolver used by file opens. This prevents the new empty-entry and
canonicalization rules from changing accepted include/read/write behavior.
Canonical reporting and its focused unit contract live in the 127-line
`include_path/report.rs` child; the request state and operational policy remain
in the 223-line owner. The implementation is built entirely from `std::path` and
`std::fs::canonicalize`; PHP 8.4's removed `restore_include_path()` is not
reintroduced, and no dependency or manifest surface changes.

Feature-off code is exact. ARM64 retains the 0x244700-byte `.text` SHA-256
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`
and 2,818,048-byte `__TEXT`; x86-64 retains
3,388,067/51,816/3,048 text/data/bss and `.text` SHA-256
`12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`.
Build-free 20-pair gates report
+0.339%/-0.213%/+0.494%/-0.491%/+0.196% on ARM64 and
+0.165%/-0.029%/-0.525%/+0.136%/-0.034% on CPU-pinned x86-64.

ARM64/x86-64 library matrices pass 195/186/302 and 195/186/327, with 201 tests
in the focused include-path build. Include-path E2E coverage grows to four
scenarios, while 31 all-feature stream
scenarios and complete x86-64 all-feature/all-target compilation remain green.
No crate, external library or lockfile change is introduced.

### Opt-in stream registry and locality checkpoint (2026-08-10)

The next Phase 5 slice introduces the dependency-free `stream-registry`
feature. `stream_get_wrappers()` reports only the integrated `php` and `file`
wrappers. `stream_get_transports()` and `stream_get_filters()` return empty
arrays because coroutine descriptors are not PHP Stream resources and RPHP has
no admitted filter chain. This truthful registry avoids promising APIs that
cannot be opened or composed through `fopen()`.

`stream_is_local()` follows the PHP 8.4.24 contract for strings and resources.
It distinguishes remote HTTP(S), FTP(S), data and remote-file URLs from local
paths, local file hosts, `php://`, local archive/compression schemes and the
unknown-wrapper fallback. Current file, memory and temporary stream resources
are local; closed or non-stream resources raise the covered TypeError. Weak
scalar/array conversion is retained. A small feature-only VM bridge reuses the
existing magic-method executor for Stringable objects, while stdClass/Closure
conversion raises PHP's exact covered Error.

The registry and locality policy live in the focused 139-line
`streams/info.rs` module. The default binary does not compile its registration,
policy or magic-call bridge. ARM64 therefore retains the exact 0x244700-byte
`.text` SHA-256
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`
and 2,818,048-byte `__TEXT`; x86-64 retains
3,388,067/51,816/3,048 text/data/bss and `.text` SHA-256
`12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`.

Build-free 20-pair gates report
+0.558%/-0.589%/+0.304%/-0.891%/-0.272% on ARM64 and
-0.191%/+0.062%/-0.892%/-1.806%/+0.216% on CPU-pinned x86-64. The x86 order
samples were noisy and their negative aggregate is not an optimization claim;
all positive controls remain below one percent.

ARM64/x86-64 library matrices pass 195/186/303 and 195/186/328. The focused
feature passes 196 library and 16 stream scenarios; all-feature stream coverage
grows from 31 to 33, and all-feature/all-target compilation passes on both
hosts. The feature adds no crate, external library or `Cargo.lock` change.

### Arbitrary stream-line checkpoint (2026-08-10)

Phase 5 now exposes `stream_get_line()` behind the independent `stream-line`
feature. PHP 8.4.24 probes establish that length zero means unbounded reading,
a positive limit counts all consumed bytes including a matched ending, and a
matched ending is consumed but excluded from the returned string. Empty,
multi-byte, overlapping and NUL endings, exact cursor position, EOF identity,
weak scalar conversion, Stringable objects, closed streams and write-only
failure are covered.

The stream backend implementation lives in the 116-line
`stream/get_line.rs` child and performs fixed 8 KiB reads. A dependency-free
KMP prefix table keeps self-overlapping and long endings linear and carries
partial matches across chunk boundaries. Surplus bytes are returned to the
seekable backend, preserving `ftell()`, later reads and writes without a hidden
buffer. The 121-line `streams/line.rs` child owns PHP-visible validation,
conversion and exact covered errors; its object conversion delegates to the
existing magic-method executor.

Feature-off ARM64/x86-64 text stays byte-identical to `c026124` at
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`
and `12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`,
with exact 2,818,048-byte ARM64 `__TEXT` and 3,388,067/51,816/3,048 x86-64
section totals. ARM64's 20-pair gate passes at
-1.958%/-0.448%/+0.038%/+0.982%/-0.886%; a 40-pair repeat of its noisy order
control settles at -0.511%. CPU-pinned x86-64 passes at
+0.603%/-0.105%/-0.967%/-0.740%/+0.181%.

ARM64/x86-64 library matrices pass 195/186/305 and 195/186/330. The focused
feature passes 197 library and 16 stream scenarios, all-feature stream coverage
reaches 35, and all-feature/all-target compilation passes on both hosts. The
feature adds no crate, external library or `Cargo.lock` change.

### Writable stream truncation checkpoint (2026-08-10)

The next dependency-free Phase 5 slice exposes `ftruncate()` behind the
independent `stream-truncate` feature. PHP 8.4.24 probes define non-negative
weak integer conversion, exact invalid/closed-resource errors, writable-policy
failure, unchanged cursor and EOF state, zero-filled growth and shrink/grow
behavior for regular files, `php://memory` and both in-memory and spilled
`php://temp` storage.

PHP memory streams retain a subtle split identity after truncation below the
logical cursor: `ftell()` remains unchanged, but subsequent writes append at
the shortened buffer end while advancing the original logical position. A
temp stream still in memory follows that rule; a temp stream already spilled
to a file and a regular file instead preserve file semantics and create a zero
gap. Feature-only state records only this post-truncation condition and is
cleared by an explicit seek. Growing temp storage beyond its memory ceiling
uses the existing private spill path before resizing.

The 42-line `stream/truncate.rs` child owns backend resizing and one shared
fallible memory helper; the 69-line `streams/truncate.rs` child owns PHP-visible
validation and dispatch. Memory growth reserves fallibly before zero fill, file
growth delegates to `File::set_len`, and neither path changes EOF or the cursor.
No buffering crate, filesystem helper or other dependency is added.

Feature-off ARM64/x86-64 text stays byte-identical to `c026124` at
`98860c8ab367e6c392b4978d316311aceb16c7acb38a79455270a6746ee6da34`
and `12df229c942df4203be1a5df086bf920da492251f5494b9a158dd170e68e2584`,
with exact 2,818,048-byte ARM64 `__TEXT` and 3,388,067/51,816/3,048 x86-64
section totals. ARM64's 20-pair gate passes at
+0.243%/-0.332%/+0.488%/+0.310%/-0.101%; CPU-pinned x86-64 passes at
-0.239%/+0.157%/-1.395%/+0.154%/+0.132%.

ARM64/x86-64 library matrices pass 195/186/307 and 195/186/332. The focused
feature passes 197 library and 16 stream scenarios, all-feature stream coverage
reaches 37, and all-feature/all-target compilation passes on both hosts. One
opt-in feature is added, with no crate, external library or `Cargo.lock` change.

## Interphase 5.5: dual-runtime PHP generics

After the current Phase 5 stream slice, add two independently measurable
generic runtimes: `php-generics-erased` and `php-generics-reified`. Enabling
either admits the shared generic source syntax; a default build admits neither
and explicitly rejects generic syntax rather than silently reinterpreting it.
The AST model, compact interned metadata and Reflection model are permanent
RPHP internals compiled in every build. Both runtime branches consume this one
canonical representation:

```text
interned metadata and Reflection
              |
       +------+------+
       |             |
 bound-erased     reified
    runtime        runtime
```

An all-features build contains both capabilities for differential tests, while
the individual builds remain the authoritative performance comparison. The
semantic baseline is the latest published
[PHP RFC: Bound-Erased Generic Types, version 0.22](https://wiki.php.net/rfc/bound_erased_generic_types)
and its linked [reference implementation](https://github.com/php/php-src/pull/21969).
The RFC is marked Declined as of 2026-08-10, so RPHP must describe this as an
experimental language extension rather than implemented PHP compatibility. If
a successor RFC is published before this interphase starts, record its semantic
delta and deliberately rebase the plan instead of silently mixing proposals.

Implement the RFC surface in measured layers. The parser/AST layer covers type
parameters on classes, interfaces, traits, functions, methods, closures and
arrow functions; `:` bounds, defaults, `+T`/`-T` variance, nested generic type
arguments, `::<...>` turbofish use sites and the 127-argument cap. The metadata
and link layer preserves the pre-erasure form, substitutes generic ancestors,
checks inheritance arity and bounds, applies parametric LSP and validates
variance. Reflection follows only after the internal metadata identity and
inheritance rules are stable.

Execution follows bound erasure: every parameter becomes its declared bound or
`mixed`, ordinary calls use the existing call path, and generic arguments do
not become per-object or per-`Value` payload. Generic declarations without a
turbofish must compile to the same runtime instruction stream as their erased
equivalent. Explicit turbofish sites may emit one validation
instruction before the ordinary call/new/attribute path; canonical argument
tuples and successful arity/bound results are cached at that site. Quick plans
and the JIT may consume a proven bound or stable argument tuple as optimization
metadata, but every specialization needs an exact guard and canonical
deoptimization path.

The reified branch preserves the canonical type-argument tuple at explicit
turbofish construction/call sites and can therefore enforce substituted
parameter, return and property contracts. Its binding lives in a shared
sidecar keyed by stable object/class or activation identity; it must not widen
the 16-byte `Value`, `FunctionCommon` or ordinary frame. Reified-only checks
and storage disappear from erased-only builds.

Performance is an admission requirement, not follow-up polish:

- Feature-off ARM64/x86-64 builds must reject generic syntax while retaining
  the generic engine internally. Ordinary-program bytecode, hot runtime symbol
  layout, 16-byte `Value` layout and runtime results must remain exact; the
  whole executable-text hash is recorded but is no longer an identity gate.
- With either generics runtime enabled, programs that declare or use no generics must
  retain bytecode identity, perform no generic side-table lookup in ordinary
  dispatch and stay inside the existing one-percent runtime gate.
- Generic declarations called without turbofish must add no steady-state heap
  allocation or per-call generic branch and must remain inside one percent of
  the equivalent manually erased declaration.
- A warmed explicit turbofish site must allocate nothing after its metadata is
  resolved and target at most five-percent overhead versus the same call with
  an already-proven ordinary bound check. Report cold metadata/link cost and
  warm dispatch cost separately.
- Metadata is interned per declaration/use site, never copied into each object
  or frame. Monomorphization is excluded from the first implementation; any
  later selective specialization must prove an end-to-end win without code-size
  explosion.

### Dual-runtime generics vertical checkpoint (2026-08-10)

The first executable slice now covers turbofish calls to named functions,
classes, instance/static methods, dynamic string callables and closures. One
interned declaration/use-site graph feeds both runtimes and the built-in
`ReflectionFunction`, `ReflectionClass` and `ReflectionMethod` views. The
default binary keeps this machinery and Reflection API compiled in but rejects
declaration, type-application and turbofish syntax. No crate or lockfile change
was added.

Erased-only sites validate arity and bounds, then cache declaration identity in
the existing 16-byte per-opline inline-cache slot. Reified-only sites keep a
LIFO binding sidecar at the end of `ExecutorGlobals` and enforce substituted
argument and return contracts without changing `Value`, `FunctionCommon`, an
object or `ExecuteData`. Putting the feature-only sidecar last preserves every
pre-existing executor-field offset.

A statically named same-unit call whose arity/bounds are proven and whose
substituted runtime signature equals bound erasure emits no generic runtime
instruction. This is semantic proof, not a benchmark-pattern exception: the
compiler compares every parameter and return runtime shape after default
substitution. Unknown, polymorphic, invalid or stricter-reified sites retain
the canonical checked path.

Three permanent five-million-call workloads separate manual erasure, an
ordinary generic declaration and `::<int>`. On the ARM64 reference host, 31
order-paired release ratios after the static proof are:

- erased ordinary generic/manual erasure: +0.681%;
- erased proven turbofish/ordinary generic: -0.268%;
- reified ordinary generic/manual erasure: -0.122%;
- reified proven turbofish/ordinary generic: +0.211%;
- reified feature binary/default binary on the non-generic manual control:
  -0.178%.

The same focused matrix passes 11 reified and 10 erased scenarios on ARM64,
including deliberately different erased/reified behavior, nested bindings,
bound/default/arity failures and Reflection. The remaining performance work is
the genuinely stricter case such as unbounded `T` reified as `int`, plus dynamic
and cross-unit sites; those may not use the zero-opcode proof and must reach the
warm-site gate through compact checked dispatch.

Cross-file linking is also executable rather than a metadata placeholder.
Runtime-compiled `include` units merge symbols, declarations and use sites into
the executor's canonical intern table, then relocate only their cold generic
validation operands. A permanent collision scenario uses local use-site zero
in both the root and included units with different bounds; erased-only and
reified-only runs validate the correct tuple and Reflection observes the
included declaration. Feature-off ARM64 fat-LTO code grows by 16 bytes. In 31
order-alternated pairs, the unrelated manual-erasure control moves -9.099%; the
large favorable movement is recorded as code-layout noise rather than an
optimization claim, while establishing that this cold linker caused no hot
path regression.

Reified class construction now has stable per-instance identity without an
object payload. A feature-only weak sidecar is populated before constructors,
copied on clone and read by substituted property guards and
`ReflectionObject::getGenericArguments()`. Typed declared and promoted
properties remain interned declaration metadata. Bound-erased writes check the
parameter bound; unbounded `T` proves to `mixed` and becomes the original
write-safe property slot after the first link. A real guard reuses the existing
16-byte property IC for class ID, slot and declaration ID. Reified repeated
receivers additionally use a weak L0 identity cache, avoiding a hash-table
lookup without permitting recycled-address aliasing.

Three permanent five-million-write workloads compare an untyped manual slot,
unbounded `T` and `T : int`. On ARM64, 31 order-alternated release pairs report
the following paired medians:

- erased unbounded/mixed property versus manual: -1.154% (no-op proof; the
  ratio of independent medians is +0.569%);
- erased checked `int` bound versus manual: +79.851%, or approximately
  8.52 ns per enforced write;
- reified `Box<int>` versus manual: +89.073%, or approximately 10.42 ns per
  substituted per-instance write.

The initial correct slow implementation took about 0.99/1.05 seconds for five
million erased/reified writes locally. Declaration/slot caching and the weak L0
reduce that to roughly 0.07/0.08 seconds on the same host. The percentage
against an unchecked direct slot remains intentionally visible; the absolute
guard cost and the removal of only semantically empty work are the admission
criteria for this slice.

Cold metadata growth initially moved the otherwise byte-identical feature-off
dispatch entry from a 256-byte boundary and produced a repeatable +12.17%
microbenchmark regression. RPHP now asks LLVM for one-cache-line (64-byte)
function alignment on ARM64 and x86-64. The final feature-off property control
is -0.612% in 31 order-alternated ARM64 pairs, while text grows by about 57 KiB
(1.6% versus the same unaligned revision). This build-level stability rule is
kept because future cold compatibility work must not repeatedly perturb the
large measured dispatch loops.

Direct generic inheritance now reaches the link layer. The parser retains
pre-erasure arguments on class/interface `extends`, `implements` and trait
`use`, while the ordinary `ClassDef` continues to carry only erased names.
Those bindings merge through the same executor-wide symbol graph as included
declarations. Class registration validates omitted arguments as arity zero,
defaults, concrete bounds and forwarded parameters by comparing the forwarding
parameter's own erased bound (bound-on-bound). This is cold registration work:
no field was added to an object, frame, `Value`, instruction or inline cache.
Declaration-site variance is also validated from the interned graph. Parameter
and return positions, read/write versus readonly/promoted properties,
bounds/defaults, static class context, inheritance forwarding and nested
generic variance composition all share one polarity walk; merged include units
re-run it when an ancestor was previously unresolved.

The first parametric-LSP slice now preserves each class-like method's
pre-erasure parameter and return types plus its required/variadic shape in the
same cold graph. Registration composes direct and transitive ancestor bindings,
substitutes every reachable class/interface/trait prototype, and rejects
incompatible staticness, arity, parameter contravariance or return covariance.
Variadic prototypes remain variadic and their substituted tail contract is
checked against added optional parameters as well as the implementation tail.
The same validation runs after include metadata is merged. It adds no runtime
method lookup, frame field or ordinary-call branch. Method-generic
alpha-renaming and deterministic diamond contract merging are now closed
below.

The receiver-specific runtime-signature slice is now executable in both
branches. A free bit in the existing method inline cache marks only methods
with a reified substitution or a stricter inherited link-time boundary.
Object-binding and declaration/use-site-plus-method L0 caches produce either a
fully substituted reified contract or a sparse bound-erased contract containing
only slots that differ from the executable parent ABI. Feature-only LIFO
sidecars carry it across the canonical call frame and validate fixed,
positional-variadic, named-variadic and return boundaries. An own override with
a widened non-parametric signature deliberately stops ancestor lookup. Nested
calls, caught exceptions, concrete/transitive descendants, concrete traits and
cross-unit inherited methods have permanent coverage. An explicit turbofish
alone still does not tighten an erased callee. No object, `Value`,
`FunctionCommon`, frame, instruction or inline-cache layout grows.

The initial correct reified method implementation took roughly 0.388 seconds
for five million `GenericMethodBox<int>::step(int): int` calls because every
call required a full frame. The final path proves that a scalar Long plan
guards every substituted argument and produces a checked Long result. A
successful proof executes frame-free and allocates no pending/active contract;
argument mismatch, non-Long return and overflow return to the canonical
generic checks. A second free IC bit records a stable linked Long-to-Long proof
for non-reifiable descendants, so their warmed success path does not even
materialize or clone the sparse contract. In 21 order-alternated ARM64 release
pairs, the permanent method benchmarks report:

- erased generic/manual paired median: -0.556% (0.048983/0.049162 seconds);
- reified generic/manual paired median: +0.463% (0.049852/0.049589 seconds).

The synchronized x86-64 host independently passes the same 21-pair gate:
erased generic/manual is -0.055% (0.065765/0.065790 seconds) and reified
generic/manual is +0.042% (0.066356/0.066457 seconds).

Method-generic identity is now preserved independently from the owning
class-like scope. The RFC's 127-parameter ceiling leaves the high bit of each
`u8` index free, so cold class-like method metadata uses that bit to distinguish
`<U, V>` positions from class `<T>` positions without adding another enum
variant or widening any hot representation. Names are diagnostic only:
registration compares method-generic arity, variance, substituted bounds,
defaults and complete positional parameter/return relationships by index. Thus
`<U, V>(U): V` and `<A, B>(A): B` are identical, while a swapped `<X, Y>(Y):
X` implementation fails Parametric LSP. Non-generic classes, interfaces and
traits with generic methods now retain the same metadata, and symbol relocation
keeps it exact across `include` units.

Runtime materialization deliberately orders the two scopes. It first
substitutes the ancestor/receiver class binding while preserving method-local
indices, then erases each method parameter through its own bound. Consequently
`Parent<T>::id<U : T>(U): U` linked by `IntChild extends Parent<int>` enforces
`int` in both runtime branches, and a reified `Parent<int>` gets the same
receiver-specific boundary. A pure method-local `U` does not create a receiver
contract; only a parameter whose bound actually reaches a class parameter does.
This work remains in the cold linker and existing sidecar/cache path: no
object, frame, `Value`, function, opcode or inline-cache layout changes, and no
JIT/native lowering changes.

Complete default, erased, reified and dual-feature `--all-targets` matrices pass
on ARM64 and x86-64. Library counts remain 197 in each single/default build,
while dual-feature coverage is 309 on ARM64 and 334 on x86-64; at this
checkpoint focused coverage is 25 generics plus 20 include scenarios in
reified/dual builds, 20 plus 19 in erased-only and 2 plus 12 in the default
build. Release order-paired method controls remain inside the one percent gate.
Balanced order-specific median ratios are:

- ARM64, 51 pairs: erased own/manual +0.271% (0.049802/0.049702 seconds) and
  reified own/manual -0.031% (0.049820/0.049772 seconds);
- ARM64, 21 pairs: erased concrete/manual +0.636% (0.049825/0.049503 seconds)
  and reified concrete/manual +0.399% (0.049717/0.049453 seconds);
- CPU-2-pinned x86-64, 51 pairs: erased own/manual +0.026%
  (0.066177/0.066153 seconds) and reified own/manual +0.023%
  (0.065513/0.065540 seconds);
- CPU-2-pinned x86-64, 21 pairs: erased concrete/manual +0.045%
  (0.066043/0.066001 seconds) and reified concrete/manual -0.118%
  (0.065054/0.065177 seconds).

Diamond inheritance now follows the RFC v0.22 merge rule rather than selecting
the first traversal path. All inherited generic method prototypes are
materialized together: parameter/write positions form a flattened union and
return positions form a flattened intersection. Inherited property storage
uses the same write-safe union. Compound members receive stable,
case-insensitive sort keys before interning and duplicate removal, so reversing
trait or interface order produces byte-for-byte identical method and property
contract shapes. `mixed` and `never` are handled as the corresponding absorbing
or identity elements. The ordinary type system now carries intersection hints
from parsing through generic substitution, executable hints, runtime checks and
LSP; the `&`/typed-reference ambiguity retains its existing parse boundary.

The canonical RFC shape is permanent coverage: two `Pipeline<T: object>`
parents bound as `Pipeline<Renderable>` and `Pipeline<Cacheable>` accept an
implementation parameter `Renderable|Cacheable` and require return
`Renderable&Cacheable`. The same merge is exercised through traits, properties,
reverse declaration order and separately compiled include units. Runtime
method checks consume one merged sidecar contract through the existing L0/IC
path; they do not walk ancestors per call. No object, frame, `Value`, function,
opcode or inline-cache layout changed, no dependency was added and no
JIT/native lowering changed.

Production-feature 31-pair order-alternated controls remain inside one percent
on both hosts:

- ARM64 erased diamond/premerged is -0.325% balanced and erased
  concrete/manual +0.259%; reified diamond/premerged is +0.246% and reified
  concrete/manual -0.132%; the reified/default manual control is -0.728%;
- CPU-2-pinned x86-64 erased diamond/premerged is -0.014% balanced and erased
  concrete/manual +0.258%; reified diamond/premerged is -0.079% and reified
  concrete/manual +0.067%; the reified/default manual control is -0.103%.

The RFC v0.22 Reflection inheritance view is now executable from
`ReflectionClass` and `ReflectionObject`. Parent-class and directly used-trait
lookups return one `list<ReflectionType>`; the parent-interface lookup returns
the RFC's plural `list<list<ReflectionType>>`, retaining a separate argument
set for every distinct diamond binding in inheritance traversal order. Invalid
parent/interface/trait targets raise `ReflectionException`. The view is built
from the linker graph, so defaults, forwarded child parameters, nested generic
arguments, unions/intersections and metadata merged after `include` need no
parallel representation.

Inheritance arguments are real `ReflectionNamedType`, `ReflectionUnionType`,
`ReflectionIntersectionType` or `ReflectionTypeParameterReference` objects.
Named types expose nested generic arguments, compound types expose their
members and every type has the pre-erasure string form. Permanent coverage
includes direct parent and trait bindings, two differently bound copies of one
interface, forwarded `U`, nested `Foo<int>`, an intersection argument, invalid
ancestor handling and a cross-unit diamond. The existing feature flags only
control syntax/runtime selection; this shared Reflection machinery remains
compiled into the default build.

The first cold-start measurement exposed the cost of growing fixed stdlib hash
tables while adding the new classes. `register_stdlib` now reserves the known
built-in envelope once; an executor that does not install stdlib remains lazy.
Against the pre-API refactor commit, final release controls report:

- ARM64: ordinary five-million-call method -0.017% balanced over 31 pairs;
  process startup +0.559% over 101 order-alternated batches of 20 starts;
- CPU-2-pinned x86-64: method -0.073% and startup +0.497% under the same
  gates.

No object, frame, `Value`, function/opcode/IC layout, external dependency or
JIT/native lowering changed. All four `--all-targets` configurations pass on
both architectures; focused dual/reified coverage is now 26 generics and 21
include scenarios, erased-only 21 and 20, and default remains 2 and 12.

The provisional generic-parameter arrays have now been replaced by the exact
[RFC v0.22 Reflection surface](https://wiki.php.net/rfc/bound_erased_generic_types).
`getGenericParameters()` returns final `ReflectionGenericTypeParameter`
objects with public `name`, position, variance, bound/default presence and
typed accessors, declaring entity and string form. Missing bounds/defaults
raise `ReflectionException`. `ReflectionGenericVariance` is a unit enum with
stable `Invariant`, `Covariant` and `Contravariant` singleton cases, including
identity through static case access. `ReflectionTypeParameterReference` now
has its public `name` and exact `getTypeParameter()` back-reference.

Method Reflection resolves the class-like and method-local generic scopes
separately, so a method parameter such as `W : V = V` points back to the
declaring `ReflectionMethod`, while `V : C` inside `Host<C>` points back to the
declaring `ReflectionClass`; neither is accidentally replaced by its erased
bound. Interfaces and traits are recognized by `ReflectionClass::isGeneric()`
through the shared class-like declaration index. Included metadata continues
to use the same relocated graph. The growing implementation was split again:
the registration/ancestor façade is 753 lines and the 525-line pre-erasure
parameter/type model lives in `stdlib/reflection/generic_parameters.rs`.

Final release controls against commit `e9893af` remain comfortably inside the
one-percent regression gate: the ordinary five-million-call method is -4.049%
on ARM64 and -1.720% on CPU-2-pinned x86-64 over 31 order-alternated pairs;
101 order-alternated batches of 20 empty starts are +0.162% and -7.132%,
respectively. The improvements are recorded only as an absence-of-regression
control because cold Reflection symbol layout can perturb the final binary;
no hot-path optimization is attributed to this slice. The final focused
matrix has 27 dual/reified generic scenarios, 22 erased-only scenarios and the
two syntax-disabled scenarios, plus 21 include scenarios. The full four-mode
all-target matrices pass on ARM64 and x86-64.

The RPHP reified extension `ReflectionObject::getGenericArguments()` now uses
that same structured `ReflectionType` model rather than provisional strings.
Its cold materializer consumes the object's canonical interned `ReifiedBinding`;
omitted defaults are substituted through the preceding effective arguments, so
`Pair<T, U = T>` instantiated as `Pair::<int>` reflects `int, int`, and nested
applications retain their own argument objects. Bound-erased and ordinary
unbound objects continue to return an empty list. No second runtime type graph
or per-object payload was introduced, and the cold binding no longer allocates
an unused owner-name copy.

Fresh release controls against `e9893af` remain within the gate. On ARM64 the
31-pair five-million-call method control is -6.827% and 101 paired batches of
20 empty starts are -0.037%; on CPU-2-pinned x86-64 the order-balanced results
are +0.484% over 101 method pairs and -4.880% over 101 startup batches of 20.
These are layout/no-regression observations, not optimization claims. The full
four-mode all-target matrix passes on both architectures. The x86 debug harness
uses an 8 MiB Rust test-thread stack because the pre-existing Ackermann test
overflows the default stack identically at the `e9893af` checkpoint; production
code and stack policy are unchanged.

Reified Reflection lifecycle coverage now proves that clones retain the exact
structured binding, ordinary non-turbofish instances remain unbound, and an
object constructed in an included unit resolves its relocated declaration,
use-site and substituted default through the executor's merged metadata graph.

The class-hierarchy and exception audit is complete as well. The stdlib now
models `Reflector`, abstract `ReflectionFunctionAbstract`, the Function/Method
parent relation and the RFC's `Reflector` contract for
`ReflectionGenericTypeParameter`. Ancestor accessors distinguish a valid
non-generic parent/interface/trait binding (an empty list) from a missing or
invalid target (`ReflectionException`). Adding the hierarchy exposed a stale
function-table envelope: capacity grew from 224 to 448 during every stdlib
installation. Reserving 256 entries selects the same final 448-slot table in
one allocation, and a permanent test now asserts that none of the four fixed
stdlib registries grows during registration.

Against `e9893af`, the exact final 101-pair release controls are -6.324% method
and +0.281% startup on ARM64, and +0.356% method/-5.366% startup on
CPU-2-pinned x86-64. These remain layout/no-regression observations. The full
four-mode all-target matrix passes on both architectures; dual/reified generic
coverage is 29 scenarios and erased-only 24.

`ReflectionFunction` now accepts closure values and exposes generic closure and
arrow-function declarations through the same RFC parameter objects as named
functions. It recovers the compiler-interned declaration name from the existing
resolved `UserFunction` pointer, so neither `Value`, `PhpClosure` nor the call
path gains a metadata payload. The checked pointer recovery now has one owner,
`PhpClosure::user_function`, shared by turbofish dispatch and a small separate
Reflection function-target unit. Closure parameter objects retain their
distinct declaration kind, including bounds, defaults and a working
declaring-entity round trip. Included closures prove metadata relocation,
ordinary closures return an empty generic view, and the feature-off build keeps
the internal Reflection model while rejecting only generic source syntax. The
four-mode all-target matrix passes on ARM64 and x86-64; dual/reified generic
coverage is now 31 scenarios and erased-only 26.

The final paired release control records both the immediate and stable
comparisons because this cold-only code change moves unrelated functions under
fat LTO. Against immediate parent `81cad28`, 101 pairs report ARM64 method
+5.252% and 20-process startup +0.015%, while CPU-2-pinned x86-64 reports
+0.092%/-0.618%. The ARM64 method delta is a text-layout shift: the handler is
never entered by that workload and the call path is unchanged. Against the
shared `e9893af` Reflection checkpoint, 201 pairs report ARM64
-1.255%/+1.252% and x86-64 +0.024%/-5.461% for method/startup respectively.
These remain layout/no-regression observations rather than optimization claims.

Reified function, method and closure calls now validate the complete variadic
boundary. Fixed parameters use the signature's canonical CV mapping; every
positional variadic slot and every pending named variadic value is checked
against the same interned binding before the call becomes active. The
bound-erased mode deliberately retains the RFC behavior: an unbounded `T`
still erases to `mixed`, including variadics. Permanent tests cover valid and
invalid function/method/closure calls, later invalid positional values, named
variadics and ordinary calls without turbofish.

The permanent one-million-call three-value variadic benchmark measures the
real array-packing call ABI plus all reified scope, argument and return checks.
In 61 order-rotated release triples, current reified generic/manual is +18.097%
on ARM64 (0.165479/0.139740 seconds) and +16.166% on CPU-2-pinned x86-64
(0.189798/0.164505 seconds), about 25-26 ns per call for the complete reified
delta. Against parent `b7f5695`, the new all-element validation is +3.650% on
ARM64 and -7.254% on x86-64; the opposite directions are recorded as fat-LTO
layout observations, not as an optimization claim. No ordinary-call, object,
frame, `Value` or function layout changed.

Explicit generic method calls now resolve metadata from the concrete method
body selected by runtime dispatch. The cold resolver follows the existing
`function_table` alias to its `FunctionCommon` pointer and recovers the single
class or trait that owns that body. It therefore covers inherited methods plus
trait-imported instance and static methods, and cannot accidentally fall
through from a selected non-generic override to a same-named generic ancestor.
The existing call-site declaration cache makes this pointer scan a first-hit
cost only. The warmed prefix returns before an explicitly cold, non-inlined
miss helper, keeping hierarchy recovery out of its register and stack plan.
Reified trait methods retain their substituted argument contract; the focused
matrix grows to 33 dual/reified and 27 erased-only scenarios.

The permanent five-million-call inherited-method turbofish benchmark records
the warm cache-hit path. Against `b7f5695`, a final order-balanced 20-pair
all-feature release gate is +0.247% on ARM64 (0.331424/0.331113 seconds) and
+1.505% on CPU-2-pinned x86-64 (0.486045/0.479224 seconds), within the explicit
five-percent warmed-turbofish ceiling. The corresponding 40-pair ordinary
method control is -5.276% on ARM64 and +0.980% on x86-64. The favorable ARM64
movement is treated as binary-layout noise, not an optimization claim. No
ordinary dispatch branch, runtime layout, dependency or JIT/native lowering
changed.

Class-context type applications now resolve through one canonical lexical
scope path. Direct and inherited `self<T>`/`parent<T>` retain their declaring
class, while trait bodies bind to the nearest class in the receiver hierarchy
that consumed the trait. The same rule covers explicit method-generic bounds,
properties, arguments and returns. A sparse feature-only class-ID sidecar is
created only for explicit signatures that actually contain a pseudo-type;
primitive turbofish sites do not grow their pending/active scope records.
Property and method L0 entries own the already-resolved scope, and object type
matching reads the immutable class name without a conflicting `RefCell`
borrow. Bound-erased linked methods discard named-application arguments before
ABI comparison, so `self<T>` does not create a redundant second outer-class
guard after erasure. The permanent pseudo-scope benchmark separates this
checked object/property path from its manually typed control.

Both hosts pass all four complete `--all-targets` matrices. Against exact
checkpoint `15f2f36`, 20 balanced release pairs put the ordinary generic method
at -0.261% ARM64 and -0.120% CPU-2-pinned x86-64. The warmed inherited
turbofish is +1.927% (0.337372/0.330994 seconds) and +2.744%
(0.504342/0.490871 seconds), within the five-percent gate. Twenty balanced
pseudo-scope pairs report 0.188390/0.123548 seconds on ARM64 and
0.247753/0.164126 seconds on x86-64 for two million combined method/property
iterations. The remaining 52.484%/50.953% delta is the real generic property
guard—about 32 ns and 42 ns per iteration—not inheritance traversal or a
redundant method sidecar; RPHP's manually typed property path does not yet
enforce that write contract.

Reified named applications now enforce their complete canonical argument
tuple instead of accepting only the erased outer class. Thus `Box<int>`
rejects a `Box<string>` and an ordinary unbound `Box`, while a reified child is
matched through its substituted generic ancestor binding and a zero-parameter
`IntBox extends Box<int>` uses its canonical link-time tuple. The same recursive
matcher covers explicit function/method arguments and returns, variadics,
substituted instance-method/constructor contracts and generic properties.
Named classes compare case-insensitively and union/intersection members are
order-independent. The compiler also retains the reified boundary opcode for
`Box<T>` -> `Box<int>` even though both executable PHP hints erase to `Box`.

The target object's tuple stays in the existing weak identity sidecar. A
feature-only one-entry L0 caches the effective direct/ancestor tuple and a
stable metadata-site proof; include-time metadata rebuilding explicitly
invalidates the address proof. Warm monomorphic checks neither traverse
inheritance nor allocate, and no `Value`, object, frame, function, instruction
or property-IC layout changes. The first correct cached ARM64 path measured
about 0.342 seconds for two million argument-plus-return calls; the final L0
reduces that to 0.283361 versus 0.160790 seconds for the otherwise identical
outer-class-only control. The real exact-tuple cost is therefore about 61.3 ns
per complete call, or 31 ns per guard. CPU-2-pinned x86-64 independently
records 0.360606/0.201027 seconds, 79.8 ns per call or 39.9 ns per guard. Against
exact checkpoint `fb29df2`, ordinary generic method and warmed turbofish
controls move -5.097%/-4.490% on ARM64 and -23.511%/-8.647% on x86-64; these
favorable movements are binary-layout observations, not optimization claims.
All four complete `--all-targets` matrices pass on both architectures; focused
generic coverage is now 35 scenarios in reified/dual builds and remains 28 in
erased-only.

Inheritance-site bound validation now resolves class pseudo-types before
subtype comparison. A class/interface ancestor's `self` remains that ancestor,
whereas a generic trait's `self` and `parent` bind to the consuming class and
its direct parent. Forwarded owner parameters retain their own lexical scope,
so two interned `self` nodes from different declarations cannot compare equal
by spelling alone. The linker can also prove `extends`/`implements` edges for
the class currently being registered before it enters the runtime class table.
An unresolved `parent` fails closed, and `static` is explicitly not
approximated as lexical `self`.

Permanent coverage includes valid and invalid class/trait `self` and `parent`
bounds plus forwarded owner bounds. All four complete `--all-targets` matrices
pass on ARM64 and x86-64; focused generic coverage is now 36 reified/dual and
29 erased-only scenarios. Against exact checkpoint `27cb47c`, twenty balanced
ARM64 pairs put the ordinary generic method at -0.019% and warmed turbofish at
+3.732%, below its five-percent ceiling. CPU-2-pinned x86-64 reports +0.358%
and -0.797%. This is cold link work: no runtime layout, dependency, JIT or
native lowering changed.

Multi-ancestor runtime contracts now keep one lexical scope with every
composed ancestor binding until that candidate has been substituted, erased
and had `self`/`parent` resolved. Only the resulting concrete type enters the
deterministic union/intersection merge. A trait reached directly or through
another trait retains its nearest non-trait consumer, while class and
interface candidates keep their declaration scope. The same scoped traversal
feeds inherited methods and properties; the previous unscoped Reflection/LSP
view is deduplicated after traversal and remains source-order compatible.
Unsupported or invalid pseudo state fails closed as `never` instead of
borrowing another diamond branch's scope.

Permanent reified coverage combines a generic parent and overriding generic
trait whose nested `Envelope<self<T>>` inputs must accept both independently
scoped branches and reject an unrelated branch. Focused generic coverage is
now 37 reified/dual and 29 erased-only scenarios. Both hosts pass all four
production `--all-targets` configurations. Against exact checkpoint
`14a9587`, final balanced controls put the ordinary method at +0.250% ARM64
(40 pairs) and +0.480% CPU-2-pinned x86-64 (20 pairs); warmed inherited
turbofish is +0.044% ARM64 (20 pairs) and +2.906% x86-64 (40 pairs), below its
five-percent ceiling. The recursive graph walk is explicitly cold and
non-inlined, while the small resolver stays non-inlined without forcing a
separate cold section. No hot representation, dependency, JIT or native tier
changed.

The late-static checkpoint is now closed without treating `static<T>` as
`self<T>`. Ordinary `: static` semantics live in the shared feature-off PHP
runtime, while `<...>` remains controlled independently by the erased and
reified syntax flags. Executable contracts keep lexical `self`/`parent` scope
separate from the actual called class. Bound-erased checks retain the outer
late-bound class; reified checks also retain and compare nested canonical type
arguments. Namespace resolution never prefixes the pseudo base.

Instance methods obtain the called class from `$this`. Static methods recover
it lazily at the existing full-call boundary and publish one tagged `(call
frame, class id)` pair in the already-existing packed pending-call storage only
when their return type actually uses `static`; no new executor field or hot
layout is added. All return, finally, generator, setup-error and unwind exits
remove that pair. `ExecuteData`, `Value`, function and instruction layouts do
not grow. The ordinary scalar and fast-return paths still pop their call frame
directly, and the full-return checker performs called-scope recovery only for
a matching contract. Warm ordinary static scalar methods reuse the existing
frame-free target-neutral plan from the non-inlined opcode helper. On ARM64,
20 balanced pairs improve the permanent static-method control by 33.472%,
while the ordinary instance-method control remains within its one-percent
gate at +0.963%. The erased generic method and explicit method-turbofish
controls are -0.200% and -1.699% against exact checkpoint `3e5a507`.
CPU-2-pinned x86-64 independently reports -44.599% static, -0.014% instance,
-1.418% erased generic method and +3.255% method turbofish, keeping both
ordinary and explicit-generic controls inside their admission ceilings. This
semantic checkpoint does not change native lowering or add a generic JIT
specialization: generics-aware JIT work remains the final interphase milestone
after the full acceptance surface is closed.

Omitted PHP default parameter values now have an explicit callee boundary.
Only a default whose declared type contains a generic parameter receives
`CheckGenericDefault`, placed after default-expression assignment. The
existing `BindDefaultParam` target is patched after that instruction, so an
explicit argument skips both initialization and the new check and ordinary
calls execute their unchanged bytecode. The handler consumes an already-active
reified scope or linked/reified instance-method contract; it neither resolves
a use site nor walks inheritance on its own.

Lazy generators retain the corresponding feature-only binding/contract in
their generator state. One centralized resume boundary installs and discards
those sidecars for normal resume and both `yield from` completion paths,
instead of duplicating lifecycle code. This closes functions, instance/static
methods, closures, traits, direct/forwarded inheritance, constructors, named
holes, nested applications and generators in reified mode, plus concrete
linked methods/constructors/generators in both modes. The permanent bytecode
invariant proves that explicit arguments jump beyond the check. Focused
coverage is now 40 reified/dual, 31 erased-only and 2 syntax-disabled scenarios.
No dependency, hot layout, native lowering or generics-aware JIT changed.

The permanent release gate compares the candidate with exact checkpoint
`615575e` in 20 balanced pairs. Existing hot controls remain admitted: ordinary
generic method is -0.594% on ARM64 and +0.571% on CPU-2-pinned x86-64, while
warmed inherited turbofish is -3.694% and -6.396%. The candidate's valid
omitted-default workload is 47.959%/51.091% above the same generic declaration
called with an explicit value. Five absolute paired samples put that required
post-materialization guard at approximately 21.9 ns per ARM64 call and 28.1 ns
per x86-64 call. The prior binary, which accepted the invalid omission without
checking it, is retained only as the structural baseline; the new delta is real
semantics. A dependency-free gate script records omitted, explicit, manually
typed, ordinary-method and turbofish lanes independently.

Detached generator resume now has one explicit ownership boundary. The
executor reports an escaped PHP exception as an outcome rather than leaving a
`Running` generator plus a sidecar exception behind; `foreach`, internal
Generator methods and nested `yield from` reinject it into their live frame.
All normal and exceptional resumes reclaim the detached frame chain after its
CV/TMP snapshot has been saved, eliminating the previous bump-stack growth.
The same materializer serves direct resume and both delegated completion
paths. A declared generator return type is validated against the created
Generator object through its Iterator/Traversable hierarchy; the internal
return value remains exclusively `getReturn()` data.

The permanent dependency-free generator gate uses 200,000 scalar yields in an
untyped declaration so exact checkpoint `294307d` completes the same workload
instead of reproducing its typed infinite loop. Two consecutive 40-pair ARM64
runs are 5.733% and 5.202% faster; the latter records candidate medians
0.012463/0.012571 seconds versus baseline 0.013160/0.013247 seconds. Two
CPU-2-pinned x86-64 runs independently improve by 30.377% and 31.171%; the
latter records 0.016495/0.016552 seconds versus 0.024031/0.023982 seconds. The
result admits the lifecycle refactor without hiding correctness inside a
benchmark timeout. Generator coverage is 30 scenarios and frame cleanup
coverage is 25; both hosts pass the default, erased, reified and dual
production all-targets matrices. No dependency, native tier or JIT changed.

Generic constructors now consume the same effective own/inherited method
contract in both runtime modes. The ordinary path validates reified or linked
arguments before the body; the property-initializer fast path proves both
those arguments and every generic destination property, then skips the frame
while retaining the existing class-binding completion check. Reified binding
lifetime is explicit as well:
caller-owned pending scopes become call-owned active scopes, and success,
abandoned argument calls plus exception unwinding remove the exact tuple. A
permanent caught-constructor regression verifies that a later ordinary `new`
does not inherit stale reification.

The one-million-allocation ARM64 constructor benchmark includes the external
weak object identity, substituted `int` constructor, generic property write
and final read. Before the direct proof, the reified path took roughly 0.219
seconds. In 21 order-alternated release pairs after the proof:

- erased generic/manual paired median: +0.231% (0.069811/0.070089 seconds);
- reified generic/manual paired median: +87.423% (0.140973/0.075958 seconds);
- reified/erased generic paired median: +102.506%.

The synchronized x86-64 host independently reports erased generic/manual at
+0.152% (0.066809/0.066967 seconds), reified generic/manual at +89.648%
(0.145605/0.076640 seconds), and reified/erased generic at +118.178%.
The remaining reified delta is approximately 65 ns per ARM64 object and 69 ns
per x86-64 object; it includes the required weak identity insertion plus real
constructor/property guards and is recorded separately from method dispatch,
which is at parity.

Generic instance properties now resolve through the same composed inheritance
graph as methods. An own declaration wins; otherwise direct and transitive
class ancestors plus generic traits are searched with their effective type
arguments. Reified writes validate the fully substituted property type, while
bound-erased writes validate the child's linked signature: a remaining child
parameter erases to that child's bound and an unbounded parameter remains
`mixed`. The property IC remains keyed by the
receiver's concrete child declaration, so included child metadata and cached
writes use the same resolver without changing its 16-byte layout.

The first correct reified implementation recomposed and cloned the inherited
type on every write and took about 1.31 seconds for five million writes. The
final path stores one owned, fully substituted property contract in a
feature-only binding-plus-name L0. Cold lookup may walk an arbitrary ancestor
chain; a warm write performs no substitution or allocation. In 31
order-alternated ARM64 release sets, bound-erased own/manual is +0.299% and
inherited/own is -0.345%; reified own/manual is +81.212% and inherited/own is
-0.226% (0.094672 versus 0.095341 seconds). On the CPU-pinned x86-64 host, 101
separate bound-erased pairs report +0.082% own/manual and +0.135%
inherited/own. Sixty-one reified pairs report +87.013% own/manual and only
+0.758% inherited/own (0.106651 versus 0.105806 seconds). Thus reification
retains the real property guard, but inheritance itself adds less than one
percent on both architectures. No object, `Value`, frame, function,
instruction or property-IC layout grows.

The linked property view now also covers a child with no type parameters or
per-object reified binding. Thus `class IntBox extends Box<int>` materializes
the inherited backed property as `int` in both runtimes, even when constructed
by ordinary `new IntBox()`. A forwarded `U` instead remains relative to the
child and erases to `U`'s bound in the bound-erased mode. The same rule reaches
transitive concrete descendants, concretely bound traits, constructor fast
property initialization and included child declarations. This is the RFC
v0.22 distinction between a turbofish argument, which does not tighten an
erased parameter, and a link-time inheritance binding, which does create the
child's substituted runtime signature.

The shared property L0 is keyed by child declaration, optional reified use
site and property name. A zero-parameter child cannot own a reified object
binding, so its warmed IC bypasses the weak-object map entirely. In 31
order-alternated ARM64 sets, the concrete linked guard is -4.533% versus the
equivalent own bound-erased guard and -5.799% versus an explicitly reified own
property. CPU-pinned x86-64 measurements report -1.429% concrete/own-erased
and -7.272% concrete/own-reified over 61 pairs. A direct 101-pair comparison
of the new erased own-bound guard against checkpoint `d2ad952` is -4.986%
(0.101582 versus 0.106627 seconds), so generalizing the L0 did not tax the
existing checked path.

The linked method/constructor view now covers concrete and transitive
descendants, forwarded child bounds in bound-erased builds, concrete traits,
return boundaries, constructor property-init fallback and separately compiled
children. Warm-cache argument and overflow regressions prove that the exact
Long path side-exits to the full linked contract. The hot interpreter also
keeps a weak per-frame receiver proof: it cannot confuse two reified objects
of the same class or a recycled allocation, yet repeated calls avoid metadata
lookups. In 21 order-alternated ARM64 pairs, the permanent concrete-method
benchmark is -0.159% in the erased build and +0.382% in the reified build
versus an ordinary `int -> int` method. The CPU-pinned x86-64 gate reports
+0.566% and -0.235%, respectively. Thus linked
inheritance is at parity on both architectures without relying on JIT
specialization. ARM64 passes 197 default, 197 erased-only, 197 reified-only and
309 all-feature library tests plus every all-feature target; x86-64 passes the
same first three sets, 334 all-feature library tests and every target.
Method-generic alpha-renaming, deterministic diamond merging and the plural
Reflection inheritance view are now closed in the cold interned graph without
weakening either exact fast-path proof. The RFC object surface for generic
parameter declarations, their type references and reified object bindings is
also closed and executable in both runtime modes. The stdlib split now keeps
the public façade, ancestry projection, generic-parameter objects and built-in
registry in separate cohesive units without introducing another metadata
representation.

Generics-aware JIT work is the final milestone of this interphase, not a
concurrent semantic shortcut. It begins only after parser/link/runtime/
Reflection coverage, inheritance merging and both runtime modes are closed.
That semantic acceptance checklist is now complete for RPHP's supported PHP
surface, so the JIT phase may consume stable interned declarations and use-site
proofs:
bound-erased specializations must add no generic guard beyond the proven erased
ABI, while reified specializations require exact tuple/class guards and a
canonical deoptimization edge to the already-tested reified executor. Final
admission requires separate erased/reified native benchmarks on ARM64 and
x86-64 plus unchanged feature-off and non-generic gates.

The first admitted generics-aware native shape is a direct Long method inside a
typed accumulate or general mixed region. `guarded_quick_long_method_target()`
shares the canonical warmed method IC: class, target and arity are checked for
every activation. Bound-erased own methods keep the existing ABI with no
generic lookup, and concretely linked descendants consume their interned
exact-Long IC proof. Only a receiver-specific reified contract resolves the
interned class/type tuple, once before the quick/native region is entered. The
region cannot write its receiver CV, so the proof spans the activation; failure
returns `GuardFailed` or the existing call resume IP and never enters native
code. Native lowering, machine code and deoptimization publication remain
target-neutral and unchanged.

Permanent ARM64/x86-64 tests assert one native entry, multiple chunks and zero
side exits for `GenericJitBox<int>`, plus the canonical reified error for a
same-class `GenericJitBox<string>` receiver. A dedicated dependency-free
`bench_generics_method_native_loop.php` lane is paired with the existing
ordinary native-method control. Relative to exact checkpoint `6fb757c`, 40
balanced ARM64 pairs are -0.340% erased/+0.015% reified for the generic lane
and -0.646%/+0.149% for the ordinary control. CPU-2-pinned x86-64 is
+0.647%/-0.163% generic and +0.141%/-0.190% ordinary. The one-percent gates
therefore hold separately in both runtime modes and on both native backends,
without a dependency or hot-layout change.

The next admission closes nested scalar call trees. A non-inlined
`guard_quick_scalar_call_tree_generics()` preflight traverses the planner's
canonical Init/Send/DoFcall shape once at region entry. The root reuses its
direct proof; every nested method validates its own warmed class/target/arity
IC and, only when required, the exact reified tuple. The baseline composed-call
executor applies the same nested boundary before it can elide the first PHP
frame, so OSR warmup cannot accidentally discharge an incompatible contract.
Failure resumes at the original initializer and native code is never entered.

Generic state deliberately does not cross into
`NativeQuickLongCallTreeBuilder`. A rejected prototype stored an
`ExecutorGlobals` reference there, growing the x86 wrapper stack from `0x2ee8`
to `0x2ef8` and moving both generic and non-generic controls by roughly five
percent. Passing it as a builder argument then destabilized ARM64. The accepted
preflight restores the exact `0x2ee8` x86 frame and leaves builder, kernel,
machine-code config and dispatch signatures at checkpoint `1ad13fa`.

Two permanent architecture-specific tests add the important outer-function →
nested-generic-method shape and its same-class wrong-tuple replay. The nested
benchmark warms compilation outside the timer but retains the once-per-
activation proof inside it. In 100 balanced pairs, ARM64 is +0.171% erased and
+0.025% reified for the generic lane, with -0.144%/-0.064% controls. CPU-2-
pinned x86-64 is +0.636%/+0.212% generic and +0.730%/-0.118% control. All lanes
remain inside the one-percent admission gate without backend-specific generic
code.

Exact Double generic methods are admitted next. A raw-Double contract proof
accepts only fixed-arity boundaries whose occupied parameters and return can
carry `float`; the existing Double plan continues to guard the actual values
and side-exit on non-Double input or invalid arithmetic. Root method admission
happens after its normal IC proof, and composed same-receiver callees repeat the
same receiver-specific check while their target-neutral program is flattened.
A shared out-of-line metadata lookup avoids duplicating method-name resolution
while leaving the inline cache, plan, native state and both code generators
unchanged.

The nested regression is intentionally stronger than another generic root: a
non-generic `composed(float, float): float` wrapper calls generic
`$this->scale(T, float): T`. Replaying the wrapper on the same class with a
different reified tuple can therefore be rejected only by the nested resolver.
Both direct and nested compatible tuples enter exactly one native region with
zero side exits; incompatible tuples resume the original PHP call and preserve
its argument diagnostic. Default, erased, reified and all-feature all-target
matrices pass on both architectures (311 ARM64 and 336 x86-64 all-feature
library tests).

Against exact checkpoint `ff9dc11`, 100 balanced ARM64 pairs are
-0.033%/-0.152% erased/reified for the direct generic lane and
-0.066%/+0.200% for the nested generic lane, with -0.079%/-0.299% and
-0.067%/+0.135% controls. CPU-2-pinned x86-64 is -0.077%/+0.182% direct
generic, +0.103%/-0.197% nested generic, +0.038%/+0.175% direct control and
+0.300%/+0.057% nested control. All eight architecture/mode generic lanes and
all controls remain within the one-percent gate.

The following admission covers compiler-composed scalar bodies whose object
argument supplies nested monomorphic methods. The previous resolver shared
class/target/arity identity but did not describe the generic ABI of each IR
call kind. It now selects one of two cold proofs: exact Long inputs and return
for `Call`, or exact Long inputs with a borrowed String return for
`StringCall`. The latter adds
`GenericMethodContract::admits_exact_long_to_string_call()` and recursively
admits only String-compatible return unions/intersections.

The check runs once while resolving the composed body, after the existing IC,
and remains outside the per-iteration evaluator. No call operation, plan,
inline cache, native context or backend changes size. Permanent tests cover
compatible Long and String receiver tuples plus same-class reified tuple
changes. A failed proof replays the outer function and reaches the canonical
nested method argument error; erased builds retain the same quick body.

Against exact checkpoint `a72fbcf`, 100 balanced ARM64 pairs are
-1.168%/-0.960% erased/reified for generic composed Long and
-1.924%/-1.113% for its ordinary control; generic composed String is
-0.303%/-0.873% with -0.667%/-0.049% controls. CPU-2-pinned x86-64 is
-0.044%/+0.403% Long generic, +0.048%/+0.196% Long control,
+0.068%/+0.169% String generic and -0.028%/+0.090% String control. The largest
positive movement is +0.403%; ARM64's larger movements are improvements shared
with the corresponding controls.

The next admission closes mixed native methods whose exact call-site ABI is a
positional combination of raw Longs and borrowed Strings with a Long result.
Previously the mixed region and backend already carried this representation,
but a generic receiver was conservatively tested as all-Long and therefore
remained in the canonical VM. `GenericMethodContract` now consumes the compact
String-position mask once at region entry. A reified mismatch rejects the
whole region before its first speculative operation and replays the original
method call and diagnostic.

Broad executable `mixed`/untyped parameters are refined to String only when
the compiler observes an operation that semantically requires one (`strlen`
or the existing literal-concat shape). Any simultaneous numeric use makes plan
construction fail. The quick resolver then verifies that the call-site source,
compiler mask, ordinary signature and receiver-specific generic contract all
describe the same ABI. Long, Long/String and Long-to-String boundaries share
one resolver, so adding a typed shape no longer duplicates IC/receiver/deopt
logic. The native IR, either backend, inline cache and hot state are unchanged.

Against exact checkpoint `5ae9284`, 20 balanced max-perf pairs improve the new
ten-million-iteration generic workload by 93.211%/98.725% in ARM64
erased/reified builds and 93.946%/98.743% on CPU-2-pinned x86-64. The large
gain is the intended transition from canonical method frames to the existing
native mixed region. One hundred-pair ordinary controls move only
+0.005%/+0.018% on ARM64 and +0.104%/-0.205% on x86-64. A separate 100-pair
ARM64 comparison of the final generic workload with its ordinary-signature
control is -0.102% erased and -0.040% reified; the warmed generic contract has
no measurable per-iteration cost.

The property-mutator admission now separates an unused call result from an
exact Long result. `LongDiscarded` still requires every occupied argument to
admit Long, but also accepts a canonical bare `void` return; a declared value
return remains Long-compatible so skipping its generic return check cannot
hide an error. The property planner rejects `return expr;` in a void method,
and the shared generic return boundary now treats a valid bare void return
consistently in both runtime modes.

At typed-region entry, every declared property slot is resolved once. An
ordinary write-safe cache keeps its existing proof. A generic cache instead
uses its interned declaration ID to validate the current exact Long against
the erased or reified property contract; immutable receiver substitution then
proves that every Long produced by the transactional property plan is valid
for the region. Native iterations use only the resolved slot shadows. A
different class/tuple, non-Long or referenced property, invalid cache, or
checked-arithmetic failure takes the existing canonical edge before an
unproved store. No property IC, instruction, frame, native state or backend
representation grows.

Against exact checkpoint `365c2b6`, 20 balanced max-perf pairs improve the
ten-million-iteration generic property-mutator lane by 98.524%/99.367% on
ARM64 and 98.555%/99.381% on CPU-2-pinned x86-64 in erased/reified mode. The
corresponding 100-pair ordinary controls are +0.289%/+0.051% on ARM64 and
+0.211%/-0.123% on x86-64, all inside the one-percent gate. Permanent tests
prove one native entry, multiple chunks and zero side exits for a bound
generic property, canonical replay for a wrong reified property tuple, and no
plan for an invalid void value return. Focused generic JIT coverage is now 17
dual/reified and 9 erased scenarios.

Property getters now reuse that activation-time property proof and shadow
binding. A zero-argument `PropertyGetterCall` lowers to the existing native
slot-to-slot move after the ordinary quick resolver has proved the receiver,
method result and current property value are exact Longs. Generic metadata is
therefore still consulted only before native entry. A getter and mutator for
the same receiver slot deliberately deduplicate to one transactional shadow,
so reads observe preceding native writes and final publication preserves call
order. Getter-only regions publish the identical checked Long; referenced,
non-Long or reified-mismatched properties reject the region and replay the
original caller operation.

Against exact checkpoint `8a36f5f`, 20 balanced max-perf pairs improve the
ten-million-iteration generic getter by 88.999%/88.905% on ARM64 and
92.037%/91.969% on CPU-2-pinned x86-64 in erased/reified mode. Forty-pair
ordinary typed-getter measurements improve by 89.097%/88.981% and
91.979%/91.947%, respectively, because they share the same target-neutral
lowering. Unchanged scalar-method controls over 200 pairs are
-0.357%/-0.337% on ARM64 and +0.569%/+0.180% on x86-64. Permanent coverage is
now 20 dual/reified and 11 erased scenarios, including shared getter/mutator
ordering and canonical reified mismatch replay. No property IC, native
instruction, kernel-state or backend layout changed, and no dependency was
added.

A behavior-preserving property-lifecycle cleanup follows this checkpoint.
Native property binding/lowering moved out of the scalar-method file into a
188-line property builder, while activation seeding and publication moved into
a 59-line runtime child included at their original item position. The scalar
builder falls from 383 to 199 lines, repeated generic-JIT test-plan discovery
uses one predicate helper, and the execute module still owns every item through
`include!`. Consequently visibility, persistent layouts, native operations,
hot-function order, final binary size, `__TEXT` size and both mixed-kernel
symbol addresses remain unchanged.

Against exact checkpoint `72969ef`, 200 balanced ARM64 pairs move the generic
property getter by +0.421%, the property mutator by +0.000% and the independent
scalar-method control by +0.064%. CPU-2-pinned x86-64 reports
-0.195%/+0.252%/-0.443%, respectively. All six lanes remain inside the
one-percent structural gate, and the complete 11 erased/20 reified focused
suite passes on both hosts. No dependency or runtime behavior changed.

The remaining `ComposedPropertyCall` object-call holdout now lowers into the
same native mixed region. Region construction resolves the outer mutator's
declared Long slots with the existing generic-aware property proof and reuses
the already guarded inner getter slot. The builder first copies the getter
shadow into a private native slot, then supplies that captured value to the
outer property plan. This preserves PHP's argument-evaluation order even when
the outer method mutates the getter's property before a later plan operation
uses its argument. Receiver-plus-object-slot binding deduplication still gives
same-object calls one transactional shadow.

The lowering adds no backend instruction: capture, arithmetic, branches and
publication use the existing native `Move`, `Binary` and conditional forms.
Both method counters share the outer plan's final completion operation, so an
arithmetic side exit publishes neither partial property writes nor a partial
composed-call count before canonical replay. Outer slots are resolved only
while building a native kernel; feature-off and quick-only resolution retain
their previous enum shape and admission behavior.

Against exact checkpoint `84be962`, 40 balanced max-perf pairs improve the new
ten-million-iteration generic composed-property workload by 99.004% erased and
99.877% reified on ARM64, and by 98.889%/99.821% on CPU-2-pinned x86-64. The
ordinary-signature control improves by 89.006%/88.848% on ARM64 and
93.031%/92.695% on x86-64 because it shares the target-neutral lowering.
One-hundred-pair unchanged getter, mutator and scalar-method controls have no
regression above +0.558% on either host or generic mode.

A final cold-boundary cleanup moved outer slot proof construction out of the
general quick resolver and into native kernel construction. A 200-pair audit
against the measured candidate keeps all five erased lanes between -0.059%
and +0.124% on ARM64 and -0.273% to +0.273% on x86-64. Reified lanes are
-0.508%/-0.616%/-1.208%/-0.472%/-2.804% on ARM64 and
+0.196%/-0.077%/-0.140%/-0.168%/-0.524% on x86-64. The final source passes
default, erased, reified and all-feature all-target matrices on both hosts;
focused generic JIT coverage is 14 erased and 24 reified scenarios, including
same-object capture, transactional overflow replay and canonical reified
mismatch. No crate, native operation or backend-specific generic path was
added.

Mixed object/property regions now retain fused conditional updates as well.
`QuickLongOp::ConditionalAddAssign` expands during target-neutral region
construction into the existing `BranchUnless` plus checked `BinaryAssign`
operations. Admission requires the encoded false edge to be the immediately
following quick operation and rejects result aliases that would make the
unmaterialized condition temporary observable. Thus neither native backend,
the persistent quick representation nor generic metadata gains a new form.

The false edge skips the checked update inside the same region. If that update
overflows, the existing operation-granular side exit publishes the already
completed property-call prefix and resumes at the assignment boundary, so the
preceding composed call is not executed twice. Permanent coverage proves one
native entry with multiple safepoint chunks and no normal side exit, plus an
exact-final overflow that preserves all 100,000 earlier composed property
calls before canonical arithmetic replay.

Against exact checkpoint `21a4af8`, 40 balanced max-perf pairs improve the new
ten-million-iteration generic conditional-property workload by 98.970%
erased and 99.869% reified on ARM64, and by 98.974%/99.821% on CPU-2-pinned
x86-64. Its ordinary-signature control improves by 90.417%/90.328% on ARM64
and 94.087%/94.031% on x86-64 because it shares the same lowering. Unchanged
composed-property and scalar-method controls remain below the one-percent
regression gate; an initially noisy ARM64 reified lane falls from +1.255% to
+0.470% in a 200-pair rerun, and the largest remaining positive lane is
+0.597%. Focused generic JIT coverage is now 16 erased and 26 reified
scenarios. No dependency was added.

A follow-up coupling cleanup moves the fused update's admission and two-op
construction into the 38-line `native_conditional_add.rs` include. The legacy
straight builder and the mixed object/string builder now consume the same
`BranchUnless`/`BinaryAssign` pair instead of duplicating its condition-kind,
post-value alias and fallthrough rules. The shared boundary also requires the
straight plan's true edge to name the immediately following quick operation,
matching the mixed builder's existing conservative check.

The helper is fully inlined in max-perf builds. ARM64 binary, `__TEXT` and
`__text` sizes are unchanged; x86-64 loses 64 binary bytes and 72 text bytes,
while both native runner symbols keep their exact sizes. In 200 balanced
erased pairs against `f72c984`, the generic/scalar conditional-property lanes
move -0.120%/-0.473% on ARM64 and -0.058%/-0.121% on CPU-2-pinned x86-64.
Quick-loop, architecture-native and both generic-mode suites pass on both
hosts. No runtime operation, persistent layout or dependency changed.

Invariant JSON projections can now feed an ordinary or generic property
mutator without breaking the surrounding native mixed region. The existing
typed-source prelude still decodes once, validates every fixed projection and
publishes the projected Long slots atomically before native entry.
`JsonProjectionStep` is therefore a zero-code control edge in the mixed
target-neutral builder, just as it already is in the scalar straight builder;
no decoder or JSON operation is emitted into machine code.

The planner no longer misclassifies the prelude's arbitrary JSON source text
as a finite native string token. The prelude owns its String/reference guard,
while any independent use of the same CV as a native string operand still adds
the slot through that consuming operation. Invalid, missing or non-Long
projections reject the prelude before native entry and replay the canonical
caller, so no transactional property shadow is published. Permanent coverage
proves one native entry across multiple safepoint chunks and the invalid-type
replay path in both erased and reified modes.

Against exact checkpoint `9499764`, 40 balanced max-perf pairs improve the new
ten-million-iteration generic JSON/property workload by 94.770% erased and
94.493% reified on ARM64, and by 96.169%/96.004% on CPU-2-pinned x86-64. The
ordinary-signature control improves by 94.785%/94.504% on ARM64 and
96.102%/96.021% on x86-64. One-hundred-pair unchanged property-method controls
remain between -0.277% and +0.066% across both hosts and modes. Focused generic
JIT coverage is now 18 erased and 28 reified scenarios. Default, erased,
reified and all-feature all-target matrices pass on ARM64 and x86-64. No
backend operation, persistent layout or dependency was added.

Fixed invariant JSON projections can also remain deferred method arguments.
The planner now walks the compiler's `FetchDimR` chain between
`InitMethodCall` and each `SendVal`/`SendVarEx`, extends the same bounded
projection paths used by standalone fetches, and gives the resulting typed
slots to the existing property-call operation. Nested paths and multiple
arguments therefore stay in one transactional native region without adding a
JSON call opcode or materializing an intermediate PHP variable. Any dynamic,
over-depth or untracked fetch still rejects the region, while invalid/missing
values fail the prelude and replay the call canonically before property state
is published.

Against exact checkpoint `b31934b`, 40 balanced max-perf pairs improve the new
two-million-iteration direct two-argument JSON/property workload by
99.637%/99.700% on ARM64 and 99.669%/99.744% on CPU-2-pinned x86-64 in
erased/reified mode. Its ordinary-signature control improves by
99.561%/99.568% on ARM64 and 99.541%/99.548% on x86-64. One-hundred-pair
materialized-JSON and scalar property-call controls remain between -0.285%
and +0.346% across both hosts and modes. Focused generic JIT coverage is now
19 erased and 29 reified scenarios. No backend operation, persistent layout
or dependency was added.

The projection planner state is subsequently centralized in one
`InvariantJsonProjectionState`. Path ownership, fetch/parent reachability,
String leaves, derived String lengths and final typed-source retention now
share one admission boundary instead of five parallel locals and duplicated
finalization in the 1,700-line region detector. The standalone-fetch and
deferred-argument paths use the same `start`/`extend`/`derive`/`retain`
contract. This is a behavior-preserving structural commit: the quick/native
IR, persistent plan layout, runtime prelude and dependencies are unchanged;
61 quick-planner unit tests, 16 JSON projection tests and 19 erased generic
JIT tests pass before the next feature is layered on it.

That boundary now admits an exact String byte length inside a deferred method
argument, for example
`$box->addPair($row['value'], strlen($row['nested']['name']))`. The shared
argument walker consumes the fixed `FetchDimR` chain and optional
`Strlen`/`Strlen_String` before the positional send. The invariant prelude
validates and publishes the Long leaf, String leaf and derived Long length
atomically; the existing property call consumes only the two Long arguments,
so arbitrary String contents never enter the finite-token native ABI. A
missing/non-String leaf rejects the prelude and replays canonical argument
coercion and the method call from `InitMethodCall` exactly once.

Against exact refactored checkpoint `0f21b93`, 40 balanced max-perf pairs
improve the new two-million-iteration generic derived-argument workload by
99.675%/99.730% on ARM64 and 99.685%/99.753% on CPU-2-pinned x86-64 in
erased/reified mode. Its ordinary-signature control improves by
99.639%/99.621% on ARM64 and 99.574%/99.580% on x86-64. One-hundred-pair
direct-JSON, materialized-JSON and property-method controls have a largest
positive movement of +0.690%; no lane breaches the one-percent regression
gate. Focused generic JIT coverage is now 21 erased and 31 reified scenarios,
and all four all-target matrices pass on both hosts. No backend operation,
persistent layout or dependency was added.

Static pseudo-owner turbofish calls now cover the RFC's supported
`self::method::<...>()` and `parent::method::<...>()` forms, including
namespaces, inherited declarations, nullsafe instance calls and forwarding
`static` return contracts. Nested compilers retain file `strict_types`,
namespace and `use` context. Class methods additionally carry a cold lexical
class/parent owner used by generic metadata, while `InitStaticCall` preserves
the original pseudo spelling needed to recover PHP called scope. The runtime
resolves that spelling only on the static-call cache miss; ordinary warmed
static calls retain their original one-load cache path.

Trait method op arrays require a separate correctness boundary because one
body is shared by every consuming class. Their pseudo owner remains dynamic,
the site publishes neither a generic declaration cache entry nor a static call
target, and both are resolved per call instead of allowing one consumer's
monomorphic state to leak into another. Erased and reified modes validate the
same selected declaration but retain their intended runtime difference:
bound-erased unconstrained arguments keep the erased PHP contract, whereas
reified calls check the explicit tuple. Syntax-disabled builds reject the same
turbofish before compilation.

The dependency-free `run_generics_static_pseudo_gate.sh` compares five million
warmed `self::step::<int>` calls with an otherwise identical explicit-owner
control. Forty balanced pairs measure -0.187%/-0.018% on ARM64 and
-4.776%/+0.707% on CPU-2-pinned x86-64 in erased/reified mode. The favorable
erased x86 movement is treated as measurement/layout noise, not an
optimization claim. Against exact checkpoint `94394c6`, the established
explicit-owner generic control is -1.221%/+0.436% on ARM64 and
-1.237%/+0.488% on x86-64 in erased/reified mode. The ordinary
20-million-call static control is -1.394%/-1.706%, and a longer 100-pair
erased instance-turbofish control is +0.032%/+0.145%, after keeping dynamic
trait resolution out of both ordinary cache hits. Default, erased, reified
and all-feature all-target matrices pass on both architectures, with no
external dependency, hot layout or JIT backend change.

Late-static call checkpoint (2026-08-11): the base PHP
`static::method()` form and RFC `static::method::<...>()` form now share a
dedicated late-bound call path. Separate call and generic-check opcodes keep
their inline caches keyed by the runtime called class; warmed explicit,
`self` and `parent` calls retain their existing opcodes and cache hits.
Inheritance alternation, selected overrides, bounds, shared traits, forwarding
return contracts and erased/reified metadata therefore resolve against the
same called class without cross-class cache publication.

Static methods carry the called scope through participating frames without
growing `CallPlan`, `ExecuteData` or `Value`. Compact frames embed the class ID
in the unused upper half of the heap-ownership bitmap, wide frames use the
existing sparse sidecar, and cleanup masks embedded metadata from ownership
bits. Closures and arrows capture and republish this scope, including after
escaping the creating method. With both syntax flags off, ordinary late-static
calls remain accepted while turbofish is rejected at the parser boundary.
Late-static properties/constants, attributes and property-hook use sites stay
behind their corresponding general PHP surfaces.

The dependency-free static pseudo gate now measures three lanes. Forty
balanced max-perf pairs record ordinary `static::` versus `self::` movement of
+3.266%/+2.309% on ARM64 and +1.072%/+0.781% on CPU-2-pinned x86-64 in
erased/reified mode. Generic late-static versus generic self moves
-0.375%/+0.327% and +0.794%/-0.406%; the established generic
`self`/explicit-owner control moves -0.078%/-0.002% and -0.456%/-0.356%.
Every lane remains inside its five-percent admission ceiling. Default, erased,
reified and all-feature test matrices plus all-feature/all-target checks pass
on both hosts. No external dependency or JIT backend path was added, and
generic-specific JIT optimization remains the final step after the runtime
surface is developed.

Late-static property-read checkpoint (2026-08-11): class metadata now keeps
instance and static property definitions in separate tables. Static defaults,
enum cases and built-in reflection cases therefore no longer widen every
object instance. Inheritance and trait composition apply the same centralized
visibility/collision rules to both tables while preserving their separate
storage domains.

`static::$property` lowers to a dedicated keyed fetch opcode. Its warmed cache
is keyed by the runtime called-class ID and stores only the resolved property
slot, so alternating base/child classes, shared trait bodies and escaped
closures cannot reuse another class's metadata. Compact static frames read the
called scope from their existing embedded metadata; wide and instance frames
use the established fallback. Lexical `self::$property`, `parent::$property`
and explicit-owner reads retain the ordinary static-property opcode and now
benefit from its own keyed lookup cache. Visibility failures remain exact.

This slice reads immutable declared defaults only. Mutable static storage,
assignment, shared inherited storage identity and typed/generic write checks
remain the next property milestone. `static::CONST` also remains rejected until
general class constants have a canonical parser/compiler/runtime surface; it
will not be introduced through a late-static-only bypass.

The four-lane dependency-free gate compares five million warmed operations.
ARM64 erased (40 balanced pairs) records `static`/`self` overhead of +2.117%
ordinary, +3.200% property and +1.280% generic, with the established generic
`self`/explicit control at +0.214%. ARM64 reified (100 pairs) records
+4.412%, +2.204%, +1.087% and -0.106%. CPU-2-pinned x86-64 erased (40 pairs)
records +0.819%, +1.021%, +0.795% and -0.014%; reified records +2.166%,
+1.166%, +0.126% and +0.321%. All lanes stay below the five-percent ceiling.

The immediately adjacent late-generic guard can additionally lend its already
validated called-class cache to the initializer on x86-64, removing a second
frame-scope resolution. This is intentionally target-gated: enabling the
marker on ARM64 perturbed the ordinary hot layout to +5.748% in a 100-pair
audit, while retaining ARM64's smaller initializer restored it to +4.412%.
The full default, erased, reified and all-feature matrices plus all-target
checks pass on both hosts. No dependency or JIT backend path was added.

Mutable static-property checkpoint (2026-08-11): declared static properties
now resolve through append-only canonical executor storage rather than cloning
their declaration default on every read. A per-class property-index map points
at that storage identity. An inherited declaration therefore shares its
parent's slot, while an explicit redeclaration receives a fresh slot. Trait
composition creates storage per consuming class, including a fresh slot when a
child uses the same trait again as required by PHP 8.3+, and still rejects
incompatible class/trait or trait/trait declarations.

Statement assignment supports named, `self`, `parent` and late-bound `static`
owners. Simple and compound forms lower to dedicated ordinary/late-static
write opcodes; compound assignment deliberately reuses the canonical read
operation before publishing the write. Read and write inline caches store the
canonical storage slot and distinguish their validated capabilities, so a
warmed write performs one called-class check and a direct append-only slot
update. Missing declarations, visibility failures and enum writes remain
checked on the cache miss. An existing reference wrapper is updated in place,
ready for the later general reference-binding surface.

The warmed scalar path copies the compact 16-byte `Value` representation
directly and retains normal clone/refcount behavior for strings, arrays,
objects and references. Cache-miss materialization stays cold and out of line.
The two new executor vectors are appended after all prior fields; an earlier
placement before reified sidecars measurably perturbed x86-64 instruction/cache
layout, while the final placement preserves every previous field offset and
restores the ordinary-call gate. No external library or generic/JIT-specific
backend operation was added.

Twenty balanced exact-baseline pairs and one hundred candidate-only write
pairs produced the following final max-perf results. Negative exact deltas are
improvements; the write column is late-bound `static::$p` relative to lexical
`self::$p` and retains the existing five-percent equivalence ceiling.

| Host / mode | Ordinary static-call control | Late-static read | Self read | Late/self write |
|---|---:|---:|---:|---:|
| ARM64 erased | -1.095% | -2.286% | -5.719% | +1.944% |
| ARM64 reified | -0.818% | -1.777% | -5.207% | +1.859% |
| x86-64 erased, CPU 2 | -3.883% | -10.120% | -6.749% | +4.496% |
| x86-64 reified, CPU 2 | -7.061% | -19.122% | -15.586% | +3.590% |

Default, erased, reified and all-feature all-target matrices and checks pass on
both hosts after this final layout. The next bounded slice replaces the current
property-definition tuple with explicit metadata for declared type,
readonly/uninitialized state and generic substitution, then enforces that
contract on every static write. General reference binding, assignment
expressions, increment/decrement and `unset` remain outside this checkpoint.
Generics-aware JIT specialization stays last, after both erased and reified
runtime semantics are complete and stable.

Typed dual-runtime static-property checkpoint (2026-08-12): the shared
property-definition tuple is now a named `PropertyDefinition` carrying the
declaring class, visibility, default, erased runtime type, readonly state and
whether the interned generic contract needs a reified check. Class, trait and
promoted-property compilation all populate that one shape. Mutable inherited
properties enforce PHP's invariant type, staticness, readonly and visibility
rules before storage is linked; private parent declarations remain independent,
and source-level static readonly properties are rejected.

Typed static properties without a default start uninitialized, including
`mixed`, while untyped declarations retain their null initialization. Reads
raise a catchable `Error`; writes apply PHP weak scalar conversion, preserve the
strict-mode boundary and always permit `int` to `float`. Invalid defaults fail
during compilation. The 16-byte instruction and inline-cache layouts do not
grow: exact scalar tags occupy the low bits of an aligned declaration pointer,
and cache misses retain the full declaration for unions, nullable, class and
pseudo-class checks. The compact late-static write path validates its runtime
called-class ID before decoding property-name operands, so a different child
cannot reuse another invocation's storage proof.

One interned generic property declaration feeds both feature-selected runtime
models. Bound-erased mode checks the ordinary erased class contract; reified
mode additionally checks the substituted nested arguments. A stable boxed
sidecar holds the declaration proof and a `Weak` object identity, so repeated
writes of the same live object avoid both recursive argument comparison and a
hash lookup without retaining the object or accepting recycled addresses. The
metadata is available to the existing generic Reflection graph; a dedicated
`ReflectionProperty` surface remains future compatibility work. Syntax-off
builds still reject generic syntax while compiling this internal metadata and
ordinary typed-property logic.

On the local ARM64 release all-features candidate, forty balanced pairs with
six warmups produce the following candidate-only ratios. Every equivalence lane
remains inside its five-percent ceiling; the large favorable reified result is
the intended same-object proof reuse relative to repeating the erased class
check, not a general allocation benchmark.

| Lane | Ratio |
|---|---:|
| Ordinary `static::` / `self::` call | +2.133% |
| Late-static / self static-property read | +1.974% |
| Late-static / self untyped property write | -3.110% |
| Typed / untyped self property write | +2.200% |
| Typed / untyped late-static property write | -0.101% |
| Reified / erased generic property write | -45.747% |
| Generic `static::` / `self::` call | +0.135% |
| Generic `self::` / explicit-owner call | -0.053% |

The same forty-pair gate pinned to CPU 2 on the x86-64 reference host records
+1.320%, +0.284%, -5.695%, +3.493%, +3.651%, -58.589%, -6.407% and +0.317%
in the table's lane order. Favorable movements outside the typed-property lanes
remain layout/equivalence evidence rather than optimization claims.

Default, erased, reified and all-features test configurations plus the
all-feature/all-target check pass locally and on the x86-64 reference host. No
dependency, `Value`, frame, JIT backend or native IR operation changed.
Instance-property uninitialized/read and write enforcement, general reference
binding, assignment expressions, increment/decrement, `unset` and
`ReflectionProperty` remain separate bounded slices. Generics-aware JIT
specialization remains the final step after these runtime semantics are
complete.

### Typed dual-runtime instance-property checkpoint (2026-08-12)

Declared instance properties now use the same canonical `PropertyDefinition`
as static properties. A typed declaration without a default starts as
uninitialized, including `mixed`, and a read raises a catchable `Error` before
the result is discarded or a warmed getter plan can bypass it. Untyped
declarations retain their null default. Writes enforce unions, nullable,
class, `self` and `parent` contracts before mutation, apply PHP weak scalar
conversion at the file boundary, retain strict-mode `int` to `float` widening,
and dereference a reference source so the property stores the assigned value.
Promoted properties and inherited readonly initialization use the same path;
the PHP 8.4 protected-set family scope permits a parent constructor or method
to initialize its declaration on a child object.

Property metadata now carries a separate lexical `type_scope`. Trait
composition rewrites that scope to each consuming class while retaining the
trait as declaration provenance, so trait `self` and `parent` types cannot leak
one consumer's meaning into another. A generic-origin marker distinguishes a
plain typed property from an inherited `T` that happens to erase to the same
scalar. Bound-erased mode therefore checks the selected erased contract and
reified mode checks the substituted runtime contract without duplicating the
ordinary typed-property machinery.

The 16-byte inline cache and instruction layouts do not grow. Assignment-only
cache state uses the existing property flag bits plus a tagged stable
declaration pointer. Generic-origin contracts are tagged complex at cache
publication, so an exact `int` write decides entirely from the warmed cache and
never dereferences cold metadata; generic checks remain authoritative. The
exact path is adjacent to the untyped dispatch branch, while coercions, unions
and exception construction stay cold in the extracted
`instance_property_cache.rs` boundary. Constructor, quick, hot and Long
property-method plans accept only proofs compatible with the value kind they
publish.

Forty balanced pairs with six warmups produce these final all-features release
ratios. The four typed/untyped equivalence lanes use a five-percent ceiling.
The last column compares the established untyped property workload with exact
checkpoint `d4c4965` under the stricter one-percent regression gate.

| Host | Read | Write | Method | Constructor | Untyped exact baseline |
|---|---:|---:|---:|---:|---:|
| ARM64 | -2.787% | +0.924% | -1.804% | +2.796% | -0.748% |
| x86-64, CPU 2 | +0.160% | -3.050% | +0.098% | +2.716% | +0.148% |

Default, erased, reified and all-features matrices plus the
all-feature/all-target check pass on both architectures. Local coroutine-I/O
tests were repeated with loopback access after the filesystem/network sandbox
correctly refused socket creation; all 313 library tests then pass. No
dependency, `Value`, frame, JIT backend or native IR operation changed.
General reference binding, assignment expressions, increment/decrement,
`unset` and a dedicated `ReflectionProperty` surface remain separate work.
Generics-aware JIT specialization remains last.

### Planned compatibility gate after typed instance properties

Immediately after the typed instance-property slice, pause feature expansion
for a reproducible upstream PHP compatibility run. Pin one public `php-src`
commit and run RPHP against the unmodified PHPT cases under `Zend/tests` and
`tests/lang`; keep the upstream checkout and generated run artifacts outside
the repository. Record the RPHP commit, upstream commit, build features, host
architecture, runner command and timeout policy so later results can be
compared against the same baseline.

Publish both an overall result and a classified breakdown: pass, fail, skip,
unsupported-by-runner and timeout/crash. The headline pass rate is
`pass / (pass + fail)` and must never silently count skipped, unsupported or
timed-out cases as passes. Also publish the raw machine-readable manifest and
the failing PHPT paths, grouped at least by parse/compile/runtime/output
mismatch, so the number is auditable and directly drives the next
compatibility backlog. A smaller supported-surface rate may be reported beside
the overall rate, but it must use an explicit, versioned exclusion manifest
rather than removing failures after inspection.

The runner must understand the relevant PHPT sections (`FILE`, `EXPECT`,
`EXPECTF`, `EXPECTREGEX`, `SKIPIF`, `INI`, `ENV`, `ARGS`, `STDIN`, `CLEAN` and
required extension declarations), preserve exit status and standard streams,
isolate temporary files, and bound each case. First validate the harness with
reference PHP on the same pinned checkout; harness disagreements are runner
bugs, not RPHP compatibility failures. Commit only the dependency-free runner,
versioned exclusions and generated summary/manifest suitable for publication,
not upstream PHP sources, host details or bulky build output.

The permanent corpus must include ambiguous comparison/shift grammar, nested
arguments, bounds/defaults/variance, inheritance forwarding and diamonds,
traits, closures, dynamic calls, reflection metadata, invalid arity/bounds and
the upstream RFC implementation tests that fit RPHP's supported PHP surface.
Add dedicated cold-compile, link, ordinary-call, generic-call and turbofish hot
benchmarks on ARM64 and x86-64. Use no parser, type-system or collection crate;
the implementation remains within the standard library and existing runtime.

## Interphase 5.58: pinned Symfony 7.4 LTS first-application gate

A static upstream audit performed on 2026-08-12 uses Symfony 7.4 LTS commit
`f61f83263e61332a2231aa6f7ddbf9722aa51fa2` as the first framework target.
Symfony 7.4 requires PHP 8.2 or newer and is a better first compatibility
boundary than the concurrently audited Symfony 8.1 commit
`e918927d3f0a78079d4b9c997da43f7f8e2bb758`, whose HttpFoundation and other
components already contain PHP 8.4 property hooks and asymmetric-set
visibility. Symfony 8.1 is the immediate forward-compatibility gate after 7.4,
not a moving target for the first boot.

The audited first-application dependency closure is FrameworkBundle plus
Cache, Config, DependencyInjection, ErrorHandler, EventDispatcher, Filesystem,
Finder, HttpFoundation, HttpKernel, Routing and VarExporter, their Symfony
Contracts/polyfills and PSR cache/container/event/log contracts. At the pinned
commit this is roughly 980 package PHP files and 132,800 lines when upstream
test directories are excluded. FrameworkBundle declares `ext-xml` even when
the first fixture uses PHP configuration. The closure contains 190 attribute
tokens, DNF types, an enum, anonymous classes, first-class callable syntax,
by-reference returns, dynamic call/member forms and `goto`; it also uses
Reflection, generated PHP, `eval`, serialization, error-handler stacks,
filesystem locks and complex PCRE extensively. This is therefore a framework
compatibility project, not a list of ten missing functions.

### Exact meaning of the first Symfony version

Keep the target narrow and reproducible. Create a repository-owned minimal
fixture manifest, but download Symfony and Composer artifacts outside the
repository. Pin the upstream commit, a generated `composer.lock`, reference
PHP version, Composer version, enabled extensions, OS/architecture and RPHP
feature set. Constrain every Symfony package in the lock to the 7.4 line so a
permissive transitive constraint cannot silently install an 8.x component.
Do not patch vendor source or classify a skipped file as compatible.

Progress is admitted through these separate gates:

| Gate | Required outcome |
|---|---|
| S0: vendor autoload | RPHP loads an unmodified Composer-generated `vendor/autoload.php` and resolves a small PSR-4 class/function fixture. Composer may run under reference PHP to create `vendor/`. |
| S1: component substrate | Unmodified EventDispatcher, HttpFoundation Request/Response, compiled Routing and a small prebuilt DI container execute with reference-equivalent output. |
| S2: warmed kernel diagnostic | A production, `debug=false` FrameworkBundle kernel loads a cache/container generated by reference PHP and handles an in-memory `/health` request. This is useful localization evidence, not the compatibility claim. |
| S3: cold kernel boot | RPHP itself builds and writes the container and route cache from PHP configuration, reloads the generated files and handles `/health` plus a missing route. This is the first Symfony-compatible RPHP checkpoint. |
| S4: first API runtime | The same app receives a real HTTP request, publishes status/headers/body, terminates and handles repeated requests without state, resource or memory leakage. This adds the web/runtime claim; it is not required to prove the earlier CLI kernel gate. |
| S5: self-hosted tooling | `bin/console` and then Composer itself run under RPHP. This follows S3 and must not block the first kernel boot. |

The first fixture uses FrameworkBundle only, `APP_ENV=prod`, `APP_DEBUG=0`,
one explicit PHP-configured route, one service/controller and a plain
HttpFoundation `Response` or `JsonResponse`. Twig, Doctrine, SecurityBundle,
Mailer, Messenger, Redis and an external HTTP client are deliberately absent.
They become named expansion gates after S3 rather than hidden prerequisites.

### S0 blockers: Composer files, symbols and autoload

Implement the real Composer loader contract before framework code. Required
behavior includes nested `include`/`require` return values and scope,
`*_once` canonical identity, `__DIR__`/`__FILE__`, case-insensitive PHP symbol
lookup, namespace function imports, class constants and deterministic error
propagation. Add `spl_autoload_register()`, `spl_autoload_unregister()` and
`spl_autoload_functions()` with stack order, prepend and throw behavior.
`class_exists()`, `interface_exists()`, `trait_exists()` and `enum_exists()`
must honor their autoload argument; `class_alias()`, `function_exists()` and
`method_exists()` must match PHP visibility/name behavior rather than merely
consulting an already-populated table.

Current S0 checkpoint: explicit callable registration, unregister/listing,
order, prepend, duplicate identity, recursion suppression and exception
propagation are implemented in request-local state. Object-method callbacks can
load a class through ordinary `require`; existence probes autoload by the
correct symbol kind, and `method_exists()` autoloads string owners while seeing
abstract and non-public declarations. The callback stack is published as an
immutable `Rc` slice, so a lookup takes an allocation-free snapshot even when a
loader mutates the live registry. S0 is not complete: default/null registration
and `spl_autoload()`, the remaining include-return/scope edges,
`class_alias()`/`function_exists()` behavior and the unmodified Composer fixture
gate remain outstanding.

Composer's generated platform check must observe truthful `PHP_VERSION_ID`,
`PHP_VERSION`, `PHP_INT_SIZE`, extension availability and exit/error behavior.
RPHP currently advertises PHP 8.4.0, so Symfony will select PHP 8.4 branches.
Before S0, make the first framework contract explicit and global: target PHP
8.2.0 for the Symfony 7.4 gate and set `PHP_MAJOR_VERSION`,
`PHP_MINOR_VERSION`, `PHP_RELEASE_VERSION`, `PHP_VERSION_ID`, `PHP_VERSION`
and `phpversion()` consistently to `8`, `2`, `0`, `80200` and `8.2.0`.
Validate that identity against the pinned PHP 8.2 PHPT baseline and Composer's
generated platform check; if the baseline disproves the claim, fix the gaps
rather than advertising a version RPHP does not yet implement. Newer syntax
may remain available as an RPHP extension, but it is not part of this first
compatibility contract.

Do not implement this as a Symfony-only override, claim an extension in
`extension_loaded()` without its required behavior, or change the identity
only to evade one discovered branch. Promote the global contract to at least
PHP 8.4.1 (`80401`) only after the PHP 8.4 language, native lazy-object and
Reflection gates are complete; this promotion is the entry condition for the
pinned Symfony 8.1 follow-up.

### S1/S3 blockers: language and object model

Close these source-level gaps against focused PHPT cases before treating a
missing Symfony class as a library problem:

- PHP attributes on classes, methods, properties, parameters and constants,
  repeatability/targets, attribute argument constant expressions and the
  built-in `Attribute` class;
- `ReflectionAttribute::IS_INSTANCEOF` and attribute instantiation, which are
  fundamental to Symfony routing, autowiring and autoconfiguration;
- anonymous classes, first-class callables such as `$this->getId(...)`, all
  callable array/string/closure forms and `Closure::bind()`, `bindTo()`,
  `call()` and `fromCallable()` scope semantics;
- parenthesized DNF types such as
  `(NodeDefinition&ParentNodeDefinitionInterface)|null`, complete union and
  intersection variance checks, `static`/`never`/literal types and exact
  nullable/default compatibility;
- `goto` within a function, functions and magic methods returning by
  reference, reference-preserving property/array access, dynamic static
  property access and variable class/method/function invocation;
- enum/readonly/promoted-property edge behavior, trait conflict/alias rules,
  magic methods, cloning, destructors, serialization hooks and exception
  chaining/unwind behavior used by generated containers and lazy services;
- `eval()` with caller namespace/scope, definitions that remain visible after
  evaluation, correct parse/fatal errors and filenames. Symfony uses it for
  lazy service code and compiled routing conditions during a cold build.

Implement the built-in object protocols consumed by userland code:
`Traversable`, `Iterator`, `IteratorAggregate`, `ArrayAccess`, `Countable`,
`Stringable`, `JsonSerializable`, `UnitEnum` and `BackedEnum`. Add the concrete
SPL/runtime objects needed by the measured load path, including `ArrayObject`,
`ArrayIterator`, `IteratorIterator`, `RecursiveIteratorIterator`,
`RecursiveDirectoryIterator`, `FilesystemIterator`, `GlobIterator`,
`SplFileInfo`, `SplObjectStorage`, `WeakMap` and `WeakReference`. Iteration,
array casts, object identity and weak lifetime behavior must be observable PHP
behavior, not empty class stubs.

RPHP's PHP 8.4 identity also activates Symfony's native lazy-object probes.
The first fixture should avoid lazy services, but S3 must fail explicitly if a
lazy definition is requested. Full Symfony 7.4 coverage then requires the
relevant `ReflectionClass` lazy ghost/proxy/reset/inspection contract and PHP
8.4 asymmetric property visibility. Symfony 8.1 additionally requires parsing
and executing property hooks, including their Reflection flags and inheritance
rules.

### Reflection closure required by DependencyInjection

The existing generics-oriented Reflection subset is not sufficient. Implement
ordinary PHP Reflection independently of generics and cover at least:

- `ReflectionClass`, `ReflectionObject`, `ReflectionFunction`,
  `ReflectionMethod`, `ReflectionProperty`, `ReflectionParameter`,
  `ReflectionClassConstant`, `ReflectionAttribute`, `ReflectionEnum` and the
  named/union/intersection type hierarchy;
- names, modifiers, visibility, declaring class, parent/interface/trait graph,
  constructor, methods, properties, constants, parameters, defaults,
  variadics, return/by-reference state and attribute queries;
- source filename and start/end lines, doc comments, namespace/short names,
  extension/internal/user-defined classification and static/default property
  values;
- `newInstance()`, `newInstanceArgs()`, `newInstanceWithoutConstructor()`,
  method/function closures, invocation, accessibility, property
  initialization/read/write and exception-field access with PHP scope rules.

Build a manifest from the pinned source and mark every invoked Reflection
method pass/fail. A class existing under the correct name but returning
placeholder metadata is a failure because Symfony compiles that metadata into
the service container.

### Standard-library and extension closure

Use execution tracing from S0--S3 to reduce priority, but retain the complete
static inventory. The minimum families observed in the pinned closure are:

| Area | Required surface before or during S3 |
|---|---|
| Introspection/call frames | `get_debug_type`, `is_a`, `is_subclass_of`, `get_parent_class`, `class_implements`, `class_parents`, `class_uses`, `get_object_vars`, `get_declared_*`, `func_get_args`, `func_get_arg`, `func_num_args`, `debug_backtrace` |
| Arrays/iterators | exact existing array functions plus `array_replace*`, `array_diff_key`, `array_intersect_key`, `array_is_list`, `array_walk_recursive`, `iterator_to_array`, `iterator_count`, `current`, `key`, `reset`, `end`, `next`, `prev`, `uasort` and `uksort` |
| Strings/PCRE | `strtr`, `preg_quote`, full `preg_*` flags/errors/captures, `strcmp`, `strcasecmp`, `strncmp`, `substr_compare`, `strspn`, `strcspn`, `strpbrk`, natural comparisons, `addcslashes`, `stripcslashes`, `strip_tags`, `ucwords` and `vsprintf` |
| Serialization/export | byte-compatible `serialize()`/`unserialize()` including references, private/protected keys, allowed classes and `__serialize`/`__unserialize`; executable `var_export()` output; `pack()`/`unpack()` where reached |
| Errors/process state | `set_error_handler`, `restore_error_handler`, `get_error_handler`, exception-handler equivalents, `error_reporting`, `trigger_error`, `error_get_last`, `register_shutdown_function`, `assert`, `ini_get`, `ini_set`, `get_cfg_var`, `extension_loaded` and accurate Throwable subclasses/fields/traces |
| Filesystem/streams | modes and metadata for `fopen`/read/write/seek/close, `flock`, `filemtime`, `filesize`, `touch`, `chmod`, `umask`, `scandir`, `is_link`, `readlink`, `symlink`, `glob`, atomic rename, temp files, stream metadata/locality and warning behavior |
| Hash/random/time | `hash` family, `hash_equals`, `md5`, `crc32`, `bin2hex`/`hex2bin`, `random_bytes`, `random_int`, `version_compare`, `strtotime`, `gmdate` and timezone APIs reached by the fixture |
| Output/HTTP/session | output-buffer stack (`ob_*`), `header`, `header_remove`, `headers_sent`, `headers_list`, `http_response_code`, `setcookie`, flush behavior and then the session API for the session expansion gate |
| XML/text extensions | truthful `ext-xml` core behavior for FrameworkBundle's declared platform requirement and compatible ctype and mbstring/polyfill decisions. libxml/DOM/SimpleXML become explicit XML-loader expansion gates rather than a fake extension flag. Optional sodium, APCu, Redis, Memcached and opcache must report unavailable cleanly until implemented. |

RPHP already implements parts of several rows; admission is based on signatures,
warnings/exceptions, references, flags and edge behavior, not only the function
name. Extract the exact call and constant manifest from each pinned vendor lock
and record newly reached symbols rather than guessing from the entire Symfony
monorepo. Complex route and container regexes become a permanent differential
PCRE corpus because a limited matcher that accepts simple benchmarks is not
enough for Symfony.

### Cold container, cache and generated-code gate

S3 must compile the container without a pre-generated cache. Begin with PHP
configuration to avoid making YAML a hidden requirement, but keep the package's
declared XML platform contract visible. The kernel must discover bundles,
compile service definitions and compiler passes, dump executable PHP into an
isolated cache directory, atomically publish it under the correct lock, include
it in a fresh RPHP process and reuse it on the next boot. Generated closures,
anonymous/lazy service code, class constants, environment placeholders and
removed/private service behavior must match the reference output.

Compare a machine-readable manifest of included files, declared symbols,
container parameters/service IDs, compiled routes, response, diagnostics and
exit status against reference PHP. File mtimes and random cache suffixes are
normalized only through documented fields; do not compare only the final
`Hello World` body. Run cold, cached, deleted-cache and malformed-cache paths.
Reference-PHP-built cache remains an S2 diagnostic and may never satisfy S3.

### Request, response and repeated-worker gate

First handle a synthetic HttpFoundation `Request` under CLI. Then provide the
S4 adapter with correctly initialized `$_SERVER`, `$_ENV`, `$_GET`, `$_POST`,
`$_FILES`, `$_COOKIE` and request body/`php://input`; trusted host/proxy data,
method, URI, query, content length and headers must match PHP. Publish response
status, duplicate/multi-value headers, cookies and binary body exactly, then
run kernel termination and service reset.

A short-lived one-request process may reach S4 before the production cycle
collector. A persistent coroutine worker may not: cancellation, exceptions,
destructors, resources, statics, superglobals, output buffers, handlers and
container reset state must be isolated between requests, and Interphase 5.6's
cycle/root model is its release gate. The first HTTP adapter stays independent
of FPM compatibility; FPM/CGI protocol support is a later deployment choice.

### Verification and performance admission

Keep one differential runner for every gate. It records the exact RPHP,
Symfony, lockfile and reference-PHP identities and classifies parse, compile,
missing symbol, unsupported extension, runtime, output, crash and timeout
separately. Maintain a shrinking failure manifest; never add a vendor patch or
blanket exclusion to improve the headline. Use upstream component tests only
after the minimal fixture localizes failures, and publish both the supported
fixture result and broader unmodified-suite result.

Correctness precedes framework-specific optimization. Once S3 is exact,
measure Composer autoload, cold container build, cached kernel boot, route
match, controller/DI invocation and repeated in-memory request throughput with
and without RPHP JIT against PHP with and without JIT. Profile missing general
regions; do not recognize Symfony class or method names in the optimizer.
Autoload and cold-build improvements may allocate, but a cached unchanged
request must not gain unbounded metadata, frames or cache entries across
iterations. Existing non-framework controls retain their one-percent
regression ceiling.

After S3, expand in this order unless corpus evidence changes it: Console and
Dotenv; Twig; PDO plus Doctrine DBAL/ORM; Security and sessions; Cache adapters
for Redis/Memcached; HttpClient/cURL; Messenger and long-lived workers; then
Symfony 8.1 language/Reflection compatibility. Each is a separate lockfile and
capability gate. Passing the minimal kernel must not be marketed as automatic
compatibility with this ecosystem.

## Interphase 5.59: unsafe invariant audit and enforcement

Before production or untrusted-code claims, reconcile the implementation with
`docs/unsafe-policy.md`. The preventive checkpoint is complete at `4d563a9`:
CI enforces a production ceiling of 1,628 explicit unsafe blocks, a ceiling of
289 unsafe function declarations, a floor of 58 `SAFETY:` annotations and the
current zero-section floor for `# Safety` contracts. The audited refactor
started from 1,669 blocks. The checker parses its baseline as data rather than
shell, includes untracked Rust sources in local diff checks, and has a
dependency-free adversarial self-test. Test-only unsafe remains reported
separately. This is a prevention ratchet, not completion of the soundness
audit, and does not block publishing an explicitly pre-alpha repository; the
remaining work is still a release gate for production embedding, long-lived
servers and a stable native extension ABI.

Every new unsafe block needs a concrete local proof, every new unsafe function
needs a caller contract, and the aggregate production budget may not rise
through an unexplained baseline edit. Changed locations are reported by file
and diff hunk so moving debt cannot hide its review context. The check is
evidence for review, not a substitute for reading the invariant. Mechanically
generated comments and a single vague proof over a large dispatcher are not
accepted.

Continue reducing the legacy surface in bounded architectural slices. Document
the tag/union and ownership rules of `Value`; frame, slot, stack and instruction
pointer lifetimes; executable-memory state transitions and ABIs; coroutine
suspension; and quick/JIT publication and side-exit rules. Centralize repeated
raw operations behind narrow types such as checked frame/slot pointers and an
executable-memory owner where measurement shows no regression. Audit
`baseline_dispatch.rs` in semantic groups rather than as one roughly
550-operation comment patch, followed by the remaining hot executor,
object/call, coroutine/resource and architecture-specific JIT code.

After a module is audited, enable `deny(unsafe_op_in_unsafe_fn)` for that module
and keep it enabled. The final gate removes the crate-wide allow, gives every
remaining unsafe function a `# Safety` section and every remaining unsafe
operation or tightly related group an adjacent concrete proof. Miri-eligible
representation tests, sanitizers, fuzzing, repeated unwind/cancellation tests,
JIT ABI and side-exit checks, and ARM64/x86-64 verification cover the portions
each tool can observe. Performance controls must remain within one percent and
native code generation must not regress; otherwise redesign the safe boundary
instead of silently restoring undocumented unsafe.

## Interphase 5.6: production memory lifecycle and cycle collection

Before a long-lived coroutine/API process is called production-ready, complete
the PHP value lifecycle beyond the current deterministic `Rc`/`Drop` and
string/array COW substrate. The common acyclic path must remain immediate and
cheap: the last reference frees its value without a tracing pass. Add cycle
collection only for graphs that can remain alive after their external owners
disappear. This milestone is not required for short-lived CLI compatibility,
but it is a release gate for request reuse, persistent workers and servers.

Keep the 16-byte `Value` and do not add a per-value tracing header or branch to
scalars and non-participating programs. Collectable arrays, objects, closures,
generators and references should enter a bounded candidate/root buffer only
when a reference decrement leaves a non-zero count or another operation makes
a cycle possible. Evaluate a trial-deletion/root-color algorithm compatible
with PHP's observable lifecycle, but admit the concrete implementation only
after it beats broader alternatives on real object graphs. Internal metadata
must continue to use weak or interned ownership where that avoids creating
artificial user-visible cycles.

Define the complete root model before collecting anything. Live roots include
VM frames and globals, static variables, pending arguments and exceptions,
closures, generators, resources, suspended coroutine stacks, channels and
waiters, scheduler queues, in-flight typed decoder state, and values published
or shadowed by quick/JIT regions. Persistent class/function metadata, interned
schemas and explicitly persistent application state form a separate lifetime
domain and must not be reclaimed at a request boundary. Cancellation and
exception unwind must unregister task-owned roots exactly once.

Collection may run only at explicit VM safepoints where native regions have
published every live PHP value and retain no untracked movable/borrowed pointer.
A JIT region may elide refcount traffic for a proven borrowed value, but the
owner remains a visible root until the region exits or reaches a publishing
safepoint. Side exits, overflow replay and coroutine suspension must expose the
same root set as canonical execution. The first collector may be non-moving;
introducing relocation before all raw-pointer and inline-cache lifetimes are
audited is out of scope.

Support PHP-compatible control surfaces once their underlying semantics are
available: automatic collection, `gc_enable()`, `gc_disable()`,
`gc_collect_cycles()` and useful status/counter reporting. Candidate recording
while collection is disabled, destructor order, object resurrection, weak
references/maps and exceptions thrown during destruction require explicit
differential coverage. Cyclic arrays, mutually linked objects, self-capturing
closures, generator/coroutine cycles and resources reachable from cycles are
permanent corpus cases.

A request/task arena may bulk-release proven request-local allocations and is a
planned optimization, not the semantic collector. Any value escaping into a
global, persistent cache, another coroutine or an external resource must be
promoted or registered in the correct lifetime domain before the source task
ends. Typed streaming should avoid materializing record graphs where possible,
thereby reducing collector pressure, but it must remain correct when a record
escapes and becomes an ordinary cyclic-capable PHP object.

Implement this interphase in measured slices:

1. audit every owning `Value` edge, destructor and raw-pointer lifetime;
2. add debug root/provenance accounting and generated cyclic-graph tests;
3. introduce a feature-gated candidate buffer and explicit collection call;
4. integrate request completion, coroutine suspension/cancellation and
   persistent-root separation;
5. add compatible automatic thresholds and memory-pressure triggers;
6. optimize candidate recording and enable the collector by default only after
   the performance and pause gates hold on both architectures.

Permanent gates include zero leaks in generated reachable/unreachable cyclic
graphs, exact destructor/resurrection behavior, stable RSS across repeated
request and coroutine-cancellation loops, and no use-after-free under forced
collection at every legal safepoint. Measure allocation count, retained bytes,
candidate-buffer traffic, collection work, maximum and p99 pause, and
throughput. Ordinary acyclic scalar/string/array/object controls may regress by
at most one percent, must add no steady-state allocation, and JIT loops that do
not create collectable graphs must not gain a per-iteration GC branch.

## Interphase 5.7: architecture-specific runtime intrinsics

Add a small, measured layer of ahead-of-time native kernels for operations
whose cost is dominated by stable byte, scalar or register work. This layer is
separate from the dynamic JIT: the JIT specializes a proved PHP region, while a
runtime intrinsic is a prebuilt implementation of one narrow operation that
canonical execution, quick regions and generated machine code may all call.
Do not translate whole PHP built-ins or their observable semantics into
assembler.

Every intrinsic starts with a portable Rust reference implementation. Prefer
`core::arch` intrinsics when they produce the intended code and introduce
handwritten assembler only after disassembly and paired measurements prove a
material remaining gap. Each native entry accepts plain buffers, lengths,
indices or fixed scalar state and returns a value, index or status code. It must
not allocate a PHP value, unwind through foreign assembly, invoke user code or
hide an exception. A Rust wrapper retains type/shape guards, allocation,
errors, refcounting and exact PHP behavior.

The first architecture-level candidates are operations that cannot be
expressed cleanly through ordinary Rust control flow: coroutine register/stack
handoff where the selected context design requires it, JIT entry/exit
trampolines, side-exit publication and compact safepoint shims. Data kernels
follow only from profiles of completed compatible functions. Likely candidates
include JSON structural/string scanning, escape detection, UTF-8 validation,
Base64, byte/string search, hashing, integer parse/format, binary-format scans
and proven packed typed-array validation or reductions. Stored-length
`strlen()`, network/database waiting, object dispatch, reflection, exception
semantics and an entire regular-expression engine are not assembler projects.
Use platform memory primitives when they already beat an RPHP implementation,
and do not create custom cryptographic assembly.

Provide runtime ISA dispatch for distributed binaries: an ARM64 baseline may
select measured NEON kernels, while x86-64 selects among its baseline, AVX2 and
later AVX-512 implementations only when CPU and operating-system state permit
them. Select instruction-set capabilities rather than branding paths as Intel
or AMD; add a microarchitecture-specific variant only when reproducible corpus
measurements justify its code-size and maintenance cost. Native-CPU builds may
resolve the same choice at build time. Unsupported CPUs always use the portable
implementation.

JIT backends consume the same internal intrinsic ABI. They may inline a tiny
operation when profitable or emit one guarded call for a larger kernel, so a
future `json_decode`, typed stream or array pipeline can combine generated
scalar/control-flow code with an optimized scanner without teaching the JIT
the complete PHP function. The intrinsic ABI must publish enough ownership and
safepoint information that collection, cancellation and side exits see the
same live values as canonical execution.

Develop this layer in bounded slices:

1. define the portable contract, architecture registry and forced-backend test
   controls without changing an ordinary call site;
2. centralize existing JIT/backend trampolines behind that contract;
3. select one profile-proven data pilot, preferably JSON structural scanning or
   UTF-8 validation, and keep its Rust implementation as the oracle;
4. integrate successful kernels with typed-data, string and array planners;
5. add handwritten assembly only where intrinsics fail a recorded codegen or
   performance gate;
6. enable automatic ISA dispatch after ARM64 and x86-64 differential, fuzz and
   application-corpus gates pass.

Permanent gates compare portable, forced architecture variants and canonical
PHP output over boundary-aligned, malformed, short and randomized inputs.
Audit register clobbers, stack alignment, unwind prohibition, sanitizer/fuzz
results and disassembly. Measure call overhead and retain scalar thresholds so
small inputs do not lose to SIMD setup. No intrinsic may change PHP output or
error timing, add an allocation to its wrapper's steady-state path, regress an
unrelated control by more than one percent or remain merely because a synthetic
microbenchmark improved.

## Interphase 5.72: native BigInt, BigDecimal and GMP/BCMath family

Build one optional, dependency-free arbitrary-precision family as the first
substantial numerical consumer of the runtime-intrinsic contract. Native
`BigInt` and `BigDecimal` types, plus GMP and BCMath compatibility surfaces,
belong to the same phase and share one magnitude, allocation, conversion and
architecture-kernel substrate. The compatibility façades must not copy through
an unrelated representation on every call. This does not change PHP `int`,
`float`, implicit numeric promotion, overflow, comparison or serialization
semantics; ordinary programs pay nothing for the opt-in family.

Start with an immutable namespaced API such as `RPHP\Math\BigInt`; decide the
final public name only after checking userland and extension compatibility.
Required construction and conversion cover signed decimal and explicit bases,
checked conversion to PHP `int`, canonical string output and a documented
binary form. The initial arithmetic surface includes compare, add, subtract,
multiply, quotient/remainder with defined signed behavior, shifts, power, GCD
and the modular operations needed by real packages. Operator syntax is a later
language decision, not a prerequisite; methods keep the first slice additive
and unambiguous.

Add an immutable `RPHP\Math\BigDecimal` in the same phase. It stores a `BigInt`
coefficient plus explicit decimal scale and applies a required rounding/context
contract at operations whose result is not exact. Construction, comparison,
add/subtract, multiply, divide, quantize/rescale and canonical formatting must
define trailing-zero, zero-sign and scale behavior before optimization.
BCMath's string API and process/request `bcscale()` behavior are compatibility
adapters over this core, not the semantic definition of the native class.

Do not widen the 16-byte `Value` or represent limbs as PHP array entries. The
shared core stores a normalized unsigned magnitude as little-endian
machine-word limbs; `BigInt` adds sign and `BigDecimal` adds sign/coefficient,
scale and rounding context. Both use immutable, reference-counted native
payloads. Rust owns allocation, normalization, algorithm selection, decimal
scale rules, exceptions and size limits. The portable oracle implements
word-vector add/subtract, carry/borrow, shifts, multiply and division first;
introduce Karatsuba or other large-operand algorithms only at measured crossover
points rather than as an up-front complexity commitment.

Architecture-specific work stays below the object API. Candidate intrinsic
contracts include add/subtract vectors with carry, multiply-add by one limb,
full limb products, shifts and selected normalization/search operations. ARM64
and x86-64 variants use the shared forced-backend and differential gates from
Interphase 5.7. Handwritten assembly is admitted only when Rust/intrinsics miss
the required carry chain or instruction schedule in normalized disassembly.
The JIT target is stronger than a faster method call. Once it proves exact
`BigInt`/`BigDecimal` classes, immutable operands, stable scale/rounding context
and non-escaping intermediates, it should fuse the common arithmetic expression
into one native region. Fixed small limb counts may remain in registers;
variable-size loops call or inline the selected machine-code kernels; temporary
normalization and publication are delayed; and only the final observable result
is materialized when safe. The goal is for nearly the entire admitted arithmetic
region to execute as generated or prebuilt ARM64/x86 machine code. Allocation
failure, division by zero, invalid scale/rounding, guard failure and uncommon
algorithms return through an exact canonical Rust boundary rather than being
reimplemented unsafely in assembly.

Develop useful `gmp_*` and `bc*` compatibility adapters in this same phase once
the shared operations and their edge semantics are covered. Common GMP integer
values should wrap the native `BigInt` payload directly; BCMath operations
should route through `BigDecimal` with compatible scale and string behavior.
Advanced GMP number-theory functions may arrive in measured slices, and the
project must not claim drop-in extension compatibility from a superficially
similar API. Typed-data plans may explicitly decode a decimal string or
compatible integer token into `BigInt` or `BigDecimal`, while ordinary
`json_decode()` retains PHP's existing number and `JSON_BIGINT_AS_STRING`
behavior. Never expose the internal limb layout as the portable serialization
contract.

Permanent correctness gates include zero, sign combinations, leading zeros,
base boundaries, carry/borrow across every limb, alias-independent immutability,
division identities, powers, adversarial shifts, conversion overflow and
generated algebraic properties over thousands of operand sizes. Add explicit
decimal scale/rounding/quantize matrices and memory/work limits for hostile
decimal strings, exponents and shifts. Differential lanes cover the admitted
GMP/BCMath surface as well as the native classes. Compare against an independent
oracle where available, while keeping the production runtime free of that
dependency.

Performance gates separate construction, parse/format and arithmetic at small,
medium and large bit widths and decimal scales. PHP `int`/`float` controls must
remain unchanged. Small native operations must not be forced through SIMD/ASM
when call setup wins, while fused non-escaping chains must measure the promised
allocation and dispatch removal. Large-operation claims include allocation,
normalization and decimal rescaling, with fair GMP/BCMath comparisons where
available. Retain a native/JIT path only when it improves end-to-end workloads
such as counters, financial decimal arithmetic, protocol identifiers or modular
arithmetic on both reference architectures.

## Interphase 5.75: first-class typed data interchange and streams

After the generic metadata/runtime contract is stable and Phase 5 has enough
production compatibility to exercise real applications, add one language-owned
typed-data path shared by textual, binary and schema-driven formats. This is not
merely a collection of convenience parsers. Its purpose is to carry a type proof
from input validation through object/collection layout into no-JIT quick regions
and the JIT, without first materializing an `array<mixed>`, reflecting over it
and hydrating a second object graph. It is not a prerequisite for PHP
compatibility and must not delay the ordinary Composer/framework surface.

Keep the existing compatible APIs and semantics. Add an explicit target-class
path that works with generic syntax disabled and a generic path that expresses
the exact result relation in the language, for example:

```php
json_decode($input);                         // compatible mixed PHP result
Json::decode($input, UserDto::class);        // explicit typed target
Json::decode::<UserDto>($input);             // exact generic result
Json::stream($input, UserDto::class);        // iterable object bridge
Json::stream::<UserDto>($input);             // iterable<UserDto>
```

The class-string and generic forms must lower to one `TypedDecodePlan` when the
target is compile-time constant. They are two user-facing contracts, not two
decoder implementations. A dynamic class string may use a guarded schema cache;
unknown targets retain a canonical dynamic path. Erased builds may link a
concrete call-site codec, while reified builds may additionally resolve and
check the exact runtime type tuple. Neither mode may add a branch, allocation or
layout field to programs that do not use the typed-data API.

The shared substrate should expose internal proof-carrying results such as
`Verified<T>`, `TypedArray<T>` and `TypedRecordBlock<T>` without requiring those
names to become public PHP classes. A decoder may establish the proof while it
already reads the input; a second whole-collection scan is not acceptable.
Self-describing inputs still contain claims, not trusted proofs: malformed
tags, lengths, offsets, encodings and schema identifiers must be validated
before admission. A homogeneous fixed-layout block may validate its header,
schema and total byte extent once; a heterogeneous tagged document records a
type/shape summary during its mandatory parse. Mutation either passes a typed
write barrier, materializes an owned PHP value, or invalidates the proof and
forces the canonical path.

Build format support as feature-gated runtime modules over that one internal
codec/plan interface rather than as Composer packages crossing an opaque
extension ABI. The planned coverage is:

- ordinary JSON encode/decode plus typed DTO and streaming-record paths;
- CSV rows driven by a target DTO or explicit schema, reusing the existing
  coroutine stream substrate and CSV compatibility parser;
- XML DOM-compatible fallback plus XSD/DTO-driven streaming hydration, with
  namespaces, attributes, mixed content and entity security kept exact;
- self-describing binary formats beginning with CBOR, MessagePack and BSON;
- schema-driven adapters for Protocol Buffers and FlatBuffers where their wire
  and evolution rules can be preserved faithfully;
- Arrow and, later, Parquet adapters for typed columnar/analytical workloads
  once the Phase 6 typed-buffer ownership contract is stable.

Do not force every format into one physical representation. Maps, ordered
records, tagged unions, packed scalar vectors, object graphs and columnar data
retain format-appropriate canonical fallbacks. What is shared is schema
resolution, validation, property-slot mapping, allocation/borrowing policy,
proof lifetime and the consumer-facing typed plan. Export/import of standard
schema descriptions should be preferred over inventing an RPHP-only wire
format; a later private snapshot format is admissible only for same-runtime
cache artifacts with explicit version and build/schema fingerprints.

The optimized owned path writes validated fields directly into known property
slots or batch-allocated record storage. A frozen/borrowed path may expose a
read-only view over a stable input buffer when the format supports it. Normal
PHP object identity, constructors, hooks, magic access, references and mutation
must not be bypassed unless the class explicitly opts into a data-only contract.
If a streamed DTO escapes the current iteration, it is materialized with normal
identity; escape-free consumers may keep its fields in temporary typed slots.

JIT admission starts only after the canonical typed decoder is correct. A loop
such as `Json::stream::<UserDto>()` followed by typed property reads should guard
the decoder/schema identity and proof generation once, then consume fixed slots
or packed columns directly. Eligible parse -> filter/map/reduce -> encode chains
may fuse without creating every DTO or intermediate array, but parsing,
UTF/number conversion, exceptions, observable partial progress and backpressure
remain exact. A format-specific parser must not become a format-specific
machine-code backend; it produces the same target-neutral typed consumer IR.

Permanent gates include malformed/adversarial inputs, schema evolution,
optional/unknown fields, unions/nullability, integer and floating boundaries,
UTF handling, JSON flags/error state, CSV dialects, XML namespaces and disabled
external entities, binary length/offset corruption, generic mismatches,
mutation/escape materialization and coroutine cancellation. Measure parse-only,
typed hydration, peak allocation, first-record latency, streaming throughput and
end-to-end decode -> typed loop -> encode against the compatible dynamic path
on ARM64 and x86-64. Admission requires no regression to the existing untyped
APIs and a demonstrated win after validation, allocation and materialization
costs are included.

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

### Late Phase 6: native technical-analysis and Trader compatibility library

After the typed CPU numerical path and its statistics primitives are stable,
ship an optional first-party technical-analysis package with a complete target
of PHP's documented `trader_*` surface. The PHP extension is based on TA-Lib;
RPHP must not vendor, translate or mechanically copy TA-Lib source. Implement
the formulas and state machines independently from public mathematical
definitions and an RPHP-owned behavioral specification, then use PHP Trader and
other independent implementations only as differential oracles. Keep a source
provenance record for every formula, convention and compatibility decision.

The coverage manifest includes vector arithmetic/transforms, overlap studies,
momentum indicators, price transforms, statistics, volatility, volume, cycle
indicators and every documented candlestick-pattern function. It also covers
`trader_errno()`, TA-Lib versus MetaStock compatibility modes, lookback/output
indexing and configurable unstable periods; matching only the headline formula
while changing warm-up or starting-index behavior is not compatibility. Newer
first-party indicators may live in the typed API, but do not silently change a
documented `trader_*` result.

Expose two surfaces over one implementation. Procedural `trader_*` functions
accept and return normal PHP arrays with compatible keys/errors. A typed API,
tentatively `RPHP\Finance\Trader`, accepts contiguous `Float64` buffers or
proof-carrying iterables and returns typed series without one `Value` allocation
per sample. Add a stateful streaming form for EMA, RSI, ATR, MACD and other
incremental indicators so coroutine-fed market data updates bounded state
rather than recomputing an entire history. The array façade adapts to the same
plans and never owns a second algorithm implementation.

Build an independent portable Rust oracle first. Share rolling-window sums,
variance, extrema/deques, EMA/Wilder recurrences, regression, OHLC transforms
and output/lookback machinery across indicators. Admit families in measured
slices: vector math and price transforms; moving windows and statistics;
overlap/momentum/volatility/volume; multi-output and cycle indicators; then the
large, branch-heavy candlestick catalogue. A generated manifest tracks every
documented function, signature, defaults, output count, lookback, unstable
period, compatibility mode, error and test status.

The optimized typed path should lower a complete admitted indicator pipeline to
machine code wherever its semantics permit. Element-wise transforms, rolling
sums/min/max, dot products, regression and numeric reductions reuse shared
NEON/AVX/runtime intrinsic kernels; sequential EMA/Wilder and indicator state
machines run as tight JIT-generated ARM64/x86 loops; non-escaping pipelines may
share windows and fuse consecutive indicators without materializing every
intermediate series. Handwritten assembly is reserved for profile-proven leaf
kernels whose Rust/intrinsic codegen remains inferior. User-visible arrays,
errors, mode changes and uncommon side exits remain in the canonical Rust
wrapper, so “all in ASM” means the proven numerical region rather than copied
PHP/TA-Lib control code.

Compatibility float paths retain the documented Trader/TA-Lib numerical model;
do not substitute `BigDecimal` and change rounding merely because the native
type exists. A separate explicit decimal indicator API may later use the shared
`BigDecimal` core for exact-price domains. Define NaN/infinity, missing/short
input, unequal series length, zero denominators, output keys, warm-up and
floating tolerance function by function. Candlestick thresholds and global
compatibility/unstable settings need isolated request/coroutine state rather
than process-global data leaking between tasks.

Permanent gates run known published examples, generated invariants, adversarial
short/constant/monotonic/NaN series and differential PHP Trader comparisons in
both compatibility modes across every function. Performance gates measure the
compatible array API, typed batch API and incremental stream API separately on
ARM64 and x86-64. They include conversion, allocation, output materialization
and multi-indicator end-to-end pipelines; no function is declared faster from a
leaf-only benchmark. Ordinary numerical/PHP programs remain bytecode- and
allocation-identical when the optional package is unused.

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
