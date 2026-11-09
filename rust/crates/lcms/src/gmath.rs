//! `GmathExt` — `f64` methods over `crate::math::*` (real glibc libm). Rust's
//! `llvm.*` trig intrinsics resolve to `compiler_builtins`/`fdlibm` in the
//! binary (~1 ULP off glibc); `the lcms math module` calls a `cc` shim that keeps the
//! libm PLT call. Apply via `.g_cos()`/`.g_sin()`/`crate::math::sin_cos()`/...
//! anywhere C calls `cos`/`sin`/`sincos`/`atan2`/`tan`/`asin`/`acos`/`atan`.

pub(crate) trait GmathExt {
    fn g_cos(self) -> f64;
    fn g_sin(self) -> f64;
    fn g_tan(self) -> f64;
    fn g_atan2(self, o: f64) -> f64;
    fn g_atan(self) -> f64;
    fn g_asin(self) -> f64;
    fn g_acos(self) -> f64;
    fn g_hypot(self, o: f64) -> f64;
    fn g_fmod(self, o: f64) -> f64;
    fn g_cbrt(self) -> f64;
    fn g_log2(self) -> f64;
    fn g_exp2(self) -> f64;
    fn g_expm1(self) -> f64;
    fn g_log1p(self) -> f64;
}

impl GmathExt for f64 {
    #[inline]
    fn g_cos(self) -> f64 {
        crate::math::cos(self)
    }
    #[inline]
    fn g_sin(self) -> f64 {
        crate::math::sin(self)
    }
    #[inline]
    fn g_tan(self) -> f64 {
        crate::math::tan(self)
    }
    #[inline]
    fn g_atan2(self, o: f64) -> f64 {
        crate::math::atan2(self, o)
    }
    #[inline]
    fn g_atan(self) -> f64 {
        crate::math::atan(self)
    }
    #[inline]
    fn g_asin(self) -> f64 {
        crate::math::asin(self)
    }
    #[inline]
    fn g_acos(self) -> f64 {
        crate::math::acos(self)
    }
    #[inline]
    fn g_hypot(self, o: f64) -> f64 {
        crate::math::hypot(self, o)
    }
    #[inline]
    fn g_fmod(self, o: f64) -> f64 {
        crate::math::fmod(self, o)
    }
    #[inline]
    fn g_cbrt(self) -> f64 {
        crate::math::cbrt(self)
    }
    #[inline]
    fn g_log2(self) -> f64 {
        crate::math::log2(self)
    }
    #[inline]
    fn g_exp2(self) -> f64 {
        crate::math::exp2(self)
    }
    #[inline]
    fn g_expm1(self) -> f64 {
        crate::math::expm1(self)
    }
    #[inline]
    fn g_log1p(self) -> f64 {
        crate::math::log1p(self)
    }
}
