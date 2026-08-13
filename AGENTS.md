# RPHP workspace rules

## Specialized workstreams

- A goal assigned to the **Compatibility Agent** must follow
  `docs/agent-strategy-compatibility.md` and
  `docs/roadmap-compatibility.md`.
- A goal assigned to the **Execution & Performance Agent** must follow
  `docs/agent-strategy-execution-performance.md` and
  `docs/roadmap-execution-performance.md`.
- Both agents use `docs/agent-goal-contract.md` for goal intake, checkpoint
  evidence and handoff. The user may supply only a role and desired outcome;
  the assigned agent is responsible for deriving the bounded checkpoint and
  its verification plan from those documents.
- Keep one active implementation goal per specialized agent. Use a dedicated
  `codex/compat-*` or `codex/perf-*` branch and an isolated worktree. Never let
  two agents edit the same checkout.
- The integrating agent owns `docs/roadmap.md`, resolves temporary ownership of
  shared compiler/runtime files, chooses merge order and runs the final joint
  gate. Specialized agents update their own roadmap only as part of an accepted
  checkpoint or when the integrating agent asks them to do so.
- User instructions and the safety, cleanup and public-repository rules in this
  file take precedence over a workstream strategy.

## Build artifact hygiene

- Treat build cleanup as a mandatory automatic lifecycle hook. Do not wait for
  a user reminder before running it, and do not ask for confirmation when the
  cleanup remains within the safe targets below.
- Run `scripts/cleanup-builds.sh` before and after a full four-configuration
  test matrix or release benchmark cycle. The script automatically cleans the
  workspace Cargo target once it exceeds the configured size limit or the
  filesystem falls below its minimum free-space reserve, and removes stale
  task-scoped candidate directories.
- On a filesystem that cannot retain every feature variant at once, run the
  same hook between matrix configurations. This is an expected automatic
  lifecycle step, not a reason to leave a full disk or skip a configuration.
- At the end of a benchmark checkpoint, run the same cleanup locally and on
  the private benchmark host named by `RPHP_BENCHMARK_HOST`, when configured,
  even when the matrix or benchmark failed. Never commit private hostnames,
  addresses, usernames, or credentials.
- Put disposable release builds in task-scoped `/tmp/rphp-candidate-*`
  directories rather than accumulating profiles in the workspace `target`.
- After a benchmark checkpoint is accepted, delete superseded candidate build
  directories locally and on the configured private benchmark host. Retain
  only the exact baseline and current candidate needed by an active comparison.
- Never delete source snapshots, exact baselines used by an active gate, or
  user files as part of automatic cleanup.

## Public repository hygiene

- Treat every tracked file, commit message, diff, test fixture, benchmark log,
  issue and push as public. Never commit private hosts or addresses, usernames,
  credentials, tokens, keys, personal filesystem paths, private source data or
  unredacted diagnostic output.
- Keep private benchmark connectivity in environment variables such as
  `RPHP_BENCHMARK_HOST`. Before every commit and push, inspect the staged diff
  and scan tracked changes for common credential and internal-network markers.
- Commit and push incrementally after each coherent, reviewed and verified
  checkpoint. Do not push knowingly failing or half-migrated states; keep a
  larger refactor local until its compatibility and relevant performance gates
  pass together.
