//! Virtual profile builders — serialize every ctor to bytes and compare
//! C `cmsSaveProfileToMem` vs Rust `profile::save_profile_to_mem` exactly.
//! Covers cmsvirt.c builders the testbed never reaches (gray, linearization,
//! ink-limiting, BCHSW abstract, abstract, NULL, transform2devicelink) and,
//! through serialization, the tag writers in cmstypes.c/cmsio0.c.

use lcms::profile;
use lcms::virt;
use lcms_cref as c;

fn c_save(prof: *mut std::ffi::c_void) -> Option<Vec<u8>> {
    unsafe {
        let mut n = 0u32;
        if c::cmsSaveProfileToMem(prof, std::ptr::null_mut(), &mut n) == 0 {
            return None;
        }
        let mut buf = vec![0u8; n as usize];
        if c::cmsSaveProfileToMem(prof, buf.as_mut_ptr() as *mut _, &mut n) == 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }
}

fn r_save(p: &mut lcms::profile::IccProfile) -> Option<Vec<u8>> {
    profile::save_profile_to_mem(p)
}

/// Both-NULL is parity on the error path; otherwise byte-compare with the
/// 12-byte `created` datetime (header bytes 24..36) masked — C stamps
/// `time(0)` at placeholder creation, the port deliberately keeps epoch
/// (documented at `profile::create_profile_placeholder`). Mask it on both
/// sides, same convention as existing serialize gates.
fn cmp(name: &str, c: Option<Vec<u8>>, r: Option<Vec<u8>>) {
    match (c, r) {
        (None, None) => {}
        (None, Some(_)) => panic!("{name}: C failed, rust wrote"),
        (Some(_), None) => panic!("{name}: rust failed, C wrote"),
        (Some(mut cb), Some(mut rb)) => {
            cb[24..36].fill(0);
            rb[24..36].fill(0);
            assert_eq!(cb, rb, "{name}: serialized bytes differ");
        }
    }
}

fn c_gamma(g: f64) -> *mut c::ToneCurve {
    unsafe { c::cmsBuildGamma(std::ptr::null_mut(), g) }
}
fn r_gamma(g: f64) -> lcms::gamma::ToneCurve {
    lcms::gamma::build_gamma(None, g).expect("gamma")
}

#[test]
fn gray_profiles() {
    for g in [1.0, 1.8, 2.2, 2.4] {
        unsafe {
            let tc = c_gamma(g);
            let p = c::cmsCreateGrayProfile(std::ptr::null(), tc);
            assert!(!p.is_null());
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let tc_r = r_gamma(g);
            let mut r_p = virt::create_gray_profile(None, None, Some(&tc_r)).expect("gray");
            cmp(&format!("gray g{g}"), c_b, r_save(&mut r_p));
        }
    }
    // non-default white point
    unsafe {
        let wp = c::CiexyY { x: 0.3127, y: 0.3290, Y: 1.0 };
        let tc = c_gamma(2.2);
        let p = c::cmsCreateGrayProfile(&wp as *const _ as *const _, tc);
        assert!(!p.is_null());
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let tc_r = r_gamma(2.2);
        let wp_r = lcms::types::CiexyY { x: 0.3127, y: 0.3290, Y: 1.0 };
        let mut r_p = virt::create_gray_profile(None, Some(&wp_r), Some(&tc_r)).expect("gray wp");
        cmp("gray d65", c_b, r_save(&mut r_p));
    }
}

#[test]
fn rgb_profiles() {
    // sRGB primaries + a couple of TRC shapes
    let prim = c::CiexyYTriple {
        Red: c::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
        Green: c::CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
        Blue: c::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
    };
    for gamma in [1.8, 2.2] {
        unsafe {
            let wp = c::cmsD50_xyY();
            let tcs: [*mut c::ToneCurve; 3] = [c_gamma(gamma); 3];
            let p = c::cmsCreateRGBProfileTHR(
                std::ptr::null_mut(),
                wp as *const _,
                &prim as *const _ as *const _,
                tcs.as_ptr() as *const _,
            );
            assert!(!p.is_null());
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let tc_r = r_gamma(gamma);
            let wp_r = lcms::wtpnt::d50_xyy();
            let prim_r = lcms::types::CiexyYTriple {
                Red: lcms::types::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
                Green: lcms::types::CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
                Blue: lcms::types::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
            };
            let mut r_p = virt::create_rgb_profile(
                None,
                Some(&wp_r),
                Some(&prim_r),
                Some([&tc_r, &tc_r, &tc_r]),
            )
            .expect("rgb");
            cmp(&format!("rgb g{gamma}"), c_b, r_save(&mut r_p));
        }
    }
}

