# Benchmark methodology

RPHP benchmarks answer narrow questions about supported program regions. They
must not be presented as evidence that RPHP is faster than PHP in general.

## Required result metadata

Every published result must include:

- exact RPHP commit and a clean/dirty tree statement;
- CPU model, architecture, memory, operating system, and relevant power mode;
- Rust version, Cargo profile, compiler flags, and RPHP feature flags;
- exact PHP version, build, loaded extensions, configuration, and JIT mode;
- benchmark source or repository path plus its input data;
- warm-up policy, measured repetitions, run ordering, aggregation method, and
  distribution data (at least median and a spread such as p10/p90 or IQR);
- output/correctness validation and any timeout, affinity, or isolation setup;
- a statement that the result covers this supported workload, not all PHP
  applications.

## Comparison procedure

1. Start from clean, named baseline and candidate commits.
2. Run `scripts/cleanup-builds.sh`, then build disposable release candidates in
   task-scoped `/tmp/rphp-candidate-*` target directories.
3. Verify identical expected output before timing.
4. Warm each executable using the declared policy.
5. Interleave and randomize baseline/candidate or RPHP/PHP order where
   practical; avoid measuring all runs of one side first.
6. Record every valid run. Define outlier handling before seeing the result and
   report both the rule and retained sample count.
7. Investigate a regression outside established noise. Use a larger,
   independent rerun rather than selectively repeating only favorable cases.
8. Run the relevant correctness matrix after the final candidate and invoke
   the cleanup hook again, even after a failed checkpoint.

Wall-clock timing should use a monotonic, sufficiently precise clock. Keep
compilation, startup, parsing, and execution costs separate when the claim
depends on that distinction. Do not silently compare a warmed RPHP runtime
with a cold PHP process, or one JIT configuration with an unnamed alternative.

## Workload selection

Microbenchmarks are useful for isolating dispatch, calls, arrays, objects,
strings, or native lowering. They are not representative applications. A
performance change should also be checked against the repository corpus and
an independent holdout whenever the optimized shape can affect them.

Benchmark-specific compiler recognition is not acceptable unless the same
general proof and execution path applies to ordinary PHP programs. Optimized
behavior must retain exact fallback semantics.

## Result wording

Prefer: "At commit `…`, on CPU/OS `…`, RPHP was 1.4x faster for workload `…`
under these configurations."

Avoid: "RPHP is 1.4x faster than PHP."
