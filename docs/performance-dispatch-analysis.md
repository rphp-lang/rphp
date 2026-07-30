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
