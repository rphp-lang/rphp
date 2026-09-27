# Rust-only PHP heap evidence

Baseline: `17c6d9e31f700406274f3020b01df6a5924fb816`, the previous default Rust
fast paths with its hand-written TLS primitive. The final candidate is the
direct standard Rust TLS getter. `manifest.json` records the exact source,
binary hashes, CPU/toolchain, profiles, code sizes and build commands.

- `micro.json`: all 240 measured runs, warm-up, checksums, pinning and seed.
- `runtime.json`: all 140 PHPStan/PHP-driver runs, wall time, RSS and faults.
- `runtime-summary.json`: medians, p10/p90, ranges and paired ratios by profile.
- `corpus.json`: all 1,010 runs of 96 representative programs and five holdouts.
- `corpus-summary.json`: per-program medians, p10/p90 and five paired ratios.
- `corpus-aggregate.json`: sum of medians and geometric-mean ratios.
- `corpus-validation.json`: reference-PHP validation for all 101 inputs.
- `holdout.json`: 90 runs with both JIT and quick loops disabled.
- `tls-controls.json`: earlier independent nine-round comparison, including
  the rejected scoped helper. Its `native` executable is byte-identical to the
  final max-perf candidate; `candidate` there means the rejected helper.
- `confirmation.json` and `confirmation-summary.json`: the final independent
  nine-round confirmation on 13 fixed inputs, including every valid sample.
- `verification.json`: final matrix counts, source fingerprint and log hashes.

No successful samples are trimmed. p10/p90 use lower rank indices on the
sorted samples. Corpus names refer to repository files; only each program's
printed final elapsed-time field is normalized during validation. Private
PHPStan inputs and raw build/test logs are not published. The
[checkpoint report](../performance-php-heap-finish.md) records limitations and
unfavorable results alongside improvements.
