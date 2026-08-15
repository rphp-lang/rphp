# Execution coverage scorecard

Status: measured on clean integrated `main` commit
`be7615240ccd1249211e4f92052df33b9e7f8f71`, 2026-08-15

This is the current selection scorecard for the representative corpus,
independent holdouts and focused execution controls. It replaces the stale
pre-Layer-1 selection at `0659191`; historical checkpoint reports remain the
record of the conditions under which earlier optimizations were accepted.

## Checkpoint contract

- **Outcome:** regenerate the complete execution-weighted scorecard after
  Layer 1 and either identify a material non-order virtual-heap graph for
  Layer 2 or reject that layer for this cycle.
- **Baseline:** exact clean integrated commit `be76152`; no production source
  changes were made during selection.
- **Proof:** identical outputs across canonical, typed, default-JIT, PHP
  no-JIT and PHP tracing-JIT lanes; 15 retained order-rotated samples per lane;
  one RSS observation and one separate `vm-stats` execution per RPHP lane; and
  a fresh ARM64 process profile of the highest-weight rejected shape.
- **Acceptance:** both architectures must agree on the owner/allocation and
  admission result. A Layer 2 slice requires a representative non-order graph
  with material hot owners and an independent materialization holdout.
- **Stop rule:** do not manufacture a DTO microbenchmark to force Layer 2
  admission. If the existing corpus has no material non-order owner cost,
  reject Layer 2 and rank the next observed execution gap instead.

## Environment and method

ARM64 used an Apple M4 MacBook Air with 24 GB memory, macOS 26.5.2,
Rust 1.93.1 and PHP 8.5.9. The accepted run started and remained on AC power
with Low Power Mode disabled. An escalated process audit immediately before
timing found no competing Cargo, Rust, RPHP, PHP, scorecard or framework-gate
process.

x86-64 used a physical AMD Ryzen 9 7950X with 31,985,372 KiB memory,
Linux 7.0.0-28 on Ubuntu 24.04, Rust 1.93.1 and PHP 8.4.24. The host reported
the `performance` governor and CPUs 0-31 available to the process. Source sync,
builds, timing, diagnostics and cleanup all ran under the project's one
non-blocking exclusive benchmark lock. No affinity override was applied by the
scorecard runner.

Both hosts used `max-perf`, fat LTO, one codegen unit and
`RUSTFLAGS=-C target-cpu=native`. The canonical and typed binaries contain the
same `quick-loops` code; the canonical lane sets
`RPHP_DISABLE_QUICK_LOOPS=1`. The default-JIT lane adds the ordinary bounded
native JIT. Separate `vm-stats` builds were never timed.

The measured value is the workload's internal `microtime(true)` interval. It
excludes parsing and process startup but includes fresh-process region
admission and native/PHP JIT compilation. Each workload had one untimed
five-lane output validation, followed by 15 samples per lane. Lane order
rotated every round, every sample was retained, no outlier was removed and the
reported spread is nearest-rank p10-p90. PHP tracing JIT activation was
verified before timing. The exact loaded PHP module sets are retained in the
raw metadata; PHP no-JIT and tracing-JIT module sets differ by host and are
controls, not interchangeable RPHP baselines.

Each accepted host artifact contains exactly:

- 1,200 timing rows: 16 workloads x 5 lanes x 15 samples;
- 80 summary rows and no missing or duplicate lane;
- 48 single-observation RSS rows: 16 workloads x 3 RPHP lanes; and
- 48 instrumented RPHP lanes containing 2,412 `vm-stats` rows.

Both runners exited zero. Every validation and measured execution produced the
same payload in all five lanes; the runner would stop at the first mismatch.
The accepted raw artifacts remain outside the repository for integration
audit. Their SHA-256 digests are
`60e304649689010dc47c0772e3f014b53ae9065af9ab6facdc2333e4d07b43fc`
on ARM64 and
`6661a33db02bdc70418da7a5dd13e2dae3ef1674f01aea94b98c1467f71aa3be`
on x86-64.

