# RPHP engineering roadmap

Status: active coordination map, 2026-09-28

This document coordinates two independent engineering workstreams. It stays
short and current; detailed plans live in the workstream roadmaps, while the
older combined documents remain evidence-rich engineering logs.

## Project contract

RPHP grows along two axes:

1. **PHP compatibility** expands the behavior that RPHP implements correctly.
2. **Execution and performance** makes proven behavior simpler and faster
   through runtime design, typed regions and native lowering.

The baseline bytecode VM is the semantic source of truth. An optimization may
guard and deoptimize around supported behavior, but it may not redefine that
behavior. A compatibility feature is not complete merely because a framework
fixture passes, and a performance change is not complete merely because one
microbenchmark improves.

## Active workstreams

| Workstream | Detailed roadmap | Agent strategy | Current frontier |
| --- | --- | --- | --- |
| PHP compatibility | [Compatibility roadmap](roadmap-compatibility.md) | [Compatibility Agent](agent-strategy-compatibility.md) | The Rector 2.5.9 bootstrap now runs an unmodified real parallel transformation and an idempotent second pass. Its vendor audit finds 339/351 observed globals present with zero call-shape mismatches; twelve conditional observations remain explicit nonclaims. Five Cargo configurations, all-targets, Composer/Symfony S0-S3 and unsafe gates pass; performance is deferred by user direction ahead of the Rust upgrade. |
| Execution and performance | [Execution and performance roadmap](roadmap-execution-performance.md) | [Execution & Performance Agent](agent-strategy-execution-performance.md) | The isolated recovered PHPStan foundation passes 36,280 matrix executions, 556 ASan and 70 default/canonical PHP comparisons. Same-input five-file analysis is 74.697G / 6.770s with GC disabled and 76.918G / 6.967s with GC enabled; PHP remains about 1s. Two confirmed micro timing losses have an explicit bounded integration tradeoff. Simultaneous parity and dual-host coverage remain open; main is unchanged. |

The [recovered foundation](performance-phpstan-recovered-foundation.md) corrects
mixed process/analysis count scopes and missing performance history, integrates
ordinary call ownership/lifetime repairs and records fresh exact-source gates.
It remains a branch checkpoint; no source has been pushed directly to main.
The next design is the [general PHP core cost and semantic model](performance-php-core-mathematical-design.md),
covering arbitrary PHP programs and all semantic families. PHPStan supplies one
acceptance witness; its three regions do not define the engine's scope.
Earlier large-input numbers below retain their different workload identity.

Only an accepted checkpoint moves a frontier. A partial implementation,
diagnostic observation or favorable but unverified benchmark remains work in
progress.

The [heap integration checkpoint](performance-php-heap-integration.md) records
the accepted assembly-removal tradeoff, fresh same-profile comparisons and the
failed 120-second larger-project gate. Subprocess tracing rules out a stuck
child wait in that serial failure. Native reproduction and Callgrind establish
quadratic graph marking during nested PHP destructor release. The subsequent
[bounded runtime repair](performance-deep-release.md) records the fixes, full
joint verification and the integrating task's measured exception. It also
corrects PHPStan's self-restart benchmark protocol; the remaining application
gap is explicitly open.

## Coordination rules

- Each specialized agent has at most one active implementation goal.
- Each goal uses the shared [goal contract](agent-goal-contract.md), a dedicated
  worktree and a short-lived `codex/compat-*` or `codex/perf-*` branch.
- The integrating agent assigns temporary ownership when goals could touch the
  same compiler, VM, value, test-runner or roadmap files.
- Compatibility normally integrates first when it establishes new semantics.
  The performance branch then rebases and repeats every affected A/B result.
- A performance change may land first only when it is semantics-neutral,
  disjoint from the active compatibility slice and leaves the joint gate green.
- Specialized agents do not push directly to `main`. They hand off a verified
  branch checkpoint; the integrating agent reviews, merges and pushes.
- `docs/roadmap.md` is maintained by the integrating agent. Each specialized
  agent may update its own roadmap with durable evidence after its checkpoint
  is accepted.

## Joint integration gate

Before a checkpoint reaches `main`, the integrating agent verifies:

1. the goal's workstream-specific definition of done;
2. formatting, unsafe-policy checks and locked all-feature/all-target checks;
3. relevant unit, integration, differential and feature-matrix tests;
4. compatibility fixtures affected by a runtime or optimization change;
5. performance A/B evidence when a hot path, representation or code layout
   changed;
6. staged-diff and tracked-change scans for private or sensitive data; and
7. a clean, coherent commit whose limitations are documented without inflating
   compatibility or performance claims.

The full matrix and release benchmark lifecycle must use the cleanup hooks in
`AGENTS.md`, including the configured private benchmark host where applicable.

## Document roles

- [Compatibility status](compatibility.md) records reproducible public evidence
  and bounded compatibility claims.
- [Compatibility roadmap](roadmap-compatibility.md) orders future compatibility
  work and its exit gates.
- [Execution and performance roadmap](roadmap-execution-performance.md) orders
  runtime, typed-region, JIT and simplification work.
- [Goal contract](agent-goal-contract.md) defines how a user outcome becomes a
  reviewable agent checkpoint.
- [Benchmark methodology](benchmarking.md) defines publishable measurement
  evidence.
- [Combined performance/JIT/compatibility log](roadmap-performance-jit-compatibility.md)
  and [runtime architecture log](roadmap-runtime-architecture.md) retain prior
  decisions, rejected candidates and detailed historical measurements. They are
  source material, not the active task queues.
