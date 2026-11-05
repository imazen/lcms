//! Safe wrappers over real glibc libm functions.
//!
//! Rust's `f64` math intrinsics (`llvm.cos`, `llvm.sin`, `llvm.sincos`,
//! `llvm.atan2`, `llvm.tan`, ...) resolve to `compiler_builtins`/`fdlibm`
//! in the final binary, which differs from the system glibc `libm` by
//! roughly one ULP on some inputs. For bit-exact parity with C code that
//! calls `cos`/`sin`/`sincos`/... these wrappers call a tiny `cc`-built
//! shim whose libm calls resolve to the real glibc implementation.

unsafe extern "C" {
    #[link_name = "lcms_g_cos"] fn g_cos(x: f64) -> f64;
    #[link_name = "lcms_g_sin"] fn g_sin(x: f64) -> f64;
    #[link_name = "lcms_g_tan"] fn g_tan(x: f64) -> f64;
    #[link_name = "lcms_g_atan2"] fn g_atan2(y: f64, x: f64) -> f64;
    #[link_name = "lcms_g_atan"] fn g_atan(x: f64) -> f64;
    #[link_name = "lcms_g_asin"] fn g_asin(x: f64) -> f64;
    #[link_name = "lcms_g_acos"] fn g_acos(x: f64) -> f64;
    #[link_name = "lcms_g_hypot"] fn g_hypot(x: f64, y: f64) -> f64;
    #[link_name = "lcms_g_fmod"] fn g_fmod(x: f64, y: f64) -> f64;
    #[link_name = "lcms_g_cbrt"] fn g_cbrt(x: f64) -> f64;
    #[link_name = "lcms_g_log2"] fn g_log2(x: f64) -> f64;
    #[link_name = "lcms_g_exp2"] fn g_exp2(x: f64) -> f64;
    #[link_name = "lcms_g_expm1"] fn g_expm1(x: f64) -> f64;
    #[link_name = "lcms_g_log1p"] fn g_log1p(x: f64) -> f64;
    #[link_name = "lcms_g_sincos"] fn g_sincos(x: f64, s: *mut f64, c: *mut f64);
}

#[inline]
pub fn cos(x: f64) -> f64 {
    unsafe { g_cos(x) }
}
#[inline]
pub fn sin(x: f64) -> f64 {
    unsafe { g_sin(x) }
}
#[inline]
pub fn tan(x: f64) -> f64 {
    unsafe { g_tan(x) }
}
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    unsafe { g_atan2(y, x) }
}
#[inline]
pub fn atan(x: f64) -> f64 {
    unsafe { g_atan(x) }
}
#[inline]
pub fn asin(x: f64) -> f64 {
    unsafe { g_asin(x) }
}
#[inline]
pub fn acos(x: f64) -> f64 {
    unsafe { g_acos(x) }
}
#[inline]
pub fn hypot(x: f64, y: f64) -> f64 {
    unsafe { g_hypot(x, y) }
}
#[inline]
pub fn fmod(x: f64, y: f64) -> f64 {
    unsafe { g_fmod(x, y) }
}
#[inline]
pub fn cbrt(x: f64) -> f64 {
    unsafe { g_cbrt(x) }
}
#[inline]
pub fn log2(x: f64) -> f64 {
    unsafe { g_log2(x) }
}
#[inline]
pub fn exp2(x: f64) -> f64 {
    unsafe { g_exp2(x) }
}
#[inline]
pub fn expm1(x: f64) -> f64 {
    unsafe { g_expm1(x) }
}
#[inline]
pub fn log1p(x: f64) -> f64 {
    unsafe { g_log1p(x) }
}
/// `sincos` — glibc computes `sin`/`cos` in one libcall (matches C's
/// GCC-merged `cos(h)`/`sin(h)`). Returns `(sin, cos)`.
#[inline]
pub fn sin_cos(x: f64) -> (f64, f64) {
    let (mut s, mut c) = (0.0, 0.0);
    unsafe { g_sincos(x, &mut s, &mut c) };
    (s, c)
}
