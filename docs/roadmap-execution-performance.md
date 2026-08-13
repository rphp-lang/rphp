# RPHP execution and performance roadmap

Status: active project direction

See the [project coordination map](roadmap.md), the
[Execution & Performance Agent strategy](agent-strategy-execution-performance.md),
and the shared [goal contract](agent-goal-contract.md) for assignment and
integration rules.

## Mission

Make supported PHP execute through one increasingly complete, measurable
execution system: canonical baseline bytecode, a unified typed IR, guarded
regions, and equivalent ARM64 and x86-64 native lowering. Performance work must
reduce general runtime costs or increase execution-tier coverage; it must not
change PHP semantics, recognize benchmark names, or accumulate another family
of unrelated fast paths.

The baseline VM is the semantic source of truth. Removing every optimization,
cache, typed plan, and native region must leave a correct program.

Compatibility breadth is planned in the compatibility roadmap. This roadmap
owns how already-supported behavior is represented and executed efficiently.

## Permanent correctness contract

Every optimized operation and region must satisfy all of these rules:

1. Its admission proof is independent of source, class, function, and benchmark
   names.
2. Runtime guards execute before the protected operation mutates observable
   state.
3. Every exit has an exact baseline resume position and a complete live-value
   map.
4. Completed effects stay committed; fallback never repeats a call, write,
   warning, exception, iterator advance, or other side effect.
5. Overflow, type changes, references, COW, aliases, magic behavior, exceptions,
   dispatch changes, and interrupts exit before behavior can diverge.
6. The typed interpreter proves an IR operation before either JIT backend may
   lower it.
7. ARM64 and x86-64 consume the same semantic IR and deoptimization contract.

## Program scorecard

Maintain one current scorecard rather than a chronological implementation log.
For the active baseline and candidate it records:

- exact commits, build profiles, feature sets, and clean/dirty state;
- microbenchmark, representative-corpus, and independent-holdout results;
- identical-output and differential-PHP results;
- execution-weighted baseline, typed-region, ARM64 JIT, and x86-64 JIT coverage;
- region entries, completions, guard failures, side exits, and deoptimizations by
  reason;
- allocations, frame traffic, value clones/drops, COW detachments, and peak
  memory for workloads affected by the goal;
- median and spread for interleaved A/B runs on both architectures;
- remaining top sampled costs and the next admitted goal.

The coverage matrix uses one row per general operation or program shape and
these columns:

| Capability | Baseline tests | Typed IR | Typed executor | ARM64 | x86-64 | Exact exit tests | Corpus evidence |
|---|---:|---:|---:|---:|---:|---:|---:|

An empty cell is a visible backlog item, not an invitation to hide the shape
inside a workload-specific recognizer.

## Measurement admission: M0

Performance development starts by establishing a reproducible measurement
system.

- Keep three workload layers: isolated primitives, representative application
  flows, and independent holdouts not used to design the change.
- Separate compile, startup, parse, warm-up, and execution time when the claim
  depends on that distinction.
- Validate expected output before timing, randomize or interleave run order, and
  report every retained run with a predeclared outlier policy.
- Profile the exact baseline configuration before selecting an implementation.
- Keep architecture, CPU, OS, Rust/PHP versions, flags, feature sets, affinity,
  power mode, repetitions, median, and spread with the result.
- Add counters only when they answer the active question and prove that disabled
  instrumentation has no ordinary-path cost.

Gate M0 is complete when the corpus and holdouts run reproducibly on ARM64 and
x86-64, output validation is automatic, established noise bands are recorded,
and a fresh operator can reproduce the comparison from documented commands.

## Baseline runtime primitives: M1

Remove structural costs that native code cannot repair:

- call and return ABI, frame creation, slot initialization, and cleanup;
- `Value` ownership, clone/drop behavior, references, and COW boundaries;
- packed/hash arrays, strings, declared and dynamic objects, and property access;
- allocation, reuse, lifecycle, inline caches, and dispatch identity;
- cold diagnostics and metadata that disturb hot code or data layout.

Each goal begins with a profile and a counted cost. A representation change must
include size/layout assertions, allocation evidence, lifecycle tests, and corpus
memory measurements. A call or frame change must cover exceptions, references,
variadics, named arguments, generators, and re-entry as applicable.

Gate M1 requires the top sampled runtime costs to be either reduced, assigned to
a later measured milestone, or proven irreducible for current semantics. No
dominant representation, allocation, or ABI problem may be deferred merely
because a JIT exists.

## Unified typed execution IR: M2

Converge scalar, call, method, property, collection, control-flow, and existing
quick plans on a small typed IR with:

- explicit inputs, outputs, effects, guards, and ownership;
- stable function, class, method, property, and cache identities;
- checked arithmetic and explicit PHP-visible failure edges;
- exact bytecode resume positions and live-value publication;
- plan construction and validation outside hot execution;
- one allocation-free typed interpreter used before native lowering.

New PHP shapes extend the common vocabulary. A specialized executor is allowed
only when profiling proves that the generic mechanics are too expensive; it
must still consume the same IR and exit contract.

Gate M2 requires one typed interpreter and one deoptimization protocol for the
representative call, loop, branch, property, and collection regions. Parallel
legacy plan types are removed once their coverage and performance are matched.

## Region formation and coverage: M3

Build regions from control flow, type facts, effects, liveness, and measured
hotness rather than source syntax names or benchmark constants.

- Support straight regions, loops, branches, and bounded call composition.
- Form partial regions around unsupported operations when exact publication and
  resume are possible.
- Rank missing shapes by execution-weighted corpus time, not raw opcode count.
- Record why each candidate was admitted, rejected, or deoptimized.
- Keep compilation and cache growth bounded; cold code remains baseline-only.
- Test guard failure at every operation boundary, including failures after
  earlier effects have committed.

