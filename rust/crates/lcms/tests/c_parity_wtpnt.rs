//! cmswtpnt white-point/adaptation machinery.

use lcms::{wtpnt, xform, types};
use lcms_cref as c;
use std::ffi::c_void;

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
        let src_wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 }; // D50
        let illum = c::CieXyz { X: 0.9504, Y: 1.0, Z: 1.0888 }; // D65
        let value = c::CieXyz { X: 0.5, Y: 0.5, Z: 0.5 };
        let mut cres = c::CieXyz::default();
        let c_ok = c::cmsAdaptToIlluminant(
            &mut cres, &src_wp, &illum, &value);
        assert_ne!(c_ok, 0);
        let rsrc = types::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let rill = types::CieXyz { X: 0.9504, Y: 1.0, Z: 1.0888 };
        let rval = types::CieXyz { X: 0.5, Y: 0.5, Z: 0.5 };
        let rres = wtpnt::adapt_to_illuminant(&rsrc, &rill, &rval);
        let r = rres.unwrap();
        assert!((cres.X - r.X).abs() < 1e-10,
            "X c={} r={}", cres.X, r.X);
        assert!((cres.Y - r.Y).abs() < 1e-10,
            "Y c={} r={}", cres.Y, r.Y);
        assert!((cres.Z - r.Z).abs() < 1e-10,
            "Z c={} r={}", cres.Z, r.Z);
    }
}

#[test]
fn adaptation_state() {
    unsafe {
        let prev = c::cmsSetAdaptationStateTHR(
            std::ptr::null_mut(), 0.5);
        let _ = c::cmsSetAdaptationStateTHR(
            std::ptr::null_mut(), prev);
        let _ = xform::set_adaptation_state(None, 0.7);
        let _ = xform::set_adaptation_state(None, prev);
    }
}
