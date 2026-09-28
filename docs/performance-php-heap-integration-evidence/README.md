# Rust heap integration evidence

See [the checkpoint report](../performance-php-heap-integration.md) for claims,
commands, limitations and the failed larger-project gate.

- `phpstan-control.json`: all 168 accepted native PHPStan timing samples,
  exact-output hashes, binary/PHAR hashes, cache/warm-up policy and random seed.
- `holdout.json` and `distributions.json`: seven-round independent holdouts and
  median/p10/p90 distributions; percentiles use inclusive linear interpolation.
- `plain-release.json` and `destructor-release.json`: three-round native size
  sweeps, separating construction and final-owner release, including PHP.
- `callgrind.json`: instruction counts and calls for the small validated
  reproductions. Parent recursive inclusive costs are not summed.
- `profile-findings.json`: qualitative late-sampling evidence; interrupted
  collector clock weights are not claimed as whole-program CPU time.
- `vm-stats.json`: separate instrumented-build counters and output validation.
- `process-probes.json`: bounded serial/enabled-process diagnostics. CPU ticks
  are process snapshots, not benchmark scores; stopped descendants are checked.
- `tools-timeout.json`: the failed premerge validation, never counted as a pass.
- `metadata.json`, `builds.json`, `source.json`, `gates.json`: machine/toolchain,
  exact source/executable fingerprints and joint verification results.
- `static-checks.json` and `cleanup.json`: final source identity, formatting,
  unsafe-policy checks and disposal of this checkpoint's build targets.

The small private project and raw stdout, stderr, traces, profile archives and
connectivity are excluded. The larger input is the five public repository PHP
files listed and hashed in `tools-timeout.json`. No native large-project timing
samples passed validation. This evidence does not claim general PHP or ARM64
performance, and no runtime optimization follows the verified merge source.
