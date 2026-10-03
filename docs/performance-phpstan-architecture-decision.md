# PHPStan: compare execution models before the next redesign

Status: verified diagnostic checkpoint; no runtime adoption and no parity claim.

RPHP should reduce the work of complete language operations. The evidence does
not support another allocator replacement, an assembler rewrite, or treating
cycle collection as the explanation of the entire gap. It also does not yet
identify a single architectural change that closes that gap. This document
separates observed mechanisms, measured costs and proposals to evaluate.

The [detailed architecture review](performance-phpstan-architecture.md) retains
earlier ledgers and rejected prototypes. Exact identities, observations and
diagnostic limits are in the [evidence packet](performance-phpstan-architecture-data.json).

## The question and the baseline

The outcome remains instruction **and** time parity on the same five-file,
twenty-finding PHPStan analysis, with PHP behavior preserved. This checkpoint
compares the execution models and application work before another runtime
rewrite. It changes documentation and private diagnostics only, on the isolated
performance branch. No implementation checkpoint is accepted by this review.

The current private prerequisite is identified by source digest
`70ed55d9c2d2ab929dbc2619679a9f66cd182deb211d18191e7a4988cac75ab6`
and binary digest
`635e22394478b487840f68a6281520814e4c6cc3f9d80a4b323c9fd3b16e7ae0`.
It has known differential lifetime failures; it is not a globally correct
replacement for production. The public documentation baseline is `c024f157`.

In the preceding ordinary-build window, two prerequisite observations have medians
**74.934355G instructions / 6.6614s**. Same-window PHP, one observation, executes
**10.338015G / 0.9703s**. Both use an analysis FIFO boundary, CPU affinity, serial
analysis and a fresh temporary directory. Startup and output rendering are
excluded. This is an exploratory x86-64 comparison, not an acceptance matrix.
The older PGO result is a different build and must not be mixed with this pair.

## First compare how much application work runs

Matching findings alone does not prove matching execution. The earlier five
parser counters match, but cover only those five sites. A broader private
diagnostic now selects the union of executable archive sources actually included
by both interpreters. It instruments traditional function-body entries, keeping
source line counts and the original operation order. It does not modify project
inputs or unexecuted reflection stubs based on their file extension.

| Analysis observation | PHP | RPHP |
| --- | ---: | ---: |
| Archive sources included after analysis | 2,587 | 2,585 |
| Instrumented body entries | 9,075,686 | 9,072,411 |
| Bodies entered at least once | 5,688 | 5,685 |

Of 18,385 instrumented traditional bodies, 5,688 are active in either runtime;
**5,617 active bodies have identical counts**. The other 71 bodies add 1,849
entries in RPHP and omit 5,124, for 6,973 absolute differences. The net difference
is **−0.036085%**, not a multiplier of application calls. Retain the individual
differences instead of relying on cancellation in the total.

The largest differences are fewer `BuilderHelpers::normalizeValue` entries
(−1,126), fewer integer-node constructors (−1,065), and three PHP-only mbstring
polyfill bodies with 943 entries each. Smaller type-description and comparison
counts differ too. The mbstring source difference is consistent with alternative
native/polyfill availability; it is not a complete causal classification of all
71 differences. Do not silently treat the runtimes' reflection environment as
identical.

For example, `NodeVisitorAbstract::leaveNode` matches at 185,774 entries,
`TokenIterator::isCurrentTokenType` at 129,527 and
`ParserAbstract::getAttributes` at 124,426. Many short method bodies are repeated,
so the surrounding call/value protocol matters even when their useful work is
small. These names identify evidence, never optimization admission rules.

Both instrumented requests preserve the exact output hashes and expected exit.
However, **this is partial work equivalence**: 1,003 arrow declarations are not
instrumented; native builtins, internal loop iterations, the archive entry stub
and a generated dependency container outside the archive are not counted.
Instrumentation inhibits some optimized body plans and adds substantial RPHP
work. Its time/instruction count is not a native performance result. These
counters weaken large-scale replay of the covered application calls as a
hypothesis; they do not prove equality of all work or bound costs inside each
body.

