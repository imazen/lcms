//! Safe wrappers over a compile-time-selectable math provider.
//!
//! Three providers, picked by Cargo feature (default `math-sys`):
//!
//! - **`math-sys`** — a tiny `cc`-built C shim whose `cos`/`sin`/`pow`/...
//!   calls stay PLT calls into the *local* system libm (glibc on GNU,
//!   Apple libm, MSVCRT, musl). This is the oracle-parity provider: the
//!   C reference resolves the same symbols on the same box.
//! - **`math-rust`** — pure `f64` methods (compiler_builtins/fdlibm).
//!   Deterministic across hosts on a given toolchain, but not oracle-exact.
//! - **`math-zen`** — reserved for a correctly-rounded `zenmath` impl.
//!   Selected by feature; no API surface yet.
//!
#![forbid(unsafe_code)]

//! Rust's `f64` intrinsics (`llvm.cos`, `llvm.sin`, `llvm.pow`, ...) can
//! resolve to `compiler_builtins`/fdlibm in the final binary, which differs
//! from the system libm by ~1 ULP on some inputs — the shim keeps the
//! call bound to the same libm the C oracle uses.

// --- provider impls — one `mod imp` per feature -------------------------

#[cfg(all(not(feature = "math-libm"), feature = "math-rust"))]
mod imp {
    #[inline] pub fn cos(x: f64) -> f64 { x.cos() }
    #[inline] pub fn sin(x: f64) -> f64 { x.sin() }
    #[inline] pub fn tan(x: f64) -> f64 { x.tan() }
    #[inline] pub fn atan2(y: f64, x: f64) -> f64 { y.atan2(x) }
    #[inline] pub fn atan(x: f64) -> f64 { x.atan() }
    #[inline] pub fn asin(x: f64) -> f64 { x.asin() }
    #[inline] pub fn acos(x: f64) -> f64 { x.acos() }
    #[inline] pub fn hypot(x: f64, y: f64) -> f64 { x.hypot(y) }
    #[inline] pub fn fmod(x: f64, y: f64) -> f64 { x % y }
    #[inline] pub fn cbrt(x: f64) -> f64 { x.cbrt() }
    #[inline] pub fn log2(x: f64) -> f64 { x.log2() }
    #[inline] pub fn exp2(x: f64) -> f64 { x.exp2() }
    #[inline] pub fn expm1(x: f64) -> f64 { x.exp_m1() }
    #[inline] pub fn log1p(x: f64) -> f64 { x.ln_1p() }
    #[inline] pub fn pow(x: f64, y: f64) -> f64 { x.powf(y) }
    #[inline] pub fn exp(x: f64) -> f64 { x.exp() }
    #[inline] pub fn ln(x: f64) -> f64 { x.ln() }
    #[inline] pub fn log10(x: f64) -> f64 { x.log10() }
    #[inline] pub fn atanh(x: f64) -> f64 { x.atanh() }
    #[inline] pub fn acosh(x: f64) -> f64 { x.acosh() }
    /// `sin_cos` — `f64::sin_cos` is the fused libcall when the stdlib
    /// provides it; falls back to separate sin/cos otherwise.
    #[inline]
    pub fn sin_cos(x: f64) -> (f64, f64) { x.sin_cos() }
}

