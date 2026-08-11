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
