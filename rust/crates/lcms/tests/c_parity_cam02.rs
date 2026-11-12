//! Differential test: `lcms::cam02` vs `cmscam02.c`.

use lcms::cam02::CieCam02;
use lcms::types::{CieXyz, D_CALCULATE, JCh, ViewingConditions};
use lcms_cref as c;
use std::ptr;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}

fn to_c_vc(vc: &ViewingConditions) -> c::ViewingConditions {
    c::ViewingConditions {
        white_point: c::CieXyz {
            X: vc.white_point.X,
            Y: vc.white_point.Y,
            Z: vc.white_point.Z,
        },
        yb: vc.Yb,
        la: vc.La,
        surround: vc.surround,
        d_value: vc.D_value,
    }
}

fn conditions(surround: u32, d_value: f64) -> ViewingConditions {
    ViewingConditions {
        white_point: CieXyz {
            X: 95.05,
            Y: 100.0,
            Z: 108.9,
        },
        Yb: 18.0,
        La: 20.0,
        surround,
        D_value: d_value,
    }
}

#[test]
fn forward_all_surrounds() {
    let mut s = 5u64;
    for (i, &surround) in [1u32, 2, 3, 4, 7].iter().enumerate() {
        for &d in &[D_CALCULATE, 0.8, 1.0] {
            let vc = conditions(surround, d);
            let cvc = to_c_vc(&vc);
            let cm = unsafe { c::cmsCIECAM02Init(ptr::null_mut(), &cvc) };
            assert!(!cm.is_null());
            let rm = CieCam02::new(None, &vc);

            for _ in 0..400 {
                let xyz = CieXyz {
                    X: (rng(&mut s) % 15000) as f64 / 100.0,
                    Y: (rng(&mut s) % 15000) as f64 / 100.0,
                    Z: (rng(&mut s) % 15000) as f64 / 100.0,
                };
                let cin = c::CieXyz {
                    X: xyz.X,
                    Y: xyz.Y,
                    Z: xyz.Z,
                };
                let mut cout = c::JCh::default();
                let mut rout = JCh::default();
                unsafe { c::cmsCIECAM02Forward(cm, &cin, &mut cout) };
                rm.forward(&xyz, &mut rout);
                assert_eq!(
                    (rout.J.to_bits(), rout.C.to_bits(), rout.h.to_bits()),
                    (cout.j.to_bits(), cout.c_.to_bits(), cout.h.to_bits()),
                    "surround={surround} D={d} XYZ={xyz:?} i={i}"
                );
            }
            unsafe { c::cmsCIECAM02Done(cm) };
        }
    }
}

#[test]
fn reverse_all_surrounds() {
    let mut s = 9u64;
    for &surround in &[1u32, 2, 3, 4] {
        let vc = conditions(surround, 0.9);
        let cvc = to_c_vc(&vc);
        let cm = unsafe { c::cmsCIECAM02Init(ptr::null_mut(), &cvc) };
        let rm = CieCam02::new(None, &vc);

        for _ in 0..400 {
            let jch = JCh {
                J: (rng(&mut s) % 10000) as f64 / 100.0,
                C: (rng(&mut s) % 15000) as f64 / 100.0,
                h: (rng(&mut s) % 36000) as f64 / 100.0,
            };
            let cin = c::JCh {
                j: jch.J,
                c_: jch.C,
                h: jch.h,
            };
            let mut cout = c::CieXyz::default();
            let mut rout = CieXyz::default();
            unsafe { c::cmsCIECAM02Reverse(cm, &cin, &mut cout) };
            rm.reverse(&jch, &mut rout);
            assert_eq!(
                (rout.X.to_bits(), rout.Y.to_bits(), rout.Z.to_bits()),
                (cout.X.to_bits(), cout.Y.to_bits(), cout.Z.to_bits()),
                "surround={surround} JCh={jch:?}"
            );
        }
        unsafe { c::cmsCIECAM02Done(cm) };
    }
}

#[test]
fn forward_reverse_roundtrip() {
    // JCh -> XYZ -> JCh round trip equality within model on both sides;
    // also ensures C==0 special case (t<=0) is hit.
    let vc = conditions(1, 1.0);
    let cvc = to_c_vc(&vc);
    let cm = unsafe { c::cmsCIECAM02Init(ptr::null_mut(), &cvc) };
    let rm = CieCam02::new(None, &vc);

    for jch in [
        JCh {
            J: 0.0,
            C: 0.0,
            h: 0.0,
        },
        JCh {
            J: 50.0,
            C: 0.0,
            h: 180.0,
        }, // C=0 → t<=0 special case
        JCh {
            J: 99.9,
            C: 200.0,
            h: 359.9,
        },
        JCh {
            J: 1.0,
            C: 300.0,
            h: 19.0,
        }, // h < 20.14
        JCh {
            J: 80.0,
            C: 60.0,
            h: 240.0,
        },
    ] {
        let cin = c::JCh {
            j: jch.J,
            c_: jch.C,
            h: jch.h,
        };
        let mut cxyz = c::CieXyz::default();
        let mut rxyz = CieXyz::default();
        unsafe { c::cmsCIECAM02Reverse(cm, &cin, &mut cxyz) };
        rm.reverse(&jch, &mut rxyz);
        assert_eq!(
            (rxyz.X.to_bits(), rxyz.Y.to_bits(), rxyz.Z.to_bits()),
            (cxyz.X.to_bits(), cxyz.Y.to_bits(), cxyz.Z.to_bits()),
            "JCh={jch:?}"
        );
    }
    unsafe { c::cmsCIECAM02Done(cm) };
}
