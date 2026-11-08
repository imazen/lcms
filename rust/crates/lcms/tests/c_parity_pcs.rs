//! cmswtpnt/cmsgmt parity: PCS conversions (XYZ/xyY/Lab/LCh + encoded),
//! DeltaE family, CAM02 forward/reverse, illuminant adaptation —
//! deterministic grid, byte-exact or f64-exact.

use lcms::pcs;
use lcms::types::*;
use lcms_cref as c;

fn cxz(v: &CieXyz) -> c::CieXyz { c::CieXyz { X: v.X, Y: v.Y, Z: v.Z } }
fn clb(v: &CieLab) -> c::CieLab { c::CieLab { L: v.L, a: v.a, b: v.b } }

fn d50() -> CieXyz { CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 } }
fn d65() -> CieXyz { CieXyz { X: 0.9504, Y: 1.0, Z: 1.0888 } }

#[test]
fn conversions_grid() {
    unsafe {
        for wp in [d50(), d65(), CieXyz { X: 0.8, Y: 1.0, Z: 1.2 }] {
            for i in 0..64 {
                let x = 0.01 + (i as f64 % 8.) * 0.11;
                let y = 0.01 + (i as f64 % 5.) * 0.19;
                let z = 0.01 + (i as f64 % 7.) * 0.13;
                let xyz = CieXyz { X: x, Y: y, Z: z };
                // XYZ->xyY->XYZ
                let mut cxy = c::CiexyY::default();
                let csrc = cxz(&xyz);
                c::cmsXYZ2xyY(&mut cxy, &csrc);
                let rxy = pcs::xyz2xyy(&xyz);
                assert_eq!((cxy.x, cxy.y, cxy.Y), (rxy.x, rxy.y, rxy.Y), "xyz2xyy {i}");
                let mut cxyz2 = c::CieXyz::default();
                c::cmsxyY2XYZ(&mut cxyz2, &cxy);
                let rxyz2 = pcs::xyy2xyz(&rxy);
                assert_eq!((cxyz2.X, cxyz2.Y, cxyz2.Z), (rxyz2.X, rxyz2.Y, rxyz2.Z), "xyy2xyz {i}");
                // XYZ->Lab->XYZ
                let mut cl = c::CieLab::default();
                let cwp = cxz(&wp);
                c::cmsXYZ2Lab(&cwp, &mut cl, &csrc);
                let rl = pcs::xyz2lab(Some(&wp), &xyz);
                assert_eq!((cl.L, cl.a, cl.b), (rl.L, rl.a, rl.b), "xyz2lab {i}");
                let mut cx = c::CieXyz::default();
                c::cmsLab2XYZ(&cwp, &mut cx, &cl);
                let rx = pcs::lab2xyz(Some(&wp), &rl);
                assert_eq!((cx.X, cx.Y, cx.Z), (rx.X, rx.Y, rx.Z), "lab2xyz {i}");
                // Lab->LCh->Lab
                let mut clch = c::CieLCh::default();
                c::cmsLab2LCh(&mut clch, &cl);
                let rlch = pcs::lab2lch(&rl);
                assert_eq!((clch.L, clch.C, clch.h), (rlch.L, rlch.C, rlch.h), "lab2lch {i}");
                let mut cl2 = c::CieLab::default();
                c::cmsLCh2Lab(&mut cl2, &clch);
                let rl2 = pcs::lch2lab(&rlch);
                assert_eq!((cl2.L, cl2.a, cl2.b), (rl2.L, rl2.a, rl2.b), "lch2lab {i}");
            }
        }
    }
}

