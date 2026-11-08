//! cmspcs — PCS conversion + encoding machinery parity.

use lcms::{pcs, types};
use lcms_cref as c;

#[test]
fn xyz_lab_roundtrip() {
    unsafe {
        for (x, y, z) in [(0.5f64, 0.4, 0.3), (0.9642, 1.0, 0.8249),
                          (0.0, 0.0, 0.0), (1.5, 1.2, 0.9)] {
            let mut clab = c::CieLab::default();
            let cxyz = c::CieXyz { X: x, Y: y, Z: z };
            c::cmsXYZ2Lab(std::ptr::null(), &mut clab, &cxyz);
            let rlab = pcs::xyz2lab(None,
                &types::CieXyz { X: x, Y: y, Z: z });
            assert!((clab.L - rlab.L).abs() < 1e-9,
                "L ({x},{y},{z})");
            assert!((clab.a - rlab.a).abs() < 1e-9,
                "a ({x},{y},{z})");
            assert!((clab.b - rlab.b).abs() < 1e-9,
                "b ({x},{y},{z})");
            // back
            let mut cxyz2 = c::CieXyz::default();
            c::cmsLab2XYZ(std::ptr::null(), &mut cxyz2, &clab);
            let rxyz2 = pcs::lab2xyz(None, &rlab);
            assert!((cxyz2.X - rxyz2.X).abs() < 1e-9, "X back");
            assert!((cxyz2.Y - rxyz2.Y).abs() < 1e-9, "Y back");
            assert!((cxyz2.Z - rxyz2.Z).abs() < 1e-9, "Z back");
        }
    }
}

#[test]
fn lab_encodings() {
    unsafe {
        // V2 (legacy 0xFF00 quantum) + V4 (uniform) encodings
        for (l, a, b) in [(50.0f64, 0.0, 0.0), (0.0, -128.0, -128.0),
                          (100.0, 127.0, 127.0), (33.3, 12.5, -45.6)] {
            let flab = c::CieLab { L: l, a, b };
            let mut cw2 = [0u16; 3];
            let mut cw4 = [0u16; 3];
            c::cmsFloat2LabEncodedV2(cw2.as_mut_ptr(), &flab);
            c::cmsFloat2LabEncoded(cw4.as_mut_ptr(), &flab);
            let rlab = types::CieLab { L: l, a, b };
            let rw2 = pcs::float2lab_encoded_v2(&rlab);
            let rw4 = pcs::float2lab_encoded(&rlab);
            assert_eq!(cw2, rw2, "v2 ({l},{a},{b})");
            assert_eq!(cw4, rw4, "v4 ({l},{a},{b})");
            // decode back
            let mut cl2 = c::CieLab::default();
            let mut cl4 = c::CieLab::default();
            c::cmsLabEncoded2FloatV2(&mut cl2, cw2.as_ptr());
            c::cmsLabEncoded2Float(&mut cl4, cw4.as_ptr());
            let rl2 = pcs::lab_encoded2float_v2(cw2);
            let rl4 = pcs::lab_encoded2float(cw4);
            assert!((cl2.L - rl2.L).abs() < 0.01, "v2 L back");
            assert!((cl4.L - rl4.L).abs() < 0.01, "v4 L back");
        }
    }
}

#[test]
fn xyz_xyy() {
    unsafe {
        for (x, y, z) in [(0.5f64, 0.4, 0.3), (0.9642, 1.0, 0.8249)] {
            let cxyz = c::CieXyz { X: x, Y: y, Z: z };
            let mut cxyy = c::CiexyY::default();
            c::cmsXYZ2xyY(&mut cxyy, &cxyz);
            let rxyy = pcs::xyz2xyy(
                &types::CieXyz { X: x, Y: y, Z: z });
            assert!((cxyy.x - rxyy.x).abs() < 1e-9, "x");
            assert!((cxyy.y - rxyy.y).abs() < 1e-9, "y");
            let mut cxyz2 = c::CieXyz::default();
            c::cmsxyY2XYZ(&mut cxyz2, &cxyy);
            let rxyz2 = pcs::xyy2xyz(&rxyy);
            assert!((cxyz2.X - rxyz2.X).abs() < 1e-9, "X back");
        }
    }
}

#[test]
fn colorspace_mappings() {
    unsafe {
        // our-notation -> ICC sig + ICC sig -> our-notation
        // C's mapping is many-to-one — compare C-vs-Rust values,
        // not roundtrip-to-identity
        for i in 1..=17i32 {
            let sig = c::_cmsICCcolorSpace(i);
            let back = c::_cmsLCMScolorSpace(sig);
            let rsig = pcs::icc_color_space(i);
            assert_eq!(sig, rsig, "icc sig {i}");
            let rback = pcs::lcms_color_space(sig);
            assert_eq!(back, rback, "lcms back {i}");
        }
    }
}
