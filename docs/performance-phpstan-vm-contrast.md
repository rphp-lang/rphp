# Zend and RPHP execution work

Status: reconciled read-only investigation; simultaneous instruction/time parity remains open.
The current accepted runtime is `724abb76`, with 68.6894 billion native analysis
instructions and 6.3610 seconds, versus PHP's 10.3377 billion and 0.9294 seconds.
The measurements below reuse earlier exact analysis-only Callgrind profiles;
they are not new native results for that source.

## Where the multiplicative difference occurs

| Same analysis boundary | RPHP | PHP |
| --- | ---: | ---: |
| Total instructions | 71,045,941,603 | 10,413,594,551 |
| Dispatched bytecode steps | 247,231,188 | 163,373,959 |
| Total instructions divided by dispatched steps | 287.37 | 63.74 |
| Main executor self instructions | 27,916,272,120 | 3,140,080,363 |
| Recognized separate Zend handler self instructions | included in other RPHP functions | 3,377,659,115 |
| Remaining functions and unclassified addresses | 43,129,669,483 | 3,895,855,073 |

The total 6.8224x instruction ratio decomposes arithmetically into 1.5133x
more dispatched steps and 4.5084x more instructions per dispatched step.
Different bytecodes, fused operations, parameter receives and native builtins
mean these steps are not identical semantic operations. This decomposition
locates the broad excess; it does not prove a particular optimization's gain.

PHP's executor must include its separate opcode handlers. Comparing only
`execute_ex` with RPHP's large executor would exaggerate the difference.
Exact installed-binary handler selection identifies 322 observed function/context
rows covering 318 distinct functions with 3.3777 billion self instructions.
Main plus recognized handlers
therefore accounts for at least 6.5177 billion, with unmatched functions kept
separate. Every self-cost partition sums to the full profile; nested inclusive
call edges are never added.

