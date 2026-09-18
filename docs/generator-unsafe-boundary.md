# Generator finalization unsafe boundary

## Scope

Commit `35ddb85a` initially added thirteen net production `unsafe` blocks while
keeping the unsafe-function count at 289. They were confined to the existing
VM frame ABI in `baseline_dispatch.rs`, `baseline_entry.rs`,
`baseline_iteration.rs` and `baseline_object_calls.rs`. The follow-up audit
replaces repeated suspension snapshot blocks with one typed helper and removes
raw instruction-pointer subtraction from generator diagnostics. The resulting
inventory returns to the canonical 1,626 blocks / 289 functions; the
`frontend-expression-boundaries` checkpoint itself adds no unsafe operation.

## Ownership invariants

- Return/finally projection reads an operand or return slot only while the
  current `ExecuteData`, its immutable `OpArray` and compiler-sized operands
  are live.
- Detached generator restoration allocates a compiler-sized VM frame before
  restoring CV/TMP slots. The stored function pointer belongs to the retained
  generator, is immutable for that lifetime and is checked as a user function
  before conversion to `UserFunction`.
- Temporary trace frames use the same retained function and snapshot layout,
  are never executed, and are cleaned in strict reverse VM-stack order.
- Yield diagnostics calculate instruction offsets only between an instruction
  pointer and the owning live `OpArray` instruction allocation. Pending-finally
  fields are read only from the active generator frame.
- Closure generator detection converts a function pointer only after its
  `FunctionType::User` tag is observed; the callable retains the closure and
  function allocation through frame initialization.

No block publishes a raw pointer, extends a pointee lifetime or changes the
layout of `ExecuteData`, `Instruction`, `Value` or a public ABI. The shared
snapshot helper owns the single raw-frame proof for CV/TMP bounds, opline
membership and the pending-finally flag, while preserving vector capacity.
Existing SAFETY comments at frame allocation, slot restoration and cleanup
sites state the remaining local caller obligations. No unsafe ceiling increase
is required.
