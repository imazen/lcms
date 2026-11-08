//! cmsvirt parameterized builders + linearization/ink-limiting/BCHSW
//! device links — the deeper virt machinery.

use lcms::{virt, named, gamma, types::{CiexyY, CiexyYTriple}};
use lcms_cref as c;
use std::ffi::c_void;

#[test]
fn parameterized_builders() {
    unsafe {
        // RGB profile with explicit primaries + a non-identity curve
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.4);
        let curves = [g; 3];
        let white = c::CieXyz { X: 0.9505, Y: 1.0, Z: 1.089 };
        let prim = [0.64f64, 0.33, 1.0, 0.30, 0.60, 1.0, 0.15, 0.06, 1.0];
        let ch = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            &white as *const _ as *const _, prim.as_ptr() as *const _,
            curves.as_ptr() as *const *mut c_void);
        let rg = gamma::build_gamma(None, 2.4).unwrap();
        let d50 = CiexyY { x: 0.3457, y: 0.3585, Y: 1.0 };
        let rec709 = CiexyYTriple {
            Red: CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
            Green: CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
            Blue: CiexyY { x: 0.15, y: 0.06, Y: 1.0 } };
        let mut rp = virt::create_rgb_profile(None,
            Some(&d50), Some(&rec709), Some([&rg, &rg, &rg]));
        assert_eq!(ch.is_null(), rp.is_none(), "rgb param build");
        // gray profile
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 2.0);
        let ch2 = c::cmsCreateGrayProfile(
            &white as *const _ as *const _, g2);
        let rg2 = gamma::build_gamma(None, 2.0).unwrap();
        let mut rp2 = virt::create_gray_profile(None,
            Some(&d50), Some(&rg2));
        assert_eq!(ch2.is_null(), rp2.is_none(), "gray build");
        if !ch.is_null() { let _ = c::cmsCloseProfile(ch); }
        if !ch2.is_null() { let _ = c::cmsCloseProfile(ch2); }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

#[test]
fn device_link_variants() {
    unsafe {
        // BCHSW abstract profile — the full param matrix
        for (lut, bright, contrast, hue, sat, tsrc, tdst) in [
            (16u32, 10.0f64, 10.0, 0.0, 10.0, 5000u32, 6500),
            (33, -20.0, 5.0, 15.0, -10.0, 6500, 5000),
            (256, 0.0, 0.0, -30.0, 0.0, 4000, 9300),
        ] {
            let ch = c::cmsCreateBCHSWabstractProfile(
                lut, bright, contrast, hue, sat, tsrc, tdst);
            let mut rp = virt::create_bchsw_abstract_profile(None,
                lut, bright, contrast, hue, sat, tsrc, tdst);
            assert_eq!(ch.is_null(), rp.is_none(),
                "bchsw {lut}/{bright}/{contrast}/{hue}/{sat}/{tsrc}/{tdst}");
            if !ch.is_null() { let _ = c::cmsCloseProfile(ch); }
        }
        // linearization device link — one curve for all channels
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let curves = [g; 3];
        let ch = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 4 /* PT_RGB */,
            curves.as_ptr());
        let rg = gamma::build_gamma(None, 1.8).unwrap();
        let mut rp = virt::create_linearization_device_link(None, 4,
            &[&rg, &rg, &rg]);
        assert_eq!(ch.is_null(), rp.is_none(), "linearization link");
        if !ch.is_null() { let _ = c::cmsCloseProfile(ch); }
        c::cmsFreeToneCurve(g);
        // ink-limiting device link
        for limit in [50.0f64, 100.0, 200.0, 350.0] {
            let ch2 = c::cmsCreateInkLimitingDeviceLinkTHR(
                std::ptr::null_mut(), 4 /* PT_CMY */, limit);
            let mut rp2 = virt::create_ink_limiting_device_link(None, 4, limit);
            assert_eq!(ch2.is_null(), rp2.is_none(), "ink limit {limit}");
            if !ch2.is_null() { let _ = c::cmsCloseProfile(ch2); }
        }
    }
}
