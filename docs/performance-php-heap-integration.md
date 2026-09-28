# Rust heap integration and PHPStan attribution

Status: Rust heap integration accepted. Fresh control measurements and root-cause
diagnostics are complete; the larger-project validation timed out and remains
an explicit failed gate for that workload.

## Contract

The user approved merging the Rust-only allocator checkpoint on 2026-09-28
with its previously reported performance tradeoffs, then requested a new
PHPStan comparison and an explanation of the remaining performance gap.

- Baselines: clean `main` at `97bff1e3` (system allocator), and the accepted
  Rust-only checkpoint `5304269c`, based on `17c6d9e3`.
- Outcome: integrate the complete Rust heap with current compatibility work,
  preserve PHP behavior, and attribute PHPStan costs on the integrated source.
- Scope: integration conflicts, verification, reproducible measurements and
  profiling. No new PHP semantics, hand-written heap assembly, or speculative
  runtime optimization is part of this checkpoint.
- Ownership: this task owns integration and the heap. The predecessor's
  checkout and uncommitted notes remain untouched. Implementation and joint
  gates run in the existing isolated performance worktree before advancing main.
- Gates: formatting, unsafe diff policy, default/no-default/erased/reified/
  all-feature tests and all-target compilation; exact PHPStan reference
  output; interleaved cold/warm/boot timing, peak RSS and profile attribution.
- Comparison: current main, integrated heap and previous Rust-only binary in
  matching Cargo profiles; reference PHP measured with identical workload and
  worker/cache policy. System-fallback experiments are explicitly labeled.
- Stop rule: fix integration correctness failures before merging; report all
  valid runs and separate observed costs from causal hypotheses. The prior
  accepted migration tradeoff is not a claim that new regressions are free.

No private project source, absolute private paths, raw profile/test logs,
connectivity or credentials may be published. Source/binary fingerprints and
public-safe summaries will identify the evidence.

## Measurement scope discovered during intake

The earlier PHPStan project contains one 234-byte PHP source file. That case
is retained as a startup-heavy control, not presented as a representative
large application. A second project snapshots the five repository PHP tools
under `scripts/phpt/`: 1,161 lines at the integration baseline, with file
hashes retained. Both projects use level 5 and the same serial worker policy.
Cold runs use a fresh temporary directory; warm runs must find the result
cache produced by cold. Each executable has a validated warm-up cycle before
interleaved measurements, and every timed output must match reference PHP.

Hardware `perf` is unavailable on this host because its system binary fails
at dynamic linking. The installed GNU gprofng collector starts successfully;
Sampling and existing VM counters provide independent diagnostic evidence.
Instrumented timings will not be substituted for uninstrumented wall times.

## Integration and verification

The heap branch incorporated current Composer compatibility at `739f927a`;
the integrating agent merged it to main at `0a0099fe`. Runtime source is
unchanged between the verified worktree and this main commit. Only reporting
and measurement-script edits remained in the worktree during the build.

The combined source fingerprint is
`1d084092e90caf488b99958d34763b394a86c28768fbfbc9f45f5ea7101005f5`.
It covers sorted repository-relative paths and contents for `src/**/*.rs`,
`tests/**/*.rs`, `examples/**/*.rs`, `Cargo.toml` and `Cargo.lock`.

| Configuration | Passed | Ignored |
| --- | ---: | ---: |
| Default | 7,077 | 15 |
| No default features | 6,726 | 15 |
| Erased generics | 7,148 | 15 |
| Reified generics | 7,170 | 15 |
| All features | 7,224 | 18 |

All-feature/all-target compilation, Cargo formatting, formatting of the
included heap source files, whitespace and the unsafe diff policy also pass.
Tests used four build jobs and four test workers, with debug assertions and
overflow checks enabled in `test-fast`, in the ordinary host environment.
This is 35,345 successful test executions; ignored tests remain nonclaims.
No PHPT packet was run. The five PHP tool files are static-analysis inputs.

Fresh baseline and merged binaries were compiled for both `release` and
`max-perf`, with a separate instrumented `release` binary for VM counters.
Each build used a task-scoped Cargo target and had to compile RPHP afresh.
Immutable executable copies and SHA-256 hashes prevent target-cache reuse
from silently substituting one variant for another.

## PHPStan native timing

The startup-heavy control uses seven shuffled rounds after one complete warm-up
cycle per executable. The table contains median process wall time in seconds;
p10/p90, all individual runs, RSS, minor faults and user/system CPU times are
retained with the public evidence. Cold means a fresh PHPStan result cache,
not cold filesystem pages. Warm is a new process reusing that result cache.

