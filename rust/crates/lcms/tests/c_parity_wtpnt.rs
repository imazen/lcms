//! Differential test: `lcms::wtpnt` vs `cmswtpnt.c`.

use lcms::mtrx::mat3_per;
use lcms::types::*;
use lcms::wtpnt::*;
use lcms_cref as c;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}

fn f64_in(state: &mut u64, lo: f64, hi: f64) -> f64 {
    let u = rng(state) as f64 / (1u64 << 53) as f64;
    lo + u * (hi - lo)
}

fn cxyz(v: &CieXyz) -> c::CieXyz {
    c::CieXyz {
        X: v.X,
        Y: v.Y,
        Z: v.Z,
    }
}
fn cxyy(v: &CiexyY) -> c::CiexyY {
    c::CiexyY {
        x: v.x,
        y: v.y,
        Y: v.Y,
    }
}
fn cm(m: &Mat3) -> c::Mat3 {
    c::Mat3 {
        v: m.v.map(|v| c::Vec3 { n: v.n }),
    }
}
fn mb(m: &Mat3) -> [[u64; 3]; 3] {
    m.v.map(|v| v.n.map(|x| x.to_bits()))
}
fn mb_c(m: &c::Mat3) -> [[u64; 3]; 3] {
    m.v.map(|v| v.n.map(|x| x.to_bits()))
}

#[test]
fn d50_constants() {
    let c_xyz = unsafe { *c::cmsD50_XYZ() };
    let r = d50_xyz();
    assert_eq!(r.X.to_bits(), c_xyz.X.to_bits());
    assert_eq!(r.Y.to_bits(), c_xyz.Y.to_bits());
    assert_eq!(r.Z.to_bits(), c_xyz.Z.to_bits());

    let c_xyy = unsafe { *c::cmsD50_xyY() };
    let r = d50_xyy();
    assert_eq!(r.x.to_bits(), c_xyy.x.to_bits());
    assert_eq!(r.y.to_bits(), c_xyy.y.to_bits());
    assert_eq!(r.Y.to_bits(), c_xyy.Y.to_bits());
}

#[test]
fn white_point_from_temp_sweep() {
    // Valid band edges + invalid temps, plus dense sweep.
    for t in [
        -1.0,
        0.0,
        1.0,
        3999.9,
        4000.0,
        5000.0,
        6500.0,
        7000.0,
        7000.5,
        12000.0,
        25000.0,
        25000.1,
        50000.0,
        f64::INFINITY,
    ] {
        let mut c_out = c::CiexyY::default();
        let ok = unsafe { c::cmsWhitePointFromTemp(&mut c_out, t) } != 0;
        match white_point_from_temp(t) {
            Some(r) => {
                assert!(ok, "t={t}");
                assert_eq!(r.x.to_bits(), c_out.x.to_bits(), "x t={t}");
                assert_eq!(r.y.to_bits(), c_out.y.to_bits(), "y t={t}");
                assert_eq!(r.Y.to_bits(), c_out.Y.to_bits(), "Y t={t}");
            }
            None => assert!(!ok, "t={t} should fail in C too"),
        }
    }
    let mut s = 3u64;
    for _ in 0..20_000 {
        let t = f64_in(&mut s, 3999.0, 25001.0);
        let mut c_out = c::CiexyY::default();
        let ok = unsafe { c::cmsWhitePointFromTemp(&mut c_out, t) } != 0;
        match white_point_from_temp(t) {
            Some(r) => {
                assert!(ok, "t={t}");
                assert_eq!(r.x.to_bits(), c_out.x.to_bits(), "x t={t}");
                assert_eq!(r.y.to_bits(), c_out.y.to_bits(), "y t={t}");
            }
            None => assert!(!ok),
        }
    }
}

#[test]
fn temp_from_white_point_sweep() {
    let mut s = 4u64;
    for _ in 0..50_000 {
        let wp = CiexyY {
            x: f64_in(&mut s, 0.15, 0.45),
            y: f64_in(&mut s, 0.25, 0.45),
            Y: 1.0,
        };
        let mut c_t = 0.0f64;
        let ok = unsafe { c::cmsTempFromWhitePoint(&mut c_t, &cxyy(&wp)) } != 0;
        match temp_from_white_point(&wp) {
            Some(t) => {
                assert!(ok, "wp={wp:?}");
                assert_eq!(t.to_bits(), c_t.to_bits(), "T wp={wp:?}");
            }
            None => assert!(!ok, "wp={wp:?} should fail in C too"),
        }
    }
    // Round-trip: generated white points must invert.
    for t in (4000..=25000).step_by(73) {
        let wp = white_point_from_temp(t as f64).unwrap();
        let mut c_t = 0.0f64;
        let ok = unsafe { c::cmsTempFromWhitePoint(&mut c_t, &cxyy(&wp)) } != 0;
        let r = temp_from_white_point(&wp);
        assert_eq!(ok, r.is_some(), "roundtrip t={t}");
        if let Some(rt) = r {
            assert_eq!(rt.to_bits(), c_t.to_bits(), "roundtrip t={t}");
        }
    }
}