### Infrastructure-only runs

These runs are not mixed into any accepted distribution:

- An initial sandboxed ARM64 invocation completed all 1,200 timing rows and 80
  summaries, then `/usr/bin/time -l` exited one while reading
  `kern.clockrate` (`Operation not permitted`). It contains no RSS or
  `vm-stats` rows and remains provisional. The complete protocol was rerun in
  one invocation outside the sandbox instead of attaching later diagnostics.
- The first remote setup shell had no Cargo in its non-login `PATH`; it built
  nothing and timed nothing. A second archive-only setup completed two builds
  but stopped before cooldown and timing because the runner correctly required
  Git metadata for commit/clean-tree validation. The accepted x86 run used a
  Git bundle and a clean detached `be76152` checkout under the same lock.

## Current performance scorecard

Times are median in-workload milliseconds with p10-p90 in parentheses. They
apply only to the named workloads and environments.

### ARM64

| Workload | Canonical | Typed | Default JIT | PHP no JIT | PHP tracing JIT |
| --- | ---: | ---: | ---: | ---: | ---: |
| Order corpus | 99.863 (98.278-101.917) | 63.496 (62.297-63.923) | 4.014 (3.967-4.122) | 90.724 (89.775-94.182) | 53.582 (53.102-53.923) |
| Typed order corpus | 121.108 (117.337-123.470) | 83.326 (82.594-85.845) | 4.170 (4.069-4.841) | 95.990 (94.082-99.959) | 55.870 (55.266-58.412) |
| Ledger corpus | 50.720 (49.857-51.922) | 22.217 (22.010-22.422) | 2.250 (2.230-2.316) | 28.144 (27.865-28.639) | 9.387 (9.153-9.627) |
| Typed ledger corpus | 50.873 (50.344-51.164) | 22.351 (21.893-22.659) | 2.235 (2.225-2.284) | 29.119 (28.810-29.457) | 9.555 (9.420-9.712) |
| Routing holdout | 191.762 (185.335-194.954) | 61.804 (61.359-61.967) | 6.158 (6.072-6.259) | 66.424 (65.625-67.070) | 28.036 (27.614-28.428) |
| Dynamic String read | 69.111 (68.009-70.515) | 69.419 (67.671-69.815) | 67.813 (66.681-68.769) | 8.999 (8.693-9.219) | 3.181 (3.158-3.225) |
| Dynamic String CV read | 74.133 (72.964-74.826) | 74.204 (72.684-75.217) | 72.497 (71.576-73.101) | 9.090 (8.901-9.433) | 3.361 (3.289-3.403) |
| Mixed String update | 183.181 (180.857-185.226) | 182.105 (179.626-187.290) | 181.017 (179.430-187.569) | 28.734 (28.425-29.281) | 10.059 (9.897-10.268) |
| Packed value `foreach` | 13.278 (13.140-13.609) | 13.333 (13.012-13.598) | 13.290 (13.049-13.418) | 1.555 (1.546-1.576) | 0.757 (0.736-0.781) |
| Hash value `foreach` | 13.440 (12.909-13.851) | 13.621 (13.296-14.109) | 13.513 (13.196-13.798) | 1.606 (1.590-1.620) | 0.817 (0.795-0.836) |
| String append | 2.525 (2.478-2.594) | 0.310 (0.308-0.322) | 0.314 (0.305-0.318) | 1.265 (1.256-1.285) | 0.953 (0.929-0.972) |
| Packed array build/read | 21.051 (20.686-22.268) | 2.897 (2.841-3.043) | 2.780 (2.741-2.984) | 4.060 (3.954-4.372) | 2.495 (2.437-2.708) |
| Closure copy/invoke | 45.956 (44.958-46.649) | 46.024 (45.057-46.668) | 0.314 (0.309-0.317) | 9.975 (9.739-10.424) | 4.083 (4.026-4.201) |
| Closure service holdout | 83.774 (82.990-84.511) | 84.058 (82.836-86.400) | 0.741 (0.733-0.822) | 18.267 (17.955-18.492) | 8.395 (8.331-8.505) |
| Closure storage | 5.404 (5.305-5.486) | 5.551 (5.382-5.615) | 5.568 (5.498-5.643) | 1.603 (1.557-1.643) | 1.348 (1.295-1.422) |
| Declared-object lifecycle | 101.722 (100.061-102.560) | 5.556 (5.211-5.778) | 0.568 (0.564-0.588) | 20.505 (20.008-20.733) | 14.297 (14.016-14.897) |

