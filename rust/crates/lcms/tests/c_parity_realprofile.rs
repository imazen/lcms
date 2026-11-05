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
            c::cmsOpenProfileFromFile(cs.as_ptr(), std::ffi::CString::new("r").unwrap().as_ptr() as *const u8)
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
