# No-JIT performance gap: evidence and execution design

Status: Phase 2c implemented and measured, 2026-07-30

## Conclusion

The largest common performance gap against PHP 8.4 without JIT is not the
native Rust call to `execute_hot_frame`. It is the baseline interpreter:

- the main script always stays in `execute_ex`;
- the hot loop pays the generic dispatch and bookkeeping cost for every opcode;
- rphp executes more dispatches for common compound operations and loop
  control;
- the current hot tier accelerates called leaf functions, but it does not
  accelerate their caller or the main script.

The call/frame boundary is still a measurable second-order cost. It should be
optimized after the baseline dispatch problem, then integrated into the same
execution engine.

Rust is not the limiting factor demonstrated by these measurements. The
limitation is the current interpreter architecture and opcode density.

## Scope and method

All comparisons used:

- arm64;
- release build of the current optimized rphp worktree;
- PHP 8.4.12 invoked with `php -n`;
- Zend OPcache and Xdebug absent;
- JIT absent on both sides;
- identical PHP source and verified output;
- five or more interleaved runs, using medians;
- timings from inside the PHP program to exclude process startup.

Four 50 million iteration workloads isolate the layers:

1. inline loop: `$sum += $i + 1`;
2. the same loop with a leaf function returning `$i + 1`;
3. the same loop with a static method;
4. the same loop with an instance method.

Absolute values are sensitive to temperature and clock state. The stable
result is the relative decomposition, which was also reproduced with shorter,
higher-resolution runs.

| Workload | rphp median | PHP median | rphp / PHP |
|---|---:|---:|---:|
| Inline loop | ~0.71 s | ~0.22 s | ~3.2x |
| Leaf function | ~1.15 s | ~0.42 s | ~2.7x |
| Static method | ~1.26 s | ~0.45 s | ~2.8x |
| Instance method | ~1.17 s | ~0.53 s | ~2.2x |

Subtracting the inline loop from the corresponding call workload gives an
approximate incremental call cost:

| Boundary | rphp | PHP | remaining rphp gap |
|---|---:|---:|---:|
| Leaf function | ~8-9 ns/call | ~4 ns/call | ~4-5 ns/call |
| Static method | ~11 ns/call | ~5 ns/call | ~6-7 ns/call |
| Instance method | ~9-10 ns/call | ~6 ns/call | ~3-4 ns/call |

The inline loop alone has a remaining rphp gap of approximately 9-11 ns per
iteration. Therefore the call boundary is real, but it is not the dominant
program-wide cause.

## Independent evidence

### Opcode counts

With `vm-stats`, one inline loop iteration in rphp executes approximately seven
hot VM instructions:

1. `IsSmaller`;
2. `JmpZ`;
3. `Add`;
4. `Add_CvTmp`;
5. `AssignCv`;
6. `PostInc`;
7. `Jmp`.

Local `phpdbg` disassembly of the same PHP loop shows five Zend instructions:

1. `ADD`;
2. `ASSIGN_OP (ADD)`;
3. `PRE_INC`;
4. `IS_SMALLER`;
5. `JMPNZ`.

Zend combines the compound assignment and uses a conditional backedge. rphp
uses a separate assignment and executes both a top conditional branch and a
bottom unconditional jump. One additional opcode does not explain a roughly
threefold difference by itself; the cost of every generic dispatch is also
higher.

The release `execute_ex` loop reloads the frame instruction pointer, performs
interrupt bookkeeping, checks the pending-finally/exception state, enters a
large opcode match, and persists execution state around control transfers.
Each operation is individually reasonable, but multiplying it by every opcode
is the central cost.

### Sampling profile

Four-second sampling profiles produce the following top-of-stack
distribution:

| Workload | `execute_ex` | `execute_hot_frame` |
|---|---:|---:|
| Inline loop | 100.0% | 0.0% |
| Leaf function calls | 77.7% | 22.3% |
| Instance method calls | 81.8% | 18.2% |

The hot-executor percentage includes execution of the callee's arithmetic and
return, not only the Rust transition into the function. It is therefore an
upper bound on the transition cost. A native Rust function call cannot be the
dominant cause when the generic caller loop accounts for roughly four fifths
of the samples.

### Main-frame blind spot

Function promotion happens in `DoFcall` after a callee's call counter crosses
the threshold. The entry point for the main script calls `execute_ex`
directly. Consequently, a hot loop in the main script never becomes eligible
for `execute_hot_frame`.

This is the most important structural blind spot: the code that runs most
often can stay permanently in the slow tier solely because it is not reached
through a PHP function call.

## Audit of the existing block planner

`src/vm/planner.rs` and the per-`OpArray` block metadata show that a
quickened-interpreter direction was already started. The planner is currently
not referenced by either execution loop, so it has no runtime effect.

It must not be wired into `execute_ex` unchanged:

- it cannot plan `Add_CvTmp`, `AssignCv`, `PostInc`, or `Jmp`, so it cannot
  handle the measured canonical loop;
- a single basic block is too small to combine a loop body with its condition
  and backedge;
- its `GuardFail` has no exact resume instruction;
- a guard or overflow failure after an earlier write or call cannot safely
  restart at the block beginning because that could repeat a side effect;
- the defined macro resume state is not integrated with the VM;
- call steps mix planning, frame allocation, execution transfer, and return
  semantics before the simpler arithmetic path has been proven.

The useful parts to retain are the computed control-flow metadata and the
idea that an optimized plan is a discardable cache beside the baseline
bytecode.

## Target architecture: quickened regions

This remains an interpreter. It creates no machine code and requires no JIT.

Every `OpArray`, including the main script, has:

```text
baseline instructions       source of semantic truth
control-flow graph          block and edge metadata
hot counters                primarily loop headers/backedges
quick regions               optional, replaceable execution cache
```

A quick region covers one or more basic blocks and may contain a loop
backedge. It uses predecoded slot addresses/indices and a small typed
micro-operation set:

```rust
struct QuickRegion {
    entry_ip: u32,
    exits: Vec<u32>,
    ops: Vec<QuickOp>,
    resume_ip: Vec<u32>,
}

enum QuickExit {
    Continue(u32),
    Call(CallEvent),
    Return(Value),
    Deopt { ip: u32 },
    Throw,
    Interrupt,
}
```

The first useful operations should be systematic combinations observed in
real PHP bytecode:

- `AddAssignLong`: combine arithmetic and assignment like Zend
  `ASSIGN_OP`;
- `IncCompareBackedgeLong`: increment, compare, and branch without a second
  generic dispatch;
- typed long arithmetic with predecoded CV/TMP slots;
- side exits to exact baseline instruction positions.

Use two layers to avoid a combinatorial opcode explosion:

1. a compact typed `QuickOp` set for general quickened regions;
2. a small measured set of superinstructions for recurring adjacent patterns.

Do not use one Rust function pointer per PHP opcode. That merely exchanges the
large `match` for an indirect native call. The performance win comes from
executing several PHP operations per dispatch and removing repeated operand
decoding and generic checks.

## Correctness and deoptimization contract

The optimized tier is only safe if it is fully discardable:

1. Baseline bytecode remains the only semantic source of truth.
2. Every quick step maps to an exact baseline instruction.
3. A guard fails before the guarded operation mutates visible state.
4. Earlier completed operations stay committed; deoptimization resumes at the
   current operation, never blindly at region start.
5. Overflow, type changes, references, copy-on-write, magic methods, and
   destructors side-exit before specialized behavior would diverge.
6. Calls, yields, exception edges, `finally`, reference writes, and observable
   destructor points terminate a region in the first implementation.
7. Interrupt checks occur at region entries and loop backedges, preserving
   bounded response time without paying the check per micro-operation.
8. Deleting every quick region at any moment must leave a correct executable
   program.

This contract is more important than the exact `QuickOp` representation.

## Unified execution engine

The production end state should use one outer frame/region loop:

```text
select baseline instruction or quick region
run until an execution event
    Continue -> select next region
    Call     -> push/switch frame
    Return   -> pop/switch frame
    Deopt    -> resume exact baseline instruction
    Throw    -> normal exception machinery
```

Initially, called leaf functions may continue using the existing hot executor.
After quickened regions prove the dispatch win, `Call` and `Return` should
become execution events in the unified loop. This removes the recursive
`execute_hot_frame` boundary and duplicate dispatch machinery as a second
phase, without making call mechanics a prerequisite for the main speedup.

## Delivery plan and proof gates

### Phase 0: preserve the evidence

- Add the four isolated workloads to the benchmark suite.
- Report medians and incremental cost after subtracting the inline loop.
- Keep PHP at `php -n` and verify equal output.

Exit gate: repeatable decomposition within a reasonable thermal/noise range.

### Phase 1: one loop-region experiment

- Count hot loop backedges for every `OpArray`, including main.
- Build one guarded long-arithmetic region for the canonical loop.
- Support exact instruction resume on every side exit.
- Keep calls and observable side effects out of regions.

This is an architectural proof, not a benchmark-specific permanent opcode.

Exit gates:

- at least 30% improvement on the isolated rphp inline loop;
- no more than 2% regression on cold/startup-oriented workloads;
- no correctness failure under long-to-float overflow, type change, reference
  aliasing, exception, and interrupt tests.

### Phase 2: general quick operations and superinstructions

- Generalize typed arithmetic, assignment, increment, comparison, and branch;
- add region selection from the control-flow graph;
- collect guard-failure and side-exit counters;
- disable unstable regions after repeated failures.

Exit gate: improvement across loop, nested-loop, array-index, and scalar
application workloads, not only one synthetic case.

### Phase 3: calls as execution events

- Move fast call, frame switch, return, and hot-region selection under one
  outer engine;
- preserve the existing full call path as a side exit;
- then revisit compact scalar frames and static-method argument fusion.

Exit gate: reduce the incremental function/method gap without regressing
dynamic calls, references, defaults, named arguments, or exception unwinding.

### Phase 4: remove duplicate machinery

- Retire `execute_hot_frame` only after equivalent quick-region coverage;
- replace or remove the unused planner prototype;
- document one promotion, deoptimization, and frame-transition model.

## Expected result

The measured inline gap is large enough that a region executor can have more
impact than another local call fast path. Reducing seven generic dispatches to a
few typed quick operations and moving checks to region/backedge boundaries
should reclaim a substantial part of the 9-11 ns per-iteration gap. A precise
number must come from Phase 1; parity should not be promised before that
experiment.

Afterwards, the remaining 3-7 ns boundary gap is small enough to address with
a unified frame event loop. This order attacks the measured bottleneck and
leaves the full semantic path intact.

## Phase 1 result

The first guarded region is implemented for the closed scalar shape:

```php
for (...; $i < $limit; $i++) {
    $accumulator += $i + INTEGER_CONSTANT;
}
```

The compiler recognizes the region after normal specialization, attaches its
plan to the header block, and rewrites only the matching backedge to
`QuickLongLoopJmp`. Ordinary `Jmp` dispatch is unchanged. After 32 baseline
backedges, the quick executor guards all CV/TMP types and heap bits and runs
the pure recurrence locally.

On overflow it restores the exact state required by `Add`, `Add_CvTmp`, or
`PostInc` and resumes that baseline instruction. References, non-long values,
large frames, and heap-bearing participating slots fail the guard and remain
in baseline execution.

Five interleaved 50 million iteration runs produced:

| Mode | Median |
|---|---:|
| rphp quick region | 0.060 s |
| rphp same binary, quick regions disabled | 1.202 s |
| PHP 8.4.12, `php -n` | 0.321 s |

For this closed region, quick execution is approximately 20x faster than the
current rphp baseline and 5.4x faster than PHP without JIT. This is a ceiling
experiment for one shape, not a claim about general application performance.

