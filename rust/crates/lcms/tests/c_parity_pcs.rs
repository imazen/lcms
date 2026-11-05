//! Differential test: `lcms::pcs` vs `cmspcs.c`.

use lcms::pcs::*;
use lcms::types::*;
use lcms::wtpnt::d50_xyz;
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
fn clab(v: &CieLab) -> c::CieLab {
    c::CieLab {
        L: v.L,
        a: v.a,
        b: v.b,
    }
}
fn clch(v: &CieLCh) -> c::CieLCh {
    c::CieLCh {
        L: v.L,
        C: v.C,
        h: v.h,
    }
}
fn cxyy(v: &CiexyY) -> c::CiexyY {
    c::CiexyY {
        x: v.x,
        y: v.y,
        Y: v.Y,
    }
}

#[test]
fn xyz_xyy_round() {
    let mut s = 1u64;
    for _ in 0..100_000 {
        let xyz = CieXyz {
            X: f64_in(&mut s, -0.5, 2.0),
            Y: f64_in(&mut s, -0.5, 2.0),
            Z: f64_in(&mut s, -0.5, 2.0),
        };
        let mut c_out = c::CiexyY::default();
        unsafe { c::cmsXYZ2xyY(&mut c_out, &cxyz(&xyz)) };
        let r = xyz2xyy(&xyz);
        assert_eq!(r.x.to_bits(), c_out.x.to_bits(), "x {xyz:?}");
        assert_eq!(r.y.to_bits(), c_out.y.to_bits(), "y {xyz:?}");
        assert_eq!(r.Y.to_bits(), c_out.Y.to_bits(), "Y {xyz:?}");

        let mut cx = c::CieXyz::default();
        unsafe { c::cmsxyY2XYZ(&mut cx, &cxyy(&r)) };
        let back = xyy2xyz(&r);
        assert_eq!(back.X.to_bits(), cx.X.to_bits(), "X back");
        assert_eq!(back.Y.to_bits(), cx.Y.to_bits(), "Y back");
        assert_eq!(back.Z.to_bits(), cx.Z.to_bits(), "Z back");
    }
}

#[test]
fn xyz_lab_both_directions() {
    let mut s = 2u64;
    let wps: [Option<CieXyz>; 3] = [
        None,
        Some(d50_xyz()),
        Some(CieXyz {
            X: 0.95047,
            Y: 1.0,
            Z: 1.08883,
        }), // D65
    ];
    for wp in &wps {
        for _ in 0..50_000 {
            let xyz = CieXyz {
                X: f64_in(&mut s, -0.2, 2.0),
                Y: f64_in(&mut s, -0.2, 2.0),
                Z: f64_in(&mut s, -0.2, 2.0),
            };
            let cw = wp.map(|w| cxyz(&w));
            let cw_ptr = cw
                .as_ref()
                .map(|x| x as *const c::CieXyz)
                .unwrap_or(std::ptr::null());

            let mut cl = c::CieLab::default();
            unsafe { c::cmsXYZ2Lab(cw_ptr, &mut cl, &cxyz(&xyz)) };
            let lab = xyz2lab(wp.as_ref(), &xyz);
            assert_eq!(lab.L.to_bits(), cl.L.to_bits(), "L {xyz:?} wp={wp:?}");
            assert_eq!(lab.a.to_bits(), cl.a.to_bits(), "a {xyz:?}");
            assert_eq!(lab.b.to_bits(), cl.b.to_bits(), "b {xyz:?}");

            let mut cx = c::CieXyz::default();
            unsafe { c::cmsLab2XYZ(cw_ptr, &mut cx, &cl) };
            let back = lab2xyz(wp.as_ref(), &lab);
            assert_eq!(back.X.to_bits(), cx.X.to_bits(), "X back {lab:?}");
            assert_eq!(back.Y.to_bits(), cx.Y.to_bits(), "Y back");
            assert_eq!(back.Z.to_bits(), cx.Z.to_bits(), "Z back");
        }
    }
}

#[test]
fn lab_encodings_v2_v4() {
    let mut s = 3u64;
    for _ in 0..100_000 {
        let w = [rng(&mut s) as u16, rng(&mut s) as u16, rng(&mut s) as u16];

        let mut cl = c::CieLab::default();
        unsafe { c::cmsLabEncoded2Float(&mut cl, w.as_ptr()) };
        let lab = lab_encoded2float(w);
        assert_eq!(lab.L.to_bits(), cl.L.to_bits(), "v4 L {w:?}");
        assert_eq!(lab.a.to_bits(), cl.a.to_bits(), "v4 a");
        assert_eq!(lab.b.to_bits(), cl.b.to_bits(), "v4 b");

        unsafe { c::cmsLabEncoded2FloatV2(&mut cl, w.as_ptr()) };
        let lab = lab_encoded2float_v2(w);
        assert_eq!(lab.L.to_bits(), cl.L.to_bits(), "v2 L {w:?}");
        assert_eq!(lab.a.to_bits(), cl.a.to_bits(), "v2 a");
        assert_eq!(lab.b.to_bits(), cl.b.to_bits(), "v2 b");

        // encode path: float Lab across the clamp domain
        let fl = CieLab {
            L: f64_in(&mut s, -20.0, 130.0),
            a: f64_in(&mut s, -140.0, 140.0),
            b: f64_in(&mut s, -140.0, 140.0),
        };
        let mut cw = [0u16; 3];
        unsafe { c::cmsFloat2LabEncoded(cw.as_mut_ptr(), &clab(&fl)) };
        assert_eq!(float2lab_encoded(&fl), cw, "v4 enc {fl:?}");
        unsafe { c::cmsFloat2LabEncodedV2(cw.as_mut_ptr(), &clab(&fl)) };
        assert_eq!(float2lab_encoded_v2(&fl), cw, "v2 enc {fl:?}");
    }
}