## How RPHP works, how PHP works, and what to change

| Complete operation | RPHP prerequisite | Pinned PHP 8.5.11 | Architectural consequence |
| --- | --- | --- | --- |
| Dispatch | One Rust activation loop and opcode match, with shared boundary checks and outlined semantic helpers. Ordinary user calls can switch activation in this loop. | This reference uses hybrid dispatch and global VM registers; generated handlers embody operand storage choices. | RPHP is already an iterative VM. Native recursion for every PHP call is not the explanation. Price repeated loads, spills, operand selection and helper boundaries before changing dispatch. A prior function-per-opcode rewrite lost. |
| Read a heap value | Address/reference resolution, cached lookup, an owned result writer, ownership metadata and separately described releases where emitted. | The generated read handler resolves inputs, copies/dereferences the result and consumes temporary operands before its exception boundary. | Replace a complete read/publication/consumption protocol. Moving only the release work to a new marker has already lost. |
| Assign a value | Eligible sources already move. Destination replacement, ownership bitmap updates and destructor/exception preparation still have their own protocol. | Assignment owns source consumption and overwritten-destination retirement; it copies an expression result when required. | Keep required owners, but establish destination state and last use once. A move alone cannot remove the surrounding cost. |
| Enter a function | A contiguous bump stack, compiler-sized cells, header initialization, selected CV/TMP initialization, argument checks and call-plan bookkeeping. Original argument tails are separate in the private prerequisite. | VM stack reservation normally advances its top, initializes call state and retains the arguments/CVs required by language semantics. | Both have stack allocation and owned arguments. Compare actual initialization, metadata and validation; neither general borrowing nor another allocator is the proposed replacement. |
| Return from a function | Publish/validate the return value, detach actual owned cells, prepare VM callback work, run pending-exception rules, perform Rust payload drop and clean/pop the frame. | Publish/validate the value, release CVs/extra arguments/receiver as needed, check exceptions and restore the caller. CV release decrements RC and destroys at zero, otherwise checks possible cycle-root admission. | Preserve callbacks and re-entry. The repeated VM preparation/host-drop classification is an observed structural difference, but its removable fraction must be measured. |
| Release nested storage | VM code prepares possible PHP callbacks before a host Rc payload can disappear. Retained handles and logical owner roles participate in the proof. | The payload release connects the decrement to its destructor, which releases its real entries. Object callbacks, resurrection, weak references and exceptions still require work. | A common semantic ownership contract can replace duplicate decisions. Adding another dispatcher for every child has already lost. Preserve the original allocation and release order. |
| Collect cycles | Candidate admission and collection are distinct from ordinary owner release; general Value drops can register candidates. | CV/storage release may admit surviving candidates; proved consumed temporary release uses a different mode. | Prove the release role once. Globally disabling GC or choosing a mode merely from a TMP index is unsound. GC savings alone have not been shown to close the gap. |

Both use 16-byte values, reference-counted payloads and array COW. Ordinary heap
handle copies are not deep payload copies. The measured physical-copy census
also does not support repeated whole-array copying as the dominant explanation.
The distinction is how many decisions, owners, metadata updates and boundary
transitions each complete language operation requires.

The RPHP anchors are [dispatch](../src/vm/execute/baseline_dispatch.rs),
[frame allocation](../src/vm/stack.rs), [operand access](../src/vm/frame.rs),
[value storage](../src/value/mod.rs) and
[retirement](../src/vm/execute/call_frames.rs). Private prerequisite changes are
identified in the evidence packet; these public links show the corresponding
source areas, not a claim that the private repair is merged.