`vm-stats` confirms that a 1000-iteration run executes 33 iterations in
baseline and 967 in the quick region. The overflow test records one
deoptimization after 413 quick iterations and finishes correctly in baseline
with a floating-point accumulator.

A binary A/B comparison on the non-matching existing `bench_loop.php` gives
0.1053 s with the quick tier compiled in versus 0.1041 s without it, a 1.1%
difference. Two hundred interleaved trivial-process starts showed no startup
regression. Both gates are within the Phase 1 limits.

## Phase 2a result: broaden the region without multiplying executors

The first region was generalized into one data-driven
`QuickLongAccumulateLoop`. It now represents both terms:

```php
$accumulator += $i;
$accumulator += $i + INTEGER_CONSTANT;
```

The bound may be another CV or an integer literal. This also covers the
compiler's fused `JmpZ_Lt_CvConst` condition, where no condition TMP is
materialized, and the equivalent `while` layout. The implementation still has
one guarded executor; the plan varies the bound and term. This avoids adding a
separate Rust executor for every syntactic spelling.

Detection remains deliberately closed. It validates the complete comparison,
exit, arithmetic, assignment, increment, and backedge layout; distinct CV and
TMP slots; integer literals; frame size; and the absence of participating heap
values. A multiply term, branch in the body, reference, non-long bound, or
other unrecognized operation stays in baseline. Overflow resumes at the exact
baseline arithmetic instruction with already-completed state committed.

Nine interleaved runs on the Phase 2a release build produced these medians.
All outputs matched PHP 8.4.12 with `php -n`.

| Shape | Iterations | quick | same binary disabled | PHP | quick / PHP |
|---|---:|---:|---:|---:|---:|
| `$sum += $i + 1`, CV bound | 50M | 0.03888 s | 0.69868 s | 0.16549 s | 0.235x |
| `$sum += $i`, literal bound | 10M | 0.00531 s | 0.10439 s | 0.02631 s | 0.202x |
| `$sum += $i + 1`, literal bound | 50M | 0.03272 s | 0.58897 s | 0.16249 s | 0.201x |

The corresponding quick-versus-baseline speedups are 18.0x, 19.7x, and
18.0x. Absolute times changed from the Phase 1 session because of machine
clock and thermal state; the interleaved ratios and identical outputs are the
relevant evidence.

Runtime statistics show one entry, one completion, zero guard failures, and
zero deoptimizations for every matching workload. They account for
`iterations - 33` quick iterations. A control loop using `$sum += $i * 2`
records zero quick entries.

A 15-round binary A/B on that non-matching multiply loop measured a 0.14055 s
median with the quick feature compiled in and 0.14152 s without it. The quick
binary was 0.7% faster in this sample, so there is no measured regression; the
difference should be treated as noise rather than a benefit.

Validation includes:

- two detector tests that assert the compiler selects direct/literal and
  plus-constant/CV plans;
- 11 end-to-end tests with the feature and the same 11 without it;
- exact-result tests in main, user functions, and `while`;
- accumulator-overflow deoptimization for both term variants;
- rejection tests for a non-long bound and reference accumulator;
- the complete `cargo test` suite both with and without the quick-loop feature.

This closes the conceptual risk that each loop spelling would need a bespoke
executor. It does not complete the broader Phase 2 exit gate. For recognized
closed scalar loops, dispatch is no longer the performance gap to PHP; rphp is
already about 4.3-5.0x faster in these measurements. The largest remaining
gap is **coverage**: branches, two-CV expressions such as nested-loop
`$i + $j`, additional arithmetic, array access, and calls still fall back to
generic dispatch.

Phase 2b below implements that typed sequence for two-CV arithmetic and a
conditional body. The dedicated recurrence executor remains as the fastest
superinstruction; other supported loops use the composable typed program.

## Phase 2b result: typed operations and measured superinstructions

Loops not handled by `QuickLongAccumulateLoop` can now be lowered to a
prevalidated `QuickLongOp` program. The initial operation set contains:

- long less-than with an internal or external false edge;
- long addition;
- scalar CV assignment;
- post-increment;
- forward and backward control flow.

Each plan carries masks for long inputs, long outputs, boolean outputs, and all
heap-sensitive slots. The executor guards once, loads participating long
values into a fixed 64-slot local array, and tracks dirty long and boolean
slots separately. Completion, interruption, guard failure, and arithmetic
overflow therefore commit only state that baseline execution would already
have produced.

Every fallible operation retains its baseline instruction position. If either
addition in a two-add sequence overflows, execution commits preceding work and
resumes the exact corresponding `Add`. A post-increment overflow resumes at
`PostInc`. No completed assignment or increment is replayed.

The first generic version still dispatched four to six `QuickLongOp` values
per iteration. A data-driven peephole pass now emits:

- `AddAssign`;
- `AddAddAssign`;
- `PostIncLoopLt`, which combines increment, comparison, and the backedge.

These are recurring instruction sequences rather than PHP-source-specific
whole-loop executors. The nested-loop kernel is reduced to three planned
operations including its initial header, and the conditional kernel to four.

Nine interleaved release runs produced:

| Workload | typed/super quick | same binary disabled | PHP 8.4.12 `php -n` | quick / baseline | quick / PHP |
|---|---:|---:|---:|---:|---:|
| Nested `$sum += $i + $j`, 2.25M | 0.01152 s | 0.02582 s | 0.00748 s | 0.446x | 1.54x |
| Conditional `$sum += $i`, 10M | 0.05539 s | 0.14839 s | 0.02820 s | 0.373x | 1.96x |

The new layer is therefore 2.24x faster than rphp baseline on the nested
workload and 2.68x faster on the conditional workload. Peephole
superinstructions improve the first generic typed implementation by another
approximately 1.58x and 1.44x respectively.

Unlike the dedicated closed recurrence, these more general programs do not
yet beat PHP. This is useful evidence: one or two remaining typed-plan
dispatches per iteration, slot-array indexing, and region entry are now the
local performance gap. Rust arithmetic itself is not the issue.

Runtime statistics report:

- nested: 1500 entries, 1500 completions, 2,248,468 quick iterations, and no
  guard failure or deoptimization;
- conditional: one entry, one completion, 9,999,967 quick iterations, and no
  guard failure or deoptimization.

An unstable plan no longer retries forever. The block counter encodes failed
activations; after three consecutive guard failures or deoptimizations the
region stays in baseline for that `OpArray`. A 1000-iteration loop with a
floating-point internal branch bound records exactly three guard failures,
then completes in baseline with the correct result.

Validation now includes four detector tests and 17 quick-loop end-to-end
tests in both feature configurations. New cases cover nested two-CV
arithmetic, a branch that changes direction during quick execution, a
never-written TMP, overflow in `AddAssign` and in the second nested addition,
and a non-long internal branch operand. The complete test suite passes both
with default features and with `--no-default-features`.

Phase 2c below adds the measured modulo/equality case and tests the proposed
conditional fusion. Scalar array reads remain the first materially different
semantic boundary.

## Phase 2c result: modulo, equality, and rejected over-fusion

The typed operation set now includes `ModConst` and long equality branches.
This admits the common parity loop:

```php
for ($i = 0; $i < $n; $i++) {
    if (($i % 2) == 0) {
        $sum += $i;
    }
}
```

Modulo uses `checked_rem`. A zero literal is rejected during planning; a
machine remainder failure commits preceding dirty slots and resumes the
baseline `Mod`. Equality supports a slot or integer constant operand and
materializes its boolean TMP when the baseline bytecode does.

When a less-than or equality branch's true path is exactly `Add` followed by
`AssignCv` and both paths converge immediately afterwards, the planner emits
`ConditionalAddAssign`. An addition overflow commits the already-completed
comparison and resumes at the original `Add`.

The benchmark host changed frequency substantially during this session, so
absolute medians are not comparable with Phase 2b. Nine runs alternated engine
order; the primary statistic is the median of each round's paired ratios:

| Workload | baseline / quick | quick / PHP |
|---|---:|---:|
| Existing `< cutoff` conditional loop | 2.65x | 1.94x |
| New modulo/equality conditional loop | 2.97x | 1.62x |

The less-than result is effectively unchanged from Phase 2b. Reducing that
plan by one enum dispatch did not create a measurable speedup. The newly
covered modulo loop is nearly three times faster than rphp baseline, although
PHP without JIT remains approximately 1.6 times faster.

A further experimental `Mod + Eq + ConditionalAddAssign` superinstruction
reduced the hot path to two dispatches but did not improve the paired
baseline/quick ratio and worsened the PHP ratio in the sample. It was removed.
The larger match arm and additional live state outweighed the saved dispatch.
This is evidence against growing whole-kernel variants merely to minimize an
operation count.

Runtime statistics for both retained conditional plans show one entry, one
completion, 9,999,967 quick iterations, zero deoptimizations, and zero guard
failures. The test matrix now contains five detector tests and 20 end-to-end
quick-loop tests in each feature mode, including negative remainder semantics,
non-zero equality constants, and conditional-add overflow deoptimization.

The next executor experiment should compact the representation rather than
add a larger superinstruction:

1. pre-resolve internal `next_ip` values to compact operation indices so the
   executor does not perform `ip_to_op` lookup after every typed operation;
2. keep baseline IPs only in the deoptimization/exit metadata;
3. measure the compact encoding on nested, less-than branch, and modulo branch
   together before extending coverage to scalar array reads.

## Phase 2d result: pre-resolved compact control flow

The typed program now represents every normal control-flow edge with a
four-byte `QuickLongTarget`. During detection it temporarily contains a
baseline instruction position. After all operation boundaries are known, the
planner rewrites it to either:

- the `u16` index of another `QuickLongOp`; or
- a forward exit instruction position.

The detector's `ip_to_op` table is therefore construction-only. It is no
longer stored in `QuickLongOpsLoop`, and the executor no longer subtracts the
region header, bounds-checks that relative position, loads the mapping entry,
and tests its sentinel after every typed operation. An internal edge now
assigns its already-resolved operation index directly.

Baseline positions were not removed from fallible operations. `Mod`, `Add`,
and `PostInc` still retain their precise resume instruction. A small
operation-index-to-IP table is retained only for the cold interrupt path;
normal forward exits carry their baseline IP directly. This keeps exact
deoptimization and interrupt semantics independent from the hot encoding.

This lowering is applied to every supported `QuickLongOp` target. It does not
inspect PHP variable names, constants used by the benchmarks, or a particular
loop source shape.

A Phase 2c release binary was saved before the change and compared directly
with the Phase 2d release. Eleven rounds alternated binary order:

| Workload | median Phase 2d / Phase 2c | CPU/elapsed reduction |
|---|---:|---:|
| Nested two-CV addition | 0.918x elapsed | 8.2% |
| Less-than conditional addition | 0.707x elapsed | 29.3% |

The host then became severely scheduler-bound: the same modulo binary varied
between roughly 0.2 and 8.5 seconds of wall time while the Codex renderer used
more than three CPU cores. Wall time was discarded for that workload. Five
alternating pairs instead measured aggregate process user CPU time over two
executions. Their Phase 2d / Phase 2c ratios were `0.667`, `0.771`, `0.675`,
`0.784`, and `0.757`; the median is `0.757x`, or 24.3% less CPU.

The three structurally different regions all improve, while the largest
benefit occurs in the programs with more typed control-flow transitions.
That is strong evidence that repeated target decoding was a real part of the
remaining interpreter cost. It does not by itself distinguish the removed
table load from secondary compiler/code-layout effects, so instruction-level
profiling remains the right tool before another representation change.

Runtime accounting is unchanged:

