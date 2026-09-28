# Rust-only PHP heap checkpoint

Status: allocator assembly removal is complete. The final direct Rust TLS
implementation passed the correctness matrix. Performance is mixed; this is a
measured Rust-only baseline, not a claim of universal improvement or a closed
performance ceiling. Additional TLS abstractions that regressed were removed.

Subsequent decision (2026-09-28): the user approved integrating this Rust-only
checkpoint with its reported costs. The results below identify the pre-merge
source; [the integration report](performance-php-heap-integration.md) records
fresh verification and PHPStan measurements after combining current main.

Performance profile: `cargo build --profile max-perf --bin rphp`. This keeps
whole-program optimization explicit; ordinary release defaults are unchanged.

## Contract and decision

The user selected a single Rust allocator and explicitly requested removal of
all hand-written allocator assembly on 2026-09-28. This includes the optional
naked fast paths, `php-heap-asm`, and the Linux local-exec TLS primitive.
The exact baseline is `17c6d9e31f700406274f3020b01df6a5924fb816`, whose heap
matches `30ab4986`. The preceding custom heap is the comparator, not a newly
rebuilt mimalloc or glibc baseline.

Work is isolated on `codex/perf-php-heap-finish`. The predecessor's checkout and
uncommitted measurement notes are preserved. No PHP value representation,
parser, JIT, or language semantics change here. The invariant envelope includes
size/alignment/zeroing/reallocation, system fallback, local and remote frees,
thread exit, versioned free stacks, reclaim and debug diagnostics.

There are two distinct decisions: remove assembly as requested, and admit only
measured improvements to the remaining Rust implementation. A microbenchmark
win alone does not admit an optimization. Confirm representative/holdout
regressions outside noise before accepting a performance claim. Report the
ordinary `release` and `max-perf` profiles separately; changing the
profile is not an allocator-only speedup.

## Implementation and ownership proof

The original 2,335-line implementation is split into private `layout.rs`,
`thread.rs`, `region.rs`, `remote.rs`, `pages.rs`, `slow.rs`, and `debug.rs`.
`mod.rs` retains `GlobalAlloc` and the small-object routing. The `include!`
boundaries preserve the lexical namespace and follow existing parser/VM
practice; before optimization, expansion reproduced the original source byte
for byte. The normalized allocator entry-point instructions also matched.
This does not promise identical whole-program code placement after extraction.

Both native TLS assembly and all naked allocator functions/wrappers are gone.
Every platform now uses the same constant-initialized `thread_local!` heap.
`ThreadHeap` has no destructor; the separate exit guard retains its existing
page-detachment and orphan protocol. Size classes, header geometry, remote
queues, page accounting, reservation and reclaim budgets are unchanged.

The private, forced-inline `heap()` helper keeps the standard TLS lookup's
closure limited to taking the heap address. The allocator body remains visible
to the optimizer at its call site. Its one unsafe dereference is inherited
from the portable implementation: the constant-initialized heap has no
destructor and stays live on its owning thread through TLS teardown. Although
the private return type is `&'static ThreadHeap`, callers consume it
synchronously on that thread and never expose or store it. `ThreadHeap` is not
`Sync`; the existing atomic ownership/pusher protocol protects remote access.
The new teardown test allocates and frees from a real TLS destructor after
explicitly orphaning the heap, checks the live payload, and verifies that
subsequent allocation uses the system fallback.

