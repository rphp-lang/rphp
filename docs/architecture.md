# Architecture

RPHP is an independent PHP-compatible runtime. It consumes PHP source and
owns the complete execution pipeline instead of embedding an existing PHP
runtime or preserving its extension ABI.

```text
PHP source
  -> lexer
  -> parser and RPHP AST
  -> bytecode compiler
  -> authoritative baseline VM
  -> optional guarded quick/native execution
  -> exact side exit back to baseline bytecode
```

## Semantic authority

Baseline bytecode is the source of truth. Optimized regions are disposable
caches: removing them must leave a correct program. A guard runs before the
operation it protects mutates observable state, and a failed guard resumes at
an exact bytecode position without repeating a completed side effect.

The compiler records declarations, literals, instructions, control-flow
metadata, and side tables in an `OpArray`. The VM uses compact tagged values,
explicit frames, copy-on-write containers, inline caches, and executor-owned
global state. The implementation is single-process and currently optimized
around a single-threaded value/runtime contract.

## Execution tiers

1. **Baseline VM.** General supported semantics and fallback behavior.
2. **Quick regions.** Typed, predecoded plans for proven hot program shapes.
3. **Native regions.** On macOS/AArch64 and Linux/x86-64, the default build can
   lower selected quick plans to machine code after their hotness threshold.

Native code is never the only implementation of an operation. Type changes,
overflow, references, copy-on-write, exceptions, dynamic dispatch, or another
failed assumption must side-exit before behavior diverges.

## Feature boundaries

Non-default Cargo features keep experimental or incomplete surfaces explicit.
Coroutines, VM statistics, generic runtime modes, streams, file operations, and
include-path support can be compiled separately. The native JIT is part of the
default build on supported targets, can be forced off at runtime with
`RPHP_DISABLE_JIT`, and can be compiled out through the no-default feature
matrix. Default, typed-only and no-default builds remain first-class CI
configurations.

Generic declarations share an interned metadata graph. Bound-erased and
reified builds select different runtime capabilities without widening the
ordinary `Value` or call frame. Coroutines use structured lexical ownership
and cooperative scheduling; they are not OS threads and do not make values
thread-safe.

## Memory and unsafe code

The VM and JIT use raw pointers, tagged unions, manually managed frame storage,
executable memory, and platform ABIs. Generated code is published through one
W^X mapping boundary and live RX mappings have a process-wide bounded budget;
allocation or budget failure keeps the typed executor authoritative. Safety
depends on documented layout, ownership, lifetime, mutation, and side-exit
invariants. See the [unsafe-code policy](unsafe-policy.md). Pre-alpha RPHP is
not a security boundary and must not execute untrusted code.

## Detailed direction

The [active roadmap](roadmap.md) coordinates the separate
[compatibility](roadmap-compatibility.md) and
[execution/performance](roadmap-execution-performance.md) workstreams. The
[runtime architecture log](roadmap-runtime-architecture.md) and the older
[combined performance/JIT/compatibility log](roadmap-performance-jit-compatibility.md)
retain source-boundary decisions, rejected candidates, accepted checkpoints and
measurement history. These documents are engineering direction, not promises
of a stable public API or release schedule.
