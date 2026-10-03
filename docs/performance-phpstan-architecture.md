# PHPStan execution architecture comparison

The next change should reduce the work required to execute ordinary PHP values,
using one compiler and VM ownership contract. Another allocator replacement or
Long-only native region does not address the evidence below. RPHP and PHP perform
the same counted iterations in the dominant parser, but our value publication,
operand consumption and frame retirement pass through more execution machinery.
This is a measured direction for a redesign, not a complete attribution of the
instruction gap or a promised speedup.

This review compares the frozen private owned-frame repair with the actual PHP
8.5.11 reference executable. It changes documentation only. The repair has not
passed the performance acceptance gates, and production still has the ownership
defects described in [the lifetime review](performance-phpstan-call-frame-ownership.md).
PHPStan instruction and time parity remains unachieved.

The subsequent [temporary ownership contract review](performance-phpstan-temp-ownership-contract.md)
finds that raw argument owners can overlap compiled scratch slots at entry and
adds a reference-PHP counterexample for reflected variadic argument retirement.
The repaired baseline is not a globally verified lifetime implementation.
A uniform entry-storage contract is required before fresh result publication.

The [physical-copy control](performance-phpstan-array-copy-census.md) subsequently
separates real array storage copies from refcount copies. One whole request
copies 2.674M items in 201,526 physical copies, predominantly COW. Repeated
standard-library snapshots do not emerge as the large-gap explanation. A
stronger argument-identity control also shows why matching counts alone did
not validate original-argument retention on the repaired baseline.

