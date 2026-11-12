//! End-to-end transform parity: C cmsCreateTransform+DoTransform vs
//! Rust create_extended_transform+do_transform, byte-identical output.

use lcms_cref as c;

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_BGR_8: u32 = 4 << 16 | 3 << 3 | 1 | 1 << 10;
const TYPE_RGBA_8: u32 = 4 << 16 | 1 << 7 | 3 << 3 | 1;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;
const TYPE_CMYK_8: u32 = 6 << 16 | 4 << 3 | 1;
const INTENT_PERCEPTUAL: u32 = 0;

#[test]
fn srgb_identity_transform() {
    let c_out = {
        let prof = unsafe { c::cmsCreate_sRGBProfile() };
        assert!(!prof.is_null());
        let t = unsafe {
            c::cmsCreateTransform(prof, TYPE_RGB_8, prof, TYPE_RGB_8, INTENT_PERCEPTUAL, 0)
        };
        assert!(!t.is_null(), "C transform failed");
        let inp: Vec<u8> = (0..255u8).collect();
        let mut out = vec![0u8; inp.len()];
        unsafe { c::cmsDoTransform(t, inp.as_ptr() as _, out.as_mut_ptr() as _, 85) };
        unsafe {
            c::cmsDeleteTransform(t);
            c::cmsCloseProfile(prof);
        }
        out
    };
    // rust side: build two sRGB profiles and a transform
    let p1 = lcms::virt::create_srgb_profile(None).expect("srgb");
    let p2 = lcms::virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0u32];
    let adapt = [1.0f64, 1.0];
    let mut t = lcms::xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, TYPE_RGB_8, TYPE_RGB_8, 0,
    )
    .expect("rust transform");
    let inp: Vec<u8> = (0..255u8).collect();
    let mut r_out = vec![0u8; inp.len()];
    t.do_transform(&inp, &mut r_out, 85);
    assert_eq!(r_out, c_out, "sRGB identity transform differs");
}