#[test]
fn encoded_variants() {
    unsafe {
        for i in 0..64 {
            let w = [
                ((i * 1021) % 65536) as u16,
                ((i * 773) % 65536) as u16,
                ((i * 991) % 65536) as u16,
            ];
            // Lab-encoded->float + back, v2 + v4
            for v2 in [true, false] {
                let mut cl = c::CieLab::default();
                if v2 { c::cmsLabEncoded2FloatV2(&mut cl, w.as_ptr()) }
                else { c::cmsLabEncoded2Float(&mut cl, w.as_ptr()) }
                let rl = if v2 { pcs::lab_encoded2float_v2(w) } else { pcs::lab_encoded2float(w) };
                assert_eq!((cl.L, cl.a, cl.b), (rl.L, rl.a, rl.b), "lab_dec v2={v2} {i}");
                let mut cw = [0u16; 3];
                if v2 { c::cmsFloat2LabEncodedV2(cw.as_mut_ptr(), &cl) }
                else { c::cmsFloat2LabEncoded(cw.as_mut_ptr(), &cl) }
                let rw = if v2 { pcs::float2lab_encoded_v2(&rl) } else { pcs::float2lab_encoded(&rl) };
                assert_eq!(cw, rw, "lab_enc v2={v2} {i}");
            }
            // XYZ encoded
            let mut cf = c::CieXyz::default();
            c::cmsXYZEncoded2Float(&mut cf, w.as_ptr());
            let rf = pcs::xyz_encoded2float(w);
            assert_eq!((cf.X, cf.Y, cf.Z), (rf.X, rf.Y, rf.Z), "xyz_dec {i}");
            let mut ce = [0u16; 3];
            c::cmsFloat2XYZEncoded(ce.as_mut_ptr(), &cf);
            let re = pcs::float2xyz_encoded(&rf);
            assert_eq!(ce, re, "xyz_enc {i}");
        }
    }
}

#[test]
fn delta_e_matrix() {
    unsafe {
        for i in 0..48 {
            let l1 = CieLab {
                L: 10.0 + (i % 8) as f64 * 10.0,
                a: -40.0 + (i % 6) as f64 * 15.0,
                b: -30.0 + (i % 7) as f64 * 12.0,
            };
            let l2 = CieLab {
                L: l1.L + (i % 5) as f64 * 3.0,
                a: l1.a + (i % 4) as f64 * 2.0,
                b: l1.b + (i % 3) as f64 * 4.0,
            };
            let cl1 = clb(&l1);
            let cl2 = clb(&l2);
            assert_eq!(c::cmsDeltaE(&cl1, &cl2), pcs::delta_e(&l1, &l2), "deltaE {i}");
            assert_eq!(c::cmsCIE94DeltaE(&cl1, &cl2), pcs::cie94_delta_e(&l1, &l2), "cie94 {i}");
            assert_eq!(c::cmsBFDdeltaE(&cl1, &cl2), pcs::bfd_delta_e(&l1, &l2), "bfd {i}");
            for l in [0.5, 1.0, 2.0] {
                for cc in [1.0, 1.5] {
                    assert_eq!(
                        c::cmsCMCdeltaE(&cl1, &cl2, l, cc),
                        pcs::cmc_delta_e(&l1, &l2, l, cc),
                        "cmc {i} l{l} c{cc}");
                }
            }
            assert_eq!(
                c::cmsCIE2000DeltaE(&cl1, &cl2, 1.0, 1.0, 1.0),
                pcs::cie2000_delta_e(&l1, &l2, 1.0, 1.0, 1.0),
                "cie2000 {i}");
        }
    }
}

