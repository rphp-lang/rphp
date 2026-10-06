# PHPStan: defer the return declaration-name fallback

Accepted checkpoint on `codex/perf-phpstan-cold`, clean baseline `8729b023`.
Independent confirmation reduces analysis **70.6979 to 70.2339 billion
instructions (-0.656%)**, with identical PHP output. Analysis medians are
6.5835/6.4852 seconds versus PHP's 0.9471 seconds and 10.3378 billion
instructions. The remaining **6.79x instruction gap** keeps the original
parity goal active. This is a narrow return-metadata improvement. All valid
samples and checks are in [the packet](performance-phpstan-return-scope-projection-samples.json).

## Exact attribution and change

A fixed-capacity private `memcpy`/`memmove` interposition probe on the accepted
PGO executable gates its tally on the same analysis FIFO enable/disable writes.
It records exactly two transitions and no overflow, and reproduces PHP output.
LLVM source mapping identifies **3,412,895 copies/110,442,751 bytes** in the eager
`return_type_callee_class` declaration-name clone at ordinary typed returns.
Native memcpy sampling and copied-byte totals are different, overlapping
measurements; copied bytes are not retired instruction budgets.

The existing pure checker already carries a `TypeCheckScope::Return` source
through nullable/union/intersection recursion and resolves lexical/called scope
only when a visited relative name needs it. Its owned declaration-name fallback
was nevertheless built eagerly by all three return sites. The sites now pass
their already-live `FunctionCommon` identity. Relative lexical lookup first
uses the same canonical `get_caller_class`; only when absent does it borrow the
same declaring-class fallback from executor metadata. Ordinary and absolute
named checks never construct that unused String.

The eager helper is removed. Value inspection clones, exact/coercion precedence,
warning callbacks, `__toString`, reference-return post-finally validation,
errors, exceptions and final return transfer remain canonical. The executor
borrow ends before mutable PHP conversion/re-entry. No new opcode, cache,
unsafe block/function, runtime conditional in Return, workload recognition or
external ABI is added. Existing inventory remains 1,749 blocks/321 functions;
Value/frame/cache/opcode sizes remain 16/64/16/16 bytes. Portable Rust changes
baseline execution only, with native measurements on x86-64; no ARM64 native
claim or JIT lowering change is made.

Final-binary interposition confirms **3,412,999 fewer copies and 110,386,759
fewer copied bytes**. Total calls fall 36,658,590 to 33,245,591; the distinct
2,574,074 array-construction copies at each of two sites are unchanged.
Instrumented probe time/counts are diagnostic overhead, not performance results.
This isolates the intended metadata removal from the larger array copy budget.

## Native selection and confirmation

| Protocol | Baseline | Candidate | Instructions |
| --- | ---: | ---: | ---: |
| Ordinary release, two interleaved phase pairs | 77.0758 G | 76.3662 G | -0.921% |
| Fixed eighteen-input PGO, first two pairs | 70.6959 G | 70.2446 G | -0.638% |
| Independent PGO confirmation, two pairs | 70.6979 G | 70.2339 G | -0.656% |

The fast release selector takes 68.384 seconds to rebuild and 38.973 seconds to
compare, before the expensive PGO/feature gates. Its saving is a selection
signal, not the final result. Baseline/candidate binaries are distinct frozen
artifacts. PGO uses the unchanged eighteen inputs, excludes PHPStan and all
seven controls, and uses Rust 1.98.1/LLVM 22.1.8, `max-perf`, fat LTO, one codegen
unit, function alignment 6 and line tables. Instrumented/profile-use builds
take 256.721/195.166 seconds, with no runtime profile checksum mismatch.
The 26 build-script profile warnings plus their summary remain visible.

Counters measure user instructions on CPU 2 and the same analysis FIFO region,
with fresh result cache, no warmup and 100% counter running time. Every valid
sample is retained. First PGO analysis times are 6.5443/6.5228 seconds; independent
confirmation is 6.5835/6.4852 seconds. The whole-command one-observation check
is 89.0066/88.5505 billion instructions, 8.6306/8.5148 seconds wall time and
622,504/622,460 KiB RSS. PHP is 15.4740 billion whole-command instructions and
177,512 KiB RSS. Whole-command and analysis-only counts must remain separate.
All variants produce the same five files, twenty findings and expected exit 1.

## Control distributions and review

The first and independent control windows each use three randomized pairs.
A +1.619% trait timing median in confirmation conflicts with -0.314% initially;
it triggers a predeclared larger **all-seven-control** review, five randomized
pairs each with a new seed. The source and binaries stay frozen and every valid
sample from all three windows remains visible. No favorable control alone is
rerun, discarded or resized. The larger review passes the one-percent ceiling.

| Untrained control | Review instructions | First time | Confirmation time | Larger review time |
| --- | ---: | ---: | ---: | ---: |
| `bench_class_constant_replay.php` | +0.0001% | -0.291% | -0.506% | -0.148% |
| `bench_inherited_method_metadata.php` | -0.0555% | -3.143% | -1.523% | -1.992% |
| `bench_regex_nested_continuation.php` | -0.0000% | -2.182% | -2.859% | +0.521% |
| `bench_return_scope_projection.php` | -3.5392% | -5.436% | -5.231% | -5.703% |
| `bench_shared_frame_release.php` | -0.0432% | +0.081% | +0.856% | +0.691% |
| `bench_shared_temp_read_results.php` | -0.0317% | -0.731% | -0.689% | -2.148% |
| `bench_trait_property_scope.php` | -0.0185% | -0.314% | +1.619% | -0.718% |

The independent mixed-return holdout saves 3.539% instructions and 5.703% time
in the larger review. Timing variation in other controls is reported explicitly;
no broad PHP speed or allocator superiority follows from this result.

## Correctness and integration

Seven CLI contracts match reference PHP and the unmodified canonical baseline
in ordinary release and final PGO: new reentrant coercion/warning/finally/reference
contract, relative union/intersection/trait/closure scope, trait fallback,
wide scopes, reference return, numeric forwarding and reference/COW getters.
The new contract is public and its expected stdout is the exact PHP output.

Four focused integration packets plus the 52 named tests defined in the return
hint source pass under default, no-default and all features: **69 per variant,
207 actual executions**. An initial `return_hints::` filter selected zero tests
because that file uses `include!`; those runs remain visible and count as zero,
not passes. The correction enumerates the existing test names and runs the exact
52 tests in each already-built executable. All-target/all-feature compilation,
formatting and unsafe-diff checks pass. One formatting-only preparation failure
is retained separately, before successful source freezing/build. Existing
compatibility gaps remain failures; no unnecessary broad matrix is run.

Main executor size falls 390,878 to 389,951 bytes, text falls 1,369 bytes and the
executable falls 8,232 bytes to 76,488,312 bytes. The checker source and fallback
remain shared rather than adding a new successful-return borrow path. The prior
borrowed-value return experiment was rejected for 13–14% control timing costs;
its source remains absent and is not repeated in this checkpoint.

All expensive work uses a verified separate aggregate user-systemd boundary:
6 GiB, no swap, whole-cgroup OOM/timeout kill and exclusive benchmark lock.
Recorded boundaries have no OOM events. Cleanup hooks run before/after checks;
superseded PGO/test targets are deleted. Exact baseline/current binaries and
source snapshots remain, with only the active ordinary-release build cache.
The already-produced all-feature test binary is preserved for structural
counter discovery only, not used as a benchmark baseline. No private benchmark
host is configured. Public staged-diff hygiene is verified before commit.
The sole integrating agent owns the two executor files; main stays untouched.
Continue with measured general frame/executor costs; parity is not complete.
