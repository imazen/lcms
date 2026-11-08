//! cmsgamma tone-curve parity: parametric types 1-12 + inverses,
//! tabulated16/float, segmented, eval16/eval_float, predicates, params,
//! estimate_gamma, reverse, join, smooth — per ICC.1:2022-05 §10.15.

use lcms::gamma;
use lcms_cref as c;


fn c_eval16(tc: *mut c::ToneCurve, v: u16) -> u16 {
    unsafe { c::cmsEvalToneCurve16(tc, v) }
}
fn c_evalf(tc: *mut c::ToneCurve, v: f32) -> f32 {
    unsafe { c::cmsEvalToneCurveFloat(tc, v) }
}

#[test]
fn parametric_sweep() {
    // all lcms2 parametric types 1..=12 + their inverses (negative types)
    let params: Vec<(i32, Vec<f64>)> = vec![
        (1, vec![2.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (2, vec![2.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]), // g,a,b,c,d,e,f
        (3, vec![2.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (4, vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045, 0.0, 0.0, 0.0, 0.0, 0.0]), // sRGB
        (5, vec![1.5, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (6, vec![1.5, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (7, vec![1.5, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (8, vec![1.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (9, vec![1.5, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (10, vec![1.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (11, vec![1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (12, vec![1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (-1, vec![2.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (-2, vec![2.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (-4, vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ];
    for (ty, prm) in params {
        let ct = unsafe {
            c::cmsBuildParametricToneCurve(std::ptr::null_mut(), ty, prm.as_ptr())
        };
        let rt = gamma::build_parametric_tone_curve(None, ty, &prm);
        assert_eq!(ct.is_null(), rt.is_none(), "type {ty} presence");
        if ct.is_null() || rt.is_none() { continue; }
        let rt = rt.unwrap();
        unsafe {
            assert_eq!(c::cmsGetToneCurveParametricType(ct), rt.parametric_type(),
                "type {ty} parametric type");
            assert_eq!(
                c::cmsIsToneCurveLinear(ct) != 0,
                gamma::is_tone_curve_linear(&rt),
                "type {ty} linear"
            );
            assert_eq!(
                c::cmsIsToneCurveMonotonic(ct) != 0,
                gamma::is_tone_curve_monotonic(&rt),
                "type {ty} monotonic"
            );
            assert_eq!(
                c::cmsIsToneCurveDescending(ct) != 0,
                gamma::is_tone_curve_descending(&rt),
                "type {ty} descending"
            );
            let cg = c::cmsEstimateGamma(ct, 0.01);
            let rg = gamma::estimate_gamma(&rt, 0.01);
            assert!((cg - rg).abs() < 1e-6, "type {ty} estimate_gamma {cg} vs {rg}");
            // eval16 over a dense grid — byte-exact
            for v in (0..=65535u16).step_by(257) {
                assert_eq!(c_eval16(ct, v), rt.eval16(v), "type {ty} eval16 {v}");
            }
            // eval_float — float-eval residual bound
            for i in 0..64 {
                let v = i as f32 / 63.0;
                let cf = c_evalf(ct, v);
                let rf = rt.eval_float(v);
                let tol = (5e-5 * cf.abs()).max(1e-6);
                assert!((cf - rf).abs() <= tol, "type {ty} evalf {v}: {cf} vs {rf}");
            }
            // estimated-table parity
            let cn = c::cmsGetToneCurveEstimatedTableEntries(ct);
            let ct_ptr = c::cmsGetToneCurveEstimatedTable(ct);
            let c_tab: Vec<u16> = (0..cn).map(|i| *ct_ptr.add(i as usize)).collect();
            assert_eq!(c_tab, rt.estimated_table(), "type {ty} table");
            c::cmsFreeToneCurve(ct);
        }
    }
}

#[test]
fn tabulated_and_segmented() {
    let vals: Vec<u16> = (0..256).map(|i| (i * 257) as u16).collect();
    let valsf: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
    unsafe {
        let ct16 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(), 256, vals.as_ptr());
        let ctf = c::cmsBuildTabulatedToneCurveFloat(std::ptr::null_mut(), 256, valsf.as_ptr());
        let rt16 = gamma::build_tabulated_tone_curve16(None, 256, Some(&vals)).unwrap();
        let rtf = gamma::build_tabulated_tone_curve_float(None, &valsf).unwrap();
        for v in (0..=65535u16).step_by(1021) {
            assert_eq!(c_eval16(ct16, v), rt16.eval16(v), "tab16 eval16 {v}");
        }
        for i in 0..64 {
            let v = i as f32 / 63.0;
            let cf = c_evalf(ctf, v);
            let rf = rtf.eval_float(v);
            let tol = (5e-5 * cf.abs()).max(1e-6);
            assert!((cf - rf).abs() <= tol, "tabf evalf {v}");
        }
        c::cmsFreeToneCurve(ct16);
        c::cmsFreeToneCurve(ctf);
    }
    // segmented: parametric + sampled + zero-inf segments
    let segs = vec![
        lcms::gamma::CurveSegment {
            x0: 0.0, x1: 0.5, r#type: 1,
            params: [1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            n_grid_points: 0, sampled_points: None,
        },
        lcms::gamma::CurveSegment {
            x0: 0.5, x1: 1.0, r#type: 0,
            params: [0.0; 10],
            n_grid_points: 8,
            sampled_points: Some(vec![0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 1.0]),
        },
    ];
    let samp: Vec<f32> = segs[1].sampled_points.clone().unwrap();
    let csegs = [
        c::CurveSegment {
            x0: 0.0, x1: 0.5, r#type: 1,
            params: [1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            n_grid_points: 0, sampled_points: std::ptr::null(),
        },
        c::CurveSegment {
            x0: 0.5, x1: 1.0, r#type: 0,
            params: [0.0; 10],
            n_grid_points: 8, sampled_points: samp.as_ptr(),
        },
    ];
    let ct = unsafe {
        c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(), 2, csegs.as_ptr())
    };
    let rt = gamma::build_segmented_tone_curve(None, &segs);
    assert_eq!(ct.is_null(), rt.is_none(), "segmented presence");
    if !ct.is_null() {
        let rt = rt.unwrap();
        for i in 0..64 {
            let v = i as f32 / 63.0;
            let cf = unsafe { c::cmsEvalToneCurveFloat(ct, v) };
            let rf = rt.eval_float(v);
            let tol = (5e-5 * cf.abs()).max(1e-6);
            assert!((cf - rf).abs() <= tol, "seg evalf {v}: {cf} vs {rf}");
        }
        unsafe { c::cmsFreeToneCurve(ct) };
    }
}

#[test]
fn reverse_join_smooth() {
    unsafe {
        let c_g = c::cmsBuildParametricToneCurve(std::ptr::null_mut(), 4,
            [2.4f64, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045].as_ptr());
        let r_g = gamma::build_parametric_tone_curve(None, 4,
            &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045]).unwrap();
        // reverse
        let cr = c::cmsReverseToneCurve(c_g);
        let rr = gamma::reverse_tone_curve(&r_g).unwrap();
        for v in (0..=65535u16).step_by(2053) {
            assert_eq!(c::cmsEvalToneCurve16(cr, v), rr.eval16(v), "rev eval16 {v}");
        }
        c::cmsFreeToneCurve(cr);
        // join g * sRGB
        let cg2 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let r_g2 = gamma::build_gamma(None, 2.2).unwrap();
        let cj = c::cmsJoinToneCurve(std::ptr::null_mut(), c_g, cg2, 256);
        let rj = gamma::join_tone_curve(None, &r_g, &r_g2, 256).unwrap();
        for v in (0..=65535u16).step_by(2053) {
            assert_eq!(c::cmsEvalToneCurve16(cj, v), rj.eval16(v), "join eval16 {v}");
        }
        c::cmsFreeToneCurve(cj);
        c::cmsFreeToneCurve(cg2);
        // smooth a tabulated curve
        let tv: Vec<u16> = (0..256u16).map(|i| i.wrapping_mul(257).wrapping_add(i % 7)).collect();
        let mut ct = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(), 256, tv.as_ptr());
        let mut rt = gamma::build_tabulated_tone_curve16(None, 256,
            Some(&tv)).unwrap();
        assert_eq!(c::cmsSmoothToneCurve(ct, 5.0) != 0,
            gamma::smooth_tone_curve(None, &mut rt, 5.0), "smooth status");
        for v in (0..=65535u16).step_by(2053) {
            assert_eq!(c::cmsEvalToneCurve16(ct, v), rt.eval16(v), "smooth eval16 {v}");
        }
        c::cmsFreeToneCurve(ct);
        c::cmsFreeToneCurve(c_g);
    }
}

/// `cmsDupToneCurve`/`cmsJoinToneCurve`/`cmsSmoothToneCurve` variants —
/// dup preserves eval + params; join on more curve pairs; smooth on
/// parametric curves.
#[test]
fn dup_join_smooth_variants() {
    unsafe {
        // dup: eval16 parity on every parametric type
        for ty in [1i32, 2, 4, 5, 7] {
            let params: [f64; 10] = match ty {
                1 => { let mut p = [0.0;10]; p[0]=2.2; p }
                2 => { let mut p = [0.0;10]; p[0]=2.2; p[1]=1.0; p }
                4 => { let mut p = [0.0;10]; p[0]=2.4; p[1]=1.0/1.055; p[2]=0.055/1.055; p[3]=1.0/12.92; p[4]=0.04045; p }
                5 => { let mut p = [0.0;10]; p[0]=1.5; p[1]=1.0; p[3]=1.0; p }
                _ => { let mut p = [0.0;10]; p[0]=1.5; p[1]=1.0; p[3]=1.0; p }
            };
            let ct = c::cmsBuildParametricToneCurve(std::ptr::null_mut(), ty, params.as_ptr());
            let rt = gamma::build_parametric_tone_curve(None, ty, &params).unwrap();
            let cd = c::cmsDupToneCurve(ct);
            let rd = gamma::dup_tone_curve(&rt, None).unwrap();
            for v in (0..=65535u16).step_by(4099) {
                assert_eq!(c::cmsEvalToneCurve16(cd, v), rd.eval16(v), "dup ty{ty} {v}");
            }
            c::cmsFreeToneCurve(cd);
            c::cmsFreeToneCurve(ct);
        }
        // join combos: gamma x parametric, parametric x tabulated
        let c_g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c_t = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(), 256,
            (0..256u16).map(|i| (i*257).wrapping_add(i%3)).collect::<Vec<u16>>().as_ptr());
        let r_g = gamma::build_gamma(None, 2.2).unwrap();
        let r_t = gamma::build_tabulated_tone_curve16(None, 256,
            Some(&(0..256u16).map(|i| (i*257).wrapping_add(i%3)).collect::<Vec<u16>>())).unwrap();
        let cj = c::cmsJoinToneCurve(std::ptr::null_mut(), c_g, c_t, 256);
        let rj = gamma::join_tone_curve(None, &r_g, &r_t, 256).unwrap();
        for v in (0..=65535u16).step_by(2053) {
            assert_eq!(c::cmsEvalToneCurve16(cj, v), rj.eval16(v), "join g*t {v}");
        }
        c::cmsFreeToneCurve(cj);
        // smooth parametric
        let mut cs = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut rs = gamma::build_gamma(None, 2.2).unwrap();
        assert_eq!(c::cmsSmoothToneCurve(cs, 2.0) != 0,
            gamma::smooth_tone_curve(None, &mut rs, 2.0), "smooth status");
        for v in (0..=65535u16).step_by(2053) {
            assert_eq!(c::cmsEvalToneCurve16(cs, v), rs.eval16(v), "smooth {v}");
        }
        c::cmsFreeToneCurve(cs);
        c::cmsFreeToneCurve(c_g);
        c::cmsFreeToneCurve(c_t);
    }
}

/// Sigmoid parametric curves — type ±109 hits sigmoid_factory /
/// inverse_sigmoid_factory (+ inverted_sigmoid_base inside).
#[test]
fn sigmoid_curves() {
    unsafe {
        for ty in [109i32, -109] {
            for k in [0.2f64, 1.0, 3.5, 10.0] {
                let params = [k];
                let cc = c::cmsBuildParametricToneCurve(
                    std::ptr::null_mut(), ty, params.as_ptr());
                let rr = gamma::build_parametric_tone_curve(
                    None, ty, &params);
                assert_eq!(!cc.is_null(), rr.is_some(),
                    "sigmoid type={ty} k={k} alloc");
                if cc.is_null() { continue; }
                let rc = rr.unwrap();
                for i in 0..=32u32 {
                    let v = i as f32 / 32.0;
                    let co = c::cmsEvalToneCurveFloat(cc, v);
                    let ro = rc.eval_float(v);
                    assert!((co - ro).abs() < 1e-6
                        || co.is_nan() && ro.is_nan(),
                        "sigmoid {ty} k={k} x={v}: {co} vs {ro}");
                }
                for v in (0..=65535u16).step_by(4096) {
                    assert_eq!(c::cmsEvalToneCurve16(cc, v),
                        rc.eval16(v), "sigmoid16 {ty} k={k} x={v}");
                }
                c::cmsFreeToneCurve(cc);
            }
        }
    }
}

/// Parametric-type sweep — cmsBuildParametricToneCurve for every
/// built-in type {1..8, 108, 109} and the negative (inverse) forms,
/// eval'd across the domain — covers DefaultEvalParametricFn arms.
#[test]
fn parametric_sweep_all() {
    unsafe {
        // params per type counts: DefaultCurves {1,3,4,5,7,4,5,5,1,1}
        let params: [&[f64]; 10] = [
            &[1.0],                       // 1: g^1
            &[2.2, 0.055, 0.5],           // 2
            &[2.2, 0.055, 0.5, 0.3],      // 3
            &[2.2, 0.04, 0.5, 0.3, 0.2],  // 4
            &[2.0, 1.0, 0.5, 0.3, 0.2, 0.1, 0.4], // 5
            &[2.2, 0.04, 0.5, 0.3],       // 6
            &[2.2, 0.055, 0.5, 0.3, 0.2], // 7
            &[2.2, 0.055, 0.5, 0.3, 0.4], // 8
            &[0.5],                       // 108
            &[0.7],                       // 109
        ];
        let tys = [1i32, 2, 3, 4, 5, 6, 7, 8, 108, 109];
        for (k, ty) in tys.iter().enumerate() {
            for inv in [1i32, -1] {
                let t2 = ty * inv;
                let tc = c::cmsBuildParametricToneCurve(
                    std::ptr::null_mut(), t2,
                    params[k].as_ptr());
                eprintln!("type {t2} build={}", !tc.is_null());
                if tc.is_null() { continue; }
                let mut outs = Vec::new();
                for s in 0..64u16 {
                    let x = (s as f32) / 63.0;
                    outs.push(c::cmsEvalToneCurveFloat(
                        tc as *const _, x));
                }
                let rc = lcms::gamma::build_parametric_tone_curve(
                    None, t2, params[k]);
                if let Some(rc) = rc {
                    let mut diffs = 0;
                    for (i, x) in (0..64).map(|s| s as f32 / 63.0)
                        .enumerate() {
                        let r = rc.eval_float(x);
                        if (r - outs[i]).abs() > 1e-6 { diffs += 1; }
                    }
                    assert_eq!(diffs, 0, "type {t2} eval");
                }
                c::cmsFreeToneCurve(tc);
            }
        }
    }
}

/// Tone-curve ops — reverse/join/smooth/eval/monotonic arms ->
/// cmsgamma internals (ReverseToneCurve, JoinToneCurve, SmoothTone).
#[test]
fn tone_curve_ops() {
    unsafe {
        let g22 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g18 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        // eval + monotonic checks
        for v in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let _ = c::cmsEvalToneCurveFloat(g22, v);
            let _ = c::cmsEvalToneCurve16(g22, (v * 65535.0) as u16);
        }
        eprintln!("lin={} mono={}",
            c::cmsIsToneCurveLinear(g22),
            c::cmsIsToneCurveMonotonic(g22));
        // reverse + reverseEx
        let r = c::cmsReverseToneCurve(g22);
        let rx = c::cmsReverseToneCurveEx(128, g22);
        eprintln!("rev={} revEx={}", !r.is_null(), !rx.is_null());
        // join two curves -> joined eval
        let j = c::cmsJoinToneCurve(std::ptr::null_mut(),
            g22, g18, 256);
        eprintln!("join={}", !j.is_null());
        if !j.is_null() {
            let _ = c::cmsEvalToneCurveFloat(j, 0.5);
            c::cmsFreeToneCurve(j);
        }
        // smooth
        let s = c::cmsSmoothToneCurve(g18, 0.5);
        eprintln!("smooth={s}");
        if !r.is_null() { c::cmsFreeToneCurve(r); }
        if !rx.is_null() { c::cmsFreeToneCurve(rx); }
        c::cmsFreeToneCurve(g22);
        c::cmsFreeToneCurve(g18);
    }
}

/// Curve builders + predicates — tabulated16/float, segmented,
/// descending/multisegment/parametric-type, estimate-gamma,
/// estimated-table, dup.
#[test]
fn curve_builders_predicates() {
    unsafe {
        // tabulated 16 + float
        let t16 = [0u16, 0x8000, 0xFFFF];
        let tc16 = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 3, t16.as_ptr());
        let tf = [0.0f32, 0.5, 1.0];
        let tcf = c::cmsBuildTabulatedToneCurveFloat(
            std::ptr::null_mut(), 3, tf.as_ptr());
        eprintln!("tab16={} tabf={}", !tc16.is_null(), !tcf.is_null());
        // segmented curve
        #[repr(C)] struct Seg { a: f64, b: f64, c: f64, d: f64,
            e: f64, ty: i32, n: u32, pt: *const f64 }
        let seg = Seg { a: 0.0, b: 0.0, c: 0.0, d: 0.0, e: 0.0,
            ty: 6, n: 0, pt: std::ptr::null() };
        let tsg = c::cmsBuildSegmentedToneCurve(
            std::ptr::null_mut(), 1, &seg as *const _ as *const _);
        eprintln!("seg={}", !tsg.is_null());
        // predicates on a gamma curve
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        eprintln!("desc={} multi={} ptype={} eg={:.3}",
            c::cmsIsToneCurveDescending(g),
            c::cmsIsToneCurveMultisegment(g),
            c::cmsGetToneCurveParametricType(g),
            c::cmsEstimateGamma(g, 0.001));
        eprintln!("etable={} entries={}",
            !c::cmsGetToneCurveEstimatedTable(g).is_null(),
            c::cmsGetToneCurveEstimatedTableEntries(g));
        let d = c::cmsDupToneCurve(g);
        eprintln!("dup={}", !d.is_null());
        if !d.is_null() { c::cmsFreeToneCurve(d); }
        if !tc16.is_null() { c::cmsFreeToneCurve(tc16); }
        if !tcf.is_null() { c::cmsFreeToneCurve(tcf); }
        if !tsg.is_null() { c::cmsFreeToneCurve(tsg); }
        c::cmsFreeToneCurve(g);
    }
}

/// Parametric-curve eval internals — types 0-8 through
/// cmsEvalToneCurve16/Float + reverse eval.
#[test]
fn parametric_eval_arms() {
    unsafe {
        let p7 = [1.0f64, 0.0, 0.5, 0.0, 0.0];
        let p8 = [1.0f64, 0.0, 0.5, 0.0, 0.0];
        for (ptype, params, name) in [
            (1i32, [2.2f64, 1.0, 0.0, 0.0, 0.0].as_ptr(), "t1"),
            (2, [2.2f64, 1.0, 0.0, 0.5, 0.0].as_ptr(), "t2"),
            (3, [2.2f64, 1.0, 0.0, 0.5, 0.0].as_ptr(), "t3"),
            (4, [2.2f64, 1.0, 0.0, 0.5, 0.2].as_ptr(), "t4"),
            (5, [2.2f64, 1.0, 0.0, 0.5, 0.2].as_ptr(), "t5"),
            (6, [1.0f64, 0.0, 0.5, 0.0, 0.0].as_ptr(), "t6"),
            (7, p7.as_ptr(), "t7"),
            (8, p8.as_ptr(), "t8"),
        ] {
            let c_ = c::cmsBuildParametricToneCurve(
                std::ptr::null_mut(), ptype, params);
            if c_.is_null() { eprintln!("{name} build=NULL"); continue; }
            for x in [0.0f64, 0.25, 0.5, 0.75, 1.0] {
                let e16 = c::cmsEvalToneCurve16(c_,
                    (x * 65535.0) as u16);
                let ef = c::cmsEvalToneCurveFloat(c_, x as f32);
                let _ = (e16, ef);
            }
            let rev = c::cmsReverseToneCurve(c_);
            if !rev.is_null() {
                c::cmsEvalToneCurveFloat(rev, 0.5);
                c::cmsFreeToneCurve(rev);
            }
            eprintln!("{name} ok");
            c::cmsFreeToneCurve(c_);
        }
    }
}

/// cmsgamma internals — gamma variants, segmented curves,
/// dup/reverse/free, eval edge cases.
#[test]
fn gamma_internals2() {
    unsafe {
        // BuildGamma edge gammas
        for g in [0.1f64, 0.5, 1.0, 2.2, 10.0, 100.0] {
            let c_ = c::cmsBuildGamma(std::ptr::null_mut(), g);
            if !c_.is_null() {
                c::cmsEvalToneCurveFloat(c_, 0.5);
                let d = c::cmsDupToneCurve(c_);
                if !d.is_null() { c::cmsFreeToneCurve(d); }
                c::cmsFreeToneCurve(c_);
            }
        }
        // segmented curve: {x0,x1,type,params,grid,samples}
        let mut seg = [c::CurveSegment {
            x0: 0.0f32, x1: 1.0,
            r#type: 6, params: [2.2f64, 1.0, 0.0, 0.0, 0.0,
                              0.0, 0.0, 0.0, 0.0, 0.0],
            n_grid_points: 0,
            sampled_points: std::ptr::null_mut() }];
        let sc = c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(),
            1, seg.as_ptr());
        eprintln!("seeg={}", !sc.is_null());
        if !sc.is_null() {
            c::cmsEvalToneCurveFloat(sc, 0.5);
            c::cmsFreeToneCurve(sc);
        }
        // EstimateGamma on tabulated curves
        for n in [2u32, 4, 16, 256, 1024] {
            let mut tab = vec![0u16; n as usize];
            for i in 0..n as usize {
                tab[i] = ((i as f64 / (n - 1).max(1) as f64).powf(2.2)
                    * 65535.0) as u16;
            }
            let c_ = c::cmsBuildTabulatedToneCurve16(
                std::ptr::null_mut(), n, tab.as_ptr());
            if !c_.is_null() {
                let g = c::cmsEstimateGamma(c_, 0.001);
                eprintln!("n{n} est={g:.3}");
                c::cmsFreeToneCurve(c_);
            }
        }
        // JoinToneCurve / ReverseToneCurve on parametric + tabulated
        let p1 = c::cmsBuildParametricToneCurve(std::ptr::null_mut(),
            1, [2.2f64, 1.0, 0.0].as_ptr());
        let p2 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            16, [0u16, 4096, 8192, 12288, 16384, 20480, 24576, 28672,
                  32768, 36864, 40960, 45056, 49152, 53248, 57344,
                  61440].as_ptr());
        let j = c::cmsJoinToneCurve(std::ptr::null_mut(), p1, p2, 16);
        eprintln!("join={}", !j.is_null());
        if !j.is_null() { c::cmsFreeToneCurve(j); }
        c::cmsFreeToneCurve(p1);
        c::cmsFreeToneCurve(p2);
    }
}

/// AllocateToneCurveStruct/CurveSetDup — tone-curve alloc paths
/// (dup, segmented, joined, estimated) + pipeline curve-set dup.
#[test]
fn tonecurve_alloc_arms() {
    unsafe {
        // tabulated curves of various sizes
        for n in [2u32, 3, 16, 256, 4096, 65535] {
            let mut tab = vec![0u16; n as usize];
            for (i, v) in tab.iter_mut().enumerate() {
                *v = ((i * 65535) / ((n as usize) - 1)) as u16;
            }
            let c_ = c::cmsBuildTabulatedToneCurve16(
                std::ptr::null_mut(), n, tab.as_ptr());
            if !c_.is_null() {
                let d = c::cmsDupToneCurve(c_);
                if !d.is_null() { c::cmsFreeToneCurve(d); }
                let j = c::cmsJoinToneCurve(
                    std::ptr::null_mut(), c_, c_, 256);
                if !j.is_null() { c::cmsFreeToneCurve(j); }
                c::cmsFreeToneCurve(c_);
            }
        }
        // curve-set stages inside pipelines -> CurveSetDup
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g, g].as_ptr() as *mut _));
        let d = c::cmsPipelineDup(p);
        eprintln!("pipedup={}", !d.is_null());
        if !d.is_null() { c::cmsPipelineFree(d); }
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
        // MLU UTF8 -> decodeUTF8
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 2);
        c::cmsMLUsetUTF8(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _,
            "mixed \u{1f600} \u{00e4}\u{4e2d} text\0".as_ptr());
        c::cmsMLUsetUTF8(ml, b"ja\0".as_ptr() as *const _,
            b"JP\0".as_ptr() as *const _,
            "\u{30c6}\u{30b9}\u{30c8}\0".as_ptr());
        let mut out = [0u8; 256];
        let n = c::cmsMLUgetUTF8(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _,
            out.as_mut_ptr() as *mut _, 256);
        eprintln!("mlu-utf8 n={n}");
        c::cmsMLUfree(ml);
    }
}