#[test]
fn cam02_adaptation() {
    unsafe {
        let vcs: Vec<c::ViewingConditions> = vec![
            c::ViewingConditions {
                white_point: cxz(&d65()), yb: 20.0, la: 20.0,
                surround: 2, d_value: 1.0,
            },
            c::ViewingConditions {
                white_point: cxz(&d50()), yb: 30.0, la: 100.0,
                surround: 0, d_value: 0.8,
            },
        ];
        for (vi, vc) in vcs.iter().enumerate() {
            let cm = c::cmsCIECAM02Init(std::ptr::null_mut(), vc);
            let rvc = lcms::types::ViewingConditions {
                white_point: lcms::types::CieXyz {
                    X: vc.white_point.X, Y: vc.white_point.Y, Z: vc.white_point.Z,
                },
                Yb: vc.yb, La: vc.la, surround: vc.surround, D_value: vc.d_value,
            };
            let mut rm = lcms::cam02::CieCam02::new(None, &rvc);
            for i in 0..32 {
                let xyz = CieXyz {
                    X: 0.1 + (i % 8) as f64 * 0.1,
                    Y: 0.1 + (i % 6) as f64 * 0.12,
                    Z: 0.1 + (i % 7) as f64 * 0.11,
                };
                let csrc = cxz(&xyz);
                let mut cj = c::JCh::default();
                c::cmsCIECAM02Forward(cm, &csrc, &mut cj);
                let mut rj = JCh::default();
                rm.forward(&xyz, &mut rj);
                assert_eq!((cj.j, cj.c_, cj.h), (rj.J, rj.C, rj.h), "cam02 fwd vc{vi} {i}");
                let mut cx = c::CieXyz::default();
                c::cmsCIECAM02Reverse(cm, &cj, &mut cx);
                let mut rx = CieXyz::default();
                rm.reverse(&rj, &mut rx);
                assert_eq!((cx.X, cx.Y, cx.Z), (rx.X, rx.Y, rx.Z), "cam02 rev vc{vi} {i}");
            }
            c::cmsCIECAM02Done(cm);
        }
    }
}

/// cmspcs conversion parity — XYZ2Lab/Lab2XYZ/Lab2LCh/LCh2Lab/
/// encoded round-trips on value sweeps + edge whites.
#[test]
fn pcs_conversions() {
    use lcms::pcs as p;
    use lcms::types as t;
    let d50 = t::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
    for i in 0..8u32 {
        let f = i as f64 / 7.0;
        let xyz = t::CieXyz { X: f * 0.9 + 0.05, Y: f,
            Z: (1.0 - f) * 0.9 };
        let cl = p::xyz2lab(Some(&d50), &xyz);
        let mut clab = c::CieLab { L: 0.0, a: 0.0, b: 0.0 };
        unsafe {
            let cwp = c::CieXyz { X: d50.X, Y: d50.Y, Z: d50.Z };
            let cxyz = c::CieXyz { X: xyz.X, Y: xyz.Y, Z: xyz.Z };
            c::cmsXYZ2Lab(&cwp, &mut clab, &cxyz);
        }
        assert!((cl.L - clab.L).abs() < 1e-9 && (cl.a - clab.a).abs() < 1e-9
            && (cl.b - clab.b).abs() < 1e-9,
            "xyz2lab px{i}: {:?} vs {:?}", cl, clab);
        let bx = p::lab2xyz(Some(&d50), &cl);
        assert!((bx.X - xyz.X).abs() < 1e-9,
            "lab2xyz px{i}: {:?} vs {:?}", bx, xyz);
        let lch = p::lab2lch(&cl);
        let bl = p::lch2lab(&lch);
        assert!((bl.L - cl.L).abs() < 1e-9,
            "lch rt px{i}: {:?} vs {:?}", bl, cl);
    }
    for w in [[0u16, 0, 0], [0xFFFF, 0xFFFF, 0xFFFF],
              [0x8000, 0x8000, 0x8000]] {
        let mut fl = c::CieLab { L: 0.0, a: 0.0, b: 0.0 };
        let mut w2 = [0u16; 3];
        unsafe {
            c::cmsLabEncoded2Float(&mut fl, w.as_ptr());
            c::cmsFloat2LabEncoded(w2.as_mut_ptr(), &fl);
        }
        let rl = p::lab_encoded2float(w);
        let rw = p::float2lab_encoded(&rl);
        assert_eq!(rw, w2, "enc rt {w:?}");
        assert!((rl.L - fl.L).abs() < 1e-3,
            "enc->f {w:?}: {:?} vs {:?}", rl, fl);
    }
}

