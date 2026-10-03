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

In the latest ordinary-build window, two prerequisite observations have medians
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
