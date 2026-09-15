# Scalar floating-point boundary

The `acosh`, `asinh` and `atanh` handlers use the host Unix C99 libm functions.
Original AMD64 PHP 8.5.10 differential specimens rejected the first Rust-only
implementation: finite `acosh(1e308)` and `asinh(1e308)` became infinity,
`acosh(1 + 2^-52)` lost precision, negative tiny `atanh` differed by one bit,
and domain NaN signs differed. Expectations were not weakened. `expm1` and
`log1p` retain the Rust operations that pass the original bit-level specimens.

## Safety and linking

`src/stdlib/scalar_float.rs` adds one explicitly reviewed unsafe external
declaration block, containing three **safe** C functions. Each signature is
`double function(double)`, verified against the host `math.h` declarations and
exported symbols. Every floating bit pattern is permitted. Domain errors return
NaN/infinity and can update thread-local errno or floating exception flags;
there are no raw pointers, callbacks, allocations or ownership obligations.
No native result is borrowed, and the wrapper introduces no mutable state.

This is a real native trust boundary despite safe call sites. It does not add
an `unsafe {}` block or unsafe Rust function, change the inventory ceiling,
or remove unrelated unsafe code to create room. The declaration and signatures
must be reviewed independently of those aggregate counts. Rust's
[external-block rules](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-extern.html)
permit safe scalar math declarations with precisely these caller obligations.

Unix builds link the system `m` library; no PHP implementation code is copied
or loaded. The dependency is already present in the tested runtime and PHP
executables. Its implementation and rounding remain platform prerequisites,
not a portable software-math or independent security claim. Non-Unix builds
retain Rust's fallback; exact parity there, other libm versions, non-default
rounding/trap modes, 32-bit behavior and OOM equivalence are unvalidated.

Acceptance requires the original finite/domain/NaN/INF/signed-zero specimens,
callable and reentrant-error tests, exact upstream targets and previous pass
sets, feature/all-target matrix, native-symbol inspection, unsafe review and
the fixed-anchor performance gate on the final immutable binary.
