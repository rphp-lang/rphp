# RPHP workspace rules

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
- At the end of a benchmark checkpoint, run the same cleanup on both the local
  workspace and `the configured private benchmark host`, even when the matrix or benchmark failed.
- Put disposable release builds in task-scoped `/tmp/rphp-candidate-*`
  directories rather than accumulating profiles in the workspace `target`.
- After a benchmark checkpoint is accepted, delete superseded candidate build
  directories on both the local machine and `the configured private benchmark host`. Retain only
  the exact baseline and current candidate needed by an active comparison.
- Never delete source snapshots, exact baselines used by an active gate, or
  user files as part of automatic cleanup.
