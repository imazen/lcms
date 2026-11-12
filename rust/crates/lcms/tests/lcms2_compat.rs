//! lcms2-crate test suite, extracted with MIT credit — see `lcms2_crate/mod.rs`.
//! Gate for the `lcms2-api` shim: every test must compile+pass verbatim.
#![cfg(feature = "lcms2-api")]
pub use lcms::lcms2::*; // `crate::Profile` etc. in their tests resolve here
mod lcms2_crate;

// -- Send/Sync audit ---------------------------------------------------------
// The lcms2 crate `unsafe impl`s Send on its foreign-types handles (C's
// thread-safety contract). Our shim is `forbid(unsafe_code)` — the
// auto-traits must prove it themselves. This test is the audit:
// whatever compiles here IS Send+Sync; what's missing is documented.
mod sync_audit {
    use super::*;
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    #[test]
    fn shim_types_send_sync() {
        assert_send::<Profile>();
        assert_sync::<Profile>();
        assert_send::<Transform<[u8; 3], [u8; 3]>>();
        assert_sync::<Transform<[u8; 3], [u8; 3]>>();
        assert_send::<ToneCurve>();
        assert_sync::<ToneCurve>();
        assert_send::<MLU>();
        assert_sync::<MLU>();
        assert_send::<NamedColorList>();
        assert_sync::<NamedColorList>();
        assert_send::<Pipeline>();
        assert_sync::<Pipeline>();
        assert_send::<Dict>();
        assert_sync::<Dict>();
        assert_send::<CIECAM02>();
        assert_send::<ThreadContext>();
        assert_sync::<ThreadContext>();
    }
}

// -- shim-specific regressions (not part of the extracted suite) ---------------

/// `PixelFormat::*_DBL` are C's `TYPE_*_DBL` (8-byte doubles), `*_FLT` are
/// `f32` — the shim once encoded `_DBL` as 4-byte floats.
#[test]
fn dbl_and_flt_formats_carry_f64_and_f32() {
    let srgb = Profile::new_srgb();
    let lab = Profile::new_lab4();
    let t: Transform<[u8; 3], [f64; 3]> = Transform::new(
        &srgb,
        PixelFormat::RGB_8,
        &lab,
        PixelFormat::LAB_DBL,
        Intent::Perceptual,
    )
    .unwrap();
    let mut out = [[0f64; 3]; 1];
    t.transform_pixels(&[[255, 255, 255]], &mut out);
    assert!((out[0][0] - 100.0).abs() < 0.01, "white L* = 100: {out:?}");

    let t: Transform<[f32; 3], [f64; 3]> = Transform::new(
        &srgb,
        PixelFormat::RGB_FLT,
        &srgb,
        PixelFormat::RGB_DBL,
        Intent::Perceptual,
    )
    .unwrap();
    let mut out = [[0f64; 3]; 1];
    t.transform_pixels(&[[0.25f32, 0.5, 0.75]], &mut out);
    for (o, i) in out[0].iter().zip([0.25, 0.5, 0.75]) {
        assert!((o - i).abs() < 1e-3, "{out:?}");
    }
}