### Physical x86-64

| Workload | Canonical | Typed | Default JIT | PHP no JIT | PHP tracing JIT |
| --- | ---: | ---: | ---: | ---: | ---: |
| Order corpus | 128.927 (122.159-131.866) | 67.878 (65.423-69.056) | 4.117 (4.079-4.169) | 98.663 (95.994-110.840) | 53.868 (52.674-54.747) |
| Typed order corpus | 151.904 (149.579-154.406) | 91.218 (89.688-92.291) | 4.162 (4.112-4.201) | 101.306 (99.740-105.061) | 55.532 (54.973-57.149) |
| Ledger corpus | 61.726 (61.239-63.568) | 26.081 (25.516-26.701) | 2.037 (2.017-2.077) | 38.131 (37.651-39.096) | 9.024 (8.881-9.199) |
| Typed ledger corpus | 62.957 (61.297-66.053) | 26.047 (25.856-26.291) | 2.044 (2.028-2.060) | 38.508 (37.610-39.645) | 9.017 (8.701-9.077) |
| Routing holdout | 209.251 (204.457-212.699) | 65.950 (65.363-66.515) | 7.264 (7.218-7.360) | 84.729 (83.639-86.066) | 26.118 (25.809-26.759) |
| Dynamic String read | 88.858 (86.878-89.590) | 87.879 (86.263-90.037) | 90.017 (89.077-90.628) | 12.244 (12.026-12.423) | 3.908 (3.480-4.366) |
| Dynamic String CV read | 95.514 (93.149-96.608) | 96.086 (92.322-96.833) | 95.480 (94.522-96.791) | 12.634 (12.439-13.182) | 3.957 (3.679-4.296) |
| Mixed String update | 206.241 (202.317-210.377) | 205.890 (203.192-211.033) | 213.687 (210.325-216.503) | 37.604 (37.004-38.022) | 12.758 (12.456-12.937) |
| Packed value `foreach` | 18.403 (18.152-18.703) | 18.540 (18.015-18.771) | 18.548 (18.084-18.896) | 2.297 (2.224-2.348) | 0.947 (0.915-0.967) |
| Hash value `foreach` | 18.594 (18.286-19.018) | 18.718 (18.454-18.905) | 18.613 (18.345-19.072) | 2.255 (2.203-2.294) | 1.011 (0.990-1.031) |
| String append | 3.633 (3.456-3.735) | 0.664 (0.637-0.692) | 0.643 (0.628-0.672) | 1.907 (1.844-1.927) | 1.459 (1.392-1.515) |
| Packed array build/read | 28.215 (27.375-28.979) | 5.480 (5.274-5.706) | 5.425 (5.269-5.610) | 8.622 (8.477-8.825) | 5.541 (5.249-5.633) |
| Closure copy/invoke | 56.398 (55.490-57.450) | 56.177 (55.463-56.828) | 0.399 (0.387-0.410) | 10.051 (9.789-10.274) | 4.774 (4.699-4.835) |
| Closure service holdout | 106.006 (104.939-108.443) | 105.857 (104.826-106.832) | 0.439 (0.410-0.448) | 23.409 (22.659-23.537) | 6.431 (6.268-6.489) |
| Closure storage | 7.676 (7.445-7.820) | 8.198 (7.876-8.339) | 7.896 (7.595-8.077) | 3.915 (3.729-4.101) | 3.213 (3.055-3.496) |
| Declared-object lifecycle | 118.137 (117.017-119.268) | 9.014 (8.814-9.189) | 1.063 (1.039-1.078) | 24.984 (24.362-26.336) | 18.170 (17.776-18.496) |