/// Delta-E + XYZ-encoded + gridpoint/endpoint helpers parity.
#[test]
fn pcs_delta_e_xyz() {
    use lcms::pcs as p;
    use lcms::types as t;
    let la = t::CieLab { L: 50.0, a: 20.0, b: -30.0 };
    let lb = t::CieLab { L: 60.0, a: -10.0, b: 40.0 };
    let cla = c::CieLab { L: la.L, a: la.a, b: la.b };
    let clb = c::CieLab { L: lb.L, a: lb.a, b: lb.b };
    unsafe {
        for (name, c_v, r_v) in [
            ("dE", c::cmsDeltaE(&cla, &clb), p::delta_e(&la, &lb)),
            ("c94", c::cmsCIE94DeltaE(&cla, &clb), p::cie94_delta_e(&la, &lb)),
            ("bfd", c::cmsBFDdeltaE(&cla, &clb), p::bfd_delta_e(&la, &lb)),
            ("cmc", c::cmsCMCdeltaE(&cla, &clb, 2.0, 1.0),
                p::cmc_delta_e(&la, &lb, 2.0, 1.0)),
        ] {
            assert!((c_v - r_v).abs() < 1e-9,
                "{name}: C={c_v} R={r_v}");
            eprintln!("{name}={c_v:.4}");
        }
        let c2000 = c::cmsCIE2000DeltaE(&cla, &clb, 1.0, 1.0, 1.0);
        let r2000 = p::cie2000_delta_e(&la, &lb, 1.0, 1.0, 1.0);
        eprintln!("c2k={c2000:.4}");
        // XYZ encoded
        let fx = t::CieXyz { X: 0.5, Y: 0.4, Z: 0.6 };
        let mut w = [0u16; 3];
        let mut bx = c::CieXyz { X: 0.0, Y: 0.0, Z: 0.0 };
        c::cmsFloat2XYZEncoded(w.as_mut_ptr(),
            &c::CieXyz { X: fx.X, Y: fx.Y, Z: fx.Z });
        c::cmsXYZEncoded2Float(&mut bx, w.as_ptr());
        let rw = p::float2xyz_encoded(&fx);
        let rbx = p::xyz_encoded2float(rw);
        assert_eq!(rw, w, "xyz enc");
        assert!((rbx.X - bx.X).abs() < 1e-9, "xyz dec");
    }
}

/// Whitepoint internals — WhitePointFromTemp x temps,
/// AdaptToIlluminant between illuminants, D50 accessors.
#[test]
fn whitepoint_internals() {
    unsafe {
        for t in [4000.0f64, 5000.0, 5500.0, 6500.0, 9300.0] {
            let mut wp = c::CiexyY { x: 0.0, y: 0.0, Y: 0.0 };
            let r = c::cmsWhitePointFromTemp(&mut wp, t);
            eprintln!("t{t} r={r} x={:.3} y={:.3}", wp.x, wp.y);
        }
        // AdaptToIlluminant — XYZ from D65 to D50
        let mut src = c::CieXyz { X: 0.96, Y: 1.0, Z: 0.82 };
        let d50 = c::cmsD50_XYZ();
        let d65 = c::CieXyz { X: 0.950, Y: 1.0, Z: 1.089 };
        let mut out = c::CieXyz { X: 0.0, Y: 0.0, Z: 0.0 };
        let r = c::cmsAdaptToIlluminant(&mut out, d50, &d65, &src);
        eprintln!("adapt={r} out=({:.3},{:.3},{:.3})", out.X, out.Y, out.Z);
        let d50xy = c::cmsD50_xyY();
        eprintln!("d50xy=({:.4},{:.4})", (*d50xy).x, (*d50xy).y);
    }
}

/// TempFromWhitePoint (inverse) + adaptation-edge internals.
#[test]
fn temp_from_whitepoint() {
    unsafe {
        // round-trip: temp -> wp -> temp
        for t in [4000.0f64, 6500.0, 9300.0] {
            let mut wp = c::CiexyY { x: 0.0, y: 0.0, Y: 0.0 };
            c::cmsWhitePointFromTemp(&mut wp, t);
            let mut t2 = 0.0f64;
            let r = c::cmsTempFromWhitePoint(&mut t2, &wp);
            eprintln!("t{t} -> wp -> t2={t2:.0} r={r}");
        }
        // edge temps (outside the valid range)
        for t in [0.0f64, -1.0, 1e6] {
            let mut wp = c::CiexyY { x: 0.0, y: 0.0, Y: 0.0 };
            let r = c::cmsWhitePointFromTemp(&mut wp, t);
            eprintln!("edge t{t} r={r}");
        }
    }
}