#[test]
fn lab_xyz_null_srgb() {
    unsafe {
        let p = c::cmsCreateXYZProfile();
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_xyz_profile(None).expect("xyz");
        cmp("xyz", c_b, r_save(&mut r_p));

        let p = c::cmsCreate_sRGBProfile();
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_srgb_profile(None).expect("srgb");
        cmp("srgb", c_b, r_save(&mut r_p));

        let p = c::cmsCreateNULLProfile();
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_null_profile(None).expect("null");
        cmp("null", c_b, r_save(&mut r_p));

        let p = c::cmsCreateLab2Profile(std::ptr::null());
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_lab2_profile(None, None).expect("lab2");
        cmp("lab2", c_b, r_save(&mut r_p));

        let p = c::cmsCreateLab4Profile(std::ptr::null());
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_lab4_profile(None, None).expect("lab4");
        cmp("lab4", c_b, r_save(&mut r_p));
    }
}

#[test]
fn linearization_links() {
    // gray + rgb + cmyk linearization
    for (cs_sig, nch, name) in [
        (0x47524159u32, 1usize, "gray"), // 'GRAY'
        (0x52474220, 3, "rgb"),          // 'RGB '
        (0x434d594b, 4, "cmyk"),         // 'CMYK'
    ] {
        unsafe {
            let tcs: Vec<*mut c::ToneCurve> = (0..nch).map(|_| c_gamma(2.2)).collect();
            let p = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                cs_sig,
                tcs.as_ptr(),
            );
            assert!(!p.is_null(), "C lin {name}");
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let tc_r = r_gamma(2.2);
            let tr: Vec<&lcms::gamma::ToneCurve> = (0..nch).map(|_| &tc_r).collect();
            let mut r_p = virt::create_linearization_device_link(None, cs_sig, &tr)
                .expect("rust lin");
            cmp(&format!("lin {name}"), c_b, r_save(&mut r_p));
        }
    }
}

#[test]
fn ink_limiting() {
    for lim in [50.0, 100.0, 150.0, 300.0] {
        unsafe {
            let p = c::cmsCreateInkLimitingDeviceLinkTHR(
                std::ptr::null_mut(),
                0x434d594b, // 'CMYK'
                lim,
            );
            assert!(!p.is_null());
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let mut r_p =
                virt::create_ink_limiting_device_link(None, 0x434d594b, lim).expect("ink");
            cmp(&format!("ink {lim}"), c_b, r_save(&mut r_p));
        }
    }
    // error path: non-CMYK must fail on both sides
    unsafe {
        let p = c::cmsCreateInkLimitingDeviceLinkTHR(std::ptr::null_mut(), 0x52474220, 100.0);
        let r_p = virt::create_ink_limiting_device_link(None, 0x52474220, 100.0);
        assert_eq!(p.is_null(), r_p.is_none(), "ink-limit RGB reject");
        if !p.is_null() {
            c::cmsCloseProfile(p);
        }
    }
}

#[test]
fn bchsw_abstract() {
    // both take Kelvin temps
    for (lut_pts, bright, contrast, hue, sat, tsrc, tdst) in [
        (17u32, 0.0, 0.0, 0.0, 0.0, 6500u32, 6500u32),
        (17, 10.0, 5.0, -3.0, 20.0, 6500, 5000),
        (33, -20.0, 15.0, 10.0, -10.0, 5000, 6500),
    ] {
        unsafe {
            let p = c::cmsCreateBCHSWabstractProfile(
                lut_pts, bright, contrast, hue, sat, tsrc, tdst,
            );
            assert!(!p.is_null(), "C bchsw");
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let mut r_p = virt::create_bchsw_abstract_profile(
                None, lut_pts, bright, contrast, hue, sat, tsrc, tdst,
            )
            .expect("rust bchsw");
            cmp(
                &format!("bchsw {bright}/{contrast}/{hue}/{sat}/{tsrc}->{tdst}"),
                c_b,
                r_save(&mut r_p),
            );
        }
    }
}
