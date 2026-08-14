# Default bounded native-JIT checkpoint

Status: verified on ARM64 and x86-64, 2026-08-14

This report records the production-admission checkpoint for the existing native
JIT. Ordinary supported builds now include the already-tested ARM64 and x86-64
backends. The typed VM remains the exact fallback, a runtime opt-out can reject
native compilation before code generation, and process-wide executable-memory
accounting bounds every live mapping.

## Contract and exact comparison

- **Outcome:** make the existing native JIT part of one ordinary build on
  macOS/AArch64 and Linux/x86-64 without changing PHP output, regressing the
  existing explicit-JIT corpus by more than one percent, or allowing
  unbounded executable mappings.
- **Integrated baseline:** clean `main` commit
  `28afd32aa999e74a91c90c08c7b818a4cd78cbd3`, built both as explicit
  typed-only and with `quick-loops,jit-prototype`.
- **Measured candidate:** clean commit
  `c52824261f657e66755d68f205c2a13b05c29d9e`. Its implementation patch has
  stable patch ID `4c92f804cb80ea6d1b7436aef68a5521758d6843`; pre-rebase commit
  `37ff6df8b0f4530f8b877943b2dfd53320f3ab28` carried the same patch.
- **Acceptance:** exact output in default, typed-only, runtime-disabled and
  zero-budget modes; the complete feature matrix; both native backends; no
  named throughput, startup or peak-RSS regression above one percent; and
  observable executable-memory counts that reconcile in every policy mode.
- **Stop rule:** reject any design that changes native semantics, executes a
  writable mapping, retains an unbounded mapping, adds a steady-state policy
  branch to hot VM dispatch, or turns opt-out/budget exhaustion into an error
  instead of exact typed fallback.

The compatibility checkpoints landed first. This patch was then rebased onto
the resulting `main`, and all correctness and performance evidence below was
regenerated from the exact commits above.

## Production policy and lifecycle

`Cargo.toml` now declares `quick-loops,jit-prototype` as the default feature
set. The benchmark scripts use an ordinary build for their JIT lane and spell
the control lane explicitly as:

```sh
cargo build --no-default-features --features quick-loops
```

The presence of `RPHP_DISABLE_JIT` rejects every native compilation entry
before assembler or machine-code generation. `RPHP_JIT_CODE_LIMIT_BYTES`
sets a process-wide live executable-mapping budget. The default is 16 MiB;
values above the 1 GiB hard cap are clamped, malformed values use the default,
and zero forces typed fallback. These are process-start policies cached once,
not environment reads in the steady-state dispatch loop.

Every backend reserves aligned mapping bytes atomically before `mmap`. A
reservation owns the live byte/count charge and releases it when the mapping
drops. The existing W^X lifecycle remains intact: code is emitted into a
writable, non-executable mapping, instruction-cache publication occurs where
the architecture requires it, and protection changes to read/execute before
entry. Mapping failure, disabled compilation and budget exhaustion all return
through the existing typed path.

With `vm-stats`, the process reports policy enabled state, configured limit,
live and peak mapping bytes/count, created mappings, and disabled, budget or
system-failure rejections. The ordinary runtime pays no telemetry cost when
that feature is absent.

The runtime opt-out is a safety and deployment escape hatch, not a promise of
typed-only build throughput. A workload that permanently wants no native tier
should use the explicit typed-only build above; it avoids carrying and probing
the native tier altogether.

## Benchmark method

Baseline and candidate were built from clean source with Rust 1.93.1,
`max-perf` (fat LTO and one codegen unit), and
`RUSTFLAGS=-Ctarget-cpu=native` in separate task-scoped targets. The measured
throughput comparison is the baseline's explicit JIT build versus the
candidate's ordinary default build. The typed-only baseline is used for the
startup and footprint context, not as the steady-state throughput opponent.

ARM64 used an Apple M4 with 24 GiB memory, Darwin 25.5/macOS 26.5.2 and PHP
8.5.9. X86-64 used an AMD Ryzen 9 7950X with 31,985,376 KiB memory, Linux
7.0.0-28, PHP 8.4.24, the `performance` governor and CPU 2 affinity.

Workload-internal monotonic time excludes parsing and process startup. Eight
alternating warm-up pairs precede 50 alternating measured pairs. ARM64 sums
ten internal executions per wrapper observation to reduce short-process
timer noise; the ledger gate invokes that wrapper twice. Displayed times are
normalized to one execution. X86-64 uses one execution for order/routing and
two for ledger. All valid observations are retained. Delta is the mean of the
two order-specific median ratios; p10-p90 uses the retained observations.

Startup is a separate trivial-program gate with 40 warm-up and 400 measured
alternating pairs. It measures fresh external process elapsed time. Peak RSS
rotates typed baseline, explicit-JIT baseline, candidate default and candidate
runtime-disabled order on each of 100 observations. No outliers are removed.
Semantic output for order, typed order, ledger, typed ledger and routing is
compared before timing in all five policy/build modes.

## Final throughput

Times are median milliseconds with p10-p90. Negative delta favors the
candidate.

| ARM64 workload | Pairs | Explicit-JIT baseline | Default candidate | Balanced delta |
| --- | ---: | ---: | ---: | ---: |
| Order corpus | 50 | 4.228 (4.182-4.372) | 4.229 (4.172-4.326) | -0.539% |
| Ledger corpus | 50 | 2.321 (2.304-2.349) | 2.328 (2.308-2.360) | +0.227% |
| Routing holdout | 50 | 6.568 (6.418-6.785) | 6.550 (6.433-6.721) | -0.368% |