The [operation ledgers below](#operation-ledgers-and-the-architecture-decision)
make the proposed redesign explicit: compare complete language operations,
including the disappearance of their last input owner. A targeted diagnostic of
the active private prerequisite now finds a stale temporary losing its final
object under a result-slot overwrite. This is a concrete ownership-contract
failure, not a measured explanation of the entire performance gap.

The subsequent [private prerequisite screen](#private-ownership-prerequisite-and-its-measured-limit)
repairs the tested entry/consumer boundaries and removes all nine false
live-slot claims in the exact application. It reduces ordinary analysis
instructions by only 0.798%. That is below the material selector: semantic
repair establishes a prerequisite, while most of the performance gap remains.

## What the measurements establish

The independent repair window uses the same five files, twenty findings,
analysis FIFO boundary, serial execution and fresh temporary cache per run.
Startup and result rendering are excluded. The RPHP values are two-pair medians;
PHP is the reference observation in that same window. This is an x86-64
comparison, not an ARM64 result or an assertion about every PHP application.

| Analysis measure | Frozen owned-frame RPHP with PGO | Reference PHP |
| --- | ---: | ---: |
| User instructions | 68.478394 billion | 10.338209 billion |
| Analysis time | 5.9759 seconds | 0.9283 seconds |
| Relative instruction count | 6.624 times PHP | 1 |

The [repair packet](performance-phpstan-call-frame-ownership-data.json) retains
all observations. The older accepted 66.8-billion result uses an ownership model
that fails lifetime controls; it cannot be the correctness baseline for a new
design. Ordinary non-PGO builds take about 75.5 billion instructions in their
separate windows. Comparing an ordinary prototype with the PGO median would
confound architecture and build optimization.

Three independent views narrow the problem:

- [Source counters](performance-phpstan-body-work.md) match PHP exactly at five
  parser sites: 331 entries, 333,081 outer iterations, 263,696 token reads,
  435,666 inner iterations and 435,335 reductions. They exclude extra iterations
  at these sites, not unequal work throughout PHPStan.
- The [native self profiles](performance-phpstan-release-boundary-control.md)
  put 27.761 billion sampled instruction periods in the repaired main executor.
  Main plus other explicitly named VM functions accounts for 67.025% of the
  full 68.458-billion sampled total. This partitions symbols, not removable
  semantic costs. Even eliminating main entirely would leave about 40.7 billion
  periods elsewhere: dispatch alone cannot explain or close the gap.
- The [canonical census](performance-phpstan-bytecode-sites.md) records
  269.826 million decoded steps over the whole diagnostic request, excluding
  direct native/typed paths. It is neither a PHP bytecode count nor an
  analysis-only native instruction denominator. Do not divide the native
  analysis total by this count and call the quotient a cost per PHP iteration.

PHP's advantage here does not require JIT. The installed reference reports
`opcache.enable_cli=0`, `opcache.jit=disable` and GC enabled. Its exported VM APIs
report hybrid mode and global VM registers. Those last two facts are also
consistent with its disassembly: frame state in r14, instruction state in r15,
and indirect dispatch through the instruction handler. The
[generated VM implementation](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_execute.h#L371)
explains that dispatch. The official release source is pinned by commit; it
does not by itself reproduce the distribution's compiler flags or patch set.

## How the two interpreters perform the same operations

| Area | RPHP today on the repaired baseline | PHP 8.5.11 | Consequence and evidence limit |
| --- | --- | --- | --- |
| Values | A 16-byte tagged Value with Rc owners and array COW. Normal string, array and object clones bump a refcount; they do not deep-copy their payload. | A 16-byte zval with refcounted payloads and array COW. Read results can also bump refcounts. | The difference is not inherently Rust versus C, or copies versus no copies. Compare complete operation protocols. |
| Operand addresses | A compact 16-byte instruction; getters select Const, CV or TMP/VAR, with reference handling. TMP/VAR indexes already resolve to absolute frame slots. | Operand-specific generated handlers use compiled variable byte offsets and literal addressing. | Some static storage choices are resolved before execution in PHP. Any saving in RPHP must exceed the cost of a new selector and metadata loads. |
| Defined variable reads | The compiler already emits direct CV operands for proven defined variables; uncertain reads use FetchCvR snapshots. | Generated CV handlers retain undefined-value and reference semantics. | FetchCvR is only 1.733 million captured steps. Removing snapshots from every imagined CV read is not an available large budget. |
| Property reads | Existing class/layout and scope caches, scalar specializations, owned result publication, temporary ownership metadata and modifying-read contexts. | A warm constant-name class/offset cache can copy the property directly into the result; exceptional and hooked cases remain separate. | RPHP already has caching. The candidate difference is what a cache hit still has to do, not the absence of an inline cache. |
| Array reads | Existing packed, small, linear and indexed storage; integer-prefix and scalar read paths. A result is published through the slot protocol, with operand releases represented separately where emitted. | Array lookup, dereferenced result copy and consumption of temporary operands occur in the read handler. | The ordinary successful read can combine semantic work and operand retirement in PHP. This is the strongest concrete vertical slice to compare. |
| Assignment | Explicit source ownership, destination replacement, bitmap updates and callback/exception handling; eligible sources already move. | Assignment owns the source-consumption decision and retires the overwritten destination; a used expression result gets its own copy. | A move opcode alone does not remove destination cleanup or the surrounding protocol. |
| Calls and frames | Contiguous VM slots plus owned ordinary arguments and receivers, call metadata and return retirement. | VM stack space is normally reserved by moving the stack top; call slots own by-value arguments. | Borrowing ordinary call arguments is not PHP's general solution and is already disproved by our lifetime controls. |
| Temporary release and GC | Statement ReleaseTemps, owner bitmaps, wide-frame scanning, snapshot provenance and pending-exception boundaries; general Value drops can register cycle candidates. | Consumed temporary operands use a refcount release without possible-root admission; compiled variables use GC-aware release. | PHP distinguishes release roles. RPHP needs a proved role contract, not globally disabled GC or a rule based solely on a TMP slot number. |
| Frame exit | Retires actual owners in slot order, clears slots before callbacks, preserves exceptions and constructor state, then pops the frame. | Releases compiled variables and call state with callback/exception rules and GC-aware CV destruction. | PHP also cleans up and can run destructors. Our extra classification and publication layers are a hypothesis to reduce, not permission to erase callbacks. |
| Dispatch and code layout | A large Rust match with inlined value and operation logic, shared loop checks and outlined helpers. | Hybrid hot labels and cold handler functions, with reserved VM state registers in this build. | A naive Rust function per opcode is a different architecture; it already increased instructions. Smaller code is useful only if it reduces real executed work. |

RPHP source anchors are
[operand resolution](../src/vm/frame.rs),
[Value ownership and array storage](../src/value/mod.rs),
[compiler variable reads](../src/compiler/compile.rs),
[the executor](../src/vm/execute/baseline_dispatch.rs),
[cached property reads](../src/vm/execute/baseline_object_calls.rs) and the
[preserved frame repair](performance-phpstan-call-frame-ownership-candidate.patch).
The repair's exact identities and PHP source hashes are in the
[architecture packet](performance-phpstan-architecture-data.json).

The PHP comparison follows the actual
[property read](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L2069),
[dimension read](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L1933),
[assignment](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L2810) and
[return](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L4508)
handlers. In particular, ZVAL_COPY_DEREF creates an owned read result. The
[temporary release macro](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_execute.c#L177),
[release primitives](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_variables.h#L32),
[CV cleanup](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_execute.c#L4271) and
[frame allocation](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_execute.h#L344)
explain the lifetime and frame distinctions. They do not prove that the same
release mode is sound for every RPHP-produced temporary.

## A concrete read and assignment

Consider an ordinary cached property containing an array:

```php
$action = $this->action[$state];
```

One captured parser region has this shape. Its local CV numbers and temporary
numbers are illustrative; the full metadata stays in the retained PC packet.

```text
FetchObjR   CV receiver -> TMP array
FetchDimR   TMP array, CV key -> TMP result
ReleaseTemps array range
AssignCv    destination <- TMP result, eligible move
ReleaseTemps result range
```

These five positions each execute 435,335 times at that site. RPHP may perform
the same necessary refcount increments as PHP, then separately rediscover which
slots own values, retire them, reconcile metadata and resume pending exceptions.
On PHP's array fast path, FetchDimR copies the element and consumes its temporary
container inside that handler. Assignment consumes its source inside assignment.
There is no separate release dispatch for those two consumptions. PHP does have
other explicit FREE operations; this example does not establish a global opcode
ratio or authorize deleting all RPHP release markers.

ReleaseTemps accounts for 38.177 million steps, 14.15% of the captured canonical
request; in the dominant parser it accounts for 18.760 million, 25.61% of that
body. Frequency identifies a shared protocol. Its native cost must still be
measured, and an empty release marker can carry an exception boundary even when
it has no owner to drop.

## Operation ledgers and the architecture decision

The comparison unit is a complete PHP action, rather than a Rust function or a
VM dispatch. The following ledgers describe ordinary successful paths; warnings,
references, hooks, callbacks and exceptional exits remain part of the operation
contract. Each proposed saving is a hypothesis until the complete path is
measured against the same-output baseline.

### Reading an array element and assigning it

| Phase | RPHP protocol | PHP protocol | Proposed architectural change |
| --- | --- | --- | --- |
| Locate inputs | Resolve storage classes, dereference inputs and obtain the existing property/array cache or storage path. | Generated operand-specific handler resolves compiled addresses and uses the existing cache/table path. | Finalize static address and lifetime facts once; preserve dynamic lookup and reference checks. |
| Read the value | Existing read paths clone or create the result as required. A heap clone normally copies a handle and increments RC. | The read handler uses ZVAL_COPY_DEREF; heap results also retain an owner. | Preserve required owners. Removing every read clone would change the language semantics. |
| Publish the temporary | The common writer accounts for previous slot ownership, metadata and possible Rust drop. | The handler writes the result into its compiler-managed temporary. | Prove the result slot's state at every entry and predecessor. A dead expression owner must already have been consumed, rather than rediscovered by the next writer. |
| Consume read inputs | Subsequent release markers reconcile a range of slots, provenance, callback work and the observable boundary where emitted. | FETCH_DIM_R performs FREE_OP2/FREE_OP1 and its exception check before the next dispatch. | Put proved last-input consumption in the read operation, with the same callback order and resume position. |
| Assign and finish | Assignment can already move its source; destination replacement and later range release remain distinct protocols. | Assignment takes care of its source, releases the overwritten destination and copies an expression result only when used. | Use the same consumption description for assignment. Delete a release marker only after replacing every remaining owner and boundary effect. |

This removes repeated ownership decisions only if the compiler and executor
agree about the entire chain. Merely fusing dispatch while retaining the old
getters, writers and release planner is the rejected eager value-graph design.
Likewise, a bitmap test added to every fresh writer would compensate for an
unproved contract rather than establish one.

### Releasing a value containing objects

PHP's ordinary
[release primitive](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_variables.h#L32)
decrements the payload's count. Only the final decrement dispatches to a payload
destructor. The
[array destructor](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_hash.c#L1814)
then releases its actual entries in storage order. The
[type dispatch](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_variables.c#L39)
connects that release to object, resource and reference destruction; PHP's
[object callback](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_objects.c#L114)
also handles pending exceptions. Cycle collection is a separate operation when
ordinary decrements leave a cycle alive. This does not mean PHP never traverses
objects, collects cycles or pays callback costs.

RPHP's Rust drop releases host Rc payloads but cannot itself run an ordinary PHP
object callback. VM code therefore prepares callbacks before allowing some
owners to disappear. The repaired baseline uses final-owner tests, value-tree
inspection, retained handles, alias counting and callback plans in different
release roles. Some general retirement paths repeat planning after a destructor
replaces an exception. There are existing shallow/shared exclusions; this is
not a claim that every scalar or shared array receives a graph walk.

| Phase | RPHP distinction | Required replacement contract |
| --- | --- | --- |
| Commit the language effect | Several writers prepare retained callback owners around their write; frame and temporary retirement have different callers. | Detach the real old owner at the opcode's committed write/consume boundary and publish the new observable state before re-entry. |
| Determine final ownership | Host owners, temporary snapshots and runtime mirrors can enter count/planning proofs. | State which owners are semantic storage edges and which are internal retained handles. Check actual final ownership when that edge is released. |
| Retire nested values | Inspection and alias plans arrange callbacks before the later Rust payload drop. | A single VM-aware owner protocol must preserve actual entry order, outside aliases, references, resurrection and remaining siblings after exceptions. |
| Finish storage teardown | Generic nested Rust drops can still lose a final PHP object if a release path bypassed the planner. | Every final language owner must reach the semantic release boundary; a later unrelated slot overwrite must not finish its lifetime. |
| Preserve GC and suspension | Candidate admission, reference cells, generators and Fibers have separate state. | Keep GC-aware storage release, proved snapshot release and resumable callback roots explicit in the same contract. |

The recommended design keeps ordinary Rust and the current Value representation
for the first slice. It moves static lifetime work to compiler finalization and
gives a consuming operation one VM-aware owner-retirement contract. It does not
require a global mutable ExecutorGlobals pointer from arbitrary Rust Drop, a
new allocator, or a custom refcount representation before establishing the
semantic boundary.

Importantly, an earlier
[unique-array owner-consumption prototype](performance-phpstan-array-retirement-rejected.md)
already yielded **+0.02449% instructions**, not a material improvement. Changing
that helper alone is rejected evidence. A new slice must actually remove the
surrounding publication/consumption work and demonstrate coverage; the simpler
release model is not itself a speedup forecast.

### Calls, argument introspection and exceptional exit

Both VMs must retain by-value arguments and preserve reference cells. PHP places
extra arguments after the compiled CV/TMP region; variadic packing does not
authorize discarding the original arguments seen by func_get_args or a trace.
RPHP's frozen repair retains originals through separate snapshot/storage paths,
but its raw entry owners can overlap compiled TMPs. The active private
prerequisite moves original tails to separate stack-owned cells while retaining
the 64-byte frame header. This addresses a semantic precondition, not the
application instruction gap.

The private second revision passes the original-argument identity/destructor
control and the closure-tail control. Its next control still fails:

```php
class TraceOwner {
    function __construct(public int $id) {}
    function __destruct() { echo "drop:", $this->id, "\n"; }
}
function original_throw(...$args) {
    $args = [];
    throw new Exception('control');
}
foreach ([1, 2] as $mode) {
    try {
        if ($mode === 1) original_throw(new TraceOwner($mode));
        else (new ReflectionFunction('original_throw'))
            ->invokeArgs([new TraceOwner($mode)]);
    } catch (Throwable $e) {
        $args = $e->getTrace()[0]['args'];
        echo get_debug_type($args[0]), ':', $args[0]->id ?? 'missing', "\n";
        unset($args, $e);
        echo "caught\n";
    }
}
```

With exception arguments enabled, PHP prints each destructor before `caught`.
The private revision preserves both identities but omits `drop:1` and defers
`drop:2` until shutdown. A feature-only last-owner trace places the first
destructor-enabled final object drop under
`bitmap_drop_and_update -> write_fetch_dim_result -> execute_ex_inner`.
The second final drop is under request shutdown after its destructor ran late.
Raw pointers, native backtraces and local paths remain private; the
[architecture packet](performance-phpstan-architecture-data.json) records exact
identities, output and the sanitized paths.

This establishes a stale owner ending at result overwrite on this control.
It does not yet establish which compiler/exception edge left it live, how often
that edge occurs in PHPStan, or an instruction saving. The next prerequisite is
to trace its real producer, consumers and exceptional successors and make the
same lifetime description govern normal, catch and resumable execution. Adding
another result-writer callback check would preserve the architectural split.
No PHPStan performance cycle is admitted while this control fails.

A subsequent small causal control replaces the compiler's outer-ArrayAccess
condition for Echo cleanup with cleanup of its complete compiled expression
interval. It fixes returned-object and wrapped-array-read lifetimes in that
fixture, but still fails a call-argument boundary:

```php
class EchoOwner {
    function __construct(public int $id) {}
    function __destruct() { echo "drop:", $this->id, "|"; }
}
function echo_type($value) { return get_debug_type($value); }
echo echo_type(new EchoOwner(2)), "after-two|";
```

PHP destroys the argument before printing `EchoOwner`; the private revision
prints `EchoOwner` before destroying it. Both retain the argument while the
function runs. Post-Echo cleanup releases the extra caller temporary too late.
This rejects that revision as a complete fix and confirms that the consuming
**operation**, including a completed call, needs its own lifetime boundary.
The compiler edit is restored after preserving exact source, binary and failed
output. The exception control is not rerun after this earlier control fails;
there is no claim that it is repaired. No PHPStan or native timing run follows.

### Private ownership prerequisite and its measured limit

A later private revision gives original positional argument tails distinct
stack-owned cells outside the compiled CV/TMP region. Its compiler proof permits
ownership transfer only for a single-definition argument temporary with one
consumer and no independent control-flow entry between producer and send.
The same rule applies to ordinary, indirect, named and callback argument sends;
by-value references retain their referent before the temporary wrapper retires.
Completed Echo expressions release their complete bounded temporary interval.
Call validation failures use the existing VM-aware owner retirement boundary.

This is ordinary Rust with the existing Value representation. It fixes the
tested lifetime boundary without another callback check in each result writer.
It does not introduce a fresh writer, borrow ordinary call arguments, remove
release instructions or implement the proposed general read-consumption model.

Eight new entry/consumer controls and twenty-four broader controls match PHP in
exit, stdout and stderr. They cover originals, references/COW/coercion, closures,
traces, generator/Fiber argument storage, nested array entry order, resurrection,
throwing destructors and suspended post-Echo retirement. All thirty-two also
match with the seven retained optimization-disable switches. These switches
exercise the available canonical controls; they are not proof that every
optimized path has been disabled.

The broader fifth revision failed post-Echo Fiber suspension because the new
ordinary owner loop bypassed the existing suspendable release entry. The sixth
revision restores that entry before retirement. A separate initial TypeError
lifetime difference came from unequal default exception-argument INI. Explicit
equal INI removes that difference; an additional validation specimen matches
both settings for type, arity, internal and named argument failures. The exact
PHPStan comparison already explicitly sets that INI equally, so it does not
explain the application gap. Failed intermediate sources and observations stay
retained in the [packet](performance-phpstan-architecture-data.json).

The unchanged conservative CFG diagnostic then checks the live frame against
its static absent-owner facts. Its six solver tests and sixteen retained
ownership controls pass. The actual five-file/twenty-finding request has **zero
violations, down from nine**. Unknown effects still kill facts, and unsupported
effects, resumable and finally bodies decline. This validates the stated
diagnostic envelope; it does not turn an unproved slot into a fresh destination.

Two alternating ordinary pairs measure this private prerequisite separately
from the earlier PGO baseline:

| Analysis measure | Frozen ordinary repair | Private ordinary prerequisite | PHP in this window |
| --- | ---: | ---: | ---: |
| User instructions, median | 75.512893 billion | 74.910462 billion | 10.338186 billion |
| Analysis time, median | 6.7201 seconds | 6.4888 seconds | 0.9245 seconds |

All observations preserve the same exit, output, five files and twenty findings.
There is no warmup; every run uses a fresh temporary cache, the same CPU and
explicit INI, with analysis FIFO boundaries. All counters ran at 100%, all valid
observations remain visible and the verified 6 GiB/no-swap boundary has no OOM.
The instruction change is **-0.797786%**, below the required -2% selector. Two
pairs do not establish a globally accepted time improvement. The frozen repair
still fails the stronger lifetime controls, and this ordinary window must not
be compared with the earlier PGO medians as a build regression.

No PGO, broad feature matrix, architecture acceptance or production runtime
adoption follows. The private contract remains a prerequisite for subsequent
consuming-publication work, rather than an accepted performance solution.

An offline screen also limits the next design: plain immediately adjacent
read/move assignment has only **1,152,255 read hits** in the retained
269,826,212-step whole-request census. This is a structural envelope before
CFG/live-owner proof, not analysis-only native coverage. No read/assignment
fusion or speedup is admitted from it. A useful shared design must reach broader
producer/consumer boundaries and demonstrate its actual removed work; simply
fusing this narrow adjacent shape is not a justified explanation of the gap.

### What this architecture review decides

The next performance slice remains a complete general read/assignment operation
with compiler-proved consumption and one result/owner protocol. The private
prerequisite now establishes the tested entry geometry and consumer boundaries,
without globally accepting its semantics or performance. Array physical-copy
counts do not support mass standard-library snapshots as the next explanation;
dispatch and allocator replacement alone cannot close the measured budget.

The unassigned budget remains substantial. Regex computation has its own native
engine, named VM helpers include lookup/scope/type work, and generic hash/drop/
allocation descendants are not fully attributed to semantic callers. A complete
VM ownership slice must be followed by disjoint attribution of those residuals.
The review establishes why the current protocol can do extra work and how to
test a cheaper one; it does not establish that one architecture change removes
the entire approximately 58-billion difference.

### Broader scratch effects: distinguish a proof from a useful redesign

A later private diagnostic broadens the same ownership model to ordinary Value
producers, explicit call/CV effects and already-proved argument consumption.
Unknown effects still kill facts. At most one explicit definition and modeled
non-escaping uses admit a scratch cell; reference/VAR exposure, hidden writes
and reserved scope cells exclude it. Actual CFG joins include the foreach empty
edge, and catch roots start unknown. Unsupported resumable/finally and skip
effects decline. These are compiler facts, without payload-type or workload
guards in ordinary execution.

Thirteen solver tests, sixteen retained controls and thirteen further
entry/consumer controls pass. The latter use the same explicit exception-argument
INI on both runtimes. The exact five-file/twenty-finding request observes
**17.616251M absent result owners** and **4.367166M empty release intervals**,
with zero live mismatches. An empty interval requires clear prefix owner bits,
even for scalar bytes, and no payload owner in initialized wide slots.

The diagnostic decodes 269.905727M whole-request steps, including 38.177072M
release steps. Its proved empty intervals cover only **11.439% of those release
executions**. These counts do not use the native analysis-only denominator and
do not measure an instruction saving. No empty release is deleted and no fresh
writer is selected. Zero observed pending exceptions at the empty intervals
does not establish that their exception boundary is unnecessary.

This limits the architectural choice: merely making the currently empty
release intervals cheaper leaves most temporary consumption intact. The next
slice must change the complete consuming operation and payload retirement,
rather than adopt this diagnostic as a performance fix. The
[packet](performance-phpstan-architecture-data.json) preserves exact source and
binary identities, all focused results and verified 6 GiB/no-swap boundaries.

One separate output-checked ordinary analysis records 7,482 native self samples
with zero lost samples and a 10,000,019-instruction sampling period. Its flat
self periods are valid, but **7,134 samples have no decoded stack**. Semantic
caller attribution is therefore rejected; plausible fragments are not a
disjoint caller partition. The ordinary build is distinct from the PGO scorecard.
An initial recorder-option failure launched no interpreter and remains recorded.
No timing result or new performance acceptance follows from either diagnostic.

## Where the cost is and what remains unexplained

The frozen repair’s [inline caller budget](performance-phpstan-main-inline-budget.md)
correlates main samples with the following source arms:

| Main source arm | Billion self instruction periods |
| --- | ---: |
| Return | 3.194 |
| AssignCv and BindCvRef | 2.947 |
| ReleaseTemps | 2.770 |
| FetchObjR | 2.396 |
| FetchDimR | 2.364 |

Together they cover 13.671 billion periods. Slot writer and operand getter
inline ancestry covers a deduplicated 5.556 billion union, **overlapping** these
arms. Recognized main inline clone/drop code covers 1.100 billion, also
overlapping. Frame retirement separately has 2.918 billion outlined self
periods. None of these is a safely removable budget; do not add overlapping
groups or infer that eliminating refcount arithmetic removes the whole gap.
Main still has 2.084 billion unresolved source-location periods.

An [older exact machine profile](performance-phpstan-pc-reconciliation.md)
found 5.129 billion instructions with explicit RSP memory operands in RPHP main.
The recognized PHP hot executor and cold handlers together used 6.518 billion
instructions, while RPHP main alone used 27.916 billion. Those are older
Callgrind builds and incomplete cross-VM semantic partitions, not current
hardware medians. They support investigation of materialization, spills and
helper boundaries without proving that stack traffic is entirely unnecessary.

The current ordinary repaired executor is 336,537 bytes with a 3,048-byte local
native stack reservation, versus 42,998 bytes and 152 local bytes for PHP's hot
execute_ex symbol. PHP's cold handlers are outside that symbol; these are not
total VM sizes or PHP heap frame sizes. The code-generation discrepancy is real,
but instruction-cache effects or removable spill costs still need causal tests.

Array layout is a separate hypothesis. PHP uses packed storage or ordered
buckets and a hash-cell area in its table allocation. RPHP's general array uses
ordered entries plus split string and integer indexes, although small/packed
and verified-prefix paths already avoid those indexes. This could change loads,
hashing and allocation locality. Without access-kind and native-budget evidence,
it does not justify replacing every Rust collection. Compare
[PHP's table layout](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_types.h#L400)
with the RPHP storage enum before selecting a layout experiment.

## Why the last optimization directions did not resolve it

The connected read-region prototype was structurally broader than the earlier
scalar plans, but still required Long leaf values, safe vacant outputs, stable
property roots and GC admission. It attempted entry **871,234 times and completed
only 15 original steps**. Property admission rejected 411,412 attempts, existing
destination owners 284,602, native exits 135,841 and poll boundaries 39,285;
warmup accounts for 93. Those rejections plus one completed entry sum exactly to
attempts.
They are this prototype's admission outcomes, not proof of expensive property
lookup, an application-wide type census or a percentage of native time.

Its two planner tests, smoke observations and sixteen ownership controls pass;
the actual request preserves findings. Its one ordinary observation is
76.382 billion instructions, with no paired performance claim. Negligible
coverage triggers the declared stop: no PGO or full matrix follows. Sources and
exact binaries are preserved, the disposable target is deleted, cleanup is
complete, and no runtime implementation is accepted.

That prototype also annotates bytecode before existing whole-function plan
detection. The interaction with those detectors is unresolved, so it is not an
isolated architecture comparison. Rejection preserves that limitation; neither
its guard counts nor its ordinary observation decide the merits of all native
regions or the proposed canonical ownership contract.

The earlier [global storage specialization](performance-phpstan-owned-storage-screen.md)
increased ordinary instructions by 2.117% and main text from 336,537 to
1,221,530 bytes. The [Rust function-per-opcode experiment](performance-phpstan-rust-dispatch-rejected.md)
increased instructions by 5.172%.
These reject those concrete representations. They do not prove that all
predecoded operand information or every smaller executor design must fail.
The common lesson is to remove repeated work without creating another selector,
interpreter or whole-body Cartesian expansion.

## Proposed shared ownership and effect contract

### Architectural decision and cost envelope

The unit of redesign is a complete PHP operation: resolve its input, perform
the language action, publish the result, consume dead inputs and handle the
observable boundary. The review must compare all those steps with PHP before
optimizing one Rust helper. Both implementations already have 16-byte values,
refcounting, COW and cached reads. Rust versus C and allocator origin are not
the demonstrated architectural distinction.

| Proposed contract | Work it should remove | Proof before adoption |
| --- | --- | --- |
| Separate CV, expression TMP, original extra-argument and capture storage roles at entry | Recovering accidental physical-slot ownership and retaining duplicate raw cells through unrelated scratch work | Ordinary, detached, variadic, closure and resumable entries preserve argument values, references, traces and callback retirement |
| A consuming operation owns input retirement and result publication | Separate dispatches and repeated slot/provenance decisions for one read/assignment | All Value kinds, alias/COW behavior, callback order and exceptional exits match canonical PHP behavior |
| One compiler lifetime/effect description used by canonical and typed/native execution | Reconstructing static lifetime facts at execution time or maintaining another interpreter's state | Actual CFG, exceptional/resumable roots and unknown effects are modeled; no owner-absence false positives |
| Smaller successful-operation state exposed to Rust code generation | Repeated materialization and spills around the large executor | Equal semantics and real native instruction reduction; no new hot selector, handler-matrix explosion or unexplained architecture regression |

These are related contracts, not a forecast that one frame-layout fix closes
the gap. The known main read/assignment/release/return arms cover 13.671G
sampled self periods, overlapping the getter/writer attribution. Even removing
all those periods would leave more than 54G in that profile. Eliminating the
entire main executor would still leave about 40.7G. Neither hypothetical
removal is implementable; their purpose is to show that a local improvement
cannot by itself justify a parity claim.

First establish the semantic entry/storage prerequisite, then measure a
complete ordinary operation before and after consuming-publication changes.
Account for remaining outlined VM, standard-library, container and external
work with disjoint native partitions. Every accepted slice must remove work
across ordinary programs rather than compensate with PHPStan-specific guards.
If the first slice fails its material instruction selector, reject the design
instead of treating a cleaner source file as performance progress.

### Shared lifetime representation

This is a proposed architecture, not implemented behavior. The compiler should
attach one lifetime/effect description to the operation that consumes a value.
Canonical execution and typed/native lowering should use that same description.
It must distinguish a retained PHP storage owner from an expression temporary,
an owned reference cell, and a read view with an explicitly proved lifetime.
These describe language ownership roles, not a Long-only fast-path eligibility
list or workload identity.

1. **Resolve static facts during finalization.** Record the operand's storage
   address class, definedness proof, last consumer, alias relationships and
   required source position. Existing direct-CV and absolute-TMP lowering stays.
   Do not introduce a second per-opcode storage-class match or a large duplicated
   handler matrix. Mutable closure/trait scope and polymorphic class/layout
   validity remain runtime facts unless a stable activation contract proves them.
2. **Consume expression operands in the consuming operation.** Publish the result
   as required, transfer eligible result ownership, then retire dead inputs in
   the original PHP-visible order. Allow all existing Value kinds. References,
   COW, magic access, diagnostics and destructor-capable last owners continue
   through the same semantic slow path. A release marker disappears only when
   all its ownership and exception-boundary effects have a proved replacement.
3. **Use an explicit release role.** Compiler/lifetime proof selects a temporary
   snapshot release or storage-owner release. A proven read-only temporary may
   avoid redundant possible-root admission; its actual final refcount still
   releases the payload. Losing the last object/resource owner can still execute
   callbacks. An arbitrary TMP, a generation stamp alone or a global no-GC switch
   is not sufficient proof. Do not hold a Rust borrow across PHP re-entry.
4. **Share result and frame publication rules.** One slot-state contract updates
   ownership metadata exactly when ownership changes. Exception, destructor,
   generator/Fiber and return boundaries materialize the state they observe.
   Ordinary call arguments and receivers remain owners. A later frame redesign
   must use this contract rather than recover the old unsound borrowing result.

The hypothesis is that fewer separate dispatches, metadata reconstructions and
ownership decisions make the same language operation cheaper. It is stronger
than changing a Drop annotation but is not yet a proof that this saves the
roughly 58-billion difference. A successful first slice must reveal how much
cost remains in lookup, frame construction, calls, builtins and data layout.

### A concrete payload-retirement design to evaluate

The tested private owner boundary calls `prepare_replaced_value_release`, may inspect
the value tree and retain a prepared root, runs callbacks, then ultimately lets
Rust drop the payload. Existing shared/final-owner exclusions already avoid
many walks. PHP's release primitive instead decrements the real payload count;
a final count invokes the payload destructor, which releases its actual child
owners. This motivates a single VM-aware retirement protocol for detached
owners. The private prototype below tests this direction; no version has been
adopted into the runtime or accepted as a performance improvement.

The proposed sequence is:

1. Commit the write or consumption and detach its actual owner from observable
   storage before any callback. Keep the operation's source position and pending
   exception policy. An ordinary call argument remains an owned language edge.
2. Release a shared payload's edge without a speculative child walk. On final
   ownership, visit the actual array entries, reference target or object
   properties in language order. Reuse the existing ownership/drop-stack
   primitives where sound; do not clone a graph merely to inspect it again.
3. Run a final object's callback while the object can still be resurrected.
   Re-check the real remaining owners after re-entry before tearing down its
   children. A Rust `Rc::try_unwrap` before that callback would destroy this
   possibility and is not a general implementation of PHP object release.
4. Preserve aliases, COW, property constraints, resource callbacks, exception
   replacement, remaining siblings, cycle admission and Fiber/generator
   suspension. Use an explicit bounded/iterative worklist for deep teardown;
   never retain a Rust borrow across PHP re-entry.
5. Let the same owner protocol serve read consumption, destination replacement,
   argument failure and frame exit. Reject a slice that only relocates the old
   graph planner or leaves its callers doing both protocols.

The possible saving includes preparation and repeated ownership decisions in
outlined helpers, not just release dispatch. The ordinary diagnostic samples
0.99G self periods in `value_tree_requires_vm_release`, 0.89G in
`prepare_replaced_value_destructor_with_references`, 0.73G in
`prepare_replaced_value_release`, and 0.95G in `retire_owned_value`. They are
disjoint function self periods in that one ordinary profile; they are not a
removable budget, an inclusive operation cost or a PGO comparison. Much larger
costs remain elsewhere, so even a successful owner protocol cannot by itself
close the approximately sixfold instruction gap.

### First actual-payload prototype: preparation removed, new protocol rejected

The private prototype retires actual children instead of cloning a root graph
for preparation. It preserves the original object allocation through callbacks
and all child retirement, rechecks resurrection, releases dynamic properties
before declared slots, and removes each typed-reference constraint at its own
edge. Native, lazy and suspension boundaries retain canonical handling.

Four extraction tests and forty focused PHP differential controls pass, also
with the seven retained optimization-disable switches. These are limited
controls, not global correctness acceptance. They expose three existing
baseline differences: dynamic/declared payload order, nested retirement after
root resurrection, and global unset. The latter published Undef through Rust
drop without a VM callback boundary; the prototype detaches the actual global
and active CV mirror before retiring them. All failed intermediate sources and
outputs remain preserved privately.

A later exception/weak-reference control also rejects this candidate's
semantics: after the parent's throwing destructor, its child sees a live weak
reference where PHP and the prerequisite see null. This failure remains visible
and is corrected only in the subsequent compact prototype's explicit phases.

Two ordinary alternating analysis pairs reject the performance hypothesis:

| Build | Instructions, round 0 | Instructions, round 1 | Median analysis time |
| --- | ---: | ---: | ---: |
| Tested private prerequisite | 74,909,308,339 | 74,908,736,137 | 6.7729 s |
| Actual-payload prototype | 78,983,992,712 | 78,985,102,310 | 7.1860 s |
| Same-window PHP, one reference observation | 10,338,060,854 | — | 0.9546 s |

The candidate adds **5.440633% instructions**, retaining the exact five files,
twenty findings and output hashes. This is the ordinary build comparison;
the historical 68.478G PGO result is not its baseline. There is no PGO cycle,
full matrix, architecture acceptance, runtime adoption or scorecard update.

One output-checked native instruction profile of the rejected candidate has
7,892 samples, zero lost samples and 78.920G sampled self periods. The
retirement dispatcher rises from 0.950G to 7.420G sampled self periods while
value-tree inspection falls from 0.990G to 0.370G; these are flat samples from
separate observations, not a precise causal subtraction. Disassembly shows
144-byte payload continuation copies. Every child also re-enters the VM
dispatcher. The hot sampled instruction region supports examining this
protocol, but imprecise sampling cannot price a particular SIMD move or supply
inclusive caller attribution.

The next private design must keep a compact owner/cursor continuation, retain
the original payload allocation, handle ownerless leaves within the release
primitive, and allocate a worklist only for genuinely unfinished nested
containers. It must separate callback exceptions from weak invalidation before
children. This is a new implementation hypothesis; compact storage alone is
not evidence of a speedup. The [packet](performance-phpstan-architecture-data.json)
records exact source/binary identities, every native observation and boundaries.

### Compact actual-owner continuation: correct focused phases, selector fails

The second private design holds the original 16-byte Value plus a cursor in a
24-byte continuation, avoiding whole-payload moves. It keeps the first
unfinished payload inline, allocates a worklist only for nested unfinished
containers, handles ownerless scalar leaves within the release primitive, and
drops ordinary shared edges without a speculative graph walk. Native and
suspension boundaries still use canonical handling. The object phases now
capture a callback exception, invalidate weak observers, retire actual weak-map
owners, then release properties; a throwing callback cannot skip invalidation.

Five extraction tests pass on the byte-identical value module; the changed VM
phases pass forty-one PHP differential controls and all forty-one with retained
optimization plans disabled. Compilation, the initial weak/exception regression
and a stale control-count preflight are preserved as failures. No application
measurement ran on those failed revisions.

| Build | Instructions, round 0 | Instructions, round 1 | Median analysis time |
| --- | ---: | ---: | ---: |
| Tested private prerequisite | 74,905,516,283 | 74,916,285,517 | 6.7412 s |
| Compact continuation | 75,585,496,530 | 75,586,013,411 | 6.9058 s |
| Same-window PHP, one reference observation | 10,338,114,608 | — | 0.9736 s |

The exact five-file/twenty-finding analysis still adds **0.900876% instructions**,
so this version is also rejected. Compact storage removes much of the first
prototype's regression but establishes no speedup over the tested prerequisite.
Both instruction and time observations stay in the packet; no PGO, full matrix,
production adoption or architecture acceptance follows. All effective aggregate
boundaries have 6 GiB maximum, zero swap and zero OOM.

This rules out these two added per-edge dispatch designs, not every possible
ownership representation. The next bounded slice returns to the complete
read/publication/consumption operation described above: compiler-proved fresh
result storage and last-use operand release must replace existing work and
preserve its observable boundary. Moving graph preparation into another
dispatcher is insufficient. The whole-operation proof and actual coverage must
precede another native selector; owner counts and static opcode frequency alone
are not a promised instruction reduction. PHPStan parity remains unachieved.

### Compiler-described read inputs: semantic boundaries repaired, performance rejected

The operation comparison finds concrete differences before performance changes.
PHP consumes the key/name before the receiver in its read handler, then checks
for exceptions. The prerequisite instead releases a temporary range in slot
order. On ArrayAccess failure, Rust receiver/key helper handles can remain alive
while the VM unwinds: final-owner checks see those handles and skip a PHP
destructor which later host drop cannot execute. These are demonstrated lifetime
faults, not an attribution of the whole native instruction gap.

An offline screen of the retained older whole-request census finds 7,510,121
complete read/input-release occurrences, including 7,090,364 dimension reads and
419,757 property reads. All selected original markers are operand releases.
This is a structural, execution-weighted envelope; it lacks catch-entry metadata
and is neither current observed ownership nor a native savings budget. The new
compiler checks actual branch and catch entries before replacing a marker.

The private prototype describes actual inputs in PHP order. Complete original
ranges become an ordered descriptor without another position. Constructor
scratch makes other ranges incomplete, and isset has no original adjacent input
release; those cases get an explicit input boundary while their remaining
expression scratch retains canonical range cleanup. The result publication
writer remains unchanged. This therefore tests input retirement, not the full
read/publication/consumption replacement proposed above.

Eight compiler tests pass, including real constructor/probe bytecode and
independent branch/catch entries. Forty differential controls agree with PHP;
all forty also pass the independent ordered single-slot canonical implementation
selected at compiler finalization. Six controls fail on the prerequisite:
ordinary order, callback exceptions, invalid receiver, throwing input
destructors, Fiber suspension and initialized wide-frame input order. Passing
these controls does not establish global correctness. Intermediate failures,
the stopped partial build, and a faulty builtin-name test fixture remain recorded.

| Build | Instructions, round 0 | Instructions, round 1 | Median analysis time |
| --- | ---: | ---: | ---: |
| Tested private prerequisite | 74,904,928,515 | 74,912,501,750 | 6.7054 s |
| Ordered actual-input prototype | 76,310,245,650 | 76,304,119,777 | 6.8485 s |
| Same-window PHP, one reference observation | 10,338,125,519 | — | 0.9481 s |

The exact five-file/twenty-finding analysis increases **1.866896% instructions**
and **2.133733% time**. Both pairs show the instruction regression. Reject this
runtime candidate without PGO or the full matrix; retain the semantic
counterexamples and exact sources/binaries. The comparison is ordinary release,
not against the historical PGO figure.

An output-checked flat instruction profile has 7,621 samples, zero lost samples
and 76.210G sampled self periods. The new read-input protocol contributes 1.090G
self periods; range retirement contributes 1.260G, actual retirement 1.190G,
release preparation 0.950G and tree inspection 1.130G. The older exact-prerequisite
flat profile observed range retirement 1.750G, actual retirement 0.950G and tree
inspection 0.990G. These separate sampled observations support investigating the
added protocol; they are not precise causal differences or inclusive costs of
a read. Main still accounts for 24.360G in this prototype, so this implementation
has not removed the large inlined lookup/publication execution machinery.

The architecture decision is now narrower: static input knowledge and correct
callback order are necessary, but routing every known input through generic
retirement is insufficient. Keep lifetime repair separate from a speedup claim.
Before another runtime rewrite, price complete cache-hit/read/publication and
call/frame operations against PHP, including their remaining helpers. A simple
ownership description does not by itself establish that it removes tens of
billions of instructions. PHPStan instruction/time parity remains unachieved.
The [packet](performance-phpstan-architecture-data.json) records every native
observation, source identity, failure and effective 6 GiB/no-swap boundary.

### Physical caller recovery identifies the complete return protocol

The same prerequisite source now has a separate diagnostic build with frame
pointers, limited debug information, no stripping and the original 64-byte
function alignment. Layout and register allocation differ from ordinary release;
its instruction periods are diagnostic evidence, not a production scorecard.
The exact five-file/twenty-finding output is unchanged.

All 7,776 samples and 77,760,147,744 self periods reconcile between physical stack
and self exports. There are 7,773 multi-frame known stacks, three unknown tails
and twenty-one stacks reaching the 64-frame limit. A separate existing-data
export exposes no lost-event records. Shared-library leaves, unknown stacks and
truncated stacks stay unassigned. Batch source mapping of 1,078 addresses takes
0.353 seconds. Each usable sample belongs once to its nearest main source site:

| Nearest main source arm | Sampled periods | Main self | Delegated |
| --- | ---: | ---: | ---: |
| DoFcall | 12.430G | 1.220G | 11.210G |
| Return | 11.720G | 1.450G | 10.270G |
| ReleaseTemps | 6.120G | 1.490G | 4.630G |
| CallUserFuncArray | 3.440G | 0.210G | 3.230G |
| AssignCv | 3.290G | 2.030G | 1.260G |
| FetchObjR | 2.840G | 1.840G | 1.000G |

These labels are **not dynamic opcode counts or removable costs**. LLVM can
merge joins and assign one source location: a BitwiseAnd self label does not
prove execution of that opcode. Sampling skid means a hot return PC does not
count that single instruction. DoFcall includes necessary regex and stdlib work.
The packet retains all categories without overlapping inclusive sums.

The actual calls to `run_return_frame_destructors` in ordinary and fast returns
account for 5.890G and 3.280G sampled periods through their complete descendants.
That prioritizes the whole return protocol: detach owners, prepare release,
run callbacks, host drop, publish exceptions and clean the frame.

The pinned PHP
[`zend_leave_helper`](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L2963)
sets caller context and releases compiled variables.
[`i_free_compiled_variables`](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_execute.c#L4271)
uses
[`i_zval_ptr_dtor`](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_variables.h#L40):
decrement the reference, destroy at zero, otherwise check possible-root admission.
RPHP detaches an actual Value, prepares VM release, possibly runs the callback/
exception loop, then enters generic Rust drop with its count/type/GC decisions.
Necessary lifetime work crosses two interacting protocols. This is an
architectural difference, not evidence that Rust or Rc is inherently too slow.

Failed stages remain visible. The first build succeeds but fails the debug-
section gate before runtime; the corrected build preserves alignment and
validates debug sections and the frame-pointer prologue. Native profiling
succeeds, but default inline-symbol export times out. Reject that incomplete
export. Physical `--no-inline` export recovers the same recording in 0.349 seconds
without rerunning the interpreter. Tuple-name parsing and mapping-clock mistakes
are corrected without changing addresses or totals. Every job has a verified
6 GiB/no-swap boundary and zero OOM; failed services retain failure exits.

### A smaller root-retirement boundary helps, but is insufficient

The private prototype inlines the existing prepare/drop decision. Only already-
prepared callback work enters an outlined owner/context/exception loop. It adds
no new Value-kind/count policy, runtime selector or bytecode. The detached owner
stays alive at the same committed boundary, including throwing callbacks,
repeated preparation and early shutdown exits. Tree proofs and payload drop
remain unchanged; only the retirement source file changes.

All forty-one controls match the independently executed unchanged prerequisite
protocol. Thirty-eight match PHP; three preserve known object ordering,
resurrection/alias and global-unset differences. These remain semantic failures;
baseline equivalence does not establish global PHP compatibility.

| Ordinary build | Instructions, round 0 | Instructions, round 1 | Median analysis time |
| --- | ---: | ---: | ---: |
| Exact prerequisite | 74,929,393,166 | 74,937,137,172 | 6.7258 s |
| Root boundary prototype | 74,180,721,045 | 74,197,971,630 | 6.7611 s |
| Same-window PHP, one observation | 10,338,037,854 | — | 0.9499 s |

Two alternating pairs remove **0.992775% instructions**, approximately 744
million; the time median is **0.525272% higher**. All outputs match, counters run
100%, and no outliers are discarded. The ordinary owner wrapper disappears from
the symbol table while the callback helper remains. This supports a cheaper
boundary but fails the declared two-percent selector and establishes no time
improvement. Retain it privately, without runtime adoption, PGO or a full matrix.

The architectural next step is **one storage-edge retirement protocol**. It
must determine lifetime once, preserving GC admission, typed/reference semantics
and the original allocation through PHP callbacks, weak invalidation and
resurrection. Internal Rc handles, native visitors, resources and suspendable
frames belong in the proof. Moving a final object out of its Rc allocation or
deferring callbacks past operation commits fails that contract. Earlier payload
dispatchers remain rejected: replace both protocols instead of adding another
per-child layer. No native saving is claimed for this unimplemented replacement.

## Next implementation checkpoint and rejection rules

Prioritize **the complete storage-owner and return contract** using recovered
caller evidence. Complete cache-hit read/publication/consumption remains the
next operation comparison; its rejected input-only prototype does not justify
another cleanup marker. Use the tested private entry-storage prerequisite for a
new semantic comparison; keep the frozen owned-frame repair as historical evidence because
it fails the stronger entry controls. Its repair is not globally adopted.
Keep one semantic executor for every Value kind; use compiler facts to combine
operations, without introducing another typed interpreter or requiring every
result to be Long. The adjacent read/move envelope is too narrow to stand in
for the general ownership design. Calls, heap writes, re-entrant access, unknown
aliases and unproved exception/interrupt boundaries end any borrowed read view.
Source ownership at a later callback cannot be guessed from syntax alone.

The later entry diagnostics make this implementation conditional: do not
enable a fresh result writer until the entry/storage role contract above is
established. The frozen repair passes the retained repair controls but fails
the newly added original-argument identity/lifetime control; it is not a
globally correct entry baseline. A new design must fix that invariant rather
than add a compensating ownership check to every hot result write.

Before editing, trace one complete canonical protocol in the retained per-PC
packet, enumerate the eliminated decisions and state the alias/lifetime proof.
During the prototype, count actual consumption coverage, retained releases,
observable-effect fallbacks and ownership changes. Counters must retain no PHP
owners. Stop early if real coverage is negligible again; a structural 18.19%
envelope from the PC screen is not successful execution coverage.

Focused differential controls must cover heap as well as scalar results,
destination overwrite, source/result aliasing, reference and COW behavior,
receiver/root rebinding, magic/undefined reads, destructor re-entry, resource
closure, pending exception replacement and catch/finally. Preserve forced
canonical execution and the existing lifetime counterexamples. Reuse retained
source/PC profiles for offline planning; do not run a full PHPStan profile for
each local edit.

After those controls and material real coverage, run two alternating ordinary
analysis pairs against the exact ordinary repair. A minimum **2% instruction
reduction** is an admission threshold for the next gates, not a speedup forecast.
Reject a candidate that merely moves time, adds another hot selector or expands
main text/stack as the earlier matrix did. All valid observations stay visible.
Only a passing selector expands to PGO, independent confirmation, representative
corpus/holdout, feature and memory gates, and both architecture checks. Global
compatibility and regression limits remain unchanged. Every expensive job keeps
the separate 6 GiB/no-swap/process-group memory boundary and cleanup lifecycle.

This checkpoint owns the owner/retirement design and its compiler/frame/value
interfaces in the isolated performance worktree; no other agent edits them.
Define representation and proof before selecting a complete replacement.
Allocator changes and native lowerings remain separate decisions. If this shared
protocol is correct but fails the instruction selector, record its causal budget and reject it rather
than extending it with compensating guards. Instruction/time parity stays open
until an accepted, reproducible application result actually reaches PHP.