#[test]
fn lab_lch_round() {
    let mut s = 4u64;
    for _ in 0..100_000 {
        let lab = CieLab {
            L: f64_in(&mut s, -10.0, 110.0),
            a: f64_in(&mut s, -128.0, 128.0),
            b: f64_in(&mut s, -128.0, 128.0),
        };
        let mut cl = c::CieLCh::default();
        unsafe { c::cmsLab2LCh(&mut cl, &clab(&lab)) };
        let lch = lab2lch(&lab);
        assert_eq!(lch.L.to_bits(), cl.L.to_bits(), "L");
        assert_eq!(lch.C.to_bits(), cl.C.to_bits(), "C {lab:?}");
        assert_eq!(lch.h.to_bits(), cl.h.to_bits(), "h {lab:?}");

        let mut cb = c::CieLab::default();
        unsafe { c::cmsLCh2Lab(&mut cb, &clch(&lch)) };
        let back = lch2lab(&lch);
        assert_eq!(back.L.to_bits(), cb.L.to_bits(), "L back");
        assert_eq!(back.a.to_bits(), cb.a.to_bits(), "a back {lch:?}");
        assert_eq!(back.b.to_bits(), cb.b.to_bits(), "b back");
    }
}

#[test]
fn xyz_encodings() {
    let mut s = 5u64;
    for _ in 0..100_000 {
        let f = CieXyz {
            X: f64_in(&mut s, -0.5, 2.5),
            Y: f64_in(&mut s, -0.5, 2.5),
            Z: f64_in(&mut s, -0.5, 2.5),
        };
        let mut cw = [0u16; 3];
        unsafe { c::cmsFloat2XYZEncoded(cw.as_mut_ptr(), &cxyz(&f)) };
        assert_eq!(float2xyz_encoded(&f), cw, "enc {f:?}");

        let w = [rng(&mut s) as u16, rng(&mut s) as u16, rng(&mut s) as u16];
        let mut cf = c::CieXyz::default();
        unsafe { c::cmsXYZEncoded2Float(&mut cf, w.as_ptr()) };
        let d = xyz_encoded2float(w);
        assert_eq!(d.X.to_bits(), cf.X.to_bits(), "dec X {w:?}");
        assert_eq!(d.Y.to_bits(), cf.Y.to_bits(), "dec Y");
        assert_eq!(d.Z.to_bits(), cf.Z.to_bits(), "dec Z");
    }
}

#[test]
fn delta_e_family() {
    let mut s = 6u64;
    for _ in 0..50_000 {
        let l1 = CieLab {
            L: f64_in(&mut s, 0.0, 100.0),
            a: f64_in(&mut s, -128.0, 128.0),
            b: f64_in(&mut s, -128.0, 128.0),
        };
        let l2 = CieLab {
            L: f64_in(&mut s, 0.0, 100.0),
            a: f64_in(&mut s, -128.0, 128.0),
            b: f64_in(&mut s, -128.0, 128.0),
        };
        let c1 = clab(&l1);
        let c2 = clab(&l2);

        let de = unsafe { c::cmsDeltaE(&c1, &c2) };
        assert_eq!(delta_e(&l1, &l2).to_bits(), de.to_bits(), "DeltaE");

        let d94 = unsafe { c::cmsCIE94DeltaE(&c1, &c2) };
        assert_eq!(cie94_delta_e(&l1, &l2).to_bits(), d94.to_bits(), "CIE94");

        let bfd = unsafe { c::cmsBFDdeltaE(&c1, &c2) };
        assert_eq!(bfd_delta_e(&l1, &l2).to_bits(), bfd.to_bits(), "BFD");

        let cmc = unsafe { c::cmsCMCdeltaE(&c1, &c2, 2.0, 1.0) };
        assert_eq!(
            cmc_delta_e(&l1, &l2, 2.0, 1.0).to_bits(),
            cmc.to_bits(),
            "CMC"
        );

        let d00 = unsafe { c::cmsCIE2000DeltaE(&c1, &c2, 1.0, 1.0, 1.0) };
        assert_eq!(
            cie2000_delta_e(&l1, &l2, 1.0, 1.0, 1.0).to_bits(),
            d00.to_bits(),
            "CIE2000 {l1:?} {l2:?}"
        );
    }
}

