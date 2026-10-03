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

## First implementation checkpoint and rejection rules

Begin with **general read-result consumption and retirement**, covering ordinary
FetchObjR and FetchDimR results used by assignment. Use the frozen owned-frame repair,
not current production ownership. Keep one semantic executor for every Value
kind; use compiler facts to combine operations, without introducing another
typed interpreter or requiring every result to be Long. Calls, heap writes,
re-entrant access, unknown aliases and unproved exception/interrupt boundaries
end any borrowed read view. Source ownership at a later callback cannot be
guessed from the earlier syntax alone.

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

This checkpoint owns compiler finalization, operand/result publication and the
repaired retirement contract in the isolated performance worktree; no other
agent currently edits them. Allocator changes, a whole VM rewrite and new native
lowerings remain separate decisions. If this shared protocol is correct but
fails the instruction selector, record its causal budget and reject it rather
than extending it with compensating guards. Instruction/time parity stays open
until an accepted, reproducible application result actually reaches PHP.
