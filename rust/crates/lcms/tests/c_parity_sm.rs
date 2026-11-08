//! cmssamp / cnvrt sampling surface: cmsDetectBlackPoint,
//! cmsDetectDestinationBlackPoint, cmsDetectTAC across every bundled test
//! profile and all intents. C oracle returns Option<XYZ>-equivalent status;
//! both-None is parity on the rejection path, otherwise f64 bit-exact.

use lcms::cnvrt;
use lcms::profile;
use lcms::sm;
use lcms_cref as c;

const PROFILES: &[&str] = &[
    "sRGBlcms2.icc",
    "aRGBlcms2.icc",
    "labv2lcms2.icc",
    "labv4lcms2.icc",
    "graylcms2.icc",
    "gray3lcms2.icc",
    "xyzlcms2.icc",
    "lcms2cmyk.icc",
    "lcms2link.icc",
    "lcms2link2.icc",
    "linlcms2.icc",
    "limitlcms2.icc",
    "nullcms2.icc",
    "bchslcms2.icc",
    "brightness.icc",
];

fn path(name: &str) -> String {
    format!("{}/../../{name}", env!("CARGO_MANIFEST_DIR"))
}

fn c_open(name: &str) -> *mut std::ffi::c_void {
    let p = format!("{}\0", path(name));
    unsafe { c::cmsOpenProfileFromFile(p.as_ptr() as *const i8, b"r\0".as_ptr()) }
}

fn bits(x: f64) -> u64 {
    x.to_bits()
}

#[test]
fn black_point_matrix() {
    for name in PROFILES {
        for intent in 0u32..4 {
            let mut c_bp = lcms::types::CieXyz::default();
            let c_ok = unsafe {
                let p = c_open(name);
                assert!(!p.is_null(), "C open {name}");
                let mut raw = [0f64; 3];
                let ok = c::cmsDetectBlackPoint(
                    raw.as_mut_ptr() as *mut _,
                    p,
                    intent,
                    0,
                );
                c_bp = lcms::types::CieXyz { X: raw[0], Y: raw[1], Z: raw[2] };
                c::cmsCloseProfile(p);
                ok
            };
            let mut r_icc =
                profile::open_profile_from_file(None, &path(name), "r").expect("rust open");
            let r = sm::detect_black_point(&mut r_icc, intent, 0);
            match (c_ok != 0, r) {
                (false, None) => {}
                (false, Some(v)) => panic!("{name} i{intent}: C rejected, rust {v:?}"),
                (true, None) => panic!("{name} i{intent}: rust None, C {c_bp:?}"),
                (true, Some(rv)) => assert_eq!(
                    (bits(c_bp.X), bits(c_bp.Y), bits(c_bp.Z)),
                    (bits(rv.X), bits(rv.Y), bits(rv.Z)),
                    "{name} i{intent} black point"
                ),
            }
        }
    }
}

#[test]
fn destination_black_point_matrix() {
    for name in PROFILES {
        for intent in 0u32..4 {
            let mut c_bp = lcms::types::CieXyz::default();
            let c_ok = unsafe {
                let p = c_open(name);
                assert!(!p.is_null(), "C open {name}");
                let mut raw = [0f64; 3];
                let ok = c::cmsDetectDestinationBlackPoint(
                    raw.as_mut_ptr() as *mut _,
                    p,
                    intent,
                    0,
                );
                c_bp = lcms::types::CieXyz { X: raw[0], Y: raw[1], Z: raw[2] };
                c::cmsCloseProfile(p);
                ok
            };
            let mut r_icc =
                profile::open_profile_from_file(None, &path(name), "r").expect("rust open");
            let r = sm::detect_destination_black_point(&mut r_icc, intent, 0);
            match (c_ok != 0, r) {
                (false, None) => {}
                (false, Some(v)) => panic!("{name} i{intent}: C rejected, rust {v:?}"),
                (true, None) => panic!("{name} i{intent}: rust None, C {c_bp:?}"),
                (true, Some(rv)) => assert_eq!(
                    (bits(c_bp.X), bits(c_bp.Y), bits(c_bp.Z)),
                    (bits(rv.X), bits(rv.Y), bits(rv.Z)),
                    "{name} i{intent} destination black point"
                ),
            }
        }
    }
}

#[test]
fn total_area_coverage() {
    for name in PROFILES {
        let c_tac = unsafe {
            let p = c_open(name);
            assert!(!p.is_null(), "C open {name}");
            let v = c::cmsDetectTAC(p);
            c::cmsCloseProfile(p);
            v
        };
        let mut r_icc = profile::open_profile_from_file(None, &path(name), "r").expect("rust open");
        let r_tac = cnvrt::dbg_detect_tac(&mut r_icc);
        assert_eq!(bits(c_tac), bits(r_tac), "{name} TAC");
    }
}

/// cmsDesaturateLab + cmsDetectTAC + DetectBlackPoint/DestBlackPoint
/// deeper.
#[test]
fn desaturate_detect() {
    unsafe {
        // desaturate_lab — clamp a/b to a box
        for (l, a, b) in [(50.0f64, 80.0, -90.0), (20.0, 30.0, 40.0),
                          (80.0, -120.0, 60.0)] {
            #[repr(C)]
            struct Clab { l: f64, a: f64, b: f64 }
            let mut cp = Clab { l, a, b };
            let _ = c::cmsDesaturateLab(&mut cp as *mut _ as *mut _,
                60.0, -60.0, 60.0, -60.0);
            let mut rp = lcms::types::CieLab { L: l, a, b };
            let _ = lcms::sm::desaturate_lab(&mut rp, 60.0, -60.0,
                60.0, -60.0);
            assert_eq!(cp.l as f32, rp.L as f32, "L ({l},{a},{b})");
            assert_eq!(cp.a as f32, rp.a as f32, "a ({l},{a},{b})");
            assert_eq!(cp.b as f32, rp.b as f32, "b ({l},{a},{b})");
        }
        // detect_tac on a corpus profile
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap()
            .filter_map(|e| e.ok()).map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "icc")
                .unwrap_or(false))
            .collect();
        for f in files {
            let data = std::fs::read(&f).unwrap();
            let ch = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            let ctac = c::cmsDetectTAC(ch);
            let mut rp = lcms::profile::open_profile_from_mem(
                None, &data).unwrap();
            let rtac = lcms::cnvrt::dbg_detect_tac(&mut rp);
            eprintln!("tac c={ctac} r={rtac}");
            // black point + dest black point
            let mut cbp = c::CieXyz::default();
            let c_ok = c::cmsDetectBlackPoint(&mut cbp as *mut _ as _,
                ch, 0, 0);
            let rbp = lcms::sm::detect_black_point(&mut rp, 0, 0);
            assert_eq!(c_ok != 0, rbp.is_some(), "detect bp");
            let mut cdp = c::CieXyz::default();
            let c_ok2 = c::cmsDetectDestinationBlackPoint(
                &mut cdp as *mut _ as _, ch, 0, 0);
            let rdp = lcms::sm::detect_destination_black_point(
                &mut rp, 0, 0);
            assert_eq!(c_ok2 != 0, rdp.is_some(), "detect dest bp");
            c::cmsCloseProfile(ch);
        }
    }
}