#[test]
fn channels_and_endpoints() {
    // Sweep a range of u32 signature values, valid and invalid alike.
    let mut s = 7u64;
    for _ in 0..20_000 {
        let sig = rng(&mut s) as u32;
        let c_n = unsafe { c::cmsChannelsOfColorSpace(sig) };
        assert_eq!(channels_of_color_space(sig), c_n, "sig={sig:#010x}");
        let c_dep = unsafe { c::cmsChannelsOf(sig) };
        assert_eq!(channels_of(sig), c_dep);
    }
    // Explicit coverage of every defined colorspace signature.
    use lcms::signatures::colorspace as cs;
    for sig in [
        cs::SigXYZData,
        cs::SigLabData,
        cs::SigLuvData,
        cs::SigYCbCrData,
        cs::SigYxyData,
        cs::SigRgbData,
        cs::SigGrayData,
        cs::SigHsvData,
        cs::SigHlsData,
        cs::SigCmykData,
        cs::SigCmyData,
        cs::SigLuvKData,
        cs::SigNamedData,
        cs::Sig1colorData,
        cs::Sig2colorData,
        cs::Sig3colorData,
        cs::Sig4colorData,
        cs::Sig5colorData,
        cs::Sig6colorData,
        cs::Sig7colorData,
        cs::Sig8colorData,
        cs::Sig9colorData,
        cs::Sig10colorData,
        cs::Sig11colorData,
        cs::Sig12colorData,
        cs::Sig13colorData,
        cs::Sig14colorData,
        cs::Sig15colorData,
        cs::SigMCH1Data,
        cs::SigMCH2Data,
        cs::SigMCH3Data,
        cs::SigMCH4Data,
        cs::SigMCH5Data,
        cs::SigMCH6Data,
        cs::SigMCH7Data,
        cs::SigMCH8Data,
        cs::SigMCH9Data,
        cs::SigMCHAData,
        cs::SigMCHBData,
        cs::SigMCHCData,
        cs::SigMCHDData,
        cs::SigMCHEData,
        cs::SigMCHFData,
    ] {
        let c_n = unsafe { c::cmsChannelsOfColorSpace(sig) };
        assert_eq!(channels_of_color_space(sig), c_n, "sig={sig:#010x}");

        let mut cw: *const u16 = std::ptr::null();
        let mut cb: *const u16 = std::ptr::null();
        let mut cn: u32 = 0;
        let ok = unsafe { c::_cmsEndPointsBySpace(sig, &mut cw, &mut cb, &mut cn) } != 0;
        match end_points_by_space(sig) {
            Some((w, b, n)) => {
                assert!(ok, "endpoints {sig:#010x}");
                assert_eq!(n, cn);
                unsafe {
                    for i in 0..n as usize {
                        assert_eq!(w[i], *cw.add(i), "white[{i}] {sig:#x}");
                        assert_eq!(b[i], *cb.add(i), "black[{i}] {sig:#x}");
                    }
                }
            }
            None => assert!(!ok, "endpoints unknown {sig:#010x}"),
        }
    }
}

#[test]
fn gridpoints_and_space_maps() {
    use lcms::signatures::colorspace as cs;
    let sigs = [
        cs::SigGrayData,
        cs::SigRgbData,
        cs::SigCmykData,
        cs::SigCmyData,
        cs::SigXYZData,
        cs::SigLabData,
        cs::SigMCH6Data,
        cs::Sig15colorData,
    ];
    for sig in sigs {
        for flags in [
            0u32,
            0x0400,
            0x0800,
            0x0001_0000,
            0x0037_0000,
            0x0400 | 0x0023_0000,
        ] {
            let c_g = unsafe { c::_cmsReasonableGridpointsByColorspace(sig, flags) };
            assert_eq!(
                reasonable_gridpoints_by_colorspace(sig, flags),
                c_g,
                "sig={sig:#x} flags={flags:#x}"
            );
        }
    }
    // _cmsICCcolorSpace / _cmsLCMScolorSpace sweeps
    for i in -3..40 {
        let c_sig = unsafe { cref_icc_color_space(i) };
        assert_eq!(icc_color_space(i), c_sig, "our={i}");
    }
    for sig in sigs.iter().copied().chain([0xdeadbeef, 0]) {
        let c_pt = unsafe { cref_lcms_color_space(sig) };
        assert_eq!(lcms_color_space(sig), c_pt, "sig={sig:#x}");
    }
}

// _cmsICCcolorSpace / _cmsLCMScolorSpace are exported; bind them here.
unsafe fn cref_icc_color_space(our: i32) -> u32 {
    unsafe { lcms_cref::_cmsICCcolorSpace(our) }
}
unsafe fn cref_lcms_color_space(sig: u32) -> i32 {
    unsafe { lcms_cref::_cmsLCMScolorSpace(sig) }
}