The PHP comparison uses pinned primary source:
[dimension read and operand consumption](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L1933),
[call-frame reservation](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_execute.h#L344),
[return to the caller](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L2963),
[CV release](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_execute.c#L4271) and
[release primitives](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_variables.h#L32).
The installed reference has CLI opcache and JIT disabled. Its advantage here
does not require a JIT or an assembler implementation of the allocator.

## Trace the same expression through both engines

Consider `$out = $object->items[$key]`, with an ordinary declared array property,
a CV key and a heap-valued element. This example names storage roles, not
runtime admission conditions. References, magic, undefined access, overwritten
owners and exceptional effects still belong to the operation contract.

The retained RPHP site census has **76** contiguous sequences of
`FetchObjR → FetchDimR → ReleaseTemps → AssignCv → ReleaseTemps`, entered
**2,786,647** times across the whole request. This establishes coverage for that
sequence, not analysis-only cost or a removable instruction budget. Other
storage shapes and intervening effects are not included in this count.

| Boundary | RPHP prerequisite event | Corresponding PHP event |
| --- | --- | --- |
| Property read | Validate the object/property cache and initialized property; copy the array handle to the first expression slot; classify/publish its ownership through the frame writer. | Validate the cache and initialized property; copy/dereference the array handle to the expression result. Operand storage is embodied in the generated handler. |
| Element read | Resolve the key, receiver and destination storage again; perform the dimension lookup and copy/dereference the element into a second owned expression slot. The first expression owner remains for a separate release. | Resolve the key/receiver, perform the lookup and publish the element. The read handler then frees its actual temporary operands before checking the exception boundary. |
| Consume first result | Dispatch a bounded release marker, load its cached mask, inspect live prefix/tail ownership, select the release role and enter VM retirement if needed. | The array-read handler already consumed that actual owner. No corresponding range-discovery operation is needed for this edge. |
| Assign second result | An eligible non-reference source already moves; clear its expression ownership, prepare the overwritten destination's observable release work, commit the CV and complete callbacks/exceptions in order. | Assignment owns source consumption and destination replacement; the assignment helper consumes operand two itself. |
| Complete expression | A second release marker still reconciles the expression interval, even when assignment consumed the last heap source. Later frame cleanup uses the retained runtime ownership state. | Consumed expression operands are already retired; function exit still releases its actual CVs, extra arguments and receiver as required. |

Both engines need the initial array/result owners when their language storage
requires them. The proposal is not to borrow across a callback, drop RC/COW,
skip destination destruction or turn references into values. It is to establish
expression-owner state and exact last use once, and let the actual consumer
perform the release at its required boundary. This can remove a separate
range-discovery/dispatch protocol and repeated old-result reconciliation;
necessary final-payload destruction remains.

The difference is already visible in pinned
[dimension consumption](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L1933)
and [assignment consumption](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L2810).
The RPHP writer already avoids reading/dropping a vacant tracked slot when its
bitmap bit is clear. A new fresh writer therefore cannot claim to eliminate a
drop on every current result write. Its gain must be the complete protocol it
replaces, with new bookkeeping charged too.

A finalized ownership proof must follow actual success edges. In particular,
`SendVarEx` chooses by-reference behavior from the selected runtime callee;
its consume-temporary flag alone does not prove consumption on that branch.
Do not enable a fresh-result/empty-release fact from that flag. Catch entries,
re-entry, mutable aliases, finally, suspension and memory-exhaustion exits also
need their actual live-owner state. A normal-path proof is not automatically an
unwind or final-owner proof.

## The measured budget limits the redesign

The recovered diagnostic caller recording partitions 77.760G sampled periods by
nearest main source site: DoFcall 12.430G, Return 11.720G, ReleaseTemps 6.120G,
CallUserFuncArray 3.440G, AssignCv 3.290G and FetchObjR 2.840G. Those six sites
sum to 39.840G; **37.920G remains in other categories**. These are disjoint source
partitions in a diagnostic build, not production opcode counts. They include
necessary delegated work and ambiguous LLVM source joins. They cannot be added
to nested inclusive totals or called entirely removable overhead.

This matters for choosing scope: removing even every period attributed to
Return would remove about 15% of that diagnostic total. Ownership and return
work deserve attention, but cannot by themselves be assumed to explain all of
a roughly sevenfold ordinary instruction gap. Small helper wins do not validate
a claim about the full execution model.

The latest private shared-owner prototype reuses the exact host-Drop primitive
for ordinary storage-edge release and returns possible final owners to the
unchanged VM protocol. Eight default and six no-default primitive tests pass;
41 controls match the canonical prerequisite, with the same three known PHP
lifetime failures. Default already includes resource lifetime support, so an
explicit resource-only rerun is a duplicate, not an independent feature variant.

Two alternating ordinary pairs reduce instructions **1.908183%**
(74.934355G → 73.504470G), while analysis time rises **1.302222%**
(6.6614s → 6.7481s). All outputs match and all observations are retained. Reject
this candidate: it fails the declared 2% instruction selector and proves no time
improvement. No PGO, full matrix, production adoption or scorecard update follows.
This prices one duplicated release decision; it is not a full replacement for
final-payload retirement.

## Price actual operations without mistaking nested work for overhead

A private Linux diagnostic now samples `instructions:u` between hardware reads
at actual canonical match entries and their exits. It observes the real opcode
tag, including delegated native helpers; source-line joins no longer choose
that tag. Every continuation/activation switch exits the same lexical scope.
Its guard owns no PHP value or frame pointer. Sampling uses a deterministic
PRNG with probability 1/1,024; raw intervals, caps, nesting and counter failures
are retained privately.

The first version records inclusive intervals. The second also subtracts
immediate nested canonical intervals: those inclusive child intervals already
contain their descendants, so deeper intervals must not be subtracted again.
Three focused tests verify the ABI/counter, nested scope invariant and empty
calibration. Both versions preserve all 41 prerequisite controls and the exact
five-file/twenty-finding output. The three known reference failures remain
failures; no global compatibility acceptance follows.

Both observe **269,824,195 canonical match entries**, **263,577 sampled
intervals** and **101,457 nested entries** covered by a sampled parent, across
request execution up to the CLI dump. The thread counter starts after the CLI
reads the source and ends at that dump; later native teardown is outside it.
No caps or multiplex failures occur. This is not the
analysis-only FIFO scope and excludes direct optimized body execution outside
canonical matches as a separately identified entry. An interval can also
contain a compound operation already implemented by its handler.

| Actual match entry | Whole-request entries | Samples | Raw exclusive interval median |
| --- | ---: | ---: | ---: |
| `DoFcall` | 12,793,539 | 12,617 | 654 |
| `Return` | 8,849,455 | 8,513 | 1,058 |
| `ReleaseTemps` | 38,176,885 | 37,617 | 191 |
| `AssignCv` | 24,697,811 | 23,988 | 275 |
| `FetchObjR` | 23,876,086 | 23,163 | 289 |
| `FetchDimR` | 12,854,824 | 12,562 | 332 |
| `CallUserFuncArray` | 356,497 | 362 | 10,450 |

These are **diagnostic hardware intervals**, not ordinary-build costs. Their
empty-scope median is 95 instructions. That separately compiled empty scope is
not an exact correction for every handler's register allocation and guard
exit. Profiling changes the build substantially: its FIFO analysis counters
are 101.292G/102.253G, whereas the unchanged ordinary prerequisite median is
74.934G. Never report the former as a production regression or subtract the
calibration and call the result a production instruction price.

For example, the second version's sampled `CallUserFuncArray` inclusive mean
is 204,030 instructions and the canonical-exclusive mean 52,551. A 9.538M
exclusive outlier has substantial influence among 362 samples. Exclusion
removes nested canonical handler intervals; it does not remove optimized PHP
bodies, native callbacks/builtins, GC or other necessary work outside them.
Thus this row is not the intrinsic cost of forwarding a call. Do not multiply
these means into a disjoint, removable whole-program budget. The diagnostic
narrows operation identity and nesting; it has not closed the instruction ledger
or justified a rewrite of that call from its mean alone.

The useful architectural conclusion is more limited: repeated result, release,
call and return protocols have broad coverage; their useful work, intrinsic
protocol and nested execution must be separated before choosing a replacement.
The private sampler supplies that distinction and distribution evidence, while
ordinary A/B measurements remain the acceptance authority. Four build/capture
jobs finish within verified 6 GiB/no-swap boundaries with zero OOM; no ordinary
runtime implementation is adopted by this diagnostic.

## Isolate packaging, process mode and actual GC state

The requested control uses both unchanged executables, the same disabled
`proc_open`/fork functions, fresh temporary directories and two alternating
rounds. Before/after markers verify that both analyses are serial. The controlled
archive and its physical extraction have identical contents for all 7,001 files.
All sixteen observations preserve the five-file/twenty-finding output.

PHPStan already calls `gc_disable()` at startup. An explicit off analysis reports
zero cycle-collector runs before and after in both interpreters. The forced-on
control enables GC just before analysis and verifies that it stays enabled after
it. These statements concern cycle collection, not required RC/drop work or
candidate bookkeeping.

| Input / analysis GC mode | PHP instructions / analysis time | RPHP instructions / analysis time |
| --- | ---: | ---: |
| PHAR, off | 10.336641G / 0.9848s | 74.938467G / 7.1535s |
| Physical extraction, off | 10.385073G / 1.0121s | 74.634258G / 7.0861s |
| PHAR, on before analysis | 10.436963G / 1.0668s | 76.711253G / 7.1945s |
| PHAR, INI GC off from request startup | 10.392245G / 1.0043s | 74.344990G / 7.2254s |

Unpacking reduces RPHP instructions only 0.406%. Enabling analysis GC increases
RPHP instructions 2.366% and PHP instructions 0.971%; PHP remains about seven
times cheaper. Disabling GC from request startup reduces RPHP instructions
0.792%, but raises median peak RSS from 558.61 to 994.01 MiB and does not improve
analysis time. None of these controls supplies a production change or explains
the large gap. Compare times within this window; do not mix them with earlier
ordinary or PGO builds/windows.

## Close the native self-period and outer application ledgers

A separate unchanged-binary capture reconciles every sampled instruction period
to one self row: RPHP 74.830022G over 7,483 samples, PHP 10.330003G over 1,033.
Both report zero lost samples. RPHP leaf symbols resolve; PHP has 4.300001G at
unresolved static-handler addresses inside its known executable. Requested
DWARF unwind does not recover most RPHP callers, so the capture does not close
causal attribution of inline work. Sampling is not an exact per-function count.
For example, RPHP main self is 24.470007G, return-owner retirement 3.780001G,
statement-temp release 2.200001G, and native memmove 1.740001G. The main self row
includes inlined semantic work and cannot be labeled pure dispatch overhead.
The public packet preserves disjoint top rows, DSO totals and the full residual;
it does not sum inclusive caller costs.

To cut the application with minimal distortion, only three archive sources gain
sparse outer phase boundaries. An external user-only hardware counter freezes
and reads each interval; it does not instrument the runtime or function bodies.
Each of four valid runs contains 37 nonoverlapping intervals whose counts sum
exactly to its final frozen total, with equal enabled/running times. The two
alternating-pair medians are **74.640175G RPHP / 10.386858G PHP**. Relative to the
preceding extracted-input control, these totals differ by only **+0.007928% /
+0.017183%** respectively; acknowledgment latency is not an accepted native
runtime speed result.

| File ordinal, node/rule interval | PHP instructions | RPHP instructions | RPHP / PHP |
| --- | ---: | ---: | ---: |
| 1 | 4.437923G | 30.839426G | 6.949x |
| 2 | 3.157479G | 24.197675G | 7.664x |
| 3 | 0.464454G | 3.254153G | 7.006x |
| 4 | 1.054703G | 7.544985G | 7.154x |
| 5 | 0.954782G | 6.766940G | 7.087x |

Together the five node/rule intervals consume **72.603179G RPHP / 10.069340G
PHP**, accounting for **97.324% of the excess instructions**. They include
called rules, inference, reflection and additional parsing, not only the
resolver's own statements. Initial per-file parsing is 1.446322G / 0.236413G;
cache processing is 0.520501G / 0.065963G. The remaining initialization,
aggregation, finalization and ignore handling are small disjoint residuals.
This localizes the difference inside language execution during node work; it
does not yet identify which repeated runtime protocol can remove it. Similar
ratios across these five files also do not prove scaling for a larger project.
The next cut must separate called parser/reflection work from rule/inference
work before choosing a runtime replacement.

The first phase attempt retains correct output but fails its counter/exit gate:
the installed perf interval mode emits one total, not phase rows. The second
fails the counter ABI-size preflight before launching PHPStan. Both failures
are retained; only the corrected external counter capture above passes. All
three limited jobs and the PHAR/GC/native captures finish with zero OOM. No
runtime, scorecard or parity acceptance follows from these diagnostics.

## The next design must eliminate a whole repeated protocol

Use ordinary Rust and one canonical executor. Define a finalized operation
description with input storage/ownership, result state, last consumption, ordered
observable effects and exact exceptional/resume state. Compiler control-flow
proofs should establish static facts once, rather than making ordinary getters,
writers, release markers and frame cleanup rediscover them independently.

Separate two lifetimes in that contract: expression temporaries have producers,
last consumers and exceptional live-owner sets proved from control flow; CVs,
references and persistent heap entries retain dynamic storage ownership. On a
proved normal path, temporary consumers end those expression owners, so frame
exit need not discover them again. Exception and suspension edges publish their
actual live owners through the same description. This is a possible replacement
for repeated TMP bitmap/publication/range reconciliation, not a claim that CV
cleanup or PHP-visible references become static. Internal retained Rust handles
must not masquerade as extra language storage edges in final-owner proofs.

For the first vertical slice, compare the whole common
`property read → array read → assignment` chain, including its called helpers.
Preserve lookup, reference/COW semantics and every required owner. A proved
consuming read retires its actual last inputs at its existing semantic boundary;
a proved fresh result writer does not reclassify an old owner; assignment moves
its eligible source and retires its actual overwritten owner. Remove the old
release/publication machinery for those edges rather than adding a second
description alongside it. Calls, mutation, re-entry, unknown aliases and
suspension end any borrowed view unless the proof explicitly covers them.

Before coding another version, write its current/proposed event ledger and
price the complete operation from existing source/PC/caller evidence. List each
eliminated decision and each new instruction or retained fallback. If coverage
or the causal budget cannot justify a material improvement, choose another
operation; do not expand the prototype to compensate. The previous input-only
and child-dispatcher failures remain constraints on this design.

Correctness controls must cover heap results, source/result aliasing, references,
COW, magic/undefined access, last-owner callbacks, ordering, weak references,
resurrection, exceptions, catch/finally, generators and Fibers where applicable.
Known prerequisite failures remain visible. Use focused controls and a two-pair
instruction selector before broader gates; a 2% selector is only admission to
PGO/confirmation/corpus/feature/architecture checks, not completion of parity.

All expensive diagnostics have verified 6 GiB/no-swap aggregate boundaries and
zero OOM. The first archive builder times out before interpreter execution;
bulk construction fixes that. The second diagnostic preserves analysis findings
but fails at PHP shutdown because a late destructor sees the removed counter
global. A uniform existence guard fixes the diagnostic side effect; only the
third version is accepted for body counts. Failed observations are retained,
cleanup runs at each boundary, and runtime/reference/archive identities stay
unchanged. The next checkpoint owns the operation contract and its compiler/VM
interfaces in the isolated performance worktree; parity remains active.