The installed Zend hybrid VM uses generated handlers specialized by operand
storage, result use, following operand data, argument position and fused branch
form. Recovering those variants maps 163,184,499 of the 163,373,959 observed
handler dispatches (99.884%). The conservative machine-CFG split still leaves
2.3964 billion main-executor instructions shared between handlers and 0.0936
billion unattributed. Per-opcode body comparisons remain lower bounds, not
complete equivalent-operation costs. Source:
[Zend VM generator](https://github.com/php/php-src/blob/PHP-8.5/Zend/zend_vm_gen.php).

## Concrete representation differences

The [current application-work control](performance-phpstan-application-work.md)
reuses the retained function-body probe on the accepted PGO runtime: 9,537,284
RPHP entries versus 9,605,729 PHP entries (-0.7125%), with identical main parser
traversal counts and findings. It excludes an order-of-magnitude increase in
counted function entries, not differences inside bodies. These whole-command
instrumented counts are separate from the native analysis-only profile above.

RPHP executes 36,411,017 separate `ReleaseTemps` markers. Their uniquely
attributable own bodies cost 3,675,814,454 instructions, before called cleanup
helpers. Zend frees consumed TMP/VAR operands inside operation handlers through
`FREE_OP`; RPHP also scans ownership intervals, checks frame bitmaps or wide
slots, proves read snapshots and may plan nested destructor work. Source:
[Zend operand release](https://github.com/php/php-src/blob/PHP-8.5/Zend/zend_execute.c).
These are structural lifetime differences, not an allocator language boundary.

Even eliminating all of that RPHP marker-body cost would remove only 5.174%
of the profiled total. The 36.4 million extra marker dispatches explain part
of the roughly 83.9 million dispatch difference, but are not the sole cause.
The [position reconciliation](performance-phpstan-pc-reconciliation.md) corrects
DoFcall, Return, assignment and object/dimension read bodies to lower bounds of
2.481, 2.320, 2.147, 2.833 and 1.940 billion own instructions respectively.
Other costs remain spread across frame retirement, scope metadata, graph
release, containers, allocation, string processing and library implementations.

RPHP already has a 16-byte value, 16-byte instruction, absolute TMP offsets,
hoisted activation metadata, reference counting and selected operand
specialization. Those features are not new fixes. Its TMP allocation is still
mostly monotonic across a function; wide frames require initialized tail
storage and different ownership scans. Earlier statement-local scratch reuse
saved only 0.3288% in its exploratory configuration and was removed. Earlier
conservative empty-release proofs marked no actual application ranges. Neither
result supports promising parity from simply reapplying those approaches.

## Decision and next investigation

Rust remains suitable. Assembly would not remove extra semantic operations,
metadata work, ownership transitions or a wide result ABI. Before another
implementation, quantify one general representation boundary with real
executed instruction sites and a conservative saving bound. Candidates are
operand/lifetime compilation, ordinary call/return state and successful helper
result transport. Check whether the large `VmError` payload forces aggregate
returns even for successful helpers; this is a hypothesis until layout,
disassembly and an exact native A/B demonstrate a benefit.

A new representation must preserve PHP reference/COW behavior, callback and
exception ordering, destructor timing, Fiber/generator suspension and canonical
execution. Do not add workload recognizers or stack small compensating changes.
The current difference requires removing broad repeated work; accumulated
half-percent checkpoints alone are not a sufficient parity strategy.

### Result-transport diagnostic

An isolated probe linked the retained release library with the exact Rust
1.98.1 toolchain. `VmError`, `Result<(), VmError>` and `Result<Value, VmError>`
are each 32 bytes. Boxing the complete error gives 8-byte unit results and
24-byte value results. The wide unit result has a hidden return-buffer argument;
its success stores only a discriminant byte, not a 32-byte error copy.

With a separately outlined cold constructor, the synthetic wide/thin success
bodies execute seven/five machine instructions including return. Without that
outlining, the boxed example acquires extra hot stack setup. Neither layout
nor assembly size alone establishes application performance.

The saved profile contains 24,821,440 confirmed edges into identified core
VM/stdlib unit-result helpers. Four hypothetical saved instructions per such
entry amount to 0.0993G, or 0.140% of the total; this is illustrative arithmetic,
not an exhaustive overhead bound or measured improvement. This evidence does
not admit a broad error API migration as the next parity implementation. The
larger operation/lifetime costs remain the priority. See
[the diagnostic](performance-phpstan-result-abi-data.json). Production source
is unchanged. Initial toolchain mismatch attempts remain failures; the
unintentionally auto-installed toolchain was removed, and final compilation
uses the project-pinned compiler with automatic installation disabled.

The direct root-index candidate was rejected after a confirmed independent
control regression; its runtime change is completely removed. See
[the rejection](performance-phpstan-direct-root-index-rejected.md).

The [executor traffic follow-up](performance-phpstan-executor-traffic.md) originally
retained 1.3171G of RPHP main-executor PCs as unmatched instruction boundaries.
The [position reconciliation](performance-phpstan-pc-reconciliation.md) resolves
that discrepancy and supplies corrected body lower bounds. Complete
profile/function totals and native A/B observations are unchanged.
An argument-only owner-transfer prototype saves just 0.5992%
in the ordinary native selector and is
[removed](performance-phpstan-argument-transfer-rejected.md); no runtime gain
or compatibility repair from it is accepted. The
[actual-owner array retirement slice](performance-phpstan-array-retirement-rejected.md)
is also completely removed after +0.02449% ordinary analysis instructions;
combining its graph traversals does not produce a material application win.

The [packed root-index follow-up](performance-phpstan-packed-root-index-rejected.md)
removes ordinary root-address hashing without growing allocations. Its 0.812%
PGO analysis instruction win fails independent controls, including +9.240%
trait-property time. The whole prototype is removed. This excludes allocation
growth as the sole explanation for the older direct-index regression, but does
not establish a particular guard or machine-code-layout cause. The accepted
runtime result stays at 68.6894G; bounded GC wins are not a parity strategy.

Exact binary identities, reconciled partitions, opcode body lower bounds and
scope limits are in [the data](performance-phpstan-vm-contrast-data.json).

The [systemic controls](performance-phpstan-systemic-controls.md) find a mean
wide-marker tail span of only 1.768 slots, +0.593% instructions when seven
existing plans are disabled, and only -0.078% instructions when LLVM machine
tail merging is disabled. No runtime change is accepted. These exclude specific
broad hypotheses without identifying all probe, code-generation or lifetime
costs as irreducible. Operation/storage and ownership boundaries remain the
priority; the accepted PGO scorecard is unchanged.

Metadata-only FFI cast attempts produced invalid maps, including one isolated
failed process; those attempts were rejected. The retained map uses checked
native pointer extraction and verified 32-byte Zend opcode geometry. Diagnostic
FFI enablement never entered a benchmark configuration. No runtime source
changed during this investigation, and no new full Callgrind cycle was needed.

The [predicate/code-generation controls](performance-phpstan-predicate-controls.md)
add 1.291% instructions when the cached property leaf is outlined, remove only
1.021% in an ineligible panic=abort diagnostic, and save just 0.446% with broader
pure predicate branches. Both production changes are removed. Existing temporary
operand/Fiber destructor gaps remain explicit failures, not passing coverage.
The accepted scorecard is unchanged; neither compiler unwinding nor a few extra
branch fusions explains or closes the several-fold application gap.

The [Value transport controls](performance-phpstan-value-transport.md) prove
that a pointer-only payload changes actual production constructors, moves and
Clone to scalar-pair register transport while preserving sixteen-byte physical
layout. Full ordinary analysis nonetheless adds 0.494% instructions. A separate
Clone normalization adds 0.362%. Both prototypes are completely removed; no PGO
scorecard update or error-API migration follows. General register transport
alone does not establish an application optimization.
