# Ordinary calls: accounting for the instruction gap

This checkpoint isolates the ordinary-call reproduction from the larger
[PHPStan profile](performance-phpstan-hot-profile.md). The user explicitly
requested systemic improvements, without another workload-specific admission
guard, and focused verification instead of a broad test matrix.

## What the reproduction measures

`benches/bench_shared_frame_release.php` repeatedly passes a retained object,
array and closure to a function. The function assigns the object and array to
two locals, then reads an integer property directly or through the array. Its
checksum is 4,500,000 for 500,000 calls. The timed loop excludes initialization
and final destruction of the caller's owners.

Callgrind uses 50,000 iterations and a separate zero-iteration control with the
same declarations and initialization. Subtracting that control measures the
loop, argument handling, function body and return together, not `call`/`ret`
alone. PHP executes approximately 516.084 machine instructions per iteration;
RPHP executes 5,694.123. The ratio is about 11.03x.

The two additional local assignments expose a particularly expensive boundary:

| Program variant | RPHP instructions/iteration | PHP instructions/iteration |
| --- | ---: | ---: |
| Original, including both local aliases | 5,694.123 | 516.084 |
| Read the original arguments without the local aliases | 2,191.066 | 419.018 |
| Pass the heap arguments but return either integer constant | 1,647.376 | 351.514 |
| Copy and read scalar arguments instead | 1,668.343 | 395.472 |

Every variant preserves its expected checksum. Removing the aliases changes
frame layout and cleanup as well as the assignments themselves; the difference
must not be interpreted as an intrinsic per-assignment cost.

## Where the original instructions go

The exact same-directory baseline profile, after startup subtraction, accounts
for the entire incremental instruction total:

| Disjoint cost | Instructions/iteration |
| --- | ---: |
| Frame destructor planner, including its callees | 2,990.001 |
| Main VM body, excluding its callees | 2,121.440 |
| Everything else | 582.683 |
| **Total** | **5,694.123** |

These boundaries do not overlap on this reproduction: its measured loop does
not execute PHP destructors or recursively enter the VM from the frame planner.
The original baseline's machine-code profile attributes about 405 instructions
per iteration to the common dispatch/control sequence. Most of the main VM
body is therefore opcode implementation work, not merely the dispatch switch.

The caller retains all three heap arguments, and argument borrowing already
exists. The new local array owner nevertheless admits the frame to the general
container release planner. It builds a retained-container graph, groups object
candidates, and checks final ownership before ordinary slot retirement.
That metadata work occurs even though this invocation releases no final object.
The original loop performs five hash-table allocation calls and four vector
growth calls per iteration. These are VM bookkeeping allocations; the PHP
program creates its objects and array before the loop.

This explains a large part of this reproduction, not the entire PHPStan gap.
Even the variant without local aliases remains over five times PHP's instruction
count. A rewrite of the allocator or dispatch alone cannot remove all of this
work.

## General planner change

The change stays inside the existing release algorithm:

1. Carry the collected owners together with their existing identity index.
   Transform that index into reference counts in place instead of allocating
   another table and hashing the same identities again.
2. Compact deferred owners into the existing candidate vector. Reuse that
   storage for the next fixed-point pass instead of constructing a second
   vector on every pass.

Owner counts, candidate order, callback conditions and fixed-point scheduling
are preserved. Early error/exception exit still drops unvisited snapshot owners
before the deferred prefix, matching the previous consuming iterator. Generator
filtering removes the same identities from the reference-count table. There is
no new size, type, function-name or workload-specific fast-path guard, no new
unsafe code, and no frame, `Value`, JIT or public ABI change.

The previous unaccepted bounded release shortcut is excluded from this change.

| Startup-subtracted profile | Baseline | Candidate |
| --- | ---: | ---: |
| Total instructions/iteration | 5,694.123 | 5,380.081 |
| Frame planner including callees | 2,990.001 | 2,676.000 |
| Main VM body | 2,121.440 | 2,121.440 |
| Hash-table allocation calls/iteration | 5 | 4 |
| Vector growth calls/iteration | 4 | 3 |

The total falls 5.52%, and the 314-instruction reduction comes from the intended
planner boundary. The main VM body's instruction count remains unchanged.

## Native measurements

Ryzen 9 7950X, x86-64 Linux 7.0.0-34-generic, approximately 32 GiB RAM; Rust 1.98.1,
LLVM 22.1.8, default features, `max-perf`, fat LTO, one codegen unit and the
repository's 64-byte function alignment. Baseline runtime source is commit
`ffe29165938decd0685932ff1bc1b64a587fef64`. Baseline and candidate are built in
the same source directory with the same flags; the candidate includes the
runtime diff in this checkpoint. Its Rust source fingerprint is
`452b8a5f5e06be97f045edb671ad009789dac4247b81710873293fe1a541f51f`.

Executable SHA-256:

- Baseline: `3caa800682f170ca7bd0f46ed051f48994c3ce5c23394cd3680a378cd4e274cd`.
- Candidate: `b27c303799a1eb45170771d0a0a7ab95c0765ddd152c12f91a5e1c179a60498d`.
- PHP 8.5.11 NTS: `2d059aa6d433b73285c68cef6fa37b66bf7f4834240b34b944ad68caade7aad6`.

The reproduction uses one validated warmup and seven randomized, interleaved
runs per runtime, seed 28092931, retaining every valid sample. Durations are
reported by `microtime(true)` around the 500,000-iteration loop. P10/P90 use
linear interpolation within the observed range. No CPU affinity is imposed in
this initial reproduction series.

