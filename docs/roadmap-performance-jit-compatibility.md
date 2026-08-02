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
