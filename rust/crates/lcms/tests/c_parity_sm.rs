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
