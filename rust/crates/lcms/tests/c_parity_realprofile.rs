//! Real-ICC-profile transform parity: open testbed profiles in C and
//! Rust, run the same transform, compare output bytes.

use lcms::profile;
use lcms::virt;
use lcms::xform;
use lcms_cref as c;

const TYPE_CMYK_8: u32 = 6 << 16 | 4 << 3 | 1;
const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TESTBED: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed";

fn run_c(in_path: &str, in_fmt: u32, out_fmt: u32, intent: u32) -> Vec<u8> {
    let mut out = vec![0u8; 85 * 3];
    unsafe {
        let p_in = {
            let cs = std::ffi::CString::new(in_path).unwrap();
            c::cmsOpenProfileFromFile(
                cs.as_ptr(),
                (std::ffi::CString::new("r").unwrap().as_ptr() as *const u8).cast(),
            )
        };
        assert!(!p_in.is_null(), "C open {in_path} failed");
        let srgb = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(p_in, in_fmt, srgb, out_fmt, intent, 0);
        assert!(!t.is_null(), "C transform failed");
        let inp = vec![0u8; 85 * 4];
        c::cmsDoTransform(t, inp.as_ptr() as *const _, out.as_mut_ptr() as *mut _, 85);
        c::cmsCloseProfile(p_in);
        c::cmsCloseProfile(srgb);
    }
    out
}

fn run_rust(in_path: &str, in_fmt: u32, out_fmt: u32, intent: u32) -> Vec<u8> {
    let mut out = vec![0u8; 85 * 3];
    let p_in = profile::open_profile_from_file(None, in_path, "r").expect("rust open");
    let srgb = virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p_in, srgb];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, in_fmt, out_fmt, 0,
    )
    .expect("rust transform");
    let inp = vec![0u8; 85 * 4];
    t.do_transform(&inp, &mut out, 85);
    out
}

#[test]
fn cmyk_prtr_to_srgb() {
    let path = format!("{TESTBED}/test1.icc");
    let c = run_c(&path, TYPE_CMYK_8, TYPE_RGB_8, 0);
    let r = run_rust(&path, TYPE_CMYK_8, TYPE_RGB_8, 0);
    assert_eq!(c, r, "CMYK prtr→sRGB mismatch");
}

#[test]
fn monitor_rgb_to_srgb() {
    let path = format!("{TESTBED}/ibm-t61.icc");
    let c = run_c(&path, TYPE_RGB_8, TYPE_RGB_8, 0);
    let r = run_rust(&path, TYPE_RGB_8, TYPE_RGB_8, 0);
    assert_eq!(c, r, "monitor→sRGB mismatch");
}

// test2.icc = CMYK/Lab-prtr (LUT-heavy like test1), test3 = RGB/Lab-spac v2.1,
// test4/test5 = RGB/Lab-spac v4.2, crayons = RGB/XYZ-mntr v4
#[test]
fn more_profiles_to_srgb() {
    for (name, in_fmt) in [
        ("test2.icc", TYPE_CMYK_8),
        ("test3.icc", TYPE_RGB_8),
        ("test4.icc", TYPE_RGB_8),
        ("test5.icc", TYPE_RGB_8),
        ("crayons.icc", TYPE_RGB_8),
    ] {
        let path = format!("{TESTBED}/{name}");
        let c = run_c(&path, in_fmt, TYPE_RGB_8, 0);
        let r = run_rust(&path, in_fmt, TYPE_RGB_8, 0);
        assert_eq!(c, r, "{name}→sRGB mismatch");
    }
}

fn run_c_out(out_path: &str, in_fmt: u32, out_fmt: u32, intent: u32) -> Vec<u8> {
    let mut out = vec![0u8; 85 * 4];
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let p_out = {
            let cs = std::ffi::CString::new(out_path).unwrap();
            c::cmsOpenProfileFromFile(
                cs.as_ptr(),
                (std::ffi::CString::new("r").unwrap().as_ptr() as *const u8).cast(),
            )
        };
        assert!(!p_out.is_null(), "C open {out_path} failed");
        let t = c::cmsCreateTransform(srgb, in_fmt, p_out, out_fmt, intent, 0);
        assert!(!t.is_null(), "C transform failed");
        let inp = vec![0u8; 85 * 3];
        c::cmsDoTransform(t, inp.as_ptr() as *const _, out.as_mut_ptr() as *mut _, 85);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(p_out);
    }
    out
}