- nested: 1500 entries and completions, 2,248,468 quick iterations;
- less-than and modulo: one entry and completion each, 9,999,967 iterations;
- all three: zero guard failures and zero deoptimizations.

The compact-target unit test verifies that internal edges are resolved to
operation indices and exits to baseline IPs. All 20 end-to-end quick-loop
tests pass, including overflow/deoptimization cases, and the complete test
suite passes with default features and with `--no-default-features`.

The next boundary should be treated as coverage rather than another
benchmark-specific fusion. Scalar array reads are the first important
candidate, but they require a guarded array layout/version contract and a
precise side-exit for missing keys. Before implementing them, profile the
current compact executor to separate the remaining enum-dispatch cost from
slot-array traffic; that determines whether a denser typed bytecode or the
array guard should come first.

## Phase 2e result: profile-guided invariant-CV recurrence

Sampling the Phase 2d less-than branch workload put 69 of 76 samples, or
90.8%, inside `run_quick_long_ops_loop`. Region entry and baseline dispatch
are therefore no longer the dominant cost. Disassembly also showed more than
30 bounds-check failure edges in the typed executor, but isolated attempts to
remove them were not generally beneficial:

- unchecked slot access made the nested loop faster but consistently made the
  less-than branch about 13% slower;
- masking every slot index to six bits helped nested arithmetic by roughly
  11-13% but made the modulo branch roughly 11-13% slower;
- moving deoptimization blocks to cold helpers helped nested arithmetic while
  regressing both branch workloads by roughly 4-5%;
- separating cold resume positions reduced `QuickLongOp` from 56 to 48 bytes,
  but six alternating long pairs still showed a 5.5% nested regression.

All four experimental changes were removed. The result is important:
`run_quick_long_ops_loop` is sensitive to global code layout and register
allocation, so smaller source, fewer checks, or a denser operation record is
not sufficient evidence of a faster interpreter. Every executor change must
continue to pass all three workload shapes.

The nested profile also exposed a coverage mistake. Its hot inner loop,

```php
$sum += $outer + $inner;
```

has the same recurrence as the existing direct accumulator executor. The only
missing case was an invariant CV addend. `QuickLongTerm` now accepts
`InductionPlusCv` in either operand order. Planning proves that this CV is
distinct from the induction and accumulator slots, is a non-heap long, and is
not written by the region. The guarded executor can consequently load it once
and retain the existing precise side exits for term, accumulator, and
post-increment overflow.

This is a class-level extension rather than a benchmark-specific whole-loop
variant. It covers inner-loop offsets, precomputed scalar coefficients, and
other invariant local integer addends with the same bytecode shape.

Six long A/B rounds alternated Phase 2d and Phase 2e binary order and measured
aggregate process user CPU time:

| Workload | median Phase 2e / Phase 2d | CPU reduction |
|---|---:|---:|
| Nested invariant-CV accumulation | 0.273x | 72.7% |
| Less-than conditional addition | 0.959x | 4.1% |
| Modulo/equality conditional addition | 0.997x | within noise |

Seven additional runs compared the elapsed time reported inside each script
with Homebrew PHP 8.4.12 and CLI opcache disabled:

| Workload | rphp median | PHP median | rphp / PHP |
|---|---:|---:|---:|
| Nested invariant-CV accumulation | 0.00204 s | 0.00747 s | 0.273x |
| Less-than conditional addition | 0.03998 s | 0.02757 s | 1.45x |
| Modulo/equality conditional addition | 0.04731 s | 0.03618 s | 1.31x |

The arithmetic recurrence is now about 3.66x faster than PHP without JIT.
The largest measured gap has moved to general typed branch execution:
approximately 45% for the less-than plan and 31% for modulo/equality. The next
performance step should therefore specialize the execution representation by
stable plan shape or split the monolithic branch executor, while retaining the
three-shape A/B acceptance gate. Array-read coverage remains the next semantic
expansion after that executor work.

Detection tests cover both commutative CV operand orders. The existing nested
overflow end-to-end test exercises exact deoptimization through the new
direct path. The complete test suite passes with default features and with
`--no-default-features`.

## Phase 2f result: stable conditional plan-shape kernels

Phase 2e left the two conditional workloads 31-45% behind PHP even though
their complete control flow had already been validated and compacted. The
remaining cost was executing three or four `QuickLongOp` dispatches on every
iteration.

Phase 2f reuses the existing typed plan as its semantic source rather than
adding another PHP-source detector. Once per region activation it recognizes
two stable operation graphs:

1. loop-header less-than, conditional less-than `AddAssign`, and fused
   `PostIncLoopLt`;
2. loop-header less-than, `ModConst`, conditional equality `AddAssign`, and
   fused `PostIncLoopLt`.

Recognition checks the entry operation, every internal target index, the
shared external exit, the copied header condition in `PostIncLoopLt`, and the
precise resume positions for modulo, addition, and post-increment. A mismatch
returns to the general typed executor. Direct equality without modulo and
modulo followed by less-than are deliberately tested fallback cases.

The selected graph runs through a shared conditional kernel. Its body
predicate is monomorphized for the two accepted shapes, so the hot loop has no
per-operation enum dispatch. It retains:

- the original one-time heap and long guards;
- the fixed scalar slot buffer and separate dirty long/boolean masks;
- exact baseline resume positions after modulo, addition, or increment
  failure;
- the 32-iteration interrupt check and the same next-IP reconstruction;
- the original completion, deoptimization, and guard accounting.

An initial four-predicate experiment produced the same speed but about 14 KiB
of additional machine code. The retained version specializes only the two
measured stable families; its extractor, dispatcher, and two kernels occupy
approximately 4.6 KiB. The general `run_quick_long_ops_loop` remains a
separate approximately 3.7 KiB fallback.

Six long rounds alternated the Phase 2e and Phase 2f release binaries:

| Workload | median Phase 2f / Phase 2e | CPU reduction |
|---|---:|---:|
| Nested invariant-CV accumulation | 1.000x | no regression |
| Less-than conditional addition | 0.559x | 44.1% |
| Modulo/equality conditional addition | 0.491x | 50.9% |

Seven runs then compared the elapsed time measured inside each script against
Homebrew PHP 8.4.12 with CLI opcache disabled:

| Workload | rphp median | PHP median | rphp / PHP |
|---|---:|---:|---:|
| Less-than conditional addition | 0.02172 s | 0.02842 s | 0.764x |
| Modulo/equality conditional addition | 0.02341 s | 0.03657 s | 0.640x |

The two branch workloads are now approximately 1.31x and 1.56x faster than
PHP without JIT. Together with Phase 2e, all three scalar acceptance workloads
now beat PHP while retaining baseline side exits.

The quick-loop end-to-end suite now contains 25 tests. New cases cover
overflow in the specialized less-than conditional addition and explicit
fallback for the two nearby unsupported graphs. The complete test suite
passes with default features and with `--no-default-features`.

The next performance work should widen useful program coverage rather than
specialize more predicate combinations without evidence. Scalar array reads
remain the first materially different boundary: they need an immutable
layout/version guard, key-existence side exits, and correct value ownership.

## Phase 2g result: guarded packed-array long reads

Packed arrays in this runtime are reference-counted and use copy-on-write.
That makes a read-only quick region possible without a global mutation
version:

- the planner rejects array writes, calls, and every other unsupported opcode;
- an array input slot cannot overlap a scalar output slot;
- reference values fail the `Array` guard;
- mutations through another ordinary array value detach when the `Rc` is
  shared, leaving the guarded allocation stable.

At region entry the executor validates `Array` plus packed storage and records
a borrowed pointer/length view. The source `Value` remains live for the whole
region. Hash storage takes a guard failure. A negative or out-of-range index,
a missing key, or a non-long element commits preceding scalar work and resumes
at the original `FetchDimR`; no fetch result is fabricated or committed before
that side exit.

The general typed program gains `FetchPackedLong`. Its array inputs have a
separate mask, while CV/TMP integer indices remain in the long-input mask.
This lets more complex closed scalar programs use packed reads without
pretending that the array itself is a scalar.

The common recurrence

```php
for ($i = 0; $i < $n; $i++) {
    $sum += $values[$i];
}
```

is also represented as `QuickLongTerm::PackedArrayIndex`. It reuses the direct
accumulator executor, so the hot loop performs one checked packed read, one
checked addition, and the induction update without typed-operation dispatch.
The general `FetchPackedLong` remains available for shapes with additional
assignments or arithmetic.

A new benchmark builds `range(1, 1_000_000)` before timing and sums every
element. Eleven rounds alternated Phase 2f, Phase 2g, and PHP process order and
used the elapsed time reported inside the script:

| Engine | Median | Relative to Phase 2f |
|---|---:|---:|
| Phase 2f rphp | 0.01696 s | 1.000x |
| Phase 2g rphp | 0.00136 s | 0.080x |
| PHP 8.4.12, CLI opcache disabled | 0.00432 s | 0.255x |

The retained implementation is therefore about 12.5x faster than the prior
rphp path and 3.18x faster than PHP without JIT on this packed-long
recurrence.

The existing scalar acceptance workloads were rerun against the saved Phase
2f binary. Nested and modulo medians did not regress. An additional eight-pair
branch run with 100 process repetitions per sample produced a median
candidate/reference ratio of `0.996x`.

Validation now includes eight quick-plan detector tests and 29 quick-loop
end-to-end tests. Array cases cover:

- direct packed-long accumulation;
- the general typed fetch operation;
- exact missing-key and non-long-element side exits after the hot threshold;
- rejection of hash storage;
- unchanged overflow and conditional fallback behavior.

The complete test suite passes with default features and with
`--no-default-features`. Future array coverage should be driven separately:
hash lookups need a stable hash-layout contract, while writes require explicit
copy-on-write and mutation-version semantics and must not reuse this borrowed
packed view.

## Phase 2h result: guarded hash-array reads

The packed-only guard was widened without weakening its ownership contract.
`QuickLongArray` now records either the existing raw packed value slice or a
borrowed pointer to an immutable `PhpArray` in hash storage. The source array
slot remains live, calls and writes are still rejected by the planner, and COW
keeps aliases from mutating the guarded allocation.

`QuickArrayIndex` distinguishes long keys from string literals. Canonical
numeric string literals use the same integer-key normalization as baseline
`FetchDimR`; other literals remain string keys. The typed program consequently
uses the general `FetchArrayLong` operation and one array-input mask for both
storage layouts. Missing keys and non-long values commit earlier scalar state
and resume at the original fetch instruction.

The direct accumulator executor also accepts constant integer and string
indices. Since both the array and literal key are invariant in that closed
region, it validates and loads the long once at activation. A failed invariant
lookup is a guard failure before any quick state is committed. Dynamic integer
indices continue to perform a checked lookup per iteration.

The array representation received two general changes based on the first hash
profile:

- integer keys use a dedicated SplitMix64-based hasher instead of the default
  string-oriented randomized hasher; string keys retain the default hasher;
- hash storage first tries a validated ordered-entry position derived from its
  first integer key. This covers arrays that transitioned from packed storage
  and contiguous non-zero integer runs. Any hole, reordering, string prefix, or
  other mismatch falls through to the integer hash index.

The positional path is an optimization, not a layout assumption: the stored
`ArrayKey` must equal the requested key before its value is returned. Removal,
cloning, negative keys, irregular keys, and the fallback index have dedicated
tests.

Before Phase 2h, five-run medians were `0.11866 s` for a one-million-element
array transitioned to hash storage and `0.40995 s` for ten million reads of one
string key. Nine final rounds alternated rphp and PHP execution order:

