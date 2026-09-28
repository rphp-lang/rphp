# Native release and PHPStan evidence

This directory records the candidate described in
[`../performance-deep-release.md`](../performance-deep-release.md).
Acceptance status is controlled by that report; partial records are not gates.

- `post-restart/`: fresh verification after the maintenance pause, using the
  checkpoint's identical source and immutable native executables. `gates.json`
  carries forward the completed default suite and records all remaining
  configurations plus all-target compilation. Exact-source ASan, seven-round
  PHPStan controls (including the same executable's system fallback), deep
  release, the full 103-case corpus and the independent 33-case/11-round
  confirmation are complete. `confirmation-summary.json` records the five
  regressions covered by the report's explicit acceptance exception.
  `regression-profile.json` contains reduced instruction diagnostics, not native
  timings. Memory events and temperature samples are retained for the complete
  bounded cycle. Timings from different machine sessions are not combined into
  one ratio.
- `current/`: current GC-sweep executable's exact build identity, seven-round
  corrected PHPStan comparisons, deep-release/shallow controls, 23-case
  confirmation, full corpus and reduced instruction diagnostics. Its
  `gates.json` and `validation-cycle.json` record the user-requested interruption
  before powering off: only the default full suite completed. The complete
  feature matrix, exact-current-source ASan and final corpus confirmation were
  pending at the pause and are now completed in `post-restart/`; historical
  root-level gates must not be presented as proof for this source revision.
- Historical root-level `build.json`, `gates.json`, `quick-check.json`,
  `corpus.json` and `asan-current-validation.json` below describe the rejected
  direct-bounds executable, not `current/build.json`.
- `metadata.json`: host, Rust/PHP configuration, features and isolation policy.
- `build.json`: candidate source and executable fingerprints, profile and build
  duration. The package was rebuilt rather than trusting a shared target cache.
- `binary-size.json`: immutable baseline/candidate executable and section sizes.
- `quick-check.json`: all seven shallow-control rounds and three deep-release
  rounds after one warm-up per runtime and size. PHP release counts/order agree.
- `corpus.json`: all five rounds across 103 PHP-output-validated inputs,
  including the fixed independent holdouts.
- `revised-confirmation.json`: eleven CPU-pinned rounds over the nine initial
  regression suspects and five fixed controls, selected before this final
  executable was built. `revised-confirmation-summary.json` provides their
  medians and p10/p90 ranges; these ranges are not confidence intervals.
- `final-corpus-confirmation.json` and its summary: the independent eleven-round
  rerun of all eighteen full-corpus suspects plus five controls. Nine confirmed
  regressions reject that executable for integration.
- `dispatch-layout.json`: hot dispatch addresses and sizes for the instruction
  diagnostics; placement alone does not prove a cache/predictor explanation.
- `phpstan-instructions.json`: completed whole-request instruction profile with
  exact executable identity and reference-output agreement. Recursive inclusive
  costs overlap; self instruction counts can be compared against the total.
- `phpstan-reference-instructions-serial.json`: complete PHP instruction profile
  with restart/fork blocked, 15.582 billion instructions and exact output
  agreement. The earlier unrestricted attempt produced an empty profile and
  is not a valid instruction comparison.
- `phpstan-instruction-call-counts.json`: observed out-of-line call counts and
  their principal callers, parsed from the same complete profile. Inline work
  does not have a separate call count.
- `tiny-runtime.json`, `tools-runtime.json`: historical seven-round PHPStan
  samples. PHP was allowed to restart, changing OPcache and dropping process
  restrictions; these are not controlled cross-runtime comparisons.
- `fair-tiny-runtime.json`, `fair-tools-runtime.json`: corrected seven-round
  comparisons with restart/fork disabled and a validated per-runtime preflight.
  These use the cold-growth executable, whose identity is separately recorded.
- `cold-grow-build.json`, `cold-grow-confirmation.json` and its summary: the
  cold page-growth boundary restores the dispatcher's original native stack
  size. One strict-call regression remains confirmed in this 23-case run.
- `gc-disabled-storage.json`: a 300,000-object diagnostic with three retained
  rounds and one warm-up. Explicit memory sampling sweeps dead weak records;
  it reduces RPHP RSS and wall time while reference PHP stays essentially flat.
  The exact diagnostic sources are in `gc-disabled-storage/`. This is
  diagnostic evidence, not a modified PHPStan benchmark.
- `tools-project-sources.json`: relative public source identities copied into
  the five-file PHPStan project. No third-party source or private input is copied
  into this evidence directory.
- `callgrind.json`: candidate instrumented instruction totals and confirmation
  that the removed checkpoint traversal no longer runs. Baseline instruction
  records are in the previous heap-integration evidence directory.
- `gates.json`: final source fingerprint, complete feature-matrix counts,
  all-target result and exact test settings. The successful test service reached
  its 6 GiB aggregate boundary; its memory-event counters were not retained.
- `asan-current-validation.json`: the byte-identical runtime source identity,
  exact reference-output comparison, completion and sampled RSS. Leak
  detection was disabled and the system allocator fallback was used.
- `stack-reproducer.json`, `nested-reference.json`: focused regression output
  against PHP, including the old nested-foreach failure.
- `memory-incident.json`: failed unbounded diagnostic, confirmed exception
  recursion cause, application-scope OOM evidence and subsequent containment.
- `final-measurements.json`: final build identity, sequential stage outcomes,
  aggregate memory limit/peak and memory-event counters for the native cycle.
- `final-regression-profile.json`: output-validated instruction diagnostics for
  the arithmetic loop and the shallow object lifecycle control. Reduced
  iteration counts, where used, are recorded; their times are not benchmarks.
- `profile-summary.json`: qualitative costs sampled over a completed PHPStan
  request. Profiler weights are not calibrated wall-clock durations.
- `rejected-value-drop/`: the rejected candidate's source/binary identities,
  passing output checks and performance regressions, kept separate from final
  acceptance evidence.
- `range-pop/`: independent confirmation of the single-range predicate; this
  variant also retains nine confirmed regressions. It is not accepted.
- `dispatch-codegen.json`: observed growth-call inlining and native stack
  differences, with explicit limits on their interpretation.
- `pre-shutdown/`: the later intermediate executable's identity, confirmation
  distributions and arithmetic instruction diagnosis, also not final gates.

Native timing retains every successful validated sample. Quantiles use linear
interpolation between sorted observations. Instrumented times are never mixed
into native timings. Reproduction uses the existing deep-release benchmark and
`scripts/bench-phpstan.py`, default features, `max-perf`, the exclusive project
benchmark lock and the exact binary/source hashes above. Full checks use the
`test-fast` profile with assertions and overflow checks explicitly enabled.
Raw logs, profiler recordings, temporary directories and third-party fixtures
stay outside the public repository.
