//! cmssamp deeper variants: cmsDesaturateLab dense grid +
//! cmsDetectTAC/BlackPoint/DestinationBlackPoint corpus matrix.

use lcms::{profile, sm, types::*};
use lcms_cref as c;
use std::ffi::c_void;

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| std::fs::read(f).unwrap()).collect()
}

#[test]
fn desaturate_lab_grid() {
    unsafe {
        let mut seed = 0x4444u64;
        for _ in 0..256 {
            seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
            let l = ((seed >> 48) as u16 % 100) as f64;
            let a = (((seed >> 32) as u16 % 200) as f64) - 100.0;
            let b = (((seed >> 16) as u16 % 200) as f64) - 100.0;
            let mut clab = c::CieLab { L: l, a, b };
            let mut rlab = CieLab { L: l, a, b };
            let c_ok = c::cmsDesaturateLab(
                &mut clab as *mut _ as *mut c_void, 80.0, -80.0, 80.0, -80.0);
            let r_ok = sm::desaturate_lab(&mut rlab, 80.0, -80.0, 80.0, -80.0);
            assert_eq!(c_ok != 0, r_ok, "desat {l},{a},{b}");
            assert_eq!(clab.L, rlab.L, "desat L");
            assert_eq!(clab.a, rlab.a, "desat a");
            assert_eq!(clab.b, rlab.b, "desat b");
        }
    }
}

#[test]
fn detect_matrix() {
    for data in corpus() {
        let mut rp = profile::open_profile_from_mem(None, &data).unwrap();
        for intent in 0..4u32 {
            for flags in [0u32, 0x0010] {
                unsafe {
                    let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
                    let mut cbp = [0.0f64; 3];
                    let c_ok = c::cmsDetectBlackPoint(
                        cbp.as_mut_ptr() as *mut c_void, h, intent, flags);
                    let r_bp = sm::detect_black_point(&mut rp, intent, flags);
                    assert_eq!(c_ok != 0, r_bp.is_some(), "bp i{intent} f{flags:#x}");
                    if c_ok != 0 {
                        let r_bp = r_bp.unwrap();
                        assert_eq!(cbp[0], r_bp.X, "bp X");
                        assert_eq!(cbp[1], r_bp.Y, "bp Y");
                        assert_eq!(cbp[2], r_bp.Z, "bp Z");
                    }
                    let mut cbp2 = [0.0f64; 3];
                    let c_ok2 = c::cmsDetectDestinationBlackPoint(
                        cbp2.as_mut_ptr() as *mut c_void, h, intent, flags);
                    let r_bp2 = sm::detect_destination_black_point(&mut rp, intent, flags);
                    assert_eq!(c_ok2 != 0, r_bp2.is_some(), "dest bp");
                    if c_ok2 != 0 {
                        let r_bp2 = r_bp2.unwrap();
                        assert_eq!(cbp2[0], r_bp2.X, "dest X");
                        assert_eq!(cbp2[1], r_bp2.Y, "dest Y");
                        assert_eq!(cbp2[2], r_bp2.Z, "dest Z");
                    }
                    c::cmsCloseProfile(h);
                }
            }
        }
        let c_tac;
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            c_tac = c::cmsDetectTAC(h);
            let _ = c::cmsCloseProfile(h);
        }
        let r_tac = lcms::cnvrt::dbg_detect_tac(&mut rp);
        assert_eq!(c_tac, r_tac, "TAC");
    }
}

/// Shadow-fit quadratic path — cmsDetectBlackPoint on profiles whose
/// L* round-trip ramp leaves >= 3 points in the lo..hi fit band
/// (lcms2cmyk + graylcms2, intents 0..2) — reaches
/// RootOfLeastSquaresFitQuadraticCurve. bp XYZ parity both sides.
#[test]
fn blackpoint_quadratic() {
    unsafe {
        let dir = env!("CARGO_MANIFEST_DIR");
        for name in ["lcms2cmyk.icc", "graylcms2.icc"] {
            let path = format!("{dir}/../../{name}\0");
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as *const i8, b"r\0".as_ptr());
            assert!(!h.is_null());
            let mut rp = lcms::profile::open_profile_from_file(
                None, &format!("{dir}/../../{name}"), "r").unwrap();
            for intent in 0u32..4 {
                let mut cbp = c::CieXyz::default();
                let c_ok = c::cmsDetectBlackPoint(
                    &mut cbp as *mut _ as *mut _, h, intent, 0);
                let rbp = lcms::sm::detect_black_point(&mut rp, intent, 0);
                assert_eq!(c_ok != 0, rbp.is_some(),
                    "{name} i{intent} status");
                if let Some(r) = rbp {
                    assert!((cbp.X - r.X).abs() < 1e-9
                        && (cbp.Y - r.Y).abs() < 1e-9
                        && (cbp.Z - r.Z).abs() < 1e-9,
                        "{name} i{intent} bp: C {},{},{} R {},{},{}",
                        cbp.X, cbp.Y, cbp.Z, r.X, r.Y, r.Z);
                }
            }
            c::cmsCloseProfile(h);
        }
    }
}
