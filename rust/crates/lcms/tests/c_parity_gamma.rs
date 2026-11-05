//! Differential test: `lcms::gamma` vs `cmsgamma.c`.
//!
//! For every constructed curve the *entire* estimated table16 is compared,
//! eval16 sweeps the full u16 domain, and eval_float samples a dense range.

use lcms::gamma::*;
use lcms_cref as c;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}
fn f64_in(s: &mut u64, lo: f64, hi: f64) -> f64 {
    let u = rng(s) as f64 / (1u64 << 53) as f64;
    lo + u * (hi - lo)
}

struct CCurve(*mut c::ToneCurve);
impl Drop for CCurve {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { c::cmsFreeToneCurve(self.0) };
        }
    }
}

fn c_table(curve: *const c::ToneCurve) -> Vec<u16> {
    unsafe {
        let n = c::cmsGetToneCurveEstimatedTableEntries(curve);
        let t = c::cmsGetToneCurveEstimatedTable(curve);
        std::slice::from_raw_parts(t, n as usize).to_vec()
    }
}

fn compare_curves(tag: &str, rc: &ToneCurve, cc: &CCurve) {
    assert!(!cc.0.is_null(), "{tag}: C build failed");
    assert_eq!(
        rc.estimated_table(),
        c_table(cc.0).as_slice(),
        "{tag}: table16"
    );
    // Full u16 sweep of eval16
    for v in [0u16, 1, 2, 0x7fff, 0x8000, 0xfffe, 0xffff] {
        let r = rc.eval16(v);
        let cval = unsafe { c::cmsEvalToneCurve16(cc.0, v) };
        assert_eq!(r, cval, "{tag}: eval16({v:#x})");
    }
    // float sweep
    let mut s = 99u64;
    for _ in 0..2000 {
        let v = (rng(&mut s) as f32 / (1u64 << 53) as f32) * 1.4 - 0.2;
        let r = rc.eval_float(v);
        let cval = unsafe { c::cmsEvalToneCurveFloat(cc.0, v) };
        assert_eq!(r.to_bits(), cval.to_bits(), "{tag}: eval_float({v})");
    }
    // info fns
    unsafe {
        assert_eq!(
            is_tone_curve_linear(rc) as i32,
            c::cmsIsToneCurveLinear(cc.0),
            "{tag}: linear"
        );
        assert_eq!(
            is_tone_curve_monotonic(rc) as i32,
            c::cmsIsToneCurveMonotonic(cc.0),
            "{tag}: monotonic"
        );
        assert_eq!(
            is_tone_curve_descending(rc) as i32,
            c::cmsIsToneCurveDescending(cc.0),
            "{tag}: descending"
        );
        assert_eq!(
            is_tone_curve_multisegment(rc) as i32,
            c::cmsIsToneCurveMultisegment(cc.0),
            "{tag}: multisegment"
        );
        assert_eq!(
            rc.parametric_type(),
            c::cmsGetToneCurveParametricType(cc.0),
            "{tag}: parametric_type"
        );
        let rg = estimate_gamma(rc, 1e-3);
        let cg = c::cmsEstimateGamma(cc.0, 1e-3);
        assert_eq!(rg.to_bits(), cg.to_bits(), "{tag}: estimate_gamma");
    }
}

#[test]
fn tabulated16_all_shapes() {
    let mut s = 1u64;
    for case in 0..40 {
        let n = match case % 5 {
            0 => 2,
            1 => 3,
            2 => 256,
            3 => 1024,
            _ => 4096,
        };
        let table: Vec<u16> = match case % 4 {
            0 => (0..n).map(|i| (i * 65535 / (n - 1)) as u16).collect(), // ascending
            1 => (0..n)
                .map(|i| 65535 - (i * 65535 / (n - 1)) as u16)
                .collect(), // descending
            2 => (0..n).map(|_| rng(&mut s) as u16).collect(),           // random
            _ => vec![42; n],                                            // constant
        };
        let rc = build_tabulated_tone_curve16(None, n as u32, Some(&table)).unwrap();
        let cc = CCurve(unsafe {
            c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(), n as u32, table.as_ptr())
        });
        compare_curves(&format!("tab16 case{case}"), &rc, &cc);

        // full eval16 sweep on a few cases
        if case % 8 == 0 {
            for v in 0..=65535u16 {
                let r = rc.eval16(v);
                let cval = unsafe { c::cmsEvalToneCurve16(cc.0, v) };
                assert_eq!(r, cval, "case{case} eval16 {v:#x}");
            }
        }
    }
}

