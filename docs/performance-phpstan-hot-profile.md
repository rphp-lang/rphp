# PHPStan hot execution: instruction amplification and file-count scaling

Diagnostic checkpoint, 2026-09-28. No runtime change is accepted by this report.
The current request is to find the cause through profiling, without another
large test matrix. The unfinished cleanup experiment remains separate.

The measured hot-analysis gap is real. For controlled inputs of 1–16 equally
complex files, growth is approximately linear, with a much larger cost per file
in RPHP. A fresh PHP instruction profile confirms that the same five-file
application request executes about ten times as many instructions in RPHP.
There is no evidence here for one quadratic traversal explaining the whole gap.

## Comparison identity and boundaries

- RPHP baseline: `ffe29165938decd0685932ff1bc1b64a587fef64`, default features,
  `max-perf`, Rust 1.98.1, LLVM 22.1.8, fat LTO and one codegen unit.
- Baseline executable SHA-256:
  `e922e13dfbe4c441e6f4d2282e9b5878d6e1ac804aadaf3a5afd11f39dd033ca`.
- PHP 8.5.11 NTS executable SHA-256:
  `2d059aa6d433b73285c68cef6fa37b66bf7f4834240b34b944ad68caade7aad6`.
- Original application archive SHA-256:
  `a7d45c01d3bd5aceb2cb9e596a67e50ff9f12b8757a373b93c0761deb8cd77e1`.
- Ryzen 9 7950X, approximately 32 GiB RAM, performance governor, x86-64.
  Every diagnostic runs serially under the exclusive benchmark lock and a
  verified aggregate 6 GiB memory limit, zero swap and whole-group termination.
- Five-file input: `scripts/phpt/{case,execution,expectation,process,report}.php`,
  1,161 lines, PHPStan level 5. Each process gets a fresh temporary directory.
  OS file cache is warm. Both runtimes disable `proc_open`, `pcntl_signal`,
  `pcntl_exec` and `pcntl_fork` to keep the application in the same serial mode.
- Timed diagnostics verify stdout, ordinary stderr, exit status and analysed
  file count against PHP. The original application reports 20 expected
  findings and exits 1; this is a successful comparison, not a zero-findings run.
- Final environment audit: PHP CLI OPcache is disabled, JIT is disabled, and
  `phpstan_turbo`, Xdebug and PCOV are absent. The observed gap cannot be
  attributed to an enabled PHP JIT or that native PHPStan extension.

## Controlled file-count experiment

Copy the 145-line `case.php` into separate projects containing 1, 2, 4, 8 and
16 files. Rename its function identifiers with a unique, fixed-width suffix
for each copy so declarations do not collide. Each project has the same level
and configuration. Instrument only the analysis boundary and pre/post-file
callbacks with `microtime`; use the same modified archive for both runtimes.

This is one diagnostic timing sample per size/runtime, with randomized size
order, not an acceptance benchmark or a statistical complexity proof.
All samples are retained. These times exclude process bootstrap; the analysis
phase still includes application initialization on the first file.

| Files | RPHP analysis | PHP analysis |
| ---: | ---: | ---: |
| 1 | 5.099 s | 0.416 s |
| 2 | 6.128 s | 0.528 s |
| 4 | 8.392 s | 0.706 s |
| 8 | 13.294 s | 1.112 s |
| 16 | 22.438 s | 1.746 s |

An affine fit is approximately `3.86 + 1.16 * files` seconds for RPHP and
`0.35 + 0.0885 * files` for PHP. More directly, post-first-file measurements
across these runs have median 1.152 s in RPHP and 0.0902 s in PHP, a 12.77x
ratio. In the 16-file run, RPHP's second through sixteenth files cost
1.104–1.199 s each; later files do not become several times more expensive.
This supports approximately linear growth within this controlled range.
It does not rule out superlinear behavior in larger or differently connected
projects. File count alone also does not measure the complexity of arbitrary
source files.

On the original heterogeneous input, the clean per-file timings are:

| File | RPHP | PHP |
| --- | ---: | ---: |
| case.php, including first-file initialization | 4.989 s | 0.429 s |
| execution.php | 4.249 s | 0.313 s |
| expectation.php | 0.860 s | 0.049 s |
| process.php | 1.377 s | 0.115 s |
| report.php | 1.441 s | 0.120 s |

The full phase in this series is 12.923 s versus 1.029 s. Earlier three-round
paired phase medians were 11.549 s versus 0.924 s; absolute times vary between
series, while the large phase-local gap remains. Compare runtimes within a
series, not measurements taken at different times as an optimization result.

An initial observational probe also called `gc_status()` and
`memory_get_usage()` after each file. It is excluded from these timing claims:
RPHP's `cycle_collection_status()` prunes and reindexes the root registry, so
the observer changes the measured state. The replacement uses timers only.