## Execution-weighted structure

The following complete-run JIT-lane counters are identical on ARM64 and
x86-64. Owner counts include cold runtime arrays; they are not allocated per
iteration unless explicitly stated.

| Workload | Array owners | Declared-object owners | Frame pushes | Optimized iterations | Native mappings | Dominant rejected backedges |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Order corpus | 9 | 4 | 9 | 499,967 | 1 | 0 |
| Typed order corpus | 9 | 4 | 9 | 499,967 | 1 | 0 |
| Ledger corpus | 8 | 2 | 7 | 499,967 | 1 | 0 |
| Typed ledger corpus | 8 | 2 | 7 | 499,967 | 1 | 0 |
| Routing holdout | 10 | 2 | 6 | 749,967 | 1 | 0 |
| Dynamic String read | 8 | 0 | 3 | 0 | 0 | 1,000,000 `array_shape` |
| Dynamic String CV read | 8 | 0 | 3 | 0 | 0 | 1,000,000 `array_shape` |
| Mixed String update | 8 | 1 | 4 | 0 | 0 | 1,000,000 `array_shape` |
| Packed value `foreach` | 8 | 0 | 3 | 499,967 | 0 | 500,000 `array_shape` |
| Hash value `foreach` | 8 | 0 | 4 | 0 | 0 | 500,001 `array_shape` |
| Closure copy/invoke | 7 | 0 | 70 | 249,967 | 1 | 0 |
| Closure service holdout | 7 | 1 | 71 | 749,967 | 1 | 0 |
| Declared-object lifecycle | 7 | 33 | 4 | 999,967 | 1 | 0 |

The order graph is already virtual in the canonical, typed and native lanes.
The ledger and routing arrays are created once after their hot loop; their
8-10 complete-process array owners are cold, not a cross-call allocation
stream. The closure holdout returns a scalar. The declared-object lifecycle
microbenchmark is a same-function create/read/discard shape already reduced
from 1,000,000 canonical owners to 33 typed/native owners. No non-order corpus
or independent holdout therefore exposes a new material DTO/small-array owner
cost across calls.

Layer 2 is rejected for this cycle. The result does not claim that general
virtual heap values lack value; it says the current representative workloads
do not pay the required new cost, so implementation would be benchmark-led
rather than evidence-led.

## Highest current execution gap

The current top gap is loss of dynamic String-key array-region admission in
the exact file-entry workloads:

- both independent read controls and the mixed method/update control reject
  one million backedges as `array_shape` on both hosts;
- all three execute zero optimized iterations and create zero native mappings;
- the mixed update is the largest row at 181.017 ms on ARM64 and 213.687 ms on
  x86-64; and
- the two read controls independently reproduce the same structural boundary
  at 67.813/72.497 ms on ARM64 and 90.017/95.480 ms on x86-64.

The focused in-memory native tests still encode the previously accepted
read-only and mutable String-key contracts. The benchmark sources are unchanged
from those accepted checkpoints, but the exact CLI/file-entry shapes are no
longer admitted on `be76152`. Historical candidate medians were approximately
1-2.5 ms; current medians are 52-85x higher. Those old numbers demonstrate
leverage and a likely coverage regression, not a valid current A/B claim.

A fresh ARM64 profile scaled only the mixed workload iteration bound. Of 2,347
top-of-stack samples, 885 (37.7%) were directly in canonical `execute_ex`.
Bitmap cleanup/update contributed 201 samples, with visible array COW/clone,
dynamic key conversion and String lookup work. No generated-code mapping was
present. The raw sample was removed after this path-only summary because it
contained local paths and executable addresses.