/// `math-libm` — the pure-Rust `libm` crate: C99 math with no libm
/// dependency. Deterministic on every platform by construction — the
/// same input produces the same bits everywhere. Diverges from the
/// system-libm oracle wherever libm's approximation differs (the
/// correctly-rounded glibc fns on 2.39+ mostly match; musl/fdlibm and
/// MSVCRT diverge more).
#[cfg(all(not(feature = "math-rust"), feature = "math-libm"))]
mod imp {
    #[inline] pub fn cos(x: f64) -> f64 { libm::cos(x) }
    #[inline] pub fn sin(x: f64) -> f64 { libm::sin(x) }
    #[inline] pub fn tan(x: f64) -> f64 { libm::tan(x) }
    #[inline] pub fn atan2(y: f64, x: f64) -> f64 { libm::atan2(y, x) }
    #[inline] pub fn atan(x: f64) -> f64 { libm::atan(x) }
    #[inline] pub fn asin(x: f64) -> f64 { libm::asin(x) }
    #[inline] pub fn acos(x: f64) -> f64 { libm::acos(x) }
    #[inline] pub fn hypot(x: f64, y: f64) -> f64 { libm::hypot(x, y) }
    #[inline] pub fn fmod(x: f64, y: f64) -> f64 { libm::fmod(x, y) }
    #[inline] pub fn cbrt(x: f64) -> f64 { libm::cbrt(x) }
    #[inline] pub fn log2(x: f64) -> f64 { libm::log2(x) }
    #[inline] pub fn exp2(x: f64) -> f64 { libm::exp2(x) }
    #[inline] pub fn expm1(x: f64) -> f64 { libm::expm1(x) }
    #[inline] pub fn log1p(x: f64) -> f64 { libm::log1p(x) }
    #[inline] pub fn pow(x: f64, y: f64) -> f64 { libm::pow(x, y) }
    #[inline] pub fn exp(x: f64) -> f64 { libm::exp(x) }
    #[inline] pub fn ln(x: f64) -> f64 { libm::log(x) }
    #[inline] pub fn log10(x: f64) -> f64 { libm::log10(x) }
    #[inline] pub fn atanh(x: f64) -> f64 { libm::atanh(x) }
    #[inline] pub fn acosh(x: f64) -> f64 { libm::acosh(x) }
    /// `sincos` — libm computes `sin`/`cos` independently (no fused
    /// kernel), matching neither GCC's merged `sincos` codegen nor
    /// Apple's `__sincos` — but deterministic.
    #[inline]
    pub fn sin_cos(x: f64) -> (f64, f64) { libm::sincos(x) }
}

#[cfg(all(
    not(feature = "math-rust"),
    not(feature = "math-libm"),
    feature = "math-zen"
))]
mod imp {
    // `zenmath` provider — correctly-rounded implementations, intended to
    // be bit-identical across platforms AND to the oracle's libm where
    // glibc is already correctly rounded. Not yet wired to a crate;
    // falls back to the pure-Rust impl until zenmath lands.
    #[inline] pub fn cos(x: f64) -> f64 { x.cos() }
    #[inline] pub fn sin(x: f64) -> f64 { x.sin() }
    #[inline] pub fn tan(x: f64) -> f64 { x.tan() }
    #[inline] pub fn atan2(y: f64, x: f64) -> f64 { y.atan2(x) }
    #[inline] pub fn atan(x: f64) -> f64 { x.atan() }
    #[inline] pub fn asin(x: f64) -> f64 { x.asin() }
    #[inline] pub fn acos(x: f64) -> f64 { x.acos() }
    #[inline] pub fn hypot(x: f64, y: f64) -> f64 { x.hypot(y) }
    #[inline] pub fn fmod(x: f64, y: f64) -> f64 { x % y }
    #[inline] pub fn cbrt(x: f64) -> f64 { x.cbrt() }
    #[inline] pub fn log2(x: f64) -> f64 { x.log2() }
    #[inline] pub fn exp2(x: f64) -> f64 { x.exp2() }
    #[inline] pub fn expm1(x: f64) -> f64 { x.exp_m1() }
    #[inline] pub fn log1p(x: f64) -> f64 { x.ln_1p() }
    #[inline] pub fn pow(x: f64, y: f64) -> f64 { x.powf(y) }
    #[inline] pub fn exp(x: f64) -> f64 { x.exp() }
    #[inline] pub fn ln(x: f64) -> f64 { x.ln() }
    #[inline] pub fn log10(x: f64) -> f64 { x.log10() }
    #[inline] pub fn atanh(x: f64) -> f64 { x.atanh() }
    #[inline] pub fn acosh(x: f64) -> f64 { x.acosh() }
    #[inline]
    pub fn sin_cos(x: f64) -> (f64, f64) { x.sin_cos() }
}

#[cfg(not(any(
    feature = "math-rust",
    feature = "math-libm",
    feature = "math-zen"
)))]
compile_error!("lcms: select a math provider: math-rust | math-libm | math-zen");

pub(crate) use imp::*;
