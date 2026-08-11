# RPHP workspace rules

## Build artifact hygiene

- Run `scripts/cleanup-builds.sh` before and after a full four-configuration
  test matrix or release benchmark cycle. The script automatically cleans the
  workspace Cargo target once it exceeds the configured size limit and removes
  stale task-scoped candidate directories.
- Put disposable release builds in task-scoped `/tmp/rphp-candidate-*`
  directories rather than accumulating profiles in the workspace `target`.
- After a benchmark checkpoint is accepted, delete superseded candidate build
  directories on both the local machine and `the configured private benchmark host`. Retain only
  the exact baseline and current candidate needed by an active comparison.
- Never delete source snapshots, exact baselines used by an active gate, or
  user files as part of automatic cleanup.

