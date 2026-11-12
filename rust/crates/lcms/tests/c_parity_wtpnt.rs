//! cmswtpnt white-point/adaptation machinery.

use lcms::{types, wtpnt, xform};
use lcms_cref as c;

#[test]
fn white_point_from_temp() {
    for temp in [4000.0f64, 5000.0, 6504.0, 50000.0] {
        unsafe {
            let mut cwp = c::CiexyY::default();
            let c_ok = c::cmsWhitePointFromTemp(&mut cwp, temp);
            let rwp = wtpnt::white_point_from_temp(temp);
            assert_eq!(c_ok != 0, rwp.is_some(), "temp {temp}");
            if c_ok != 0 {
                let r = rwp.unwrap();
                assert_eq!(cwp.x, r.x, "x {temp}");
                assert_eq!(cwp.y, r.y, "y {temp}");
            }
        }
    }
}

#[test]
fn adapt_to_illuminant() {
    unsafe {
        let src_wp = c::CieXyz {
            X: 0.9642,
            Y: 1.0,
            Z: 0.8249,
        }; // D50
        let illum = c::CieXyz {
            X: 0.9504,
            Y: 1.0,
            Z: 1.0888,
        }; // D65
        let value = c::CieXyz {
            X: 0.5,
            Y: 0.5,
            Z: 0.5,
        };
        let mut cres = c::CieXyz::default();
        let c_ok = c::cmsAdaptToIlluminant(&mut cres, &src_wp, &illum, &value);
        assert_ne!(c_ok, 0);
        let rsrc = types::CieXyz {
            X: 0.9642,
            Y: 1.0,
            Z: 0.8249,
        };
        let rill = types::CieXyz {
            X: 0.9504,
            Y: 1.0,
            Z: 1.0888,
        };
        let rval = types::CieXyz {
            X: 0.5,
            Y: 0.5,
            Z: 0.5,
        };
        let rres = wtpnt::adapt_to_illuminant(&rsrc, &rill, &rval);
        let r = rres.unwrap();
        assert!((cres.X - r.X).abs() < 1e-10, "X c={} r={}", cres.X, r.X);
        assert!((cres.Y - r.Y).abs() < 1e-10, "Y c={} r={}", cres.Y, r.Y);
        assert!((cres.Z - r.Z).abs() < 1e-10, "Z c={} r={}", cres.Z, r.Z);
    }
}

#[test]
fn adaptation_state() {
    unsafe {
        let prev = c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), 0.5);
        let _ = c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), prev);
        let _ = xform::set_adaptation_state(None, 0.7);
        let _ = xform::set_adaptation_state(None, prev);
    }
}

/// Temp<->whitepoint round trips — cmsWhitePointFromTemp /
/// cmsTempFromWhitePoint drive CHAD2Temp/Temp2CHAD.
#[test]
fn temp_whitepoint_roundtrip() {
    unsafe {
        for t in [2500.0f64, 4000.0, 5000.0, 6500.0, 9300.0, 15000.0] {
            let mut cwp = c::CiexyY::default();
            let c_ok = c::cmsWhitePointFromTemp(&mut cwp, t);
            let rwp = lcms::wtpnt::white_point_from_temp(t);
            assert_eq!(c_ok != 0, rwp.is_some(), "wpt from temp {t}");
            if let Some(r) = rwp {
                assert!(
                    (cwp.x - r.x).abs() < 1e-9
                        && (cwp.y - r.y).abs() < 1e-9
                        && (cwp.Y - r.Y).abs() < 1e-9,
                    "wp {t}: C {},{},{} R {},{},{}",
                    cwp.x,
                    cwp.y,
                    cwp.Y,
                    r.x,
                    r.y,
                    r.Y
                );
                // and back
                let mut ct = 0.0f64;
                let c_ok2 = c::cmsTempFromWhitePoint(&mut ct, &cwp);
                let rt = lcms::wtpnt::temp_from_white_point(&r);
                assert_eq!(c_ok2 != 0, rt.is_some(), "temp from wp {t}");
                if let Some(rt) = rt {
                    assert!((ct - rt).abs() < 1e-6, "temp {t}: C {ct} R {rt}");
                }
            }
        }
        // out-of-gamut / extreme temps
        for t in [0.0f64, 100.0, 40000.0, 1e9] {
            let mut cwp = c::CiexyY::default();
            let c_ok = c::cmsWhitePointFromTemp(&mut cwp, t);
            let rwp = lcms::wtpnt::white_point_from_temp(t);
            assert_eq!(c_ok != 0, rwp.is_some(), "extreme temp {t}");
        }
    }
}