| Runtime | Boot | Cold analysis | Warm analysis |
| --- | ---: | ---: | ---: |
| Premerge system, release | 1.2000 | 4.1824 | 1.5079 |
| Merged Rust heap, release | 0.9840 | 3.4012 | 1.2230 |
| Premerge system, max-perf | 1.1583 | 4.0947 | 1.4554 |
| Merged Rust heap, max-perf | 0.9415 | 3.2312 | 1.1671 |
| Previous Rust heap, max-perf | 0.9387 | 3.2603 | 1.1776 |
| Previous hybrid TLS, max-perf | 0.9329 | 3.2069 | 1.1693 |
| Merged system fallback, max-perf | 1.1985 | 4.2320 | 1.5171 |
| PHP 8.5.11, CLI OPcache off | 0.5328 | 1.4673 | 0.5802 |

Against the clean premerge `max-perf` build, the integrated heap reduces the
control's cold wall time by 21.1% and warm time by 19.8%. Its cold peak RSS rises
from 671.4 to 724.3 MiB (+7.9%); warm RSS rises from 280.2 to 344.8 MiB (+23.1%).
Reference PHP uses 147.1 MiB cold and 131.3 MiB warm. Much of RPHP's memory gap
therefore predates the custom allocator; this experiment does not attribute
that existing memory to a specific live-data structure.

The hybrid control means the predecessor's Rust allocation paths plus custom
TLS assembly, not its optional naked ASM allocator. On this control, merging
Composer does not establish a slowdown against the previous pure Rust binary:
median cold/warm changes are both approximately -0.9%, with overlapping spreads.
The `RPHP_HEAP=system` variant retains heap routing and TLS overhead and is a
within-binary diagnostic, not a substitute for the clean system-allocator build.

The ordinary release build is slower than merged `max-perf` by about 5% cold.
Every allocator comparison above matches the Cargo profile; LTO is not an
allocator improvement and must not be mixed into the same claim.

## Larger-project gate and subprocess hypothesis

The five-file tool project did **not** complete its validation gate. Reference
PHP finished, but the clean premerge `main-maxperf` cold warm-up exceeded the
predeclared 120-second timeout. The runner killed the whole process group and
accepted no native timing samples for this project. This is an existing-runtime
failure, not evidence of an allocator regression or a successful large-project
speedup. The original failed output and timeout log remain private.

At the user's suggestion, two separate 30-second `strace` diagnostics checked
for a child process that fails to exit. With `proc_open` disabled, there was one
initial exec and no `wait4`: the runtime itself consumed almost one CPU second
per wall second after startup, on one thread. Parser threads finished normally.
With `proc_open` enabled, PHPStan spawned a worker; the parent polled and waited,
while the worker consumed CPU. No process-group descendants remained after
either diagnostic was terminated. This rules out waiting on an already-dead
child in the observed serial failure; it does not prove that every possible
process-lifecycle path is correct.

## Root cause: repeated deep-release graph marking

The observed large-project stall is active runtime work. Two late sampling
windows point to `Value::mark_deep_drop_tree_checkpoints_with_root_policy` and
`Value::clone_cycle_handle`, reached through frame/statement cleanup and
`run_final_object_destructor_tree_inner`. Initial gprofng runs reported interval
timer changes and are not used for quantitative attribution. Delayed sampling
started after ten seconds of startup; those interrupted profiles had no timer
warnings, but their clock weights are not reported as wall time or calibrated
whole-program CPU percentages. Reference PHP's instrumented larger-project run
finished in about 2.5 seconds; that number is not a native benchmark median.

The source explains the repeated work. At `src/vm/execute/call_frames.rs:862`,
each final object's release constructs a fresh `CycleNodeSet` and marks the
complete reachable graph. The recursive child release enters the same code
again. The graph walk at `src/value/mod.rs:7253` is linear within one invocation,
but the set is not shared across successive child invocations. On a chain of
objects requiring PHP destructors, the work sums over suffixes of lengths
N, N-1, ..., 1: quadratic traversal despite linear allocation volume.

Two public reproductions separate this from allocation and callback count:

- `benches/bench_deep_object_release.php`: an acyclic object chain without PHP
  destructors. At 4,096 nodes, merged release takes 0.829 ms; it does not reproduce
  the quadratic path.
- `benches/bench_deep_object_destructor_release.php`: the same shape with a
  destructor that increments a counter. Every run checks that all N destructors
  ran. Construction is timed separately, and cycle GC is disabled identically
  in both runtimes, isolating ordinary final-owner destruction.

Three shuffled measured rounds follow a full warm-up at each size/runtime.
The table contains medians for **release only, in milliseconds**, not total
process time. All samples, including the noisier sub-millisecond plain control,
are retained.

| Objects | PHP | Premerge system heap | Merged Rust heap |
| --- | ---: | ---: | ---: |
| 512 | 0.0391 | 4.8063 | 4.5855 |
| 1,024 | 0.0820 | 17.3116 | 17.1387 |
| 2,048 | 0.1600 | 65.4585 | 63.9198 |
| 4,096 | 0.3490 | 274.4665 | 248.2762 |