## Where the extra instructions go

Valgrind 3.22.0 Callgrind profiles of the original, unmodified application on
the same five-file input count:

| Runtime | Whole-request instructions |
| --- | ---: |
| RPHP baseline | 155,356,633,415 |
| PHP reference, freshly profiled | 15,576,704,707 |

The ratio is 9.97x. Both profiles preserve the reference output. These are
whole-request instruction counts, not seconds or phase-only counts; the
separate application timers establish that a similarly large gap remains
inside analysis. Instruction counting does not measure cache misses or cycles.

Selected RPHP exclusive costs are disjoint and can be added:

| Cost centre | Instructions | Whole-request share |
| --- | ---: | ---: |
| Main `execute_ex_inner` body | 30.027 billion | 19.33% |
| Explicit VM release-planning and retirement helpers | 21.614 billion | 13.91% |
| Class lookup, class relations and method-info helpers | 9.452 billion | 6.08% |
| `register_cycle_candidate_with_admission` | 7.391 billion | 4.76% |

The release group includes statement/frame planning, retained-container scans,
value-tree checks, destructor-child enumeration and bitmap retirement, including
their outlined closures. It does not include arbitrary callees' work. These are
selected costs, not a complete ownership or allocator accounting. Inlined work
is attributed to its enclosing function, so the main VM body is not just the
dispatch switch. Inclusive costs overlap and must not be added to this table.

Two concrete repeated operations stand out:

1. **Class metadata is repeatedly resolved by name.** RPHP calls `find_class`
   50,375,971 times. Of these, 36,580,994 come from `class_is_a`, which resolves
   both class names before checking identity or cached ancestry. The two
   lookups repeat even for an unchanged class relationship. The diagnostic
   counter build observes 18,289,195 relation checks, including 5,599,776 with
   byte-identical input names. The PHP profile has 146,967 observed calls to
   `zend_lookup_class_ex` across its contexts, while still executing 10,968,615
   calls to `instanceof_function_slow`. The next useful optimization target is
   avoiding repeated name resolution using valid request-local class identity,
   rather than accelerating each repeated hash lookup. Aliases, unresolved
   symbols and reuse of compiled code across requests still require guards.
2. **Dropping aliases repeatedly enters GC bookkeeping and VM release planning.**
   RPHP calls the cycle-admission helper 83,914,755 times, versus 1,643,283
   observed calls to PHP's `gc_possible_root`. The RPHP helper checks whether a
   candidate is already present, usually through the identity index after a
   one-entry tail check. Statement cleanup executes 18,105,267 times and frame
   destructor planning 9,205,511 times. A cheap, valid already-buffered check
   and fewer unnecessary temporary owners are stronger targets than allocator
   instruction tuning. Simply disabling GC or dropping ownership checks would
   change PHP behavior and is not an optimization demonstrated here.

The cross-runtime helper counts are observed out-of-line calls, not identical
semantic events: PHP and RPHP have different inlining and fast-path boundaries.
They identify where RPHP repeatedly pays for work, not a literal claim that PHP
performs exactly 51 times fewer reference-count updates or 343 times fewer type
checks.

The existing counter-only cleanup-candidate build also records 14.3 million
frame pushes, 49.1 million object-handle clones, 37.8 million array-handle clones
and 5.45 million array-owner allocations. A handle clone is not necessarily a
deep copy. Only two optimized loop entries execute 478 iterations; these counts
show little admission for this workload, not an execution-time coverage ratio.
The native PHP reference has JIT disabled too, so lack of RPHP JIT coverage does
not explain the gap as a PHP-JIT advantage.

## Decision and limits

The current evidence favors excessive repeated interpreter and ownership work
per analysed file, not confirmed quadratic growth with file count. There is no
single demonstrated fix worth the entire 10–13x gap. The previous cleanup
candidate improves the phase only about 3.25% and remains unaccepted; it cannot
be presented as resolving the application bottleneck.

The next implementation should eliminate a measured repeated operation, with
a narrow output check and before/after profile first. Class identity resolution
and admission of already-buffered GC owners are concrete candidates. Further
assembler or allocator tuning is not justified as the primary response to
these profiles. No new runtime patch or broad test matrix is part of this
diagnostic checkpoint.

The earlier gprofng sample has an unreliable timer/coverage warning and is not
used for CPU-time percentages. This report does not convert Callgrind shares
into native seconds. Both completed new diagnostic services record zero OOM,
zero swap and no memory-limit pressure; their measured aggregate peaks are
about 1.19 GB for scaling and 0.29 GB for the PHP reference profile. Cleanup
hooks run after both. Raw profiles and local paths remain private evidence.
