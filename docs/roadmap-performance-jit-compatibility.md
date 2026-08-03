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

The mixed routing holdout remains intentionally outside this native shape
because it contains object calls, strings, dynamic arrays, and internal
branches. Eight order-rotated runs retain identical output with medians of
approximately 0.06182 s RPHP JIT, 0.02575 s PHP tracing JIT, 0.06029 s RPHP
without JIT, and 0.06432 s PHP without JIT. The RPHP prototype adds about 2.5
percent overhead on this unadmitted path, while PHP's mature tracing JIT is
about 2.40x faster than RPHP. This is now the clearest coverage gap: the next
widening decision should be driven by mixed typed regions and calls. Direct and
nested scalar-method composition plus non-materialized arithmetic are now
covered, including pure scalar branches. Mixed object effects, strings, arrays,
and their internal control flow remain outside native code.

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