| Workload | Phase 2h rphp | PHP 8.4.12, no CLI opcache | Result |
|---|---:|---:|---:|
| Packed integer keys, 1M reads | 0.00160 s | 0.00444 s | rphp 2.78x faster |
| Hash storage, contiguous integer keys, 1M reads | 0.00508 s | 0.00535 s | rphp 1.05x faster |
| Hash storage, irregular stride-7 keys, 250K reads | 0.00693 s | 0.00175 s | rphp 3.96x slower |
| Invariant string key, 10M reads | 0.00837 s | 0.07850 s | rphp 9.38x faster |

The contiguous hash recurrence is about 23.4x faster than the previous rphp
path; the invariant string recurrence is about 49.0x faster. The irregular
benchmark deliberately defeats the positional path. Quick execution still
reduces its median from `0.01320 s` with quick loops disabled to `0.00693 s`,
but its remaining gap is now the next concrete target: the generic typed-op
dispatch around each lookup and the fallback integer table, rather than
baseline PHP value ownership.

Validation contains ten quick-plan tests and 35 quick-loop end-to-end tests.
New cases cover integer and string hash reads, numeric-string normalization,
the general typed string-fetch shape, missing/non-long dynamic-key side exits,
and hash-index behavior after removal and clone. The complete suite passes
with default features and with `--no-default-features`. Five 200-process
aggregate CPU comparisons of the nested scalar workload also stayed within
normal run noise of the saved Phase 2f binary.

## Phase 2i result: strided hash-read kernel

The irregular-key benchmark compiles to one stable five-operation typed graph:

1. loop-header less-than branch;
2. dynamic integer `FetchArrayLong`;
3. accumulator `AddAssign`;
4. key-step `AddAssign`;
5. `PostIncLoopLt`.

Phase 2i recognizes only that complete graph, including every sequential
target, the shared exit target, the copied loop condition, and the body
backedge. It reuses the already validated `QuickLongOpsLoop`; no second bytecode
analysis or weaker semantic assumption is introduced.

The kernel executes both additions with checked overflow, retains the original
resume IP for the fetch, both additions, and the post-increment, and commits
only operations that baseline execution would already have completed at each
side exit. Interrupt handling commits the same long/bool masks and records
either the body or exit target before invoking the existing handler.

To reduce short-run noise, the irregular stride-7 benchmark was increased from
250,000 to 1,000,000 reads. Eleven rounds alternated Phase 2i, the saved Phase
2h binary, and PHP:

| Engine | Median | Relative to Phase 2h |
|---|---:|---:|
| Phase 2h rphp, typed-op dispatch | 0.03781 s | 1.000x |
| Phase 2i rphp, specialized kernel | 0.02884 s | 0.763x |
| PHP 8.4.12, CLI opcache disabled | 0.00699 s | 0.185x |

Removing typed dispatch therefore reduces this recurrence by about 23.7%.
The remaining rphp/PHP gap is still 4.12x. This experiment isolates the next
boundary cleanly: approximately nine nanoseconds per iteration came from typed
operation dispatch, while the remaining cost is dominated by the fallback
integer index lookup and its indirect entry load.

Acceptance reruns showed no material change in packed, contiguous-hash,
invariant-string, or conditional scalar loops. Validation now includes eleven
quick-plan tests and 38 quick-loop end-to-end tests. The new cases cover the
exact stride graph, successful key/accumulator state, a non-long fetch side
exit, and accumulator overflow after the hot threshold.

## Phase 2j result: guard-time integer lookup routing

Profiling the Phase 2i kernel showed that an irregular integer read still
entered `PhpArray::get_int()` on every iteration. That general method first
tries the validated ordered-entry position and only then enters the integer
hash index. The stride-7 workload therefore paid for a positional probe that
was known to fail for the entire immutable guarded region.

`PhpArray` now exposes two separate pieces of that existing contract:

- `prefers_positional_int_lookup()` classifies hash storage from its ordered
  entry prefix. It is a routing hint, not a correctness assumption;
- `get_indexed_int()` performs the canonical integer-index lookup directly.

At quick-region activation, hash-array inputs are classified once. Packed and
positionally useful hash arrays retain the original array-stride runner
unchanged. Only an irregular hash input selects a separate indexed runner,
which skips the failed positional probe inside the loop. That runner preserves
the same checked additions, dirty-slot commits, exact resume IPs, interrupt
handling, and missing/non-long side exits as Phase 2i.

Two broader implementations were measured and rejected:

- replacing `HashMap` with a custom open-addressed integer table reduced the
  lookup median from `0.02978 s` to `0.02720 s` (about 8.7%) but increased the
  full construction CPU time from roughly `0.07 s` to `0.09 s` (about 29%);
- inlining all packed, positional, and indexed layouts into a widened common
  runner improved irregular reads but created measurable register pressure in
  the packed hot loop.

The retained design changes neither the array representation nor construction.
It also deliberately keeps the established packed/positional machine-code
path separate from the new irregular-index path.

Twenty-one final rounds rotated Phase 2j, the saved Phase 2i binary, and PHP
process order:

| Engine | Median | Relative to Phase 2i |
|---|---:|---:|
| Phase 2i rphp | 0.02990 s | 1.000x |
| Phase 2j rphp | 0.02277 s | 0.761x |
| PHP 8.4.12, CLI opcache disabled | 0.00694 s | 0.232x |

Guard-time routing therefore removes another 23.9% from the irregular
recurrence. The remaining rphp/PHP gap falls from 4.12x in the Phase 2i
measurement to about 3.28x here. The result also confirms the conceptual
diagnosis: a material part of the remaining cost was not hashing itself, but
repeating a layout decision after the layout had already become stable.

Twenty-one-pair acceptance medians versus the saved Phase 2i binary were
`0.988x` for packed reads, `1.030x` for contiguous-hash reads, `0.950x` for
invariant string reads, `1.008x` for the branch loop, and `1.007x` for the
modulo-branch loop. These small sub-millisecond differences are within the
observed process-level noise and show no broad regression. Full tests pass
with default features and with `--no-default-features`; dedicated array tests
cover both routing classes and direct indexed hits/misses, while the Phase 2i
end-to-end tests continue to exercise successful execution and exact
non-long/overflow deoptimization on the newly routed path.

## Phase 2k result: guard-time array-loop templates

Phase 2j solved the exact stride recurrence, but realistic array code commonly
materializes the fetched value, updates more than one aggregate, or filters the
aggregate conditionally. Two new one-million-element workloads keep array
construction outside the timed region and exercise those shapes:

- `bench_hash_sparse_int_array_transform.php` assigns the fetched value and
  updates two aggregates before stepping the irregular integer key;
- `bench_hash_sparse_int_array_filter.php` assigns the fetched value and
  conditionally updates an aggregate from the fetched data.

Both workloads already entered the guarded typed program and completed
999,967 iterations there after the hot threshold. A four-second sample of a
100-pass transform attributed 3,073 of 3,263 hot samples (94.2%) to
`run_quick_long_ops_loop` itself and only 190 samples (5.8%) to the hash lookup.
The largest remaining cost was therefore dynamic typed-operation execution and
slot traffic, not the integer hash function.

Several attempts to make the generic runner cheaper were measured and
rejected:

- a per-fetch indexed-layout mask repeated a stable decision in the hot body
  and did not improve the workloads;
- duplicating the complete runner with a const-generic indexed mode increased
  code size and caused broad 1–3% acceptance regressions;
- cloning the typed program into runtime-specific fetch operations, including
  a raw array pointer, did not improve the direct lookup after register
  pressure was considered;
- a sequential-target sentinel removed target decoding but made transform,
  string, nested, and exact-stride measurements worse;
- a general linear-body runner still interpreted each body operation and was
  8.5% slower on transform, 18.2% slower on filter, and 4.9% slower on the
  nested acceptance workload.

The retained design instead selects a complete array-loop template once after
the existing type and array guards. The template detector validates the entry
branch, sequential body targets, shared exit, loop condition, backedge, fetch,
and post-increment. It currently covers three structural typed bodies:

1. a fetch followed by two checked assignments (the Phase 2j stride shape);
2. a materialized fetch followed by an add, fused add-plus-add, and key step;
3. a materialized fetch followed by a conditional add and key step.

The common loop skeleton is monomorphized with the selected body and fetch
layout, so no operation-kind dispatch remains inside those bodies. Irregular
integer hash input continues to use the guard-time direct index route. The
original two-add indexed shape retains a fully direct static body, while packed
and other array layouts use the same validated template contract.

`FetchDimR` followed immediately by `AssignCv` is also fused in the typed plan.
The fetch operation records the optional destination and commits it only after
a successful typed read. This peephole is general to every eligible compiled
PHP loop; it does not depend on benchmark names, source variable names, or
literal iteration counts.

All arithmetic remains checked. Every template carries the original resume IP
for fetch, each constituent addition, and post-increment. A side exit commits
only the temporary/CV writes completed before that point. New end-to-end cases
exercise a non-long fetched value, overflow in the fused transform body without
replaying an earlier accumulator update, and conditional-filter overflow.

Thirty-one final rounds rotated Phase 2k, the saved fusion-only binary, the
saved Phase 2j binary, and PHP process order:

| Workload | Phase 2j | Phase 2k | PHP 8.4.12, no CLI opcache | Phase 2k / Phase 2j |
|---|---:|---:|---:|---:|
| Materialized transform, 1M reads | 0.13700 s | 0.08067 s | 0.02510 s | 0.589x |
| Conditional filter, 1M reads | 0.12403 s | 0.09066 s | 0.01951 s | 0.731x |

The structural template removes about 41.1% from the transform and 26.9% from
the filter. PHP remains 3.21x and 4.65x faster respectively, so the result
reduces rather than closes the no-JIT gap.

Twenty-one-round acceptance medians versus the saved Phase 2j binary were
`0.999x` for the exact irregular stride loop, `0.981x` for packed reads,
`1.001x` for nested scalar loops, and `1.003x` for the string workload. A
100-pass exact-stride comparison also favored Phase 2k in both execution
orders. The complete test suite passes with default features and with
`--no-default-features`; the planner now has 13 focused tests and the quick-loop
end-to-end suite has 42 tests.

## Phase 2l result: validated sparse-position routing

A fresh profile after Phase 2k changed the diagnosis substantially. In the
100-pass transform, 2,404 of 3,995 hot samples (60.2%) were now inside the
direct integer-index fetch closure and 1,591 (39.8%) were in the template body.
The filter attributed 1,247 of 4,007 samples (31.1%) to fetch and 2,760 (68.9%)
to its conditional body. Phase 2k had therefore removed the generic typed-op
bottleneck; the next common cost was the hash index itself.

Sparse PHP arrays are often inserted and scanned in a stable arithmetic key
progression even though they cannot use packed storage. Phase 2l derives a
guard-time `(first_key, stride)` hint from at most the first eight ordered
integer entries. The short prefix check rejects genuinely irregular leading
layouts without scanning the whole array or changing construction cost.
Negative strides are supported.

The hint is not a correctness assumption. Every read:

1. computes an entry position with checked subtraction, remainder, division,
   and integer conversion;
2. bounds-checks the ordered entry and validates that its stored integer key is
   exactly the requested key;
3. falls back to the canonical integer hash index after any arithmetic,
   bounds, key, hole, removal, or interleaved-entry mismatch.

The array remains immutable only for the duration of the already guarded quick
region. Existing copy-on-write and side-exit rules are unchanged. The hint is
stored in a separate guard-time table rather than adding a variant to
`QuickLongArray`. An initial implementation widened that enum and made the
short packed benchmark about 11.5% slower; it was rejected. Keeping the old
array view byte-for-byte unchanged isolated the new route from packed and
generic execution.

