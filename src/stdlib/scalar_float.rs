//! Scalar libm policy for the PHP inverse-hyperbolic functions.
//!
//! Rust does not promise the same rounding as the host C implementation. The
//! original differential cases also expose intermediate overflow at 1e308.
//! Keep the platform boundary here, with no runtime/value representation work.

#[cfg(unix)]
mod native {
    // SAFETY: C99 math.h declares each symbol as double function(double).
    // Rust f64 has that C ABI on the supported Unix targets. Every bit pattern
    // is a permitted input, including NaNs, infinities and out-of-domain values;
    // these return floating results and may set thread-local errno/fenv flags.
    // There are no pointers, ownership, allocation or lifetime obligations.
    #[link(name = "m")]
    unsafe extern "C" {
        pub(super) safe fn acosh(number: f64) -> f64;
        pub(super) safe fn asinh(number: f64) -> f64;
        pub(super) safe fn atanh(number: f64) -> f64;
    }
}

#[inline]
pub(super) fn acosh(number: f64) -> f64 {
    #[cfg(unix)]
    return native::acosh(number);
    // Non-Unix builds retain Rust's implementation; bit parity is unvalidated.
    #[cfg(not(unix))]
    number.acosh()
}

#[inline]
pub(super) fn asinh(number: f64) -> f64 {
    #[cfg(unix)]
    return native::asinh(number);
    #[cfg(not(unix))]
    number.asinh()
}

#[inline]
pub(super) fn atanh(number: f64) -> f64 {
    #[cfg(unix)]
    return native::atanh(number);
    #[cfg(not(unix))]
    number.atanh()
}