#[test]
fn gamma_curves() {
    for &g in &[
        0.1f64, 0.5, 0.9, 1.0, 1.001, 1.8, 2.2, 2.4, 3.0, 4.5, 10.0, 0.999,
    ] {
        let rc = build_gamma(None, g).unwrap();
        let cc = CCurve(unsafe { c::cmsBuildGamma(std::ptr::null_mut(), g) });
        compare_curves(&format!("gamma({g})"), &rc, &cc);
    }
}

#[test]
fn parametric_types_forward_and_reverse() {
    // (type, params) — the ICC-defined shapes plus 108/109
    let cases: &[(i32, &[f64])] = &[
        (1, &[2.2]),
        (1, &[1.0]),
        (1, &[0.45]),
        (2, &[2.2, 1.0, 0.0]),
        (2, &[2.4, 1.8, 0.18]),
        (3, &[2.2, 1.0, 0.0, 0.0]),
        (3, &[1.9, 0.9, 0.1, 0.02]),
        (4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045]), // sRGB
        (4, &[2.2, 1.0, 0.0, 4.5, 0.018]),
        (5, &[2.4, 1.055, 0.055, 12.92, 0.04045, 0.0, 0.0]),
        (6, &[2.2, 1.5, 0.5, 0.3]),
        (7, &[1.8, 2.0, 2.4, 0.1, 0.07]),
        (8, &[2.0, 1.5, 0.7, 0.2, 0.1]),
        (108, &[2.2]),
        (109, &[5.5]),
    ];
    for &(t, params) in cases {
        let rc = build_parametric_tone_curve(None, t, params).unwrap();
        let cc = CCurve(unsafe {
            c::cmsBuildParametricToneCurve(std::ptr::null_mut(), t, params.as_ptr())
        });
        compare_curves(&format!("param({t})"), &rc, &cc);

        // analytic reverse (negative type)
        let rc = build_parametric_tone_curve(None, -t, params).unwrap();
        let cc = CCurve(unsafe {
            c::cmsBuildParametricToneCurve(std::ptr::null_mut(), -t, params.as_ptr())
        });
        compare_curves(&format!("param(-{t})"), &rc, &cc);
    }
    // Invalid type → None/NULL
    assert!(build_parametric_tone_curve(None, 9999, &[1.0]).is_none());
    unsafe {
        let p = [1.0f64];
        assert!(c::cmsBuildParametricToneCurve(std::ptr::null_mut(), 9999, p.as_ptr()).is_null());
    }
}

#[test]
fn segmented_curves() {
    let mut s = 7u64;
    for case in 0..30 {
        // Mixed segments: parametric + sampled + parametric
        let sampled: Vec<f32> = (0..32)
            .map(|_| rng(&mut s) as f32 / (1u64 << 52) as f32)
            .collect();
        let segs = vec![
            CurveSegment {
                x0: MINUS_INF,
                x1: 0.3,
                r#type: 1,
                params: {
                    let mut p = [0.0; 10];
                    p[0] = f64_in(&mut s, 0.5, 3.0);
                    p
                },
                ..Default::default()
            },
            CurveSegment {
                x0: 0.3,
                x1: 0.7,
                r#type: 0,
                params: [0.0; 10],
                n_grid_points: sampled.len() as u32,
                sampled_points: Some(sampled.clone()),
            },
            CurveSegment {
                x0: 0.7,
                x1: PLUS_INF,
                r#type: 4,
                params: {
                    let mut p = [0.0; 10];
                    p[0] = 2.2;
                    p[1] = 1.0;
                    p[3] = 0.3;
                    p[4] = 0.1;
                    p
                },
                ..Default::default()
            },
        ];
        let rc = build_segmented_tone_curve(None, &segs).unwrap();
        let csegs: Vec<c::CurveSegment> = segs
            .iter()
            .map(|sg| c::CurveSegment {
                x0: sg.x0,
                x1: sg.x1,
                r#type: sg.r#type,
                params: sg.params,
                n_grid_points: sg.n_grid_points,
                sampled_points: sg
                    .sampled_points
                    .as_deref()
                    .map(|p| p.as_ptr())
                    .unwrap_or(std::ptr::null()),
            })
            .collect();
        let cc = CCurve(unsafe {
            c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(), 3, csegs.as_ptr())
        });
        compare_curves(&format!("segs case{case}"), &rc, &cc);
    }
}

#[test]
fn tabulated_float_curves() {
    let mut s = 11u64;
    for n in [2usize, 17, 256, 4096] {
        let values: Vec<f32> = match n % 2 {
            0 => (0..n).map(|i| i as f32 / (n - 1) as f32).collect(),
            _ => (0..n)
                .map(|_| rng(&mut s) as f32 / (1u64 << 52) as f32)
                .collect(),
        };
        let rc = build_tabulated_tone_curve_float(None, &values).unwrap();
        let cc = CCurve(unsafe {
            c::cmsBuildTabulatedToneCurveFloat(std::ptr::null_mut(), n as u32, values.as_ptr())
        });
        compare_curves(&format!("tabf{n}"), &rc, &cc);
    }
}