Value-only `foreach` is lower priority. It records roughly half the rejected
backedges and measures only 13.3-18.7 ms. The packed control also completes a
separate 499,967-iteration build loop, so its rejection count is not one
uniform uncovered program. Restore/generalize the higher-weight dynamic String
family first, then regenerate the scorecard before reconsidering `foreach`.

## Capability matrix

| General capability | Typed executor | ARM64 native | x86-64 native | Current evidence |
| --- | --- | --- | --- | --- |
| Scalar arithmetic, branches and exact calls | fresh | fresh | fresh | order, ledger, routing |
| Direct immutable closure/property calls | fresh | fresh | fresh | target plus independent service holdout |
| Existing order virtual aggregate | fresh | fresh | fresh | 9 arrays/4 objects, one mapping |
| Declared-object same-function virtualization | fresh | fresh | fresh | 1,000,000 to 33 owners |
| Dynamic String-key reads and mutable update | tree tests | **missing in file-entry scorecard** | **missing in file-entry scorecard** | three one-million-backedge `array_shape` rows |
| Value-only `foreach` | partial current coverage | missing for rejected iterator loop | missing for rejected iterator loop | lower-weight packed/hash controls |
| General non-order virtual heap values across calls | no selected graph | no selected graph | no selected graph | Layer 2 rejected this cycle |

## Resource and build envelope

Cold accepted scorecard builds took 39/41 seconds on ARM64 and 43/45 seconds
on x86-64 for typed/default-JIT respectively. Binary sizes were
4,574,128/4,823,328 bytes on ARM64 and 5,357,216/5,669,728 bytes on x86-64.
The default JIT therefore adds 249,200 bytes on ARM64 and 312,512 bytes on
x86-64 under these exact profiles.

The 48 RSS rows per host are one fresh-process diagnostic observation per RPHP
lane, not a distribution. For the selected dynamic String family, observations
range from 4.36-4.80 MiB on ARM64 and 6.32-6.83 MiB on x86-64. They show no
unbounded footprint event and are not used to claim a memory improvement.
Generated-code cache bytes, allocator calls and COW detachments remain
unmeasured rather than zero.

## Selected next checkpoint

The next goal is **evidence-bounded restoration and generalization of existing
dynamic String-key read/update region coverage**, not Layer 2 implementation.

1. Bisect the structural admission change between the accepted native
   String-read checkpoint (`a8a7ac2`) and integrated `be76152` using the exact
   file-entry controls, while retaining the focused no-`microtime` tests as a
   comparison.
2. Explain why CLI/file-entry read and mutable-update loops reach
   `array_shape` although their operation-level tests retain the established
   typed/native contract.
3. Add a file-entry regression that fails on the first lost general boundary;
   do not recognize benchmark names, variable names, literal keys or timing
   wrappers.
4. If restoration is semantically valid, require one common typed proof,
   exact canonical fallback/side exits, both native backends, one million
   admitted iterations and one native mapping for the target and both read
   holdouts.
5. Rerun the full dual-host scorecard and keep corpus/holdout regressions below
   one percent. If the rejection is required by newer PHP semantics, preserve
   canonical execution and select a different measured gap instead of weakening
   the guard.

This selection checkpoint intentionally does not implement that goal.

## Reproduction

On a clean checkout of `be76152`:

```sh
RPHP_SCORECARD_RUNS=15 RPHP_SCORECARD_COOLDOWN_SECONDS=60 \
    ./benches/run_execution_scorecard.sh
```

The x86 run must use a real Git checkout rather than a source-only archive,
because the runner records `git rev-parse HEAD` and rejects a dirty tree. Run
the complete source sync, build, scorecard, diagnostics and cleanup inside the
project's exclusive benchmark lock. Raw samples, host connectivity, local
paths and executable addresses stay outside the repository.
