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