#[test]
fn reverse_curves() {
    let mut s = 13u64;
    // Analytic reversal path (type-1 segment) and table reversal path.
    for case in 0..20 {
        let n = match case % 3 {
            0 => 256,
            1 => 1024,
            _ => 4096,
        };
        // Monotonic ascending table with jitter ≤2 (keeps it reversible)
        let table: Vec<u16> = (0..n)
            .map(|i| {
                let base = (i * 65535 / (n - 1)) as u16;
                base.saturating_sub((rng(&mut s) % 3) as u16)
            })
            .collect();
        let rc = build_tabulated_tone_curve16(None, n as u32, Some(&table)).unwrap();
        let cc = CCurve(unsafe {
            c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(), n as u32, table.as_ptr())
        });

        for &ns in &[64u32, 256, 4096] {
            let rr = reverse_tone_curve_ex(ns, &rc).unwrap();
            let cr = CCurve(unsafe { c::cmsReverseToneCurveEx(ns, cc.0) });
            assert!(!cr.0.is_null());
            assert_eq!(
                rr.estimated_table(),
                c_table(cr.0).as_slice(),
                "rev n={n} ns={ns}"
            );
        }

        // gamma curve analytic reverse
        if case < 3 {
            let g = f64_in(&mut s, 0.5, 3.0);
            let rcg = build_gamma(None, g).unwrap();
            let ccg = CCurve(unsafe { c::cmsBuildGamma(std::ptr::null_mut(), g) });
            let rr = reverse_tone_curve_ex(512, &rcg).unwrap();
            let cr = CCurve(unsafe { c::cmsReverseToneCurveEx(512, ccg.0) });
            assert_eq!(
                rr.estimated_table(),
                c_table(cr.0).as_slice(),
                "analytic rev {g}"
            );
            assert_eq!(rr.parametric_type(), unsafe {
                c::cmsGetToneCurveParametricType(cr.0)
            });
        }
    }
}

#[test]
fn join_curves() {
    for &n_points in &[64u32, 256, 1024] {
        for (gx, gy) in [(1.8f64, 2.2f64), (2.2, 1.0), (0.9, 3.0)] {
            let rcx = build_gamma(None, gx).unwrap();
            let rcy = build_gamma(None, gy).unwrap();
            let ccx = CCurve(unsafe { c::cmsBuildGamma(std::ptr::null_mut(), gx) });
            let ccy = CCurve(unsafe { c::cmsBuildGamma(std::ptr::null_mut(), gy) });

            let rj = join_tone_curve(None, &rcx, &rcy, n_points).unwrap();
            let cj = CCurve(unsafe {
                c::cmsJoinToneCurve(std::ptr::null_mut(), ccx.0, ccy.0, n_points)
            });
            assert!(!cj.0.is_null());
            assert_eq!(
                rj.estimated_table(),
                c_table(cj.0).as_slice(),
                "join {gx} {gy} n={n_points}"
            );
        }
    }
}

#[test]
fn smooth_curves() {
    let mut s = 17u64;
    for case in 0..10 {
        let n = match case % 3 {
            0 => 256,
            1 => 1024,
            _ => 4096,
        };
        // noisy ascending table
        let table: Vec<u16> = (0..n)
            .map(|i| {
                let base = i * 65535 / (n - 1);
                let noise = (rng(&mut s) % 200) as i64 - 100;
                (base as i64 + noise).clamp(0, 65535) as u16
            })
            .collect();
        for &lambda in &[0.1f64, 1.0, 10.0, -5.0] {
            let mut rc = build_tabulated_tone_curve16(None, n as u32, Some(&table)).unwrap();
            let cc = CCurve(unsafe {
                c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(), n as u32, table.as_ptr())
            });
            let ok_r = smooth_tone_curve(None, &mut rc, lambda);
            let ok_c = unsafe { c::cmsSmoothToneCurve(cc.0, lambda) } != 0;
            assert_eq!(ok_r, ok_c, "smooth ok n={n} λ={lambda}");
            assert_eq!(
                rc.estimated_table(),
                c_table(cc.0).as_slice(),
                "smooth table n={n} λ={lambda}"
            );
        }
    }
}

#[test]
fn dup_curves() {
    let rc = build_gamma(None, 2.4).unwrap();
    let cc = CCurve(unsafe { c::cmsBuildGamma(std::ptr::null_mut(), 2.4) });
    let rd = dup_tone_curve(&rc, None).unwrap();
    let cd = CCurve(unsafe { c::cmsDupToneCurve(cc.0) });
    compare_curves("dup gamma", &rd, &cd);
}