The array-loop dispatcher selects the validated-position fetch closure once
before entering the template. Arrays whose eight-entry prefix is irregular
retain the Phase 2k direct hash-index route. A permanent
`bench_hash_irregular_prefix_int_array_transform.php` workload verifies that
negative classification: its 31-round median was `0.05245 s` in Phase 2l and
`0.05351 s` in Phase 2k (`0.980x`).

Fifty-one final rounds rotated Phase 2l, the saved Phase 2k binary, and PHP
process order:

| Workload | Phase 2k | Phase 2l | PHP 8.4.12, no CLI opcache | Phase 2l / Phase 2k |
|---|---:|---:|---:|---:|
| Materialized transform, 1M reads | 0.05697 s | 0.01473 s | 0.02092 s | 0.259x |
| Conditional filter, 1M reads | 0.05689 s | 0.01343 s | 0.01710 s | 0.236x |
| Exact sparse stride, 1M reads | 0.03612 s | 0.01131 s | 0.01351 s | 0.313x |

The transform is about 3.87x faster than Phase 2k and 29.6% faster than PHP.
The filter is about 4.24x faster than Phase 2k and 21.4% faster than PHP. The
exact sparse recurrence is about 3.19x faster than Phase 2k and 16.2% faster
than PHP. These are interpreter results without JIT or CLI opcache.

Forty-one-round short acceptance medians versus Phase 2k were `1.004x` for
packed reads and `1.003x` for the string workload. Longer paired runs reduced
the apparent short-run variance: a 500-pass contiguous-hash workload had
median CPU times of `3.345 s` for Phase 2l and `3.370 s` for Phase 2k, while a
1,000-pass nested scalar workload stayed within about 1–2% depending on order.

Post-change profiles no longer contain the integer `HashMap` lookup as a
separate hot symbol. The validated-position closure accounts for about 39.8%
of transform samples and 40.7% of filter samples; their actual template bodies
now account for roughly 60%. The next performance boundary is consequently
checked positional arithmetic and scalar slot work, not hashing.

New unit tests cover negative strides and rejection of irregular prefixes.
Quick-loop end-to-end tests cover descending scans and exact hash fallback
after removal/reinsertion changes ordered positions. The focused array suite
now has four tests and the quick-loop end-to-end suite has 44 tests. The
complete suite passes with default features and with
`--no-default-features`; warning-free checks also pass for the minimal and
all-feature builds.

## Phase 2m result: general one-add array template

The first Phase 2m measurement appeared to improve the existing
`$sum += $values[$i]` benchmark by several percent. Inspection of plan
selection disproved that interpretation: this exact short recurrence is
claimed first by `QuickLongAccumulateLoop`, so the new typed-array template
does not execute it. A 500-pass comparison subsequently put both binaries at
about `5.045 s` of CPU time on the sparse transform, confirming that the
apparent short-run regression elsewhere was also process noise.

The real uncovered shape is equally common but structurally more general:

```php
$value = $values[$i];
$sum += $value;
```

Materializing the fetched value adds a PHP assignment that the narrow
accumulator plan cannot represent. The typed planner already fuses that
assignment into `FetchArrayLong`, producing the stable four-operation graph
`BranchUnlessLt -> FetchArrayLong -> AddAssign -> PostIncLoopLt`. Phase 2m
recognizes this complete graph as a one-add array body and runs it through the
same prevalidated array-loop skeleton used by the Phase 2k templates. The
body no longer returns to the general typed-operation dispatcher on every
iteration.

This is a structural optimization, not a benchmark-specific rewrite. It is
selected from the typed plan after the existing long and immutable-array
guards, is independent of source variable names and loop bounds, and applies
to packed, integer-hash, sparse-position, indexed-hash, and string-key fetch
routes. Checked addition, the original fetch/add/post-increment resume
positions, precise dirty-slot commits, interrupt handling, and non-long or
overflow side exits remain shared with the existing template machinery.
Direct non-materialized accumulations continue to use the older,
more-specialized accumulator runner.

Fifty-one rounds rotated Phase 2m, the saved Phase 2l binary, and PHP process
order:

| Workload | Phase 2l | Phase 2m | PHP 8.4.12, no CLI opcache | Phase 2m / Phase 2l |
|---|---:|---:|---:|---:|
| Contiguous integer hash, materialized value, 1M reads | 0.01929 s | 0.01372 s | 0.01233 s | 0.711x |
| Packed array, materialized value, 1M reads | 0.01047 s | 0.00772 s | 0.00971 s | 0.737x |

The general template removes about 28.9% from the integer-hash case and 26.3%
from the packed case. Packed materialized reads are about 20.5% faster than
PHP without JIT; the contiguous-hash gap is reduced to about 11.2%. A
supplemental 31-round invariant string-key measurement improved from
`0.27034 s` to `0.22157 s` (`0.820x`), although PHP remains about 1.35x faster
on that materialized string shape.

The same 51-round acceptance run measured `0.946x` for the sparse transform,
`1.027x` for the conditional filter, and `1.009x` for the exact sparse loop
relative to Phase 2l. The latter two sub-millisecond differences are within
observed process noise, while all three workloads remain faster than PHP.
Planner validation now proves both the fused-fetch graph and that the compiler
actually selects `QuickLongOps` rather than the older accumulator plan for
the materialized source. End-to-end cases exercise overflow deoptimization
for integer-hash and string-key layouts while also validating the final
materialized value. The quick-loop suite now has 46 tests, and the complete
suite passes with default features and with `--no-default-features`;
warning-free checks also pass for the minimal and all-feature builds.

## Phase 2n result: materialized invariant array accumulation

Phase 2m removed typed-operation dispatch from a materialized one-add loop, but
its general array template still performed a complete lookup on every
iteration for a literal key:

```php
$value = $values['hot'];
$sum += $value;
```

The guarded region already proves that the source array cannot change while
the quick runner owns it. Repeating `get_str()` for the same array and literal
therefore carried no new semantic information. The same issue applied to a
literal integer index such as `$values[7]`.

The first implementation cached the lookup in the general array-template
dispatcher. It reduced the string workload to roughly `0.037 s`, but also
introduced another wide generic closure instantiation. A 500-pass conditional
filter comparison measured a `1.227x` regression against Phase 2m. Narrowing
that approach reduced but did not reliably eliminate the layout effect, so
the implementation was rejected.

The retained design extends the older `QuickLongAccumulateLoop` instead. Its
detector now accepts the eight-instruction materialized form only when the
array index is a literal string or integer. `QuickLongTerm::ArrayIndex` records
the optional materialized destination, while the existing accumulator runner
performs its already established guard-time invariant fetch. Dynamic indexes
continue to use the Phase 2m one-add template and Phase 2l position routing
unchanged.

The accumulator runner keeps induction, fetched term, and accumulator in Rust
scalars. It writes the materialized PHP variable at every observable boundary:
normal completion, checked-add or post-increment deoptimization, and interrupt
handling. Missing or non-long values fail the guarded activation and resume
the baseline loop. Planner checks prevent the destination from aliasing the
array, induction, accumulator, or loop bound, and the existing heap/type
guards cover the new destination slot.

Fifty-one rounds rotated Phase 2n, the saved Phase 2m binary, and PHP process
order:

| Workload | Phase 2m | Phase 2n | PHP 8.4.12, no CLI opcache | Phase 2n / Phase 2m |
|---|---:|---:|---:|---:|
| Materialized invariant string key, 10M reads | 0.22808 s | 0.01543 s | 0.16910 s | 0.068x |
| Materialized invariant integer key, 10M reads | 0.10616 s | 0.01803 s | 0.13936 s | 0.170x |

The retained accumulator path removes about 93.2% from the string-key
recurrence and 83.0% from the integer-key recurrence. It is approximately
14.8x and 5.9x faster than Phase 2m respectively. Against PHP without JIT,
rphp is about 11.0x faster on the materialized string key and 7.7x faster on
the materialized integer key.

The same 51-round acceptance run measured `0.937x` for the original direct
string recurrence, `1.014x` for dynamic materialized integer-hash reads,
`1.016x` for packed materialized reads, `1.008x` for the sparse transform,
`1.009x` for the conditional filter, and `0.995x` for the exact sparse loop.
All non-target differences are within 1.6% process-level variance. Focused
tests cover selected string/integer plans, successful destination
materialization, missing and non-long fallback, and overflow after a
successful materialized fetch. The planner suite now has 14 focused tests and
the quick-loop end-to-end suite has 49 tests. The complete suite passes with
default features and with `--no-default-features`; warning-free checks also
pass for the minimal and all-feature builds.

## Phase 2o result: regular suffix position routing

A fresh non-PGO audit exposed one remaining array outlier:
`bench_hash_irregular_prefix_int_array_transform.php` was about 4.81x slower
than PHP even though the million timed keys form the same stride-7 progression
that Phase 2l handles efficiently. The array deliberately inserts eight
irregular metadata keys before that progression. Phase 2l inspected only the
first eight ordered entries, rejected the layout, and sent every timed read to
the integer hash index.

Phase 2o keeps guard-time classification constant-time but checks two bounded
windows: at most eight entries at the prefix and, only when that fails, at
most eight entries at the suffix. A regular suffix may begin at any ordered
entry position. Rather than widening `QuickLongIntPositionHint`, the classifier
derives the virtual key that would occupy entry position zero:

```text
position_zero_key = anchor_key - stride * anchor_position
position          = (requested_key - position_zero_key) / stride
```

The runtime hint therefore remains the same two `i64` values as Phase 2l and
does not add hot-loop state or register pressure. The derived position is
still only a prediction: `get_positioned_int()` bounds-checks the entry and
compares its stored integer key with the requested key before returning it.
Holes, separators, removals, arithmetic failure, and unrelated keys retain
the canonical integer-index fallback.

The benchmark runner itself also had a pre-audit failure: under `pipefail`,
its unconditional `find ... | head -1` lookup could terminate the
`--no-pgo` path before printing output. LLVM-tool discovery now runs only for
PGO builds, uses the active Rust sysroot, and stops through `find -print -quit`
without a SIGPIPE pipeline.

Fifty-one rounds rotated Phase 2o, the saved Phase 2n binary, and PHP process
order:

| Workload | Phase 2n | Phase 2o | PHP 8.4.12, no CLI opcache | Phase 2o / Phase 2n |
|---|---:|---:|---:|---:|
| Regular sparse suffix after irregular prefix, 1M reads | 0.07468 s | 0.01492 s | 0.01981 s | 0.200x |
| Irregular prefix and deliberately irregular suffix, 1M reads | 0.06471 s | 0.06455 s | 0.01965 s | 0.998x |

Suffix routing removes about 80.0% from the target recurrence and makes rphp
about 24.7% faster than PHP without JIT. The negative classifier workload is
effectively unchanged, proving that a suffix which does not establish a
progression stays on the Phase 2n indexed path.

The same acceptance run measured `0.996x` for the original sparse transform,
`0.982x` for the sparse filter, `0.988x` for the exact sparse loop, `0.994x`
for dynamic materialized hash reads, and `1.001x` for packed materialized
reads. Unit coverage verifies virtual-origin derivation and rejection after a
suffix outlier. End-to-end coverage adds an interleaved string separator: the
regular tail takes the positional route while earlier keys miss the predicted
position and fall back exactly to the hash index. The quick-loop end-to-end
suite now has 50 tests. The complete suite passes with default features and
with `--no-default-features`; warning-free checks also pass for the minimal
and all-feature builds.

## Phase 2p result: guarded value-only foreach accumulation

The broad non-PGO audit after Phase 2o identified `foreach` as the largest
remaining common loop gap. The existing `bench_foreach.php` workload was about
2.95x slower than PHP even though indexed packed-array accumulation was already
faster. The difference was structural. Every element still crossed the full
baseline sequence:

```text
ForeachNext -> JmpZ -> Add -> AssignCv -> Jmp
```