| X86-64 workload | Pairs | Explicit-JIT baseline | Default candidate | Balanced delta |
| --- | ---: | ---: | ---: | ---: |
| Order corpus | 50 | 4.374 (4.301-4.436) | 4.386 (4.316-4.454) | -0.013% |
| Ledger corpus | 50 | 2.005 (1.987-2.045) | 2.005 (1.988-2.065) | +0.033% |
| Routing holdout | 50 | 7.735 (7.323-7.916) | 7.668 (7.336-7.886) | -0.800% |

The candidate therefore preserves more than 99% of the old explicit-JIT
throughput on every named target and control. This is a parity/admission
result, not a claim that RPHP is generally faster on arbitrary PHP programs.

## Startup, footprint and executable memory

Fresh-process startup remains inside the one-percent ceiling:

| Host | Typed baseline median (p10-p90) | Default candidate median (p10-p90) | Balanced delta |
| --- | ---: | ---: | ---: |
| ARM64 | 3.043 ms (2.733-3.833) | 3.034 ms (2.711-3.860) | -1.013% |
| X86-64 | 1.531 ms (1.513-1.587) | 1.542 ms (1.522-1.596) | +0.700% |

The 100-observation peak-RSS medians are:

| Host | Typed baseline | Explicit-JIT baseline | Default candidate | Disabled candidate | Candidate vs explicit JIT |
| --- | ---: | ---: | ---: | ---: | ---: |
| ARM64 | 5,324,800 B | 5,799,936 B | 5,718,016 B | 5,718,016 B | -1.412% |
| X86-64 | 7,190,528 B | 7,516,160 B | 7,528,448 B | 7,391,232 B | +0.163% |

The expected footprint relative to the old typed-only default is +7.38% on
ARM64 and +4.70% on x86-64 because the ordinary binary now carries the native
tier and the representative program creates executable code. The relevant
production comparison against the already-supported explicit JIT is within
the gate on both hosts.

The ordinary candidate binary is 4,716,480 bytes on ARM64, 1,024 bytes
(+0.022%) above the old explicit-JIT build and 214,528 bytes (+4.77%) above
typed-only. It is 5,538,104 bytes on x86-64, 4,744 bytes (+0.086%) above the
old explicit-JIT build and 272,800 bytes (+5.18%) above typed-only. Clean-build
elapsed observations were discarded as a decision metric because unrelated
host load made them non-repeatable; binary size and runtime evidence remained
stable.

Untimed policy telemetry reconciles as follows:

| Host/workload | Default peak mapping | Created | Disabled mode | Zero-budget mode | System failures |
| --- | ---: | ---: | ---: | ---: | ---: |
| ARM64 order | 16,384 B / 1 | 1 | 0 mappings / 1 rejection | 0 mappings / 1 rejection | 0 |
| ARM64 ledger | 16,384 B / 1 | 1 | 0 mappings / 2 rejections | 0 mappings / 2 rejections | 0 |
| ARM64 routing | 16,384 B / 1 | 1 | 0 mappings / 1 rejection | 0 mappings / 1 rejection | 0 |
| X86-64 order | 8,192 B / 1 | 1 | 0 mappings / 1 rejection | 0 mappings / 1 rejection | 0 |
| X86-64 ledger | 4,096 B / 1 | 1 | 0 mappings / 2 rejections | 0 mappings / 2 rejections | 0 |
| X86-64 routing | 8,192 B / 1 | 1 | 0 mappings / 1 rejection | 0 mappings / 1 rejection | 0 |

All default observations use the 16,777,216-byte policy limit. Disabled and
zero-budget runs create no executable mapping and preserve the exact semantic
output. No system mapping failure occurred.

## Correctness gates and rejected variants

The final commit passes formatting, shell syntax, unsafe-policy enforcement,
all-target checks in default/all-feature/no-default/typed-only forms, and the
default, no-default, erased-generics, reified-generics and all-features test
matrix on ARM64. The first sandboxed all-features attempt denied eight
permanent loopback coroutine socket tests; the identical run with loopback
socket permission passed. X86-64 passes the complete all-features suite,
including 34 native backend tests and the subprocess runtime-policy test.
Unsafe inventory is 1,618 production unsafe blocks against the 1,623 ceiling,
289 unsafe functions against the 289 ceiling, and 147 SAFETY annotations.

Two admission designs were measured and fully removed:

- rejecting disabled JIT only at executable mapping time still paid assembler
  and code-generation work, moving short disabled-process controls by roughly
  3-8%; the accepted design rejects at every native compilation entry; and
- checking policy in hot VM executor dispatch narrowed one disabled control
  but regressed an independent ARM64 routing-JIT run by 3.03%; the accepted
  design has no new steady-state executor branch.

The runtime opt-out retained a noisy roughly +1.6% disabled short-process
control during investigation even after early admission. It remains the exact
safety fallback. The explicit typed-only build is the supported choice when
maximum no-JIT throughput is the objective.

## Reproduction

Build the old explicit-JIT baseline and ordinary candidate from the exact
commits above, then use supplied binaries with the runtime gate:

```sh
RPHP_RUNTIME_GATE_PAIRS=50 \
RPHP_RUNTIME_GATE_WARMUPS=8 \
RPHP_RUNTIME_GATE_ONLY=corpus_order_pipeline.php \
RPHP_RUNTIME_GATE_CANDIDATE_BINARY=CANDIDATE_BINARY \
RPHP_RUNTIME_GATE_BASELINE_BINARY=BASELINE_JIT_BINARY \
benches/run_runtime_gate.sh CANDIDATE_ROOT BASELINE_ROOT
```

Use `corpus_ledger_pipeline.php` and `holdout_routing_pipeline.php` for the
other two gates. Pass an isolated CPU number as the third argument on Linux.
Set `RPHP_VM_STATS=1` on a `vm-stats` build to reproduce mapping counters, and
run `scripts/cleanup-builds.sh` before and after the complete cycle.