The constant initializer/no-destructor choice follows the
[standard library's documented TLS optimization](https://doc.rust-lang.org/std/macro.thread_local.html).
Actual generated code remains profile-dependent: `release` retains register
saves around a general-dynamic TLS access that the linker relaxes later;
`max-perf` uses whole-program LTO and removes those saves before code emission.
Neither a shorter source file nor fewer instructions alone proves a speedup.

## Rejected experiments

- A direct indexed TLS load in inline assembly improved the allocation probes
  by approximately 3–6% and PHPStan by about 1%. The 96-program corpus summed
  to only 0.5% improvement, with independently confirmed regressions of about
  4–7% in JSON, static callbacks and late static properties. It was rejected
  before the user's final assembly-removal instruction.
- A base-load simplification in naked free had no separately demonstrated
  native-time benefit. The entire optional implementation is now removed.
- Moving cached bounds next to the first six size-class pointers was tried
  together with a fully scoped standard `LocalKey::with` body. This candidate
  regressed dynamic allocation probes by 25–49%. Disassembly exposed the main
  problem: `LocalKey::with` was outlined around the full free body, introducing
  a call and closure arguments on the stack. Both changes were removed; no
  isolated benefit or harm is attributed to the field order.
- A smaller scoped-callback TLS helper removed that outlined call but still
  lost to the direct Rust getter in the independent control: approximately
  4.4% on `call_user_func` and 8.0% on the composed float workload. It was
  removed. The final code keeps the simpler getter and original field order.
- A signed `unchecked_sub` prototype for the page counter did not shorten
  the emitted instruction sequence (28 versus 30 bytes in the isolated
  probe). It was not integrated; the existing wrapping counter is retained.

## Measurement method

Rust 1.98.1 / LLVM 22.1.8, Ryzen 9 7950X, Linux x86-64, performance governor,
repository function-alignment flags and default features. `release` and
`max-perf` are compared within the same profile; the latter already existed
and uses fat LTO with one codegen unit. No compiler-profile defaults changed.
Each source tree has a separate Cargo target; copied executables are immutable
and hashed before timing. The exclusive benchmark lock prevents build/test
activity from overlapping native timing. Cleanup runs before and after cycles.
No private benchmark host is configured.

The repository probe (`examples/heap_alloc.rs`, `scripts/bench-heap.py`) uses
15 seeded, interleaved rounds of 50 million allocation/free pairs and one
million warm-up pairs per case/variant, pinned to one CPU. Every run validates
its initialized payload and the runner verifies matching checksums. Burst
counts round up to complete batches of 256. Dynamic cases retain global
allocator calls; `specialized` exposes a constant 40-byte layout directly to
`GlobalAlloc`. All samples are retained.

PHPStan uses seven shuffled rounds per executable. Cold is a fresh result
cache with warm filesystem caches; warm reuses that result cache. Worker
functions are disabled identically. Each analysis must match current reference
PHP byte for byte, including two intentional diagnostics and exit code 1.
A monotonic clock measures wall time and `/usr/bin/time` captures peak RSS.
For the use-statement driver, the stable file/token/use-count prefix is checked;
its elapsed-time suffix is excluded.
Reference PHP and socket integration gates run in the ordinary host environment.
The private PHPStan project is not included in public artifacts.

The representative set has 96 existing repository programs, with five
independent holdouts. The `max-perf` comparison uses five shuffled
rounds after validation against reference PHP. Only the scripts' explicitly
printed final elapsed-time field is normalized. A separate nine-round holdout
also disables both JIT and quick loops to exercise canonical execution.

## Final results

Seven interleaved rounds, seconds (median); baseline and Rust use the same
profile in each pair. The comparator is the previous custom heap's default
Rust fast paths plus its hand-written TLS primitive, not the optional naked
ASM implementation. Analysis outputs match reference PHP.

| Workload | Baseline release | Rust release | Baseline max-perf | Rust max-perf |
| --- | ---: | ---: | ---: | ---: |
| PHPStan boot | 0.9614 | 0.9656 | 0.8608 | 0.8683 |
| PHPStan cold | 3.3488 | 3.3826 | 2.9248 | 2.9116 |
| PHPStan warm | 1.2020 | 1.2068 | 1.0766 | 1.0841 |
| Allocation PHP workload | 0.9788 | 0.9499 | 0.9264 | 0.8499 |
| Use-statement PHP workload | 0.3343 | 0.3579 | 0.3209 | 0.3133 |

With `max-perf`, cold analysis changes by -0.45% and
warm by +0.70%, within the observed overlap. The allocation
PHP workload is 8.3% faster and the use-statement
workload 2.4% faster by ratio of medians. All seven
paired allocation-workload ratios are below 1. This remains a narrow workload
result: changes in whole-program code placement can affect unrelated runtime
paths, so elapsed time is not attributed entirely to allocator instructions.

Ordinary `release` retains a measurable cost in the use-statement workload
(+7.1%). Cold analysis changes by +1.01%,
warm by +0.39%, and allocation workload by
-2.96%. All runs, including the wider release spread,
remain in the evidence; no outliers are discarded. LTO is a separately named
build choice, not an allocator-only speedup or an omitted release result.

Median `max-perf` cold peak RSS is 720.62 → 720.77 MiB;
warm is 342.83 → 343.16 MiB.
There is no material memory reduction.

Allocation probe: 15 rounds, median milliseconds for 50 million pairs
(burst rounded to a complete batch of 256).

| Case | Baseline release | Rust release | Baseline max-perf | Rust max-perf |
| --- | ---: | ---: | ---: | ---: |
| lifo | 155.687 | 169.881 | 141.627 | 148.215 |
| mixed | 209.154 | 225.493 | 163.433 | 158.872 |
| burst | 170.170 | 181.914 | 119.635 | 105.119 |
| specialized | 146.948 | 149.617 | 141.356 | 155.321 |

Even with LTO, LIFO and the constant-layout probe regress while mixed sizes
and bursts improve. The Rust-only migration is not a per-allocation speedup
across every usage pattern.

The 96-program `max-perf` corpus sums to
52.1457 → 50.6881 s
(sum of per-program medians, -2.80%). Its geometric-mean
ratio is 0.9699. All 101 representative/holdout outputs
match reference PHP. Per-program ratios and spread are published; the aggregate
must not hide individual regressions.

An independent nine-round confirmation used a selection fixed before that
run: five largest improvements, five largest regressions, and every additional
regression greater than 1% whose candidate p10 exceeded baseline p90 in the full corpus.
This selected 13 inputs. All outputs again matched reference PHP.

| Program | Rust/baseline median | Paired ratio range |
| --- | ---: | ---: |
| `bench_type_scalar_function_typed.php` | 0.9072 | 0.8587–0.9419 |
| `bench_type_scalar_function_untyped.php` | 0.9094 | 0.8701–0.9696 |
| `bench_scalar_expression_chain_loop.php` | 0.9166 | 0.8474–1.2006 |
| `bench_static_self_property.php` | 0.9008 | 0.8599–0.9790 |
| `bench_regex_repeated_callback_retains.php` | 1.0451 | 0.8131–1.1836 |
| `bench_array.php` | 0.9856 | 0.8868–1.0831 |
| `bench_generics_method_diamond_manual.php` | 1.0464 | 1.0283–1.0738 |
| `bench_array_multisort_long.php` | 0.9858 | 0.8949–1.1161 |
| `bench_json.php` | 0.9552 | 0.7141–1.2965 |
| `bench_declared_object_lifecycle.php` | 1.0727 | 0.8144–1.4130 |
| `bench_instance_property_write_typed.php` | 1.0080 | 1.0013–1.0336 |
| `bench_call_user_func.php` | 1.0504 | 0.9960–1.1166 |
| `bench_stdclass_string_property_strlen.php` | 1.0150 | 0.9244–1.0403 |

Persistent costs (ratio of medians >1.01 and all nine paired ratios >1.01):

- `bench_generics_method_diamond_manual.php`: +4.64%.

These prevent a claim that the migration satisfies a universal performance
improvement gate. The requested Rust-only implementation is complete, but the
checkpoint remains local for review of these costs. No optional optimization
with a demonstrated regression is retained. The ordinary release TLS overhead
and whole-program layout effects remain targets for a separate bounded change.

With JIT and quick loops disabled, the independent nine-round holdout is:

| Holdout | Baseline ms | Rust ms | Change |
| --- | ---: | ---: | ---: |
| `bench_string.php` | 8.988 | 9.115 | +1.42% |
| `bench_array.php` | 37.967 | 36.756 | -3.19% |
| `bench_json.php` | 15.667 | 14.929 | -4.71% |
| `bench_declared_object_lifecycle.php` | 100.312 | 101.147 | +0.83% |
| `bench_regex_repeated_callback_retains.php` | 8.583 | 7.670 | -10.64% |

Short programs remain sensitive to startup and code placement. These are
measurements of entire executions, not isolated allocator savings.

| Code size | Baseline release | Rust release | Baseline max-perf | Rust max-perf |
| --- | ---: | ---: | ---: | ---: |
| `.text` bytes | 15,400,862 | 14,778,014 | 13,523,374 | 13,921,262 |
| Allocation entry bytes | 108 | 156 | 100 | 130 |
| Free entry bytes | 120 | 129 | 108 | 145 |

Removal of handwritten assembly does not guarantee smaller generated machine
code. Final release/max-perf runtime and example rebuilds took 80/178 seconds
with existing dependencies; these are not clean-build compile-cost comparisons.

All public-safe samples and source/binary fingerprints are in
[the evidence directory](performance-php-heap-evidence/README.md).

## Verification

The measured source fingerprint matches the completed matrix. All tests ran
on the ordinary host, with four Cargo jobs/four test workers and debug
assertions/overflow checks enabled. No failing or filtered test was accepted.

| Configuration | Passed | Ignored |
| --- | ---: | ---: |
| default | 7,063 | 15 |
| no-default | 6,712 | 15 |
| erased | 7,134 | 15 |
| reified | 7,156 | 15 |
| all-features | 7,210 | 18 |

Total: **35,275 successful test executions**.
All-feature/all-target compilation also passes. Existing tests cover size
classes, alignment, zeroing, realloc, versioned stacks/ABA, reclaim, retirement,
remote frees, orphan adoption, concurrency and debug detectors. The added test
exercises allocation/free during TLS destruction. No PHPT packet was run.

The source inventory falls from 1,754 to 1,747 unsafe blocks and 329 to 321
unsafe functions, including one new test-only block under `src/`. Commands,
source fingerprint and private-log hashes are recorded in
[verification.json](performance-php-heap-evidence/verification.json).
Formatting (including the private include files), the unsafe-policy diff gate
and whitespace checks pass after the matrix, without changing its source.

Cleanup hooks completed after the matrix and confirmation. All seven task-owned
Cargo build directories were removed after preserving the immutable baseline
and final executables, source snapshots and evidence. No private benchmark
host is configured.

## Reproduction and limits

Build the same `heap_alloc` source in baseline and candidate trees using
`cargo build --locked --offline --release --example heap_alloc` or
`cargo build --locked --offline --profile max-perf --example heap_alloc`.
The baseline predates the probe: copy only that example into its checkout.
Use distinct `CARGO_TARGET_DIR` directories, preserve executables under separate
names, and hold the exclusive benchmark lock while running
`scripts/bench-heap.py --variant baseline=EXECUTABLE --variant
candidate=EXECUTABLE --output RESULTS.json`. Compilation is separately checked.
Cargo stale-cache incidents were detected through build logs and binary hashes;
all affected samples were discarded and the final binaries were genuinely
rebuilt before this cycle. The final source-to-binary and test fingerprints
are retained.

The corpus uses the repository paths in `corpus-validation.json`, default
features and five shuffled rounds with seed 270927, after one validation run
per executable and reference PHP. The canonical holdout uses seed 270928 and
nine rounds with `RPHP_DISABLE_JIT=1 RPHP_DISABLE_QUICK_LOOPS=1`. Runtime timing
includes startup and uses inherited CPU affinity. PHPStan uses seed 270926 and
seven rounds; its private project is not published, so public artifacts alone
cannot reproduce that application input. All successful samples are retained.

Only Linux x86-64 was measured. No AArch64 target or physical AArch64 host is
available, so this checkpoint makes no ARM performance claim. It also does not
claim an absolute performance ceiling or a fresh mimalloc/glibc comparison.
The next optimization checkpoint should isolate standard Rust TLS code
generation and the confirmed method-call regression. Allocation-count
reductions in string ownership and typed pools remain separate changes to
value lifetimes; they are not implemented or claimed here.
