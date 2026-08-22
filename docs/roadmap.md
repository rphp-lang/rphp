# RPHP engineering roadmap

Status: active coordination map, 2026-08-22

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
| PHP compatibility | [Compatibility roadmap](roadmap-compatibility.md) | [Compatibility Agent](agent-strategy-compatibility.md) | The pinned PHP 8.5.6 AMD64 checkpoint is now 4,074/5,599 with zero timeouts and zero crashes. `get_defined_functions()` exposes RPHP's real internal/user inventory and PHP 8.5 optional-argument diagnostics without pretending that unavailable PHP extensions exist. The exact delta is +1/-0 over `73147de1`, solely `get_defined_functions_basic.phpt`, with two byte-identical full manifests, every prior pass preserved and no other category movement. All five feature configurations, unsafe ratchet and Composer/Symfony S0-S3 gates pass; paired release changes are +0.082% for 100 empty requests and -2.618% for two million existing `is_float()` calls, below the +5% gate. Exact extension inventory, disabled-function configuration, the independently exposed `chunk_split()` boundary and broader PHP 8.5 clusters remain explicit work. |
| Execution and performance | [Execution and performance roadmap](roadmap-execution-performance.md) | [Execution & Performance Agent](agent-strategy-execution-performance.md) | Bisect the lost file-entry dynamic String-key array admission, restore the common typed/ARM64/x86-64 contract only when semantically valid, then rerun the full dual-host scorecard. |

Only an accepted checkpoint moves a frontier. A partial implementation,
diagnostic observation or favorable but unverified benchmark remains work in
progress.

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
