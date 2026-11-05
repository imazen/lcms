//! Transform parity across intents + formats + a non-sRGB profile.
//! Each case runs the same input through C's `cmsCreateTransform` and
//! Rust's `create_extended_transform`, comparing the raw output bytes.

use lcms::types::*;
use lcms::virt;
use lcms::xform;
use lcms_cref as c;

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_BGR_8: u32 = 4 << 16 | 3 << 3 | 1 | 1 << 10;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;
const TYPE_LAB_16: u32 = 10 << 16 | 3 << 3 | 2;

fn run_both(
    in_fmt: u32,
    out_fmt: u32,
    intent: u32,
    inp_bytes: usize,
    out_bytes: usize,
    n_pixels: usize,
) -> (Vec<u8>, Vec<u8>) {
    // C side
    let mut c_out = vec![0u8; n_pixels * out_bytes];
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(prof, in_fmt, prof, out_fmt, intent, 0);
        assert!(!t.is_null(), "C create_transform failed");
        let inp = vec![0u8; n_pixels * inp_bytes];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            n_pixels as u32,
        );
        c::cmsCloseProfile(prof);
    }
    // Rust side
    let mut rust_out = vec![0u8; n_pixels * out_bytes];
    let p1 = virt::create_srgb_profile(None).expect("srgb in");
    let p2 = virt::create_srgb_profile(None).expect("srgb out");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, in_fmt, out_fmt, 0,
    )
    .expect("rust create_extended_transform failed");
    let inp = vec![0u8; n_pixels * inp_bytes];
    t.do_transform(&inp, &mut rust_out, n_pixels as u32);
    (c_out, rust_out)
}

#[test]
fn srgb_all_intents_rgb8() {
    for intent in [0u32, 1, 2, 3] {
        let (c, r) = run_both(TYPE_RGB_8, TYPE_RGB_8, intent, 3, 3, 85);
        assert_eq!(c, r, "intent {intent} rgb8 mismatch");
    }
}

#[test]
fn srgb_16bit() {
    let (c, r) = run_both(TYPE_RGB_16, TYPE_RGB_16, 0, 6, 6, 85);
    assert_eq!(c, r, "rgb16 mismatch");
}

#[test]
fn srgb_bgr8() {
    let (c, r) = run_both(TYPE_BGR_8, TYPE_BGR_8, 0, 3, 3, 85);
    assert_eq!(c, r, "bgr8 mismatch");
}

#[test]
fn lab_to_srgb() {
    // Lab4 (abstract) → sRGB exercises the devicelink read path +
    // the output matrix-shaper
    let mut c_out = vec![0u8; 85 * 3];
    unsafe {
        let lab = c::cmsCreateLab4Profile(std::ptr::null());
        let srgb = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(lab, TYPE_LAB_16, srgb, TYPE_RGB_8, 0, 0);
        assert!(!t.is_null(), "C Lab→sRGB failed");
        let inp = vec![0u8; 85 * 6];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            85,
        );
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(srgb);
    }
    let mut rust_out = vec![0u8; 85 * 3];
    let p1 = virt::create_lab4_profile(None, None).expect("lab");
    let p2 = virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_LAB_16,
        TYPE_RGB_8,
        0,
    )
    .expect("rust Lab→sRGB failed");
    let inp = vec![0u8; 85 * 6];
    t.do_transform(&inp, &mut rust_out, 85);
    assert_eq!(c_out, rust_out, "Lab→sRGB mismatch");
}

#[test]
fn srgb_to_xyz() {
    const TYPE_XYZ_DBL: u32 = 1 << 22 | 9 << 16 | 3 << 3 | 0;
    let mut c_out = vec![0u8; 85 * 24];
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let xyz = c::cmsCreateXYZProfile();
        let t = c::cmsCreateTransform(srgb, TYPE_RGB_8, xyz, TYPE_XYZ_DBL, 0, 0);
        assert!(!t.is_null(), "C sRGB→XYZ failed");
        let inp = vec![0u8; 85 * 3];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            85,
        );
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(xyz);
    }
    let mut rust_out = vec![0u8; 85 * 24];
    let p1 = virt::create_srgb_profile(None).expect("srgb");
    let p2 = virt::create_xyz_profile(None).expect("xyz");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_RGB_8,
        TYPE_XYZ_DBL,
        0,
    )
    .expect("rust sRGB→XYZ failed");
    let inp = vec![0u8; 85 * 3];
    t.do_transform(&inp, &mut rust_out, 85);
    assert_eq!(c_out, rust_out, "sRGB→XYZ mismatch");
}

#[test]
fn lab2_to_srgb() {
    const TYPE_LAB_8: u32 = 10 << 16 | 3 << 3 | 1;
    let mut c_out = vec![0u8; 85 * 3];
    unsafe {
        let lab = c::cmsCreateLab2Profile(std::ptr::null());
        let srgb = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(lab, TYPE_LAB_8, srgb, TYPE_RGB_8, 0, 0);
        assert!(!t.is_null(), "C Lab2→sRGB failed");
        let inp = vec![0u8; 85 * 3];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            85,
        );
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(srgb);
    }
    let mut rust_out = vec![0u8; 85 * 3];
    let p1 = virt::create_lab2_profile(None, None).expect("lab2");
    let p2 = virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, TYPE_LAB_8, TYPE_RGB_8, 0,
    )
    .expect("rust Lab2→sRGB failed");
    let inp = vec![0u8; 85 * 3];
    t.do_transform(&inp, &mut rust_out, 85);
    assert_eq!(c_out, rust_out, "Lab2→sRGB mismatch");
}

#[test]
fn srgb_forced_clut() {
    // cmsFLAGS_FORCE_CLUT exercises OptimizeByResampling + PrelinEval
    // paths that the matrix-shaper shortcut skips
    const FLAGS_FORCE_CLUT: u32 = 0x0002;
    let mut c_out = vec![0u8; 85 * 3];
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(prof, TYPE_RGB_8, prof, TYPE_RGB_8, 0, FLAGS_FORCE_CLUT);
        assert!(!t.is_null(), "C sRGB FORCED_CLUT failed");
        let inp = vec![0u8; 85 * 3];
        c::cmsDoTransform(t, inp.as_ptr() as *const _, c_out.as_mut_ptr() as *mut _, 85);
        c::cmsCloseProfile(prof);
    }
    let mut rust_out = vec![0u8; 85 * 3];
    let p1 = virt::create_srgb_profile(None).expect("srgb");
    let p2 = virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
        TYPE_RGB_8, TYPE_RGB_8, FLAGS_FORCE_CLUT,
    ).expect("rust sRGB FORCED_CLUT failed");
    let inp = vec![0u8; 85 * 3];
    t.do_transform(&inp, &mut rust_out, 85);
    assert_eq!(c_out, rust_out, "sRGB FORCED_CLUT mismatch");
}
