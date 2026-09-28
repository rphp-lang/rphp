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

On a development desktop, run the entire build/test/benchmark job in a separate
memory-limited service, including the driver and all descendants. For a 32 GiB
host, the default boundary is 6 GiB with swap disabled. For example:

```sh
systemd-run --user --wait --pipe --collect --service-type=exec \
  --property=MemoryMax=6G --property=MemorySwapMax=0 \
  --property=OOMPolicy=kill --property=KillMode=control-group \
  --property=RuntimeMaxSec=3600 /absolute/path/to/checkpoint-driver
```

Verify `memory.max`, `memory.swap.max` and `memory.oom.group` in the driver's
cgroup before launching expensive work. A virtual-address-space limit is not
an adequate substitute for the aggregate resident-memory boundary, especially
with AddressSanitizer's shadow mappings. Record a limit hit as a failed run;
never remove the boundary to retry. Use the same boundary for both sides of a
timing comparison and record whether it was reached. Retain the exclusive
benchmark lock and perform cleanup even when a diagnostic fails.

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

## Application restarts and PHPStan

A configuration probe before launch does not establish the configuration after
an application re-executes its interpreter. The measured PHPStan PHAR can use
`pcntl_exec()` to enable CLI OPcache; its replacement command does not preserve
the caller's disabled-function flags. Disabling only `proc_open` and
`pcntl_signal` therefore does not establish serial analysis or unchanged INI
settings. An instruction profiler must follow such execs or explicitly prevent
them; an empty profile from the replaced process is not valid evidence.

`scripts/bench-phpstan.py` disables `proc_open`, `pcntl_signal`, `pcntl_exec` and
`pcntl_fork`. It checks that all four functions are unavailable before timing
and records the flags and startup OPcache/JIT configuration for each runtime.
Each cold run gets a fresh result-cache directory; a validated cold run creates
the cache used by its subsequent warm run. OS filesystem caches remain warm.
This protocol measures serial execution without PHPStan's process restart,
not its unconstrained default worker configuration.

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