Gate M3 requires typed regions to cover at least 80% of execution time that the
current corpus classifies as eligible, with no unowned general missing shape
accounting for more than 5% of sampled hot time. Completion and exit counters
must reconcile, and adversarial side-exit tests must show zero repeated effects.
Eligibility and exclusions are published with the percentage.

## Dual-architecture JIT coverage: M4

Lower the proven typed IR to ARM64 and x86-64 without creating backend-specific
PHP semantics.

- Share admission, range analysis, liveness, register-independent lowering,
  guards, exit metadata, and code-cache policy.
- Keep encoding, calling-convention details, register allocation, executable
  memory, and architecture intrinsics behind narrow backend boundaries.
- Add operations vertically: IR semantics, typed execution, exact exits, ARM64,
  x86-64, differential tests, corpus coverage, and A/B evidence in one goal.
- Measure compile cost, code size, cache growth, native-entry overhead, and
  deoptimization as well as steady-state execution.
- Reject native regions whose dispatch savings do not repay admission and code
  generation on representative programs.

Gate M4 requires backend parity for every admitted IR operation, identical
validated output, exact fallback on both architectures, and native coverage of
at least 90% of executions for regions declared JIT-eligible in the scorecard.
Each architecture must show a statistically credible benefit or an explicit
evidence-backed holdout; neither backend may silently lag as an untested port.

## Representation, ABI, and allocation convergence: M5

Continue structural work across M1-M4 when profiles expose a common boundary:

- compact values and immutable metadata sharing;
- frame-free or borrowed arguments only under compiler and runtime proofs;
- region-local virtual values with exact materialization at exits;
- allocation-free steady-state caches and reusable bounded storage;
- layouts that are measured for footprint, locality, and clone/drop cost on
  both architectures.

Gate M5 is not a one-time rewrite. Each accepted slice must reduce a named
counter or sampled cost, keep peak memory within the stated budget, introduce
no lifetime unsafety, and retain exact canonical materialization.

## Remove superseded paths and simplify: M6

Coverage is incomplete while old special paths remain the only implementation
of common shapes. After a typed/JIT vertical slice is accepted:

1. identify recognizers, plan types, caches, and executors it supersedes;
2. migrate all callers to the common contract;
3. compare code size, compile time, source ownership, and runtime A/B;
4. delete the old path only after feature and architecture matrices pass.

Source layout should expose stable IR, planning, runtime execution, and backend
ownership separately. Prefer plain data and free functions; do not add trait
objects, allocations, reference counting, or unpredictable branches to a hot
loop for aesthetic modularity. Codegen-neutral physical splits may precede real
module boundaries.

Gate M6 requires a net reduction in duplicated mechanisms or a clearly narrower
responsibility boundary. Production refactors are rejected if either host
regresses outside the established noise band, even when generated code size or
symbol addresses appear unchanged.

## Bounded coroutine continuation: M7

Do not expand coroutine work while baseline primitives, typed-region coverage,
or the dual-backend JIT have higher-priority measured gaps. Existing coroutine
substrate remains opt-in until its ordinary-runtime cost is proven absent.

Later work must preserve pay-for-use behavior: no ordinary-call allocation or
poll, O(1) context exchange independent of frame depth and live slots, lazy
pooled storage, exact exception/finally ownership, and native-region exits at
known suspension points. Cooperative single-threaded readiness remains the
bounded target; M:N scheduling is a separate future decision.

Gate M7 requires the established one-percent non-coroutine regression ceiling,
zero new ordinary-call allocations, permanent depth/slot scaling tests, and
separate creation, hand-off, channel, timer, and I/O measurements. If these
conditions conflict with normal PHP performance, coroutine work stays opt-in.

## Global acceptance gates

A milestone may advance only when:

- formatting, unsafe-policy enforcement, all-target checks, and the relevant
  default/no-default/all-feature test matrices pass;
- supported behavior matches reference PHP and the baseline VM on affected
  differential tests;
- target, corpus, and holdout outputs are identical before timing;
- the target improvement survives an independent rerun and established corpus
  medians do not regress by more than one percent without an investigated,
  reproducible explanation;
- ARM64 and x86-64 evidence is current for codegen, layout, ABI, or hot-runtime
  changes;
- benchmark artifacts and private-host details satisfy `AGENTS.md` and
  `docs/benchmarking.md`.

## Current ordering

1. Freeze the current scorecard and regenerate the typed/JIT coverage matrix.
2. Profile the representative corpus and choose the largest general uncovered
   region or baseline structural cost.
3. Deliver one complete baseline → typed IR → exact exit → ARM64/x86-64 vertical
   slice where applicable.
4. Remove the superseded special mechanism and remeasure after convergence.
5. Repeat by execution-weighted impact; keep new coroutine expansion deferred.

Completion checkpoint (2026-08-13): the next profiled M1/M5 structural slice
is accepted. Closure-valued `Value` copies now share one immutable payload and
captured environment instead of allocating and cloning both on every copy.
Exact output, lifecycle and reference-capture tests, allocation telemetry, the
full feature matrix and dual-architecture target/corpus/holdout gates pass
against the integrated compatibility baseline. The 250,000-copy target improves
by 22.69% on ARM64 and 8.84% on x86-64; retained-copy peak RSS falls by 72.23%
and 74.29%. Detailed method, distributions and semantic boundaries are in
[`performance-closure-ownership.md`](performance-closure-ownership.md). The
next goal returns to scorecard/profile selection rather than extending shared
ownership to mutable representations without new evidence.

Update this section and the scorecard when priorities change. Put detailed
benchmark records in dedicated reports, not in this roadmap.