That sequence repeatedly distinguished generators from arrays, decoded the
array layout, cloned the current value, materialized position/value/done/sum
VM slots, dispatched general arithmetic, and returned through the interpreter
backedge. The actual integer load and addition were a small part of the work.

Phase 2p recognizes the exact closed source shape:

```php
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
```

The detector is independent of variable names, array size, and packed versus
hash storage. It requires a value-only `ForeachNext`, the matching conditional
exit, one CV-to-CV `Add`, assignment back to that accumulator, and the immediate
backedge. A key variable, additional body instruction, generator, reference,
heap scalar, or different type keeps the ordinary interpreter path.

After the existing 32-backedge hot threshold, the guarded runner keeps iterator
position, current value, and accumulator in native Rust scalars. Packed arrays
use the stable `Value` slice directly. Hash arrays expose their immutable ordered
entry slice to the guarded region, allowing positional value reads without
re-matching `ArrayStorage` or rechecking a slice boundary at every iteration.
The foreach copy owns the source allocation for the region, and the proven body
cannot call PHP or mutate it, so both views stay valid until completion or a
side exit. The runner lives in a separate module/code-generation unit so its
machine-code size does not disturb the established indexed-array kernels in
the main executor.

The optimization retains exact baseline resume points:

- a non-integer element is not consumed; prior quick state is committed and
  execution resumes at `ForeachNext`;
- accumulator overflow commits the consumed position and current value, then
  resumes at the original `Add`, which performs PHP's integer-to-float result;
- interrupts commit the state after the completed body and retain the normal
  interrupt handling bound;
- normal exhaustion materializes the final position, value, done flag,
  accumulator, and sum temporary exactly as the skipped baseline instructions
  would have left them.

Fifty-one rounds rotated Phase 2p, the saved Phase 2o binary, and PHP process
order:

| Workload | Phase 2o | Phase 2p | PHP 8.4.12, no CLI opcache | Phase 2p / Phase 2o |
|---|---:|---:|---:|---:|
| Packed value-only foreach, 500K values | 0.01151 s | 0.000759 s | 0.004205 s | 0.066x |
| Hash value-only foreach, 500K + string-tail values | 0.01483 s | 0.004075 s | 0.003331 s | 0.275x |

The packed recurrence is about 15.17x faster than Phase 2o and 5.54x faster
than PHP without JIT. The ordered hash recurrence is about 3.64x faster than
Phase 2o, but remains about 22.3% slower than PHP. Dispatch and VM-slot traffic
are therefore no longer the hash bottleneck. The release assembly advances
40 bytes per rphp `(ArrayKey, Value)` entry, while a 64-bit Zend `Bucket`
occupies 32 bytes. The remaining ratio closely follows that extra memory
traffic. Closing it requires a denser hash-entry representation or a separate
contiguous value view, not more interpreter dispatch specialization. A
permanent `bench_hash_foreach.php` workload now tracks this boundary.

Keeping the first implementation in `execute.rs` caused up to an 8.4% code
layout regression in an unchanged materialized hash loop. After module
isolation, the final 31-round array acceptance ratios were `1.015x` for packed
materialized reads, `1.015x` for hash materialized reads, and `1.010x` for the
sparse filter. A separate 15-round audit measured `1.002x` for the basic loop,
`0.973x` for the extended scalar loop, `0.994x` for calls, `1.008x` for
properties, and `0.968x` for strings. No retained non-target slowdown exceeded
about 1.6%.

Focused planner coverage proves selection for value-only accumulation and
rejection of key-value foreach. Four end-to-end cases cover packed and hash
completion, a non-integer side exit, overflow conversion, final accumulator,
and final loop value. The quick-loop end-to-end suite now has 54 tests. The
complete suite passes with all features and with `--no-default-features`;
warning-free checks pass for both configurations.

## Phase 2q result: compact shared hash keys

Phase 2p removed interpreter dispatch from value-only `foreach`, but its hash
scan remained about 22.3% slower than PHP. The release loop exposed the next
structural limit: rphp advanced 40 bytes per `(ArrayKey, Value)` entry, while a
64-bit Zend `Bucket` occupies 32 bytes. The public `ArrayKey::String(String)`
representation is useful at the API boundary, but its three-word `String`
header makes the enum 24 bytes even when the actual key is an integer.

Phase 2q separates that public representation from the ordered hash storage.
The internal entry uses a 16-byte `ArrayEntryKey`; an integer remains inline,
while a string is held by the thin `SharedStringKey(Rc<String>)` handle. The
ordered entry and string index clone only that handle, so they share the same
immutable key allocation instead of allocating and copying the text twice.
Together with the 16-byte `Value`, an ordered entry is now exactly 32 bytes:

```text
public boundary: ArrayKey::String(String)
                         ^ materialized only when a public key is requested

ordered entry:   [ ArrayEntryKey: 16 B ][ Value: 16 B ] = 32 B
                            |
                            +---- shared Rc<String> ---- string index
```

Public iteration still yields `(ArrayKey, &Value)` and therefore preserves the
observable key type. `PhpArrayIter` is now an opaque wrapper so the compact
private key cannot leak through its public variants. The guarded hash foreach
runner obtains only the first value address and the private entry stride; it
does not depend on, expose, or duplicate the key representation.

Fifty-one rotated rounds compared Phase 2q with the saved Phase 2p binary and
PHP. A longer repeated-scan control used 20 passes over the same array:

| Workload | Phase 2p | Phase 2q | PHP 8.4.12, no CLI opcache | Phase 2q / Phase 2p |
|---|---:|---:|---:|---:|
| Hash value-only foreach, cold process scan | 0.003939 s | 0.003105 s | 0.003469 s | 0.788x |
| Hash value-only foreach, 20 repeated scans | 0.018490 s | 0.017266 s | 0.065211 s | 0.934x |
| Packed value-only foreach, 20 repeated scans | 0.014494 s | 0.013964 s | 0.080324 s | 0.963x |

The denser entry removes about 21.2% from the cold hash scan and moves it from
22.3% behind PHP in Phase 2p to about 10.5% ahead of PHP without JIT. Repeated
hot-cache scans improve by 6.6%, as expected when memory traffic matters less.
The packed control does not use the new entry representation and stays within
normal process-level variance.

Non-target acceptance measurements found no persistent regression. The sparse
hash filter measured `0.929x`; materialized integer-hash reads measured
`1.006x`; dynamic string-key reads measured `1.005x`; and invariant string-key
reads measured `0.990x` against Phase 2p. Exact unit assertions lock the
16-byte internal key and 32-byte entry sizes, and another assertion proves
that the ordered entry and string index share the same allocation. The full
suite passes with all features and with `--no-default-features`; warning-free
checks pass for both configurations.

## Phase 2r result: borrowed and invariant runtime string keys

The Phase 2q follow-up audit separated literal and runtime string offsets. A
literal such as `$values['hot']` was already hoisted by Phase 2n, but storing
the same key in `$key` made the recurrence about 4.78x slower than PHP. The
general `FetchDimR` path first converted the borrowed PHP string into an owned
`ArrayKey::String`, allocating and copying it on every read, and the guarded
accumulator recognized only literal string keys or integer index slots.

Phase 2r fixes both layers without changing the public `ArrayKey` API. The
interpreter normalizes offsets into a private borrowed `ArrayKeyRef`; ordinary
reads now pass `&str` directly to `PhpArray::get_str()`. Canonical decimal key
normalization also validates the byte syntax before `i64::parse()` and no
longer allocates `i64::to_string()` merely to reject `01`, `+1`, whitespace,
`-0`, or an out-of-range integer.

The guarded accumulator adds `QuickArrayIndex::ValueSlot` for code such as:

```php
$key = get_runtime_key();
for ($i = 0; $i < $n; $i++) {
    $sum += $values[$key];
}
```

The detector is independent of variable names and key contents. It proves
that the key CV, source array, induction variable, bound, accumulator, and an
optional materialized destination do not overlap. At hot activation the runner
applies the ordinary PHP key normalization, performs one guarded array lookup,
and verifies that the result is a long. The closed body cannot write the key
or array, so the native loop can retain the fetched long just as it already did
for literal keys. A missing key, illegal offset type, non-long result, alias,
or unsupported body keeps the baseline instructions and their exact behavior.

Thirty-one rounds rotated Phase 2r, the saved Phase 2q binary, and PHP process
order:

| Workload | Phase 2q | Phase 2r | PHP 8.4.12, no CLI opcache | Phase 2r / Phase 2q |
|---|---:|---:|---:|---:|
| Invariant runtime string key, 10M reads | 0.896637 s | 0.015345 s | 0.172879 s | 0.0171x |
| String key changed inside loop, 1M reads | 0.105720 s | 0.060803 s | 0.022366 s | 0.5751x |
| Existing literal string recurrence, 10M reads | 0.015501 s | 0.015415 s | 0.155887 s | 0.9944x |
| Existing materialized literal recurrence, 10M reads | 0.015299 s | 0.015234 s | 0.167168 s | 0.9958x |

Hoisting the proven invariant runtime lookup removes about 98.3% of its old
time: it is approximately 58.4x faster than Phase 2q and 11.3x faster than PHP
without JIT. Even when the key changes inside the loop and cannot be hoisted,
the allocation-free interpreter read removes about 42.5%. That workload is
still 2.72x slower than PHP because the current typed loop state carries longs
and booleans, not mutable strings, so every iteration retains baseline dispatch
and hashing. The two permanent CV and changing-key benchmarks keep both sides
of this boundary visible.

Two broader alternatives were measured and rejected. Replacing the standard
string hasher with a seeded lightweight implementation slowed the changing-key
workload by about 6%. A two-position cache in every hash array either enlarged
the inlined invariant path enough to regress the established literal kernel by
about 11%, or, when isolated, merely exchanged hashing for call overhead. The
final implementation retains neither experiment.

Acceptance ratios were `0.990x` for materialized integer-hash reads, `1.021x`
for the sparse hash filter, and `0.994x` for hash foreach. A 51-round repeated
scan control measured `0.980x` for packed foreach and `0.945x` for hash foreach,
rejecting a noisy sub-millisecond short-run regression. Planner coverage proves
direct and materialized CV selection; end-to-end cases cover string and numeric
string keys plus final materialization, and normalization tests lock canonical
and noncanonical decimal behavior. The quick-loop end-to-end suite now has 57
tests. The complete suite passes with all features and with
`--no-default-features`; warning-free checks pass for both configurations.

## Phase 2s result: retained changing string-key state

Phase 2r removed allocation from the baseline string-offset conversion and
hoisted invariant runtime keys, but a key assigned inside the loop still ran
every bytecode instruction. The remaining cost was no longer string ownership
alone: each iteration dispatched the fetch, arithmetic, modulo, branches, and
assignment, cloned or dropped the selected string, wrote it back to the frame,
and repeated a hash lookup. That made the changing-key workload about 2.72x
slower than PHP even though the same invariant lookup was already much faster.

Phase 2s extends the existing closed typed region instead of adding a special
loop for one benchmark. Before building the plan, the detector finds CVs
assigned string literals within the region. A CV is accepted as retained string
state only when it is also read as an array index, remains disjoint from every
long, bool, and array slot, and every string output is a guarded string input.
Conflicting assignments, calls, aliases, unsupported instructions, or an
initial non-string value reject the fast activation and preserve the baseline
path.

The runner represents each accepted string CV as a pointer to either its live
frame value or an immutable OpArray literal. `AssignCv` therefore redirects a
pointer and marks the CV dirty; it does not clone, drop, or rewrite an `Rc`
string on every iteration. Dirty strings are committed on normal completion,
interrupt handling, and every deoptimization return. The existing long and
bool commit masks remain independent, so the baseline instruction can resume
with the exact completed state.

