# Unsafe-code policy

RPHP permits unsafe Rust where it is required for the VM representation,
manually managed frames, FFI/platform APIs, or native-code execution. Unsafe
is an implementation boundary that needs explicit proof, not a performance
annotation.

## Requirements for new or changed unsafe code

- Minimize the unsafe region and expose a safe interface when a local invariant
  can be enforced once.
- Add a `// SAFETY:` comment immediately around each non-obvious unsafe
  operation. State the concrete validity, alignment, initialization, aliasing,
  lifetime, ownership, thread, and unwind facts that make it sound.
- Every `unsafe fn` must have a `# Safety` section describing obligations the
  caller must uphold. Call sites must make those obligations reviewable.
- Prefer typed pointers, `NonNull`, checked offsets, and explicit owner types
  over integer/pointer round trips or unbounded raw arithmetic.
- Do not create two live mutable references to overlapping storage. A raw
  pointer does not relax Rust's reference aliasing rules.
- Initialize storage before producing a typed reference and drop each owned
  value exactly once. Deoptimization and exception paths must preserve the
  same rule as normal completion.
- Any executable-memory transition must keep writable and executable phases
  separate, flush instruction caches where required, and use the documented
  platform ABI.
- Unsafe hot-path changes require focused success, guard-failure, side-exit,
  cleanup, and repeated-execution tests on every affected architecture.

The crate currently allows `unsafe_op_in_unsafe_fn`; this is legacy debt, not
permission to omit local unsafe blocks or safety reasoning in new code. Avoid
expanding the allow-list. Reducing it should be done in reviewable slices.

## Legacy baseline and enforcement

The enforced production-source baseline, measured at commit `5b3ee92` over
`src/**/*.rs`, contains 1,630 explicit `unsafe` blocks, 289 declared unsafe
functions (including unsafe extern functions), 54 `SAFETY:` annotations, and no
`# Safety` sections. The audit started from 1,669 blocks and centralized 39
repeated property-cache, class-constant, object-identity, generic-call, and
static-property boundaries. This is four blocks below the earlier `d4c4965`
inventory even after the typed-property and class-constant capabilities added
since that checkpoint. These numbers describe debt; they are not an accepted
soundness level. Test-only Rust is reported separately and does not provide a
budget for production code.

Run the policy ratchet before committing a change that touches unsafe code:

```sh
scripts/check-unsafe-policy.sh --diff-base HEAD
```

CI runs the same check against the pull-request base. It rejects an increase
above the committed production baseline, newly added unsafe blocks without a
nearby added `// SAFETY:` proof, and newly added unsafe functions without an
added `# Safety` contract. An intentional baseline increase requires a
separately reviewable update to `scripts/unsafe-baseline.env` and an explicit
security rationale in the pull request. Moving unsafe between files or adding
boilerplate comments is not a valid rationale.

The checker is a review ratchet, not a Rust soundness proof. It cannot decide
whether a comment is true, whether one proof covers the correct operations, or
whether a changed safe line invalidates an existing invariant. Reviewers must
still inspect the complete unsafe region and its callers. Generated or
mechanically repeated comments that do not name concrete invariants should be
rejected.

Legacy cleanup proceeds by subsystem rather than by comment count:

1. Write representation and lifetime contracts for values, frames, stacks,
   instruction pointers, executable memory, coroutine suspension, and side
   exits.
2. Introduce narrow safe owner/pointer wrappers where one checked constructor
   can enforce an invariant for many hot-path uses.
3. Audit the baseline dispatcher and hot executor in bounded slices, then the
   object/call, coroutine/resource, and JIT backends. Do not land a bulk patch
   that merely prefixes hundreds of operations with identical comments.
4. Enable `deny(unsafe_op_in_unsafe_fn)` module by module after each module's
   caller obligations are documented, then remove the crate-wide allow.
5. Add Miri, sanitizer, fuzz, stress, ABI, side-exit, and cross-architecture
   checks where the represented operation can be exercised by that tool.

The debt is closed only when every remaining unsafe function has a concrete
`# Safety` contract, every remaining unsafe operation or tightly related group
has an adjacent proof, the crate-wide allow is gone, and unsafe implementation
details have been reduced or centralized behind reviewed interfaces.

## Core invariant areas

### Values and containers

The tag determines the initialized union field and ownership behavior. Heap
values must preserve reference counts, copy-on-write separation, weak identity,
and exactly-once cleanup across assignment, calls, exceptions, and side exits.

### Frames and execution

Frame allocation must provide correct size and alignment for the header and all
slots. Frame links, function pointers, instruction pointers, result slots, and
heap bitmaps must remain valid for their documented lifetime. Suspension or
deferred calls may not leave references into movable/reallocated storage.

### JIT

Generated code may access only the state admitted and guarded by its plan. It
must preserve callee-saved registers, stack alignment, calling convention,
return/side-exit encoding, and every committed VM mutation. A failed guard
must leave the baseline executor a precise, valid resume state.

## Review and verification

Pull requests that change unsafe code must list the invariants touched and the
tests/targets used. Reviewers should reason about failure and unwind paths, not
only the successful hot path. Miri, sanitizers, platform debuggers, and stress
tests are encouraged where the involved representation supports them, but
tool success never replaces the invariant proof.