| Runtime | Median | P10–P90 |
| --- | ---: | ---: |
| Baseline | 142.622 ms | 139.964–156.662 ms |
| Candidate | 133.505 ms | 132.066–136.684 ms |
| PHP | 12.246 ms | 12.189–12.703 ms |

The candidate's median improves 6.39% on this reproduction. It remains much
slower than PHP. The original archived baseline was also retained as a control:
its median is 142.805 ms, versus 142.622 ms for the same-directory rebuild.

Candidate executable size is 21,456,264 bytes versus 21,452,608 bytes, an increase
of 3,656 bytes. Its release rebuild took 162.16 seconds. Builds, checks and
measurements run serially with the exclusive benchmark lock in separate user
services with `MemoryMax=6G`, `MemorySwapMax=0`, `OOMPolicy=kill` and
`KillMode=control-group`; effective limits are verified before work. No OOM or
memory-limit event occurred. The cleanup hook runs before and after each job.

### Application and independent controls

Application measurements use the same five-file PHPStan input and analysis
timers as the earlier report. Each request gets a fresh result-cache directory;
OS file cache is warm. `proc_open`, `pcntl_signal`, `pcntl_exec` and `pcntl_fork`
are disabled in both runtimes. PHP CLI OPcache/JIT are disabled. Output, normal
stderr, exit 1 and five analysed files must match the PHP reference.

The initial three-round series does not demonstrate an application improvement:
baseline analysis median is 12.616 s and candidate is 12.767 s (+1.19%), with
substantial drift in both RPHP and PHP during the series. All three randomized
rounds happened to run baseline before candidate. A separate fixed-CPU ABBA
confirmation is recorded below before accepting any application conclusion.

The three independent controls use one warmup and seven interleaved runs per
variant, the same profile and validated PHP output:

| Control | Baseline median | Candidate median |
| --- | ---: | ---: |
| `bench_foreach_declared_property_reads.php` | 285.225 ms | 280.374 ms |
| `bench_mixed_trace_guard_loop.php` | 503.597 ms | 489.978 ms |
| `bench_deep_object_destructor_release.php 2048`, release | 3.403 ms | 3.372 ms |

These controls show no median regression in this series. They do not establish
a general application speedup or substitute for the PHPStan result.

The independent confirmation pins all processes to CPU 2, uses one warmup each,
and measures four requests per RPHP variant in ABBA ABBA order. Analysis medians
are 11.890 s baseline and 11.844 s candidate (-0.38%); whole-request medians are
14.928 s and 14.883 s. Analysis P10–P90 intervals overlap (baseline
11.854–11.999 s; candidate 11.752–11.888 s). Peak-RSS medians are approximately
1,045 MiB for both. The initial +1.19% signal is not confirmed. This checkpoint
claims no meaningful PHPStan speedup.

The earlier full application profile invokes the changed collected-owner
routine about 1.40 million times. Its direct hash reserve/insert callees cost
329 million instructions and its vector-growth callee 86 million, out of the
155.36-billion whole-request total. Removing duplicated metadata there is a
real application cost reduction target, but its scale is consistent with the
small native effect; it cannot explain the 139.78-billion instruction gap.
These baseline costs are not a measured candidate application instruction delta.

[All measured native samples](performance-ordinary-value-flow-samples.json)
include the original reproduction control, initial application series,
confirmation and independent controls. No valid measured run was discarded.

## Correctness and limits

The focused default-feature packet records 24 successful tests across
`e2e_release_plan`, `e2e_shared_release`, `e2e_deep_release`,
`e2e_object_release_boundaries` and `e2e_weak_objects`; two force-close generator
tests also pass. The same affected packet excluding the five shared-release
cases records 19 passes with all features. Total: 45 successful test executions.
Both new PHP fixtures match reference PHP, unchanged baseline and candidate.
Formatting and the unsafe-policy check pass. No broad compatibility matrix or
ARM64 runtime benchmark is claimed for this bounded checkpoint.

One exploratory fixture exposes an existing baseline mismatch: a later array
member's destructor can be omitted when an earlier destructor throws during
function return. The candidate reproduces that baseline result unchanged; this
fixture was not counted as a pass.

A second suspected mismatch, where a shared argument survived explicit array
retirement until shutdown, was a comparison-configuration error. Rechecking both
runtimes with the same `zend.exception_ignore_args` value gives identical
behavior: with `0`, the exception trace retains the argument; with `1`, it does
not. This is not a runtime ownership leak. The initial classification as a
second baseline bug is withdrawn.

The large structural cost remains eager whole-frame release planning. Removing
that cost requires changing how VM ownership retirement dispatches PHP cleanup,
with explicit treatment of aliases, exceptions and re-entry. The confirmed return
exception failure above must remain visible when evaluating such a change.
Adding another admission shortcut would not satisfy this checkpoint's user
constraint.

The [reusable retirement workspace experiment](performance-release-workspace.md)
measures removal of repeated planner allocations and exercises return/callback
repairs. Its confirmed control regressions prevent acceptance; its runtime
changes have been removed.

The subsequent [ordered frame-retirement experiment](performance-frame-retirement.md)
replaces eager planning at committed returns and records its independent
correctness and performance gates. It is also rejected: a large target-loop
gain does not override its scalar regression or shutdown ordering mismatch.
The independently confirmed wide-frame owned-reference leak is isolated in a
[separate correction](performance-owned-reference-cleanup.md).