Changing keys also use a region-local successful-fetch cache keyed by the
immutable string allocation identity, length, and source array slot. The
planner proves that the source array cannot change within the closed region,
and both frame inputs and OpArray literals remain alive while cached identities
can be observed. A cache miss still performs canonical decimal-string
normalization followed by the ordinary integer or string lookup. Missing keys
and non-long values are deliberately not cached and deoptimize at the original
`FetchDimR` instruction.

The inline cache has no allocation and a hard four-entry storage bound, but its
active width is selected from the distinct string literals in the loop plan.
A two-way branch checks two entries; a three-way branch checks three. Explicit
inline probes avoid imposing a general cache loop on the common two-key case.
More than four literal identities retain exact behavior and use round-robin
replacement after a miss.

Fifty-one rounds rotated Phase 2s, the saved Phase 2r binary, and PHP process
order. The second workload is a separate three-literal control using the same
general planner:

| Workload | Phase 2r | Phase 2s | PHP 8.4.12, no CLI opcache | Phase 2s / Phase 2r |
|---|---:|---:|---:|---:|
| Two changing string keys, 1M reads | 0.059482 s | 0.024049 s | 0.022045 s | 0.404x |
| Three changing string keys, 1M reads | 0.065926 s | 0.028546 s | 0.025647 s | 0.433x |

The permanent two-key recurrence is about 59.6% faster than Phase 2r and now
9.1% behind PHP rather than 172% behind it. The three-key control improves by
56.7% and remains 11.3% behind PHP, showing that the result is not tied to one
pair of key names. At these durations normal process-level variance is several
percent, so the defensible boundary is a high-single- to low-double-digit PHP
gap rather than one exact percentage.

The 31-round non-target acceptance ratios against Phase 2r were `0.997x` for
an invariant runtime string key, `0.998x` for a literal string key, `0.997x`
for a materialized literal fetch, `0.969x` for materialized integer-hash reads,
`1.011x` for the sparse hash filter, and `1.006x` for hash foreach. Twenty-pass
controls measured `0.998x` for packed foreach and `0.957x` for hash foreach.
No established path has a retained regression outside normal short-run noise.

Planner tests lock both typed string selection and cache sizing from two and
three distinct loop literals. End-to-end cases cover ordinary string keys,
canonical numeric string keys, final string materialization, and an exact
non-long fetch side exit. The quick-loop end-to-end suite now has 60 tests.
The complete suite passes with all features and with `--no-default-features`;
warning-free checks pass for both configurations.
Arbitrary strings produced by calls, array reads, concatenation, or other
unsupported operations are intentionally not retained yet; supporting those
requires a broader string-producing typed IR rather than weakening these
guards.

## Phase 2t result: propagated immutable string-key CVs

Phase 2s retained keys selected directly from string literals inside a loop,
but the equivalent real-world form still stayed in baseline dispatch:

```php
$left = 'left';
$right = 'right';
$key = $left;
for ($i = 0; $i < $n; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = $right;
    } else {
        $key = $left;
    }
}
```

The loop bytecode assigns one CV to another rather than assigning a literal.
Treating every such assignment as a string would displace valid integer-key
plans, while cloning the source `Value` on every iteration would reintroduce
the ownership cost removed in Phase 2s.

Phase 2t adds conservative preheader type propagation. For each CV used as a
changing array key, the planner validates every visible assignment. A source CV
is accepted only when its last definition before the loop is a direct string
literal and no instruction in the closed region can write it. The write audit
covers normal assignments, concat assignment, increment/decrement, global and
static binding, direct CV results, and encoded foreach key/value destinations.
All selected key and source CVs are still guarded as strings at activation.
An ambiguous source, temporary expression, mutable source, conflicting scalar
role, call, or alias therefore keeps the ordinary interpreter path.

The typed program represents an accepted assignment as `AssignStringSlot`.
The retained string state redirects the destination pointer to the already-live
source frame value, marks only the destination dirty, and materializes the
observable CV on completion or a side exit. It performs no per-iteration
`Rc<String>` clone, drop, or frame write. Source identities also contribute to
the existing allocation-free adaptive fetch-cache width, so two and three
source CVs use two and three active entries respectively. Numeric strings still
pass through canonical PHP array-key normalization on the first cache miss.

Fifty-one rounds rotated Phase 2t, the saved Phase 2s binary, and PHP process
order:

| Workload | Phase 2s | Phase 2t | PHP 8.4.12, no CLI opcache | Phase 2t / Phase 2s |
|---|---:|---:|---:|---:|
| Two immutable string source CVs, 1M reads | 0.034719 s | 0.010903 s | 0.011729 s | 0.314x |
| Three immutable string source CVs, 1M reads | 0.037828 s | 0.013634 s | 0.013704 s | 0.360x |

The permanent two-source recurrence is about 68.6% faster than Phase 2s and
7.0% faster than PHP without JIT. The three-source control improves by 64.0%
and is effectively tied with PHP. Runtime statistics for both workloads report
one quick entry, one completion, 999,967 quick iterations, and zero guard
failures or deoptimizations.

The 31-round acceptance ratios against Phase 2s were `0.932x` for the existing
changing-literal string workload, `1.006x` for an invariant runtime string key,
`1.007x` for a literal string key, `0.903x` for the equivalent changing integer
source-CV program, and `1.005x` for the sparse hash filter. Longer twenty-pass
controls measured `0.990x` for hash foreach, `1.000x` for packed foreach, and
`0.991x` for materialized integer-hash reads.

Planner coverage proves string-source propagation and separately locks the
existing long selection for integer source CVs. End-to-end coverage verifies
ordinary and canonical numeric string sources plus final key materialization.
The quick-loop end-to-end suite now has 62 tests. The complete suite passes with
all features and with `--no-default-features`; warning-free checks pass for both
configurations. Parameters, values loaded from arrays or calls, concatenated
strings, and temporary-producing expressions remain deliberately outside this
proof until the typed IR can represent and guard their producers directly.

## Phase 3a result: first region between call events

The representative order/service corpus demonstrated the limitation of the
closed-loop selector. Its outer loop contains scalar preparation, DTO
construction, a heap-returning service call, associative result extraction,
and aggregation. Requiring one typed plan to accept the entire backedge meant
that no quick region could run even though the straight-line result-consumer
after the service call was already expressible by `FetchArrayLong` and
`AddAssign`.

The compiler now considers a bounded straight region beginning at an array
read inside one basic block. Calls, returns, control-flow edges, mutation, and
observable side effects terminate the candidate. Detection uses the existing
`QuickLongOp` graph and target resolver. A read-before-write pass derives the
true external Long inputs, excluding temporaries produced earlier in the same
region; this is required for repeated activation in fresh function frames.
`FetchDimR` carries the discardable block-plan index in its otherwise-unused
extended field, so ordinary array reads pay no global per-opcode region lookup.

The first implementation deliberately ran the selected seven-operation graph
through the generic typed executor. It was semantically exact—499,968
completions, zero guard failures, and zero deoptimizations—but changed the
corpus from approximately 0.310 s to 0.343 s. Region entry setup and typed-op
dispatch cost more than the ten baseline instructions removed. The result was
not retained as a performance regression.

Instead, the general graph is inspected once during compilation for a dense
array-result accumulation shape: one to four adjacent
`FetchArrayLong`/`AddAssign` pairs and an optional trailing fetch over the same
immutable array. No source variable, class, method, key name, or key value is
part of selection. If no dense shape exists, the compiler keeps baseline rather
than installing the slower generic short region. Successful operations commit
in PHP order; a missing or non-Long fetch resumes that `FetchDimR`, and overflow
materializes the completed fetch before resuming its baseline addition.

The dense application region measures approximately 0.299 s RPHP versus
0.079 s PHP on 500,000 order quotes, about 3.78x. It is roughly 3.7 percent
faster than the cached-property baseline and removes 4.5 million baseline
fetch/add/assignment dispatches. Fresh-frame activation and a type change in
the middle of the region have dedicated end-to-end coverage. The complete
release suite passes, and all 31 no-PGO comparison workloads remain strict RPHP
wins.

## Phase 3b result: validated string-key positions at read sites

Fresh sampling after Phase 3a showed that the dense region had removed most of
its opcode dispatch, but each associative result read still hashed the same
literal key. The same repeated hashing also exists in ordinary baseline
`FetchDimR`, so solving it only inside the dense kernel would have made the
optimization workload-specific.

Every string-key `FetchDimR` site now retains the last ordered-entry position
in its existing inline-cache slot. A hit is accepted only after loading that
position from the current array and comparing the actual entry key with the
current lookup key. A different array, a COW detach, insertion, removal,
reordering, or a changing dynamic key can therefore only turn the hint into a
miss. The miss path performs canonical PHP key normalization and the ordinary
string-index lookup, then refreshes the position. Numeric strings still take
the integer-key path before this cache is considered.

The baseline interpreter and the straight application region share this
single helper and validation contract. No array pointer or mutation version is
cached, and no array representation is changed. Unit coverage moves a cached
entry by removing an earlier key; end-to-end coverage alternates arrays whose
identical keys occupy different positions at one bytecode site.

Two final independent best-of-five runs of the representative order pipeline
measure 0.281--0.283 s RPHP versus 0.078--0.079 s PHP, about 3.60x. This is a
further 5--6 percent reduction from the Phase 3a result and about 46 percent
from the original 0.521 s RPHP baseline. The complete release suite passes. In
the latest no-PGO matrix RPHP has 30 strict wins and one timing-level tie; no
related regression was found.

## Phase 3c result: allocation-free small ordered hashes

The post-Phase-3b sample separated lookup cost from lifecycle cost. Constructing
each three-key result still allocated an entries vector and a string hash index,
hashed all three insertions, and freed both allocations shortly afterward.
Together construction, insertion, and destruction accounted for roughly
17--18 percent of samples even though repeated reads had become positional.

`PhpArray` now has a third dynamically selected representation. `SmallHash`
stores up to three explicit integer or string entries directly inside the
existing array allocation. The dense prefix is terminated by its first empty
slot, avoiding separate length metadata. Three optional entries occupy 96
bytes and the complete storage enum remains 104 bytes, so `PhpArray` remains
112 bytes: packed and general hash arrays do not grow to fund the optimization.

Lookup and mutation preserve normal PHP behavior through short linear scans or
validated ordered positions. Overwriting an existing key remains inline. A
fourth new key promotes once to the existing ordered entries plus split string
and integer indexes, moving keys and values without cloning them. The general
representation has no size limit. Remove, pop, shift, iteration, cloning, COW,
integer renumbering, and guarded ordered-value layouts support both explicit
representations under the same public API.

Two independent best-of-five corpus runs measure 0.2404--0.2406 s RPHP versus
0.0785--0.0789 s PHP, about 3.05--3.06x. This is approximately 14.5 percent
below Phase 3b and 54 percent below the original 0.521 s RPHP baseline. The
complete release suite passes, and the no-PGO matrix remains 30 strict RPHP
wins plus one timing-level tie. Fresh sampling reduces `with_hash_capacity`
from 88 samples to 3 and leaves the complete small-array lifecycle near 6
percent. Object creation and destruction are now the largest named runtime
lifecycle cost at roughly 20 percent combined.

## Phase 3d result: declared scalar types as an execution contract

Before continuing into object layout, an isolated type-declaration matrix
measured the cost of function and method hints. With identical integer bodies,
RPHP initially took 0.1636 s for typed parameters, 0.1705 s for typed parameters
plus return, 0.2742 s for the strict variant, and 0.2154 s for the typed method.
The corresponding untyped function and method took 0.1219 s and 0.1157 s. This
showed that declarations were selecting slower call machinery even when every
runtime value already had the declared representation.