fn run_rust_out(out_path: &str, in_fmt: u32, out_fmt: u32, intent: u32) -> Vec<u8> {
    let mut out = vec![0u8; 85 * 4];
    let srgb = virt::create_srgb_profile(None).expect("srgb");
    let p_out = profile::open_profile_from_file(None, out_path, "r").expect("rust open");
    let mut profs = vec![srgb, p_out];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, in_fmt, out_fmt, 0,
    )
    .expect("rust transform");
    let inp = vec![0u8; 85 * 3];
    t.do_transform(&inp, &mut out, 85);
    out
}

#[test]
fn srgb_to_cmyk_prtr() {
    // sRGB → CMYK printer exercises read_output_lut + K-generation +
    // the B2A pipeline of a real LUT-heavy profile
    let path = format!("{TESTBED}/test1.icc");
    let c = run_c_out(&path, TYPE_RGB_8, TYPE_CMYK_8, 0);
    let r = run_rust_out(&path, TYPE_RGB_8, TYPE_CMYK_8, 0);
    assert_eq!(c, r, "sRGB→CMYK prtr mismatch");
}

fn run_cmm(in_path: &str, out_path: &str, in_fmt: u32, out_fmt: u32, intent: u32) -> Vec<u8> {
    let mut out = vec![0u8; 85 * 4];
    unsafe {
        let p_in = {
            let cs = std::ffi::CString::new(in_path).unwrap();
            c::cmsOpenProfileFromFile(
                cs.as_ptr(),
                (std::ffi::CString::new("r").unwrap().as_ptr() as *const u8).cast(),
            )
        };
        let p_out = {
            let cs = std::ffi::CString::new(out_path).unwrap();
            c::cmsOpenProfileFromFile(
                cs.as_ptr(),
                (std::ffi::CString::new("r").unwrap().as_ptr() as *const u8).cast(),
            )
        };
        assert!(!p_in.is_null() && !p_out.is_null());
        let t = c::cmsCreateTransform(p_in, in_fmt, p_out, out_fmt, intent, 0);
        assert!(!t.is_null(), "C transform failed");
        let inp = vec![0u8; 85 * 4];
        c::cmsDoTransform(t, inp.as_ptr() as *const _, out.as_mut_ptr() as *mut _, 85);
        c::cmsCloseProfile(p_in);
        c::cmsCloseProfile(p_out);
    }
    out
}

fn run_rustmm(in_path: &str, out_path: &str, in_fmt: u32, out_fmt: u32, intent: u32) -> Vec<u8> {
    let mut out = vec![0u8; 85 * 4];
    let p_in = profile::open_profile_from_file(None, in_path, "r").expect("in");
    let p_out = profile::open_profile_from_file(None, out_path, "r").expect("out");
    let mut profs = vec![p_in, p_out];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, in_fmt, out_fmt, 0,
    )
    .expect("rust transform");
    let inp = vec![0u8; 85 * 4];
    t.do_transform(&inp, &mut out, 85);
    out
}

#[test]
fn cmyk_to_cmyk_konly() {
    // test1→test2 (both CMYK-prtr) with K-preserving-perceptual intent
    let a = format!("{TESTBED}/test1.icc");
    let b = format!("{TESTBED}/test2.icc");
    let c = run_cmm(&a, &b, TYPE_CMYK_8, TYPE_CMYK_8, 10); // K_ONLY_PERCEPTUAL
    let r = run_rustmm(&a, &b, TYPE_CMYK_8, TYPE_CMYK_8, 10);
    assert_eq!(c, r, "CMYK→CMYK K-only mismatch");
}

#[test]
fn cmyk_to_cmyk_kplane() {
    let a = format!("{TESTBED}/test1.icc");
    let b = format!("{TESTBED}/test2.icc");
    let c = run_cmm(&a, &b, TYPE_CMYK_8, TYPE_CMYK_8, 13); // K_PLANE_PERCEPTUAL
    let r = run_rustmm(&a, &b, TYPE_CMYK_8, TYPE_CMYK_8, 13);
    assert_eq!(c, r, "CMYK→CMYK K-plane mismatch");
}
