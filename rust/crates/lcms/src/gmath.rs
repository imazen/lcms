//! `GmathExt` — `f64` methods over `lcms_sys::*` (real glibc libm). Rust's
//! `llvm.*` trig intrinsics resolve to `compiler_builtins`/`fdlibm` in the
//! binary (~1 ULP off glibc); `lcms-sys` calls a `cc` shim that keeps the
//! libm PLT call. Apply via `.g_cos()`/`.g_sin()`/`lcms_sys::sin_cos()`/...
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
        lcms_sys::cos(self)
    }
    #[inline]
    fn g_sin(self) -> f64 {
        lcms_sys::sin(self)
    }
    #[inline]
    fn g_tan(self) -> f64 {
        lcms_sys::tan(self)
    }
    #[inline]
    fn g_atan2(self, o: f64) -> f64 {
        lcms_sys::atan2(self, o)
    }
    #[inline]
    fn g_atan(self) -> f64 {
        lcms_sys::atan(self)
    }
    #[inline]
    fn g_asin(self) -> f64 {
        lcms_sys::asin(self)
    }
    #[inline]
    fn g_acos(self) -> f64 {
        lcms_sys::acos(self)
    }
    #[inline]
    fn g_hypot(self, o: f64) -> f64 {
        lcms_sys::hypot(self, o)
    }
    #[inline]
    fn g_fmod(self, o: f64) -> f64 {
        lcms_sys::fmod(self, o)
    }
    #[inline]
    fn g_cbrt(self) -> f64 {
        lcms_sys::cbrt(self)
    }
    #[inline]
    fn g_log2(self) -> f64 {
        lcms_sys::log2(self)
    }
    #[inline]
    fn g_exp2(self) -> f64 {
        lcms_sys::exp2(self)
    }
    #[inline]
    fn g_expm1(self) -> f64 {
        lcms_sys::expm1(self)
    }
    #[inline]
    fn g_log1p(self) -> f64 {
        lcms_sys::log1p(self)
    }
}
