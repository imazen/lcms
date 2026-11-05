//! Differential gate for cmsvirt.c — C and Rust build the same virtual
//! profiles; byte-compare the serialized output.

use lcms::profile;
use lcms::virt;
use lcms_cref as c;
use std::ptr;

fn c_save(p: *mut std::ffi::c_void) -> Vec<u8> {
    unsafe {
        let mut needed = 0u32;
        assert!(c::cmsSaveProfileToMem(p, ptr::null_mut(), &mut needed) != 0);
        let mut buf = vec![0u8; needed as usize];
        assert!(c::cmsSaveProfileToMem(p, buf.as_mut_ptr() as *mut _, &mut needed) != 0);
        buf.truncate(needed as usize);
        buf
    }
}

fn r_save(icc: &mut lcms::profile::IccProfile) -> Vec<u8> {
    profile::save_profile_to_mem(icc).expect("rust save")
}

#[test]
fn srgb_profile_bytes() {
    let c_prof = unsafe { c::cmsCreate_sRGBProfile() };
    assert!(!c_prof.is_null());
    let c_bytes = c_save(c_prof);
    unsafe { c::cmsCloseProfile(c_prof) };

    let mut r_prof = virt::create_srgb_profile(None).expect("rust srgb");
    let r_bytes = r_save(&mut r_prof);

    assert_eq!(r_bytes.len(), c_bytes.len(), "size srgb");
    let (mut rb, mut cb) = (r_bytes.clone(), c_bytes.clone());
    rb[24..36].fill(0); // Created = time() in C, epoch in port
    cb[24..36].fill(0);
    assert_eq!(rb, cb, "sRGB profile bytes differ");
}

#[test]
fn lab2_lab4_xyz_profiles() {
    let c_lab2 = unsafe { c::cmsCreateLab2Profile(ptr::null()) };
    let c_lab4 = unsafe { c::cmsCreateLab4Profile(ptr::null()) };
    let c_xyz = unsafe { c::cmsCreateXYZProfile() };
    assert!(!c_lab2.is_null() && !c_lab4.is_null() && !c_xyz.is_null());
    let b_lab2 = c_save(c_lab2);
    let b_lab4 = c_save(c_lab4);
    let b_xyz = c_save(c_xyz);
    unsafe {
        c::cmsCloseProfile(c_lab2);
        c::cmsCloseProfile(c_lab4);
        c::cmsCloseProfile(c_xyz);
    }

    let mut r_lab2 = virt::create_lab2_profile(None, None).unwrap();
    let mut r_lab4 = virt::create_lab4_profile(None, None).unwrap();
    let mut r_xyz = virt::create_xyz_profile(None).unwrap();
    for (name, mut rb, mut cb) in [
        ("lab2", r_save(&mut r_lab2), b_lab2),
        ("lab4", r_save(&mut r_lab4), b_lab4),
        ("xyz", r_save(&mut r_xyz), b_xyz),
    ] {
        rb[24..36].fill(0);
        cb[24..36].fill(0);
        assert_eq!(rb, cb, "{name} bytes");
    }
}

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;

/// Display P3→sRGB transform parity. u8 hits `OptData::MatShaper` (C folds
/// the two adaptation matrices), u16 resamples to `ClutOnly` — C's
/// `OptimizeMatrixShaper` is 8-bit-only. Both must be byte-exact.
#[test]
fn p3_to_srgb_transform() {
    use lcms::types::{CiexyY, CiexyYTriple};
    const D65: CiexyY = CiexyY { x: 0.3127, y: 0.3290, Y: 1.0 };
    const P3: CiexyYTriple = CiexyYTriple {
        Red: CiexyY { x: 0.680, y: 0.320, Y: 1.0 },
        Green: CiexyY { x: 0.265, y: 0.690, Y: 1.0 },
        Blue: CiexyY { x: 0.150, y: 0.060, Y: 1.0 },
    };
    let srgb_p = [2.4f64, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
    let n = 8192usize;
    let inp8: Vec<u8> = (0..n * 3).map(|i| ((i * 37 + i / 3 * 11) % 251) as u8).collect();
    let inp16: Vec<u8> = inp8.iter().flat_map(|&b| (b as u16 * 257).to_le_bytes()).collect();

    for intent in [0u32, 1] {
        for (lbl, fmt, input, bpp) in [
            ("u8", TYPE_RGB_8, &inp8, 3usize),
            ("u16", TYPE_RGB_16, &inp16, 6usize),
        ] {
            // C side
            let (p3_c, srgb_c, tc) = unsafe {
                let g = c::cmsBuildParametricToneCurve(ptr::null_mut(), 4, srgb_p.as_ptr());
                let prim: [*mut c::ToneCurve; 3] = [g, g, g];
                let p3 = c::cmsCreateRGBProfileTHR(
                    ptr::null_mut(),
                    &D65 as *const CiexyY as *const _,
                    &P3 as *const CiexyYTriple as *const _,
                    prim.as_ptr() as *const *mut _,
                );
                let srgb = c::cmsCreate_sRGBProfile();
                (p3, srgb, c::cmsCreateTransform(p3, fmt, srgb, fmt, intent, 0))
            };
            let mut c_out = vec![0u8; input.len()];
            unsafe { c::cmsDoTransform(tc, input.as_ptr() as *const _, c_out.as_mut_ptr() as *mut _, n as u32) };
            let _ = (p3_c, srgb_c);

            // Rust side
            let g = lcms::gamma::build_parametric_tone_curve(None, 4, &srgb_p).unwrap();
            let p1 = virt::create_rgb_profile(None, Some(&D65), Some(&P3), Some([&g, &g, &g])).unwrap();
            let p2 = virt::create_srgb_profile(None).unwrap();
            let mut profs = vec![p1, p2];
            let mut bpc = [false, false];
            let intents = [intent, intent];
            let adapt = [1.0f64, 1.0];
            let mut t = lcms::xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0, fmt, fmt, 0,
            ).unwrap();
            let mut r_out = vec![0u8; input.len()];
            t.do_transform(input, &mut r_out, n as u32);

            assert_eq!(r_out, c_out, "P3→sRGB intent{intent} {lbl} ({bpp}bpp) differs");
        }
    }
}