Scalar hints now use the compact call and return boundary. A separate
`FastTypedScalar` ABI is selected only for fixed-arity, non-reference functions
whose public parameters are all `int` and whose return is absent, `mixed`, or
`int`. It deliberately adds no metadata or type checks to the original
baseline `FastScalar` call/return path. Compiler-proven Long leaf,
composed-call, and monomorphic method plans accept both scalar ABIs: their
existing Long input guards and
checked arithmetic satisfy the declared contract without allocating a frame.
Other `int`, `float`, `string`, and `bool` signatures use compact inline checks
and retain the canonical checker for failed or unsupported cases.

The type contract is enforced at every tier. Baseline calls validate already
bound arguments; nested hot and macro calls repeat the guard at their call
boundary; typed returns are checked even when the caller discards the result.
A failed guard side-exits at the untouched call or return so the canonical path
produces the normal `TypeError`. Dedicated warmup tests cover invalid direct
arguments, nested hot calls, methods, typed returns, and return-only hints.

Final best-of-five no-PGO measurements are:

| Variant | RPHP | PHP 8.4.12, no CLI opcache | RPHP / PHP |
|---|---:|---:|---:|
| Function, untyped | 0.1176 s | 0.0419 s | 2.81x |
| Function, `int` parameters | 0.1318 s | 0.0465 s | 2.83x |
| Function, `int` return | 0.1280 s | 0.0445 s | 2.87x |
| Function, parameters + return | 0.1319 s | 0.0492 s | 2.68x |
| Function, strict parameters + return | 0.1323 s | 0.0496 s | 2.67x |
| Method, untyped | 0.1129 s | 0.0535 s | 2.11x |
| Method, parameters + return | 0.1256 s | 0.0614 s | 2.05x |
| Scalar-plan function, untyped | 0.0446 s | 0.0756 s | 0.59x |
| Scalar-plan function, typed | 0.0448 s | 0.0838 s | 0.53x |
| Scalar-plan method, untyped | 0.0435 s | 0.0451 s | 0.96x |
| Scalar-plan method, typed | 0.0435 s | 0.0517 s | 0.84x |

The generic strict typed function is about 52 percent faster than its initial
RPHP measurement. The typed generic function and method retain roughly 11--12
percent overhead versus their untyped RPHP controls, which is now the compact
boundary check rather than a different frame protocol. When the body is a
compiler-proven scalar plan, typed and untyped RPHP are effectively equal;
the typed function is 47 percent faster than PHP and the typed method 16
percent faster.

Two independent corpus runs measure 0.2344--0.2369 s RPHP versus
0.0781--0.0787 s PHP, about 2.98--3.03x. This is slightly below the preceding
0.2404--0.2406 s checkpoint, so the typed ABI carries no measured tax for the
untyped application. The complete release suite passes with default features
and with `--no-default-features`; the type-hint suite has 72 tests. The 31-case
no-PGO matrix remains 30 RPHP wins with one marginal packed-array build loss at
1.05x. Declared object layout and allocation therefore remain the next
corpus-driven no-JIT target.

## Phase 3e result: exact scalar facts across function boundaries

The typed ABI removed the separate slow frame protocol, but a declaration was
still forgotten immediately after its boundary check. A caller receiving an
`int` or `string` result therefore repeated Value-tag and coercion probes in
every following operation. The hot executor repeated the same probes even
after canonical bytecode had selected a typed opcode.

The compiler now propagates exact scalar representations through already
planned function and method bodies. `int`, `string`, and `bool` parameter
declarations seed immutable, non-reference CVs. Statically resolved named
function returns seed `DoFcall` results; literals, exact operation results, and
safe straight-line assignments extend the fact. Branch-assigned parameters,
defaults, references, `global`, `static`, and foreach boundaries conservatively
discard it. Weak `float` declarations remain unproven because they do not
currently guarantee one representation.

The fact occupies three previously unused high padding bits in the existing
16-byte instruction. Proven operations lower to `LongLong` arithmetic and
modulo/xor instructions, `StringString` concat, exact-string `strlen`, and exact
scalar echo. Both baseline and hot executors consume those opcodes without
repeating tag guards. Checked arithmetic preserves PHP overflow to `double`,
and modulo preserves `PHP_INT_MIN % -1 == 0`. A proven return operand skips only
the redundant scalar return check; unknown or overflow-capable results retain
canonical validation and `TypeError` construction.

Argument facts also flow into statically resolved calls. A `DoFcall` is marked
only when every supplied argument is already proven to satisfy its declared
type. Arity and hole checks remain. Unknown, named, reference, dynamic, method,
or aliased inputs retain the ordinary runtime type guard. This establishes the
intended contract for a later JIT: validate once, carry the proof, and
deoptimize before any assumption can become invalid.

Best-of-five no-PGO measurements for the new cross-call workloads are:

| Variant | RPHP | PHP 8.4.12, no CLI opcache | RPHP / PHP |
|---|---:|---:|---:|
| Integer return chain, untyped | 0.2341 s | 0.0970 s | 2.41x |
| Integer return chain, typed | 0.2506 s | 0.1054 s | 2.38x |
| String return chain, untyped | 0.0923 s | 0.0251 s | 3.67x |
| String return chain, typed | 0.0922 s | 0.0258 s | 3.58x |
| Integer return fan-out, untyped | 0.1411 s | 0.0571 s | 2.47x |
| Integer return fan-out, typed | 0.1513 s | 0.0615 s | 2.46x |
| String return fan-out, untyped | 0.0780 s | 0.0265 s | 2.94x |
| String return fan-out, typed | 0.0866 s | 0.0289 s | 3.00x |

The integer typed-chain RPHP time fell from the pre-pass 0.318 s to 0.251 s,
about 21 percent; the typed string chain fell from 0.105 s to 0.092 s, about 12
percent. Untyped code also benefits when operation semantics alone prove the
next result, which is intentional. Typed code is not universally faster than
untyped code yet: parameter/return boundaries and heap string returns still
carry real costs. The important result is that declarations now remove work
downstream instead of only adding validation.

Two independent order/service corpus runs measure 0.2207--0.2273 s RPHP versus
0.0788--0.0806 s PHP, about 2.80--2.82x and below the preceding
0.2344--0.2369 s checkpoint. The 31-case no-PGO matrix remains 30 RPHP wins
with one marginal array-build loss at 1.03x. Both complete release
configurations pass, and the focused type suite now contains 80 tests. The
next typed-boundary extension is guarded monomorphic method-return propagation;
string heap return/ownership remains a separate hot-executor problem.

## Cold compatibility additions and hot-layout admission

The 2026-08-10 `fgetcsv()` checkpoint exposed a separate optimization risk:
adding unreachable cold source can perturb LLVM's generation or placement of
an unrelated large hot function under release LTO. A five-second ARM64 sample
still attributed approximately 97 percent of the ledger workload to
`run_quick_long_ops_loop`; CSV code was absent from the runtime stack. The
regression was therefore treated as a code-layout failure, not optimized by
changing CSV semantics or adding padding.

The admitted build preserves the established textual dispatcher include on
x86-64 Linux. ARM64 Apple builds isolate that dispatcher in a real child module
and place the CSV parser and handler in a dedicated cold text section. This is
a narrow target-specific translation-unit boundary, documented next to the
source attributes and covered by the same cross-host admission gates as an
ordinary runtime optimization. The CSV implementation itself uses only `std`;
neither Cargo manifest changes nor a third-party parser are part of the fix.

The following `fputcsv()` slice deliberately does not repeat the target-layout
workaround. Moving the writer into its own normal `streams::csv_write` module
made the existing reader and stream files byte-identical to their accepted
baseline, but enabling the additional cold code by default still moved the
ARM64 ledger workload by roughly 4--5 percent. Whole-crate codegen-unit and LTO
experiments stabilized individual symbols only by causing substantially larger
regressions in other runtime workloads. Those profile changes and custom
section experiments were rejected. The dependency-free writer is therefore
kept behind the explicit `csv-write` feature while the default build compiles
it out and retains its admitted hot layout. This is the intended gate behavior:
compatibility work may land independently, but it does not become production
default merely because its own handler is cold.

The same rule applies to the exact CSV argument-error surface. A first
default-linked implementation added `ValueError`, open-resource validation and
the PHP 8.5 error messages without executing any of those paths in the runtime
corpora. Nevertheless, the fresh ARM64 gate moved scalar by +3.867 percent and
ledger by +5.094 percent. Cold annotations and a separately outlined class
registrar did not restore the accepted dispatcher shape. The admitted design
therefore compiles the checked reader and `ValueError` registration only with
`csv-errors`; `csv-write` implies that feature. With both features disabled,
the established reader handler, `.text` size and monitored hot-symbol
addresses are identical to the prior checkpoint. No linker/profile workaround
is retained.

Final-alias resource ownership exposed the same boundary from a genuinely
owned runtime type. A standard-library `Rc` handle can preserve the 16-byte
`Value`, correctly close on the last alias and participate in every existing
frame cleanup path. The first default-linked build nevertheless regressed the
ledger control by +2.102 percent in a balanced 20-pair ARM64 gate while the
other four controls remained within +1 percent. This is a codegen admission
failure, not evidence that reference counting is executed by the ledger: that
workload creates no stream resource.

The admitted implementation is therefore behind `resource-lifetime`.
Compiling without it restores the exact `f5d4e68` text size and monitored hot
symbol addresses; compiling with it supplies final-alias close, explicit-close
idempotence and nested-destructor safety. The handle calls the registry through
an indirect callback so `Value` ownership does not directly depend on the
stdlib registry module. No external library or lockfile change is involved.
The independent CPU-pinned x86-64 gate against the same exact checkpoint
passes at +0.106%/+0.378%/-0.274%/-0.549%/-0.191% for scalar, packed array,
String, order and ledger.

`stream_get_contents()` follows the same pay-for-use boundary. The
`stream-contents` feature compiles in the handler, a fixed-chunk backend reader
and the shared checked-argument helpers; without it, those modules and the
`ValueError` registrar remain absent. Moving the duplicated CSV stream/long
validation into `streams::checked_args` did not create a default dependency:
the helper module is compiled only for `csv-errors` or `stream-contents`.

The default ARM64 image retains the exact 2,818,048-byte `f5d4e68` text size
and monitored hot addresses. X86-64 text/data/bss sizes and the same addresses
also match its exact baseline. A pinned 20-pair x86 gate measured
-0.020%/+0.049%/-0.241%/-0.267%/+1.933% for scalar, packed array, String,
order and ledger. The lone ledger failure contained large outliers on both
sides despite identical static layout; its required isolated 20-pair rerun
passed at +0.571%. No result-size preallocation, external buffering library or
lockfile change was introduced: the backend uses an 8 KiB stack chunk and
fallible incremental `Vec` growth.

The follow-up `stream-copy` surface is also fully absent from default builds.
Unlike `stream_get_contents()`, it allocates no result vector: one 8 KiB stack
array is alternately read from the source and written to the destination. The
operation deliberately releases the request-registry borrow between those
steps, avoiding any new dual-payload borrowing primitive and preserving
same-resource cursor behavior. Generalizing the feature-only checked stream
helper by argument index and name leaves all ordinary registration and handler
code compiled out.

ARM64 again retains the exact 2,818,048-byte `f5d4e68` text size and monitored
addresses. X86-64 text/data/bss and those addresses are exact as well. A fresh
pinned 20-pair x86 gate passes without a rerun at
-0.797%/-0.138%/-0.795%/+0.309%/-0.332% for scalar, packed array, String,
order and ledger. The feature adds no crate, heap-sized copy request or
lockfile change.