Callgrind independently confirms the mechanism. With the merged binary, 512
callback-bearing nodes invoke the marker 512 times, costing 50,441,812 inclusive
instructions. At 1,024 nodes it runs 1,024 times and costs 201,827,739 instructions
(4.00x). Calls from this marker to `clone_cycle_handle` grow from 263,676 to
1,051,644. The marker consumes 83.9% of the 1,024-node reproduction's total
instruction count. These are instruction counts, not elapsed-time speedups;
recursive parent inclusive costs are deliberately not summed. The premerge
system-allocator binary shows the same structure and nearly the same marker
instruction counts. The 1,024-node plain control never enters this marker.

This establishes an allocator-independent quadratic release path and identifies
the same hot functions in the larger PHPStan diagnostic. It does not establish
that this is the only cost in a completed large PHPStan run: that gate did not
finish. A separate checkpoint should remove repeated graph scans while preserving
PHP destructor order, callbacks that mutate or resurrect objects, aliases,
cycles and deep-stack safety. Simply deleting the checkpoint protection or
reusing a visited set across arbitrary user callbacks is not a proven fix.

## Remaining costs in the completed control

The instrumented `vm-stats` build produces the same PHPStan output as reference
PHP after its separate stats block is removed. Its times are excluded from
native comparisons. The CLI dumps these counters before final request shutdown,
so shutdown-only work is outside their scope. Cold analysis records 1,830,979 pushed call frames,
11,823,553 scanned cleanup slots and 1,112,402 array-owner allocations. It takes
1,682,427 full call paths versus 134,565 fast call paths. Array `Value::clone`
counts are reference/ownership operations, not evidence of that many deep copies.

Although JIT is enabled, the run executes only two native typed loop regions,
covering 478 recorded quick-loop iterations. The planner records 763,088 rejected
backedge executions for `array_shape`. These counts identify interpreter, call,
ownership and region-coverage work that a faster allocator cannot eliminate;
they are not a percentage-of-time coverage estimate.

Seven-round independent holdouts validate identical PHP results. In `max-perf`,
merged versus clean premerge main medians are 0.5722 versus 0.5756 seconds for
method calls, 0.5488 versus 0.5617 for `call_user_func`, and 0.7999 versus 0.8158
for object/string property work. The short array test is 0.0171 versus 0.0168
seconds with overlapping distributions. The old hybrid TLS method-call control
remains faster at 0.5402 seconds: the previously accepted localized migration
tradeoff is still visible. This does not justify claiming the new allocator is
faster for every program. Reference PHP needs only 0.0633 seconds on the callback
holdout; allocator replacement does not close the general call-path gap.

## Reproduction and evidence

[Public evidence](performance-php-heap-integration-evidence/README.md) contains
all accepted native samples, distributions, source/binary hashes, test counts,
VM counters and sanitized process/profile findings. Raw traces, private input,
absolute host paths and generated caches are excluded. PHPStan is version
2.2.14; reference PHP is 8.5.11, with CLI OPcache and JIT disabled. RPHP defaults
include JIT and the heap. Both use identical disabled `proc_open,pcntl_signal`
settings for the native control. The separate enabled-process diagnostic keeps
only `pcntl_signal` disabled.

The host is x86-64 Linux on a Ryzen 9 7950X with 32 GiB RAM and the performance
CPU governor. Rust is 1.98.1, with repository function-alignment flags. Build
and timing windows hold the exclusive project lock; no RPHP build/test work
runs concurrently. Ordinary desktop activity remains. CPU affinity is inherited,
all valid runs are retained, and p10/p90 use inclusive linear interpolation.
The complete metadata includes PHP extensions and compiler configuration.

With exact baseline/current executables and the same PHAR/project available:

```sh
flock -x /tmp/rphp-benchmark.lock python3 scripts/bench-phpstan.py \
  --variant "baseline=$BASELINE_BINARY" --variant "candidate=$CANDIDATE_BINARY" \
  --phar "$PHPSTAN_PHAR" --project "$PHPSTAN_PROJECT" \
  --rounds 7 --seed 280926 --output "$FRESH_REPORT_JSON"

"$CANDIDATE_BINARY" benches/bench_deep_object_release.php 4096
"$CANDIDATE_BINARY" benches/bench_deep_object_destructor_release.php 4096
```

Native builds use `cargo build --offline --locked --profile release --bin rphp`
and the same command with `--profile max-perf`, in fresh task-scoped target
directories, with four jobs and incremental compilation disabled. Stats add
`--features vm-stats` to release and set `RPHP_VM_STATS=1` only for diagnostic runs.
The test matrix uses `cargo test --offline --locked --profile test-fast` with
four workers, assertions and overflow checks enabled, for default, no-default,
erased, reified and all-feature configurations. All-target checking uses the
same locked offline profile with `--all-features --all-targets`.

The lifecycle cleanup ran before and after the joint matrix and release cycle.
Completed test/build target directories were removed; source snapshots, logs
and exact standalone baseline/current executables remain available for audit.
No private benchmark host was configured.

No additional runtime optimization was folded into the merge after the joint
matrix. This checkpoint integrates the accepted Rust heap, validates its narrow
PHPStan benefit, and supplies a reproducible next runtime problem. It makes no
large-application performance or ARM64 performance claim.