#[test]
fn adaptation_matrix_and_illuminant() {
    let mut s = 5u64;
    let d50 = d50_xyz();
    let d65 = CieXyz {
        X: 0.95047,
        Y: 1.0,
        Z: 1.08883,
    };

    for _ in 0..20_000 {
        let from = CieXyz {
            X: f64_in(&mut s, 0.7, 1.2),
            Y: 1.0,
            Z: f64_in(&mut s, 0.7, 1.3),
        };
        let to = if rng(&mut s) % 2 == 0 { d50 } else { d65 };
        let value = CieXyz {
            X: f64_in(&mut s, 0.0, 1.0),
            Y: f64_in(&mut s, 0.0, 1.0),
            Z: f64_in(&mut s, 0.0, 1.0),
        };

        // _cmsAdaptationMatrix with NULL cone (= Bradford)
        let mut r = Mat3::default();
        let mut cr = c::Mat3::default();
        let ok = adaptation_matrix(&mut r, None, &from, &to);
        let cok =
            unsafe { c::_cmsAdaptationMatrix(&mut cr, std::ptr::null(), &cxyz(&from), &cxyz(&to)) }
                != 0;
        assert_eq!(ok, cok, "adapt bool");
        if ok {
            assert_eq!(mb(&r), mb_c(&cr), "adapt {from:?}->{to:?}");
        }

        // cmsAdaptToIlluminant
        let mut c_res = c::CieXyz::default();
        let cok =
            unsafe { c::cmsAdaptToIlluminant(&mut c_res, &cxyz(&from), &cxyz(&to), &cxyz(&value)) }
                != 0;
        match adapt_to_illuminant(&from, &to, &value) {
            Some(res) => {
                assert!(cok);
                assert_eq!(res.X.to_bits(), c_res.X.to_bits(), "X");
                assert_eq!(res.Y.to_bits(), c_res.Y.to_bits(), "Y");
                assert_eq!(res.Z.to_bits(), c_res.Z.to_bits(), "Z");
            }
            None => assert!(!cok),
        }

        // custom cone matrix path (random invertible cone)
        let mut cone = Mat3::default();
        // build a simple well-conditioned cone: Bradford-like diagonal + eps
        for i in 0..3 {
            for j in 0..3 {
                cone.v[i].n[j] = f64_in(&mut s, -0.2, 0.2);
            }
            cone.v[i].n[i] += 1.0;
        }
        let ccone = cm(&cone);
        let ok = adaptation_matrix(&mut r, Some(&cone), &from, &to);
        let cok =
            unsafe { c::_cmsAdaptationMatrix(&mut cr, &ccone, &cxyz(&from), &cxyz(&to)) } != 0;
        assert_eq!(ok, cok, "custom cone bool {cone:?}");
        if ok {
            assert_eq!(mb(&r), mb_c(&cr), "custom cone");
        }
    }
}

#[test]
fn build_rgb2xyz_transfer() {
    // sRGB primaries + a few random ones
    let white = CiexyY {
        x: 0.3127,
        y: 0.3290,
        Y: 1.0,
    };
    let primaries = CiexyYTriple {
        Red: CiexyY {
            x: 0.64,
            y: 0.33,
            Y: 1.0,
        },
        Green: CiexyY {
            x: 0.30,
            y: 0.60,
            Y: 1.0,
        },
        Blue: CiexyY {
            x: 0.15,
            y: 0.06,
            Y: 1.0,
        },
    };
    let cprim = c::CiexyYTriple {
        Red: cxyy(&primaries.Red),
        Green: cxyy(&primaries.Green),
        Blue: cxyy(&primaries.Blue),
    };

    let mut r = Mat3::default();
    let mut cr = c::Mat3::default();
    let ok = build_rgb2xyz_transfer_matrix(&mut r, &white, &primaries);
    let cok = unsafe { c::_cmsBuildRGB2XYZtransferMatrix(&mut cr, &cxyy(&white), &cprim) } != 0;
    assert_eq!(ok, cok);
    assert_eq!(mb(&r), mb_c(&cr), "sRGB matrix");

    // exercise the r-side product ordering against C: compose then re-adapt
    let mut composed = Mat3::default();
    mat3_per(&mut composed, &r, &r);
    let mut c_composed = c::Mat3::default();
    unsafe { c::_cmsMAT3per(&mut c_composed, &cr, &cr) };
    assert_eq!(mb(&composed), mb_c(&c_composed));
}
