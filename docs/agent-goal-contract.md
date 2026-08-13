# Specialized agent goal contract

This contract lets a user give either RPHP specialist a concise outcome while
keeping implementation, evidence and handoff consistent.

## Minimal goal input

The user only needs to identify the agent and desired outcome:

```text
Compatibility Agent
Goal: <observable PHP behavior or compatibility gate to achieve>
```

```text
Execution & Performance Agent
Goal: <measurable runtime, region, JIT or simplification outcome to achieve>
```

Optional constraints may name a deadline, forbidden scope, target workload,
required fixture or acceptable regression budget. Missing implementation detail
is not a blocker: the assigned agent derives it from its strategy and roadmap.

## Agent normalization

Before editing, the agent turns the outcome into a checkpoint containing:

- **Outcome:** externally observable result, not an implementation preference.
- **Baseline:** exact commit, clean/dirty state and current reproducible result.
- **Scope:** the smallest general semantic or execution slice that can achieve
  the outcome without a fixture- or benchmark-specific special case.
- **Proof:** tests, reference output, profiles or measurements required before
  implementation begins.
- **Acceptance gates:** objective pass/fail conditions.
- **Ownership:** likely files and any collision with the other active agent.
- **Stop conditions:** evidence that rejects the approach or requires the
  integrating agent to resolve scope.

The agent reports this compact normalization as its first checkpoint update and
then proceeds autonomously with safe, in-scope local work.

## Goal lifecycle

```text
proposed -> baselined -> active -> verified -> handed off -> accepted
                    \-> rejected
                    \-> blocked
```

- **Rejected** means evidence disproved the implementation hypothesis; it is a
  useful result and must include the measurements or semantic counterexample.
- **Blocked** means progress requires a product choice, new authority, missing
  external state or ownership of files held by the other workstream. Difficulty
  alone is not a blocker.
- The agent keeps one implementation goal active. It may perform read-only
  discovery for the next candidate but does not start a second code change.

## Shared operating boundaries

- Work in an isolated worktree on `codex/compat-*` or `codex/perf-*`.
- Do not push to `main`, rewrite another agent's branch or combine unrelated
  cleanup with the goal.
- Treat the repository, diffs, commits, fixtures and logs as public. Private
  connectivity is supplied only through environment variables.
- Use task-scoped paths on shared hosts. Performance measurements require an
  exclusive benchmark window; compatibility activity must not overlap it.
- Do not weaken tests, unsafe-policy limits, diagnostics or compatibility gates
  to make a checkpoint pass.
- Preserve user changes and stop for integration when file ownership overlaps.

## Required handoff

A handoff is a review packet, not merely a commit hash. It contains:

1. normalized goal and exact baseline;
2. root cause or measured hypothesis;
3. implementation summary and important invariants;
4. exact validation commands and outcomes;
5. compatibility deltas or A/B distributions, as applicable;
6. rejected alternatives and remaining limitations;
7. changed-file ownership notes and likely merge conflicts;
8. branch and commit identifiers; and
9. a statement that public-data and staged-diff hygiene checks passed.

The integrating agent independently reviews the diff, resolves merge order,
runs the joint gate, updates the coordination roadmap and decides whether the
checkpoint is accepted and pushed.
