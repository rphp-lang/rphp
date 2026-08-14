# Guarded direct-call regions

Status: dual-host candidate verified; integrating-agent handoff pending

## Checkpoint contract

- **Outcome:** execute a stable immutable closure call inside an existing typed
  region without constructing and cleaning a canonical PHP frame on every
  iteration, while retaining exact canonical fallback semantics.
- **Baseline:** clean `main` commit
  `c646f3bb456f9fea05cc5a8549b1e195c039b636`.
- **Candidate:** `codex/perf-direct-call-regions` commit
  `501892c567603503d4103d57621d836c5e936b00` plus its preceding Layer 1
  property-call checkpoints.
- **Scope:** an exact positional by-value wrapper whose callable is either one
  immutable receiver property or a proven-dead local alias of the first public
  argument. The leaf closure has a stable user-function identity, no binding
  or called scope, a pure scalar Long body and at most eight public/capture
  inputs. Immutable by-value Long captures and String captures consumed by
  `strlen` are admitted.
- **Stop conditions:** references, by-reference captures/arguments, `$this`,
  changed scope or binding, generators, globals, statics, try regions,
  unsupported types/operations, live aliases, changed callable identity or a
  failed guard return to the original canonical instruction before mutation.
- **Regression ceiling:** one percent on representative order, ledger and
  routing controls in both default-JIT and typed-only lanes.

## Implementation

The compiler records a cold capture-aware scalar plan on the closure's
`UserFunction`. It describes public arguments and immutable capture inputs but
does not retain request-owned capture values. At region entry the runtime
guards the actual closure identity, scope, binding, capture layout, reference
state and types. It then substitutes immutable capture values into an ordinary
ephemeral `ScalarLongFunctionPlan`.

Both closure-property calls and public-argument wrapper calls use the existing
scalar call ABI and the same typed/native lowering as named functions and
monomorphic methods. There is no second closure executor. Ephemeral plans are
boxed for pointer stability and live for exactly one region invocation.

The public-argument slice admits one assignment of the form
`$copy = $callback` only when whole-function liveness proves that `$copy` has
no other read or write and is used immediately as the first argument of the
named wrapper call. The optimized region does not perform the redundant Rc
copy. Every entry failure or native side exit resumes at the original
assignment, so the canonical call protocol and exception/error ordering remain
the source of truth.

All new planning and execution state is compiled only when both `quick-loops`
and `jit-prototype` are enabled. The typed-only max-perf binary remains exactly
4,574,032 bytes, and its `run_quick_long_ops_loop` symbol remains at the exact
baseline address.

## ARM64 evidence

The physical host is Apple ARM64. Each timing row is 101 alternating A/B
observations after three warmups. Reported values are medians from RPHP's
in-workload timer; raw task-local TSVs are retained outside the repository.

| Workload | Baseline (s) | Candidate (s) | Delta |
| --- | ---: | ---: | ---: |
| Closure copy and invoke | 0.046490908 | 0.000320911 | -99.310% |
| Independent closure-property holdout | 0.098253965 | 0.000873089 | -99.111% |
| Typed order control | 0.131003857 | 0.130287170 | -0.547% |
| Default-JIT order control | 0.004256964 | 0.004252911 | -0.095% |
| Default-JIT ledger control | 0.002307177 | 0.002310038 | +0.124% |
| Default-JIT routing control | 0.006485939 | 0.006479979 | -0.092% |

The default-JIT max-perf binary grows from 4,805,040 to 4,839,744 bytes:
34,704 bytes or 0.722%. One admitted region consumes one bounded 16 KiB native
mapping and reports zero side exits on both target and holdout.

The separate `vm-stats` build records the structural change:

| Closure-copy counter | Baseline | Candidate |
| --- | ---: | ---: |
| Frame pushes | 500,004 | 70 |
| Frame cleanups | 500,004 | 70 |
| Cleanup slots scanned | 1,500,013 | 211 |
| Frame-slot writes | 1,250,008 | 173 |
| Heap-value frame-slot writes | 500,002 | 68 |
| Quick-region entries/completions | 0 / 0 | 1 / 1 |
| Quick iterations | 0 | 249,967 |
| Native executions / side exits | 0 / 0 | 1 / 0 |

The independent property holdout falls from 1,500,005 frame pushes, 750,013
cleanups, 3,750,024 scanned slots and 1,500,001 heap-value frame writes to
71, 46, 189 and 67 respectively. It completes 749,967 quick iterations in one
native execution with zero side exits.

## Correctness evidence

The focused suites cover empty and captured closure properties, immutable
Long and String captures, reference captures, public-argument wrappers, dead
and live aliases, changed callable identity, stale cache reuse and exact
overflow replay. A forced overflow resumes at the original property call or
alias assignment and raises the same canonical PHP error exactly once.

The final ARM64 matrix passed:

```text
cargo test --locked
cargo test --locked --no-default-features --quiet
cargo test --locked --features php-generics-erased --quiet
cargo test --locked --features php-generics-reified --quiet
cargo test --locked --all-features --quiet
cargo check --locked --all-features --all-targets
```

One pre-rebase all-features trial exhausted the shared filesystem after several
accumulated feature variants. The mandatory build cleanup removed only
generated workspace artifacts. The complete post-rebase matrix used the same
hook between configurations when the reserve fell low and passed. The
repository-wide unsafe-policy inventory passes at exactly 1,623 production
blocks and 289 unsafe functions, matching both ceilings. This checkpoint adds
no line containing a new `unsafe` block or function.

## x86-64 evidence

The exact baseline and candidate archives were checksum-validated on the
physical x86-64 host. One exclusive lock covered all six native release builds,
focused tests, instrumentation and timing. Each row below is again 101
alternating A/B observations after three warmups.

| Workload | Baseline (s) | Candidate (s) | Delta |
| --- | ---: | ---: | ---: |
| Closure copy and invoke | 0.056732655 | 0.000393629 | -99.306% |
| Independent closure-property holdout | 0.108528376 | 0.000428677 | -99.605% |
| Typed order control | 0.093195438 | 0.092185497 | -1.084% |
| Default-JIT order control | 0.004178524 | 0.004146814 | -0.759% |
| Default-JIT ledger control | 0.002067804 | 0.002055883 | -0.577% |
| Default-JIT routing control | 0.007446527 | 0.007411242 | -0.474% |

The default-JIT binary grows from 5,632,128 to 5,671,728 bytes: 39,600 bytes
or 0.703%. The typed-only candidate is 32 bytes smaller than its 5,359,376-byte
baseline. Its hot loop symbol moves by 16 bytes, so the independent typed A/B
distribution, rather than address equality, proves neutrality.

The x86-64 `vm-stats` counts exactly reproduce the ARM64 structural table for
both target and holdout. The focused canonical suite passes 3/3 and the native
suite passes 6/6, including forced side exits and reference-capture fallback.
All remote build cleanup ran while the exclusive lock was still held.

## Ownership and handoff

The implementation touches shared compiler/runtime files, including
`src/compiler/mod.rs`, `src/vm/function.rs`, `src/vm/planner.rs`,
`src/vm/quick.rs`, `src/vm/quick_long_region_plan.rs` and the quick/native
execution adapters. The integrating and Compatibility agents must account for
that overlap before merge. The performance branch remains the only branch this
agent may push; final merge order and the joint gate belong to the integrator.

Public-data hygiene is clean: no private hostname, address, username,
credential or raw remote log is stored in tracked content.
