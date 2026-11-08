//! cmsgamma curve machinery — join/smooth/predicate/parametric-type.

use lcms::gamma;
use lcms_cref as c;

#[test]
fn tone_curve_predicates() {
    unsafe {
        // gamma, linear, descending, multisegment curves
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let lin = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 2,
            [0u16, 65535u16].as_ptr());
        let desc = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 2,
            [65535u16, 0u16].as_ptr());
        for (name, tc) in [("gamma", g), ("linear", lin), ("desc", desc)] {
            assert!(!tc.is_null(), "{name} alloc");
            let c_lin = c::cmsIsToneCurveLinear(tc);
            let c_mon = c::cmsIsToneCurveMonotonic(tc);
            let c_desc = c::cmsIsToneCurveDescending(tc);
            let c_multi = c::cmsIsToneCurveMultisegment(tc);
            let c_ptype = c::cmsGetToneCurveParametricType(tc);
            eprintln!("{name}: lin={c_lin} mon={c_mon} desc={c_desc} \
                multi={c_multi} ptype={c_ptype}");
        }
        // rust side — same predicates
        let rg = gamma::build_gamma(None, 2.2).unwrap();
        assert_eq!(c::cmsIsToneCurveLinear(g) != 0,
            gamma::is_tone_curve_linear(&rg), "lin predicate");
        assert_eq!(c::cmsIsToneCurveMonotonic(g) != 0,
            gamma::is_tone_curve_monotonic(&rg), "mon predicate");
        assert_eq!(c::cmsIsToneCurveDescending(g) != 0,
            gamma::is_tone_curve_descending(&rg), "desc predicate");
        assert_eq!(c::cmsIsToneCurveMultisegment(g) != 0,
            gamma::is_tone_curve_multisegment(&rg), "multi predicate");
        assert_eq!(c::cmsGetToneCurveParametricType(g),
            rg.parametric_type(), "ptype");
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(lin);
        c::cmsFreeToneCurve(desc);
    }
}

#[test]
fn join_tone_curve() {
    unsafe {
        // join two monotonic curves — y = X^-1(Y(t))
        let cx = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let cy = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let cj = c::cmsJoinToneCurve(std::ptr::null_mut(),
            cy, cx, 256);
        assert!(!cj.is_null(), "join");
        // eval the joined curve — should be ~identity
        let cin = c::cmsEvalToneCurve16(cj, 32768);
        let rx = gamma::build_gamma(None, 2.2).unwrap();
        let ry = gamma::build_gamma(None, 1.8).unwrap();
        let rj = gamma::join_tone_curve(None, &ry, &rx, 256);
        if let Some(rjc) = rj {
            let rin = rjc.eval16(32768);
            eprintln!("join eval c={cin} r={rin}");
            assert_eq!(cin, rin, "join eval");
        } else {
            panic!("join failed");
        }
        c::cmsFreeToneCurve(cx);
        c::cmsFreeToneCurve(cy);
        c::cmsFreeToneCurve(cj);
    }
}

#[test]
fn smooth_tone_curve() {
    unsafe {
        // build a noisy tabulated curve, smooth it
        let vals: Vec<u16> = (0..=255).map(|i|
            ((i as u32 * 65535) / 255 + (i % 3) as u32 * 10) as u16
        ).collect();
        let ct = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 256, vals.as_ptr());
        assert!(!ct.is_null());
        let c_ok = c::cmsSmoothToneCurve(ct, 1.0);
        eprintln!("smooth c_ok={c_ok}");
        // eval before/after
        let c_v = c::cmsEvalToneCurve16(ct, 32768);
        // rust side
        let mut rt = gamma::build_tabulated_tone_curve16(
            None, 256, Some(&vals)).unwrap();
        let r_ok = gamma::smooth_tone_curve(None, &mut rt, 1.0);
        assert_eq!(c_ok != 0, r_ok, "smooth status");
        c::cmsFreeToneCurve(ct);
    }
}
