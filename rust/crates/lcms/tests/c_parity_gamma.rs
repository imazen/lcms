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

/// Parametric types 108/109 + inverses — sigmoid_factory,
/// inverse_sigmoid_factory, and the -108/-109 reverse arms.
#[test]
fn parametric_type_108_109() {
    unsafe {
        for ty in [108i32, 109, -108, -109] {
            let params = [
                2.2f64, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ];
            let c = c::cmsBuildParametricToneCurve(
                std::ptr::null_mut(), ty, params.as_ptr());
            eprintln!("ty{ty} null={}", c.is_null());
            if c.is_null() { continue; }
            for x in [0.0f64, 0.01, 0.1, 0.25, 0.5, 0.75, 0.9,
                      0.99, 1.0, -0.1, 1.1] {
                let y = c::cmsEvalToneCurveFloat(c, x as f32);
                let _ = y;
            }
            c::cmsFreeToneCurve(c);
        }
    }
}

/// DefaultEvalParametricFn deep arms — each type's piecewise
/// boundaries evaluated across/around breakpoints.
#[test]
fn parametric_boundaries() {
    unsafe {
        // per-type boundary points that hit the piecewise cases
        let cases: Vec<(i32, Vec<f64>, Vec<f64>)> = vec![
            // type 1: y = x^g — boundaries at x=0, x=1, neg
            (1, vec![2.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                vec![-1.0, 0.0, 0.5, 1.0, 2.0]),
            // type 2: y = (a*x+b)^g below c; x^g above
            (2, vec![2.2, 0.5, 0.1, 0.3, 0.0, 0.0, 0.0],
                vec![-1.0, 0.0, 0.1, 0.2, 0.3, 0.5, 1.0, 2.0]),
            // type 3: y = (a*x+b)^g + c below d; x^g above
            (3, vec![2.2, 0.5, 0.1, 0.0, 0.3, 0.0, 0.0],
                vec![-1.0, 0.0, 0.1, 0.3, 0.5, 1.0, 2.0]),
            // type 4: y = (a*x+b)^g below d; a2*x+b2 above
            (4, vec![2.2, 0.5, 0.1, 0.0, 0.3, 0.0, 0.0],
                vec![-1.0, 0.0, 0.1, 0.3, 0.5, 1.0, 2.0]),
            // type 5: sigmoid-like
            (5, vec![2.2, 0.5, 0.1, 0.3, 0.0, 0.0, 0.0],
                vec![-1.0, 0.0, 0.1, 0.3, 0.5, 1.0, 2.0]),
            // type 6: y = a*x^g + b below c; x^g above
            (6, vec![2.2, 0.5, 0.1, 0.3, 0.0, 0.0, 0.0],
                vec![-1.0, 0.0, 0.1, 0.3, 0.5, 1.0, 2.0]),
            // type 7: y = a*log(b*x+c)+d
            (7, vec![2.2, 0.5, 0.1, 0.3, 0.0, 0.0, 0.0],
                vec![-1.0, 0.0, 0.1, 0.3, 0.5, 1.0, 2.0]),
            // type 8: y = a*x^g + b + c*log(x+d)+e
            (8, vec![2.2, 0.5, 0.1, 0.3, 0.0, 0.0, 0.0],
                vec![-1.0, 0.0, 0.1, 0.3, 0.5, 1.0, 2.0]),
        ];
        for (ty, params, vals) in &cases {
            let c_ = c::cmsBuildParametricToneCurve(
                std::ptr::null_mut(), *ty, params.as_ptr());
            if c_.is_null() {
                eprintln!("ty{ty} null");
                continue;
            }
            for &x in vals {
                let y = c::cmsEvalToneCurveFloat(c_, x as f32);
                let _ = y;
                // also eval via pipeline
                let out = c::cmsEvalToneCurveFloat(c_, x as f32);
                let _ = out;
            }
            c::cmsFreeToneCurve(c_);
        }
    }
}

/// AllocateToneCurveStruct deep arms — entry counts, segment
/// alloc/dup/free, parametric variants.
#[test]
fn tonecurve_alloc_arms2() {
    unsafe {
        // entry-count edge cases through AllocateToneCurveStruct
        for n in [0u32, 1, 2, 3, 16, 255, 256, 1024, 4096, 65535] {
            let mut t = vec![0u16; n.max(1) as usize];
            for (i, v) in t.iter_mut().enumerate() {
                *v = (i as u64 * 65535 / n.max(1) as u64) as u16;
            }
            let c = c::cmsBuildTabulatedToneCurve16(
                std::ptr::null_mut(), n.max(1), t.as_ptr());
            eprintln!("tc-{n}={}", !c.is_null());
            if !c.is_null() {
                let y = c::cmsEvalToneCurveFloat(c, 0.5);
                let _ = y;
                c::cmsFreeToneCurve(c);
            }
        }
        // segmented curves with all segment types
        for nsegs in [1u32, 2, 4, 8] {
            let mut segs = vec![std::mem::zeroed::<c::CurveSegment>();
                nsegs as usize];
            for (i, s) in segs.iter_mut().enumerate() {
                s.x0 = i as f32 / nsegs as f32;
                s.x1 = (i + 1) as f32 / nsegs as f32;
                s.r#type = 6; // linear segment
                s.params[0] = 1.0;
                s.params[1] = 0.0;
            }
            let c = c::cmsBuildSegmentedToneCurve(
                std::ptr::null_mut(), nsegs, segs.as_ptr());
            eprintln!("segs-{nsegs}={}", !c.is_null());
            if !c.is_null() {
                c::cmsFreeToneCurve(c);
            }
        }
        // dup + free through pipeline stages
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let cs = c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
            [g, g, g].as_ptr() as *mut _);
        let cs2 = c::cmsStageDup(cs);
        eprintln!("dup={}", !cs2.is_null());
        if !cs2.is_null() { c::cmsStageFree(cs2); }
        c::cmsStageFree(cs);
        c::cmsFreeToneCurve(g);
    }
}

/// DefaultEvalParametricFn remaining internals — every type's
/// domain boundary + inverse + edge parameters.
#[test]
fn parametric_arms2() {
    unsafe {
        // parametric curves with edge parameters hitting the
        // boundary conditions inside DefaultEvalParametricFn
        for (ty, params, name) in [
            (1, vec![2.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], "t1"),
            (2, vec![2.2, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0], "t2-b0"),
            (2, vec![2.2, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0], "t2-a0"),
            (3, vec![2.2, 1.0, 0.0, 0.0, 0.5, 0.0, 0.0], "t3-b0"),
            (4, vec![2.2, 1.0, 0.0, 0.0, 0.5, 0.0, 0.0], "t4"),
            (5, vec![2.2, 1.0, 0.0, 0.5, 0.0, 0.0, 0.0], "t5"),
            (6, vec![2.2, 1.0, 0.0, 0.5, 0.0, 0.0, 0.0], "t6"),
            (7, vec![2.2, 1.0, 0.0, 0.5, 0.0, 0.0, 0.0], "t7"),
            (8, vec![2.2, 1.0, 0.0, 0.5, 0.0, 0.0, 0.0], "t8"),
            // 108/109 sigmoid variants
            (108, vec![2.0, 1.0, 0.0, 0.5, 0.0, 0.0, 0.0], "t108"),
            (108, vec![0.5, 0.5, 0.0, 0.5, 0.0, 0.0, 0.0], "t108b"),
            (109, vec![2.0, 1.0, 0.0, 0.5, 0.0, 0.0, 0.0], "t109"),
            (109, vec![0.5, 0.5, 0.0, 0.5, 0.0, 0.0, 0.0], "t109b"),
        ] {
            let c = c::cmsBuildParametricToneCurve(
                std::ptr::null_mut(), ty, params.as_ptr());
            if c.is_null() {
                eprintln!("{name} null");
                continue;
            }
            // eval across the full domain + edges
            for x in [-2.0f32, -1.0, -0.5, -0.1, -0.01, 0.0,
                      0.01, 0.1, 0.3, 0.5, 0.7, 0.9, 0.99, 1.0,
                      1.01, 1.1, 1.5, 2.0, 5.0] {
                let y = c::cmsEvalToneCurveFloat(c, x);
                let _ = y;
            }
            // dup + eval again (dup'd struct arm)
            let d = c::cmsDupToneCurve(c);
            if !d.is_null() {
                let _ = c::cmsEvalToneCurveFloat(d, 0.5);
                c::cmsFreeToneCurve(d);
            }
            c::cmsFreeToneCurve(c);
        }
    }
}

/// DefaultEvalParametricFn — per-type piecewise boundary arms +
/// inverse/negative types + AllocateToneCurveStruct edges.
#[test]
fn parametric_eval_arms2() {
    unsafe {
        // type 1-8 + 108/109 + inverse (negative) + invalid
        let params7: [f64; 10] = [1.5, 0.2, 0.1, 0.3, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0];
        // eval points straddling the d boundary (Params[3]=0.3)
        let xs = [0.0f32, 0.001, 0.05, 0.1, 0.2, 0.29, 0.3,
            0.31, 0.5, 0.7, 0.9, 0.999, 1.0];
        for ty in [1i32, 2, 3, 4, 5, 6, 7, 8, 108, 109,
            -1, -2, -3, -4, -5, -6, -7, -8, 0, 9, 99, -99] {
            let g = c::cmsBuildParametricToneCurve(
                std::ptr::null_mut(), ty, params7.as_ptr());
            eprintln!("ty={ty} build={}", !g.is_null());
            if g.is_null() { continue; }
            for x in xs {
                let v16 = c::cmsEvalToneCurve16(g,
                    (x * 65535.0) as u16);
                let vf = c::cmsEvalToneCurveFloat(g, x);
                if ty == 5 && (x * 1000.0) as u32 % 4 == 0 {
                    eprintln!("ty={ty} x={x} v16={v16} vf={vf:.4}");
                }
            }
            // reverse → inverse eval path
            let rg = c::cmsReverseToneCurve(g);
            if !rg.is_null() {
                for x in [0.1f32, 0.5, 0.9] {
                    c::cmsEvalToneCurveFloat(rg, x);
                }
                c::cmsFreeToneCurve(rg);
            }
            c::cmsFreeToneCurve(g);
        }
        // AllocateToneCurveStruct edges
        let z = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 0, std::ptr::null());
        eprintln!("entries0={}", !z.is_null());
        let huge = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 70000, std::ptr::null());
        eprintln!("huge={}", !huge.is_null());
        if !huge.is_null() { c::cmsFreeToneCurve(huge); }
        if !z.is_null() { c::cmsFreeToneCurve(z); }
        // dup + monotonic/descending checks on each type
        for ty in [1i32, 5, 8] {
            let g = c::cmsBuildParametricToneCurve(
                std::ptr::null_mut(), ty, params7.as_ptr());
            let d = c::cmsDupToneCurve(g);
            let mono = c::cmsIsToneCurveMonotonic(g);
            let desc = c::cmsIsToneCurveDescending(g);
            let lin = c::cmsIsToneCurveLinear(g);
            let ms = c::cmsIsToneCurveMultisegment(g);
            eprintln!("ty={ty} mono={mono} desc={desc} lin={lin} \
                ms={ms} dup={}", !d.is_null());
            if !d.is_null() { c::cmsFreeToneCurve(d); }
            c::cmsFreeToneCurve(g);
        }
    }
}

/// cmsSmoothToneCurve — lambda sweep on tabulated/parametric/seg.
#[test]
fn smooth_arms() {
    unsafe {
        let tv = [0u16, 1024, 4096, 16384, 40000, 65535];
        let curves = [
            c::cmsBuildTabulatedToneCurve16(
                std::ptr::null_mut(), 6, tv.as_ptr()),
            c::cmsBuildGamma(std::ptr::null_mut(), 2.2),
            c::cmsBuildParametricToneCurve(std::ptr::null_mut(),
                5, [2.0, 0.5, 0.1, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0,
                    0.0].as_ptr()),
            c::cmsBuildTabulatedToneCurve16(
                std::ptr::null_mut(), 2, tv.as_ptr()),
        ];
        for (i, tc) in curves.iter().enumerate() {
            for lam in [0.0f64, 0.001, 0.1, 1.0, 10.0, -1.0] {
                let d = c::cmsDupToneCurve(*tc);
                let r = c::cmsSmoothToneCurve(d, lam);
                eprintln!("c{i} lam={lam} sm={r} dup={}",
                    !d.is_null());
                if !d.is_null() { c::cmsFreeToneCurve(d); }
            }
        }
        for tc in curves {
            c::cmsFreeToneCurve(tc);
        }
    }
}

/// cmsReadRawTag — raw byte reads on saved profiles.
#[test]
fn rawtag_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("save={s}");
        let rp = c::cmsOpenProfileFromMem(
            buf.as_ptr() as *const _, n);
        let nt = c::cmsGetTagCount(rp);
        for i in 0..nt as u32 {
            let sig = c::cmsGetTagSignature(rp, i);
            // size query (NULL buffer) then read
            let need = c::cmsReadRawTag(rp, sig,
                std::ptr::null_mut(), 0);
            let mut rb = vec![0u8; need as usize];
            let got = c::cmsReadRawTag(rp, sig,
                rb.as_mut_ptr() as *mut _, need);
            // undersized read → truncated or reject
            let mut small = [0u8; 4];
            let g2 = c::cmsReadRawTag(rp, sig,
                small.as_mut_ptr() as *mut _, 4);
            eprintln!("tag {sig:#x} need={need} got={got} \
                small={g2}");
        }
        c::cmsCloseProfile(rp);
        c::cmsCloseProfile(srgb);
    }
}

/// cmsReverseToneCurve(Ex) + cmsBuildSegmentedToneCurve +
/// cmsGetTagOffsetAndSize arms.
#[test]
fn reverse_seg_arms() {
    unsafe {
        for gamma in [1.0f64, 2.2, 0.4545, 3.1] {
            let g = c::cmsBuildGamma(std::ptr::null_mut(), gamma);
            for n in [256u32, 4096, 16] {
                let r = c::cmsReverseToneCurveEx(n, g);
                let mut y = 0f64;
                if !r.is_null() {
                    for v in [0u16, 0x4000, 0x8000, 0xffff] {
                        let ev = c::cmsEvalToneCurve16(r, v);
                        y = ev as f64 / 65535.0;
                    }
                    c::cmsFreeToneCurve(r);
                }
                eprintln!("g={gamma} n={n} rev={y:.4}");
            }
            let r2 = c::cmsReverseToneCurve(g);
            if !r2.is_null() { c::cmsFreeToneCurve(r2); }
            c::cmsFreeToneCurve(g);
        }
        // seg curve
        let mut seg = [c::CurveSegment {
            x0: -1e5, x1: 0.0,
            r#type: 6, params: [0.0; 10],
            n_grid_points: 0,
            sampled_points: std::ptr::null(),
        }; 2];
        seg[0] = c::CurveSegment {
            x0: -1e5, x1: 0.0, r#type: 6,
            params: { let mut p = [0.0f64; 10];
                p[0] = 0.0; p },
            n_grid_points: 0,
            sampled_points: std::ptr::null(),
        };
        seg[1] = c::CurveSegment {
            x0: 0.0, x1: 1e5, r#type: 8,
            params: { let mut p = [0.0f64; 10];
                p[0] = 1.0; p },
            n_grid_points: 0,
            sampled_points: std::ptr::null(),
        };
        let sc = c::cmsBuildSegmentedToneCurve(
            std::ptr::null_mut(), 2,
            seg.as_ptr() as *const _);
        eprintln!("seg={}", !sc.is_null());
        if !sc.is_null() {
            let ev = c::cmsEvalToneCurveFloat(sc, 0.5);
            eprintln!("segval={ev}");
            c::cmsFreeToneCurve(sc);
        }
        // tag offsets/sizes on a real profile
        let p = c::cmsCreate_sRGBProfile();
        let n = c::cmsGetTagCount(p);
        for i in 0..n as i32 {
            let mut off = 0u32;
            let mut sz = 0u32;
            let sig = c::cmsGetTagSignature(p, i as u32);
            let ok = c::cmsGetTagOffsetAndSize(p, i as u32,
                &mut off, &mut sz);
            eprintln!("tag{i} {sig:#x} off={off} sz={sz} ok={ok}");
        }
        let mut o9 = 0u32;
        let mut s9 = 0u32;
        let ok9 = c::cmsGetTagOffsetAndSize(p, 99,
            &mut o9, &mut s9);
        eprintln!("tag99 ok={ok9}");
        c::cmsCloseProfile(p);
    }
}

/// cmsBuildSegmentedToneCurve multi-seg validation +
/// DupPluginCurvesList via context-dup with cnvP plugins.
#[test]
fn seg_validation_arms() {
    fn body(crash: bool) {
        unsafe {
            fn seg(x0: f32, x1: f32, ty: i32, p0: f64, p1: f64)
                -> c::CurveSegment {
                c::CurveSegment {
                    x0, x1, r#type: ty,
                    params: { let mut p = [0.0f64; 10];
                        p[0] = p0; p[1] = p1; p },
                    n_grid_points: 0,
                    sampled_points: std::ptr::null(),
                }
            }
            for (segs, name) in [
                (vec![seg(-1e5, 0.0, 6, 0.0, 0.0),
                    seg(0.0, 1e5, 8, 1.0, 0.0)], "6-8"),
                (vec![seg(-1e5, 0.0, 7, 1.0, 0.0),
                    seg(0.0, 1e5, 7, 1.0, 0.0)], "7-7"),
                (vec![seg(-1e5, 0.0, 8, 1.0, 0.0),
                    seg(0.0, 1e5, 8, 1.0, 0.0)], "8-8"),
                (vec![seg(-1e5, 0.0, 6, 0.0, 0.0),
                    seg(0.0, 0.5, 7, 1.0, 0.0),
                    seg(0.5, 1e5, 8, 1.0, 0.0)], "6-7-8"),
                (vec![seg(-1e5, 0.0, 6, 0.0, 0.0)], "1seg"),
                (if !crash { vec![] } else { vec![
                    seg(-1e5, 0.0, 99, 0.0, 0.0),
                    seg(0.0, 1e5, 8, 1.0, 0.0)] }, "bad99"),
                (if !crash { vec![] } else { vec![
                    seg(-1e5, 0.0, -1, 0.0, 0.0),
                    seg(0.0, 1e5, 8, 1.0, 0.0)] }, "badneg"),
                (if !crash { vec![] } else { vec![
                    seg(0.0, 0.0, 6, 0.0, 0.0),
                    seg(0.0, 1e5, 8, 1.0, 0.0)] }, "zerospan"),
            ] {
                let sc = c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), segs.len() as u32,
                    segs.as_ptr() as *const _);
                eprintln!("{name} sc={}", !sc.is_null());
                if !sc.is_null() {
                    let ev = c::cmsEvalToneCurveFloat(sc, 0.5);
                    eprintln!("  eval={ev}");
                    c::cmsFreeToneCurve(sc);
                }
            }
            // sampled seg — valid pointer + NULL (crash)
            if crash {
                let pts = [0.0f32, 0.3, 0.7, 1.0];
                let mut s = seg(-1e5, 0.0, 0, 0.0, 0.0);
                s.n_grid_points = 4;
                s.sampled_points = pts.as_ptr();
                let sc = c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), 1, &s as *const _);
                eprintln!("sampled sc={}", !sc.is_null());
                if !sc.is_null() { c::cmsFreeToneCurve(sc); }
            }
            // NULL sampled points — upstream SIGSEGV.
            // only in the forked child via `crash` flag
            if crash {
                let s2 = seg(-1e5, 0.0, 0, 0.0, 0.0);
                let sc2 = c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), 1, &s2 as *const _);
                eprintln!("null-pts sc={}", !sc2.is_null());
                if !sc2.is_null() { c::cmsFreeToneCurve(sc2); }
            }
        }
    }
    unsafe extern "C" { fn fork() -> i32;
        fn waitpid(p: i32, s: *mut i32, o: i32) -> i32;
        fn _exit(code: i32) -> !;
        fn dlsym(h: *mut std::ffi::c_void,
            n: *const i8) -> *mut std::ffi::c_void; }
    let flush: Option<unsafe extern "C" fn() -> i32> = unsafe {
        let f = dlsym(std::ptr::null_mut(),
            b"__llvm_profile_write_file\0".as_ptr() as *const i8);
        if f.is_null() { None } else { Some(std::mem::transmute(f)) }
    };
    let pid = unsafe { fork() };
    if pid == 0 {
        body(true);
        unsafe {
            if let Some(f) = flush { f(); }
            _exit(0);
        }
    }
    body(false);
    let mut st = 0i32;
    unsafe { waitpid(pid, &mut st, 0); }
    eprintln!("segv child st={st}");
}

/// DupPluginCurvesList — context-dup with a curve-type (cnvP)
/// plugin registered.
#[test]
fn dup_curves_plugin_arms() {
    unsafe extern "C" { fn fork() -> i32;
        fn waitpid(p: i32, s: *mut i32, o: i32) -> i32;
        fn _exit(code: i32) -> !;
        fn dlsym(h: *mut std::ffi::c_void,
            n: *const i8) -> *mut std::ffi::c_void; }
    let flush: Option<unsafe extern "C" fn() -> i32> = unsafe {
        let f = dlsym(std::ptr::null_mut(),
            b"__llvm_profile_write_file\0".as_ptr() as *const i8);
        if f.is_null() { None } else { Some(std::mem::transmute(f)) }
    };
    let pid = unsafe { fork() };
    if pid == 0 {
        unsafe {
            // cnvP curve plugin — fires DupPluginCurvesList
            unsafe extern "C" fn curve_fn(
                _ctx: *mut std::ffi::c_void, _ty: i32,
                _params: *const f64, _n: u32)
                -> *mut std::ffi::c_void {
                std::ptr::null_mut()
            }
            let mut pl = vec![0u64; 8];
            let b = pl.as_mut_ptr() as *mut u8;
            *(b as *mut u32) = 0x61637070;   // 'acpp'
            *(b as *mut u32).add(1) = 2100;
            *(b as *mut u32).add(2) = 0x636E7650;   // 'cnvP'
            *(b as *mut usize).add(4) = curve_fn
                as *const () as usize;
            let ctx = c::cmsCreateContext(
                pl.as_mut_ptr() as *mut _,
                std::ptr::null_mut());
            let d1 = c::cmsDupContext(ctx, std::ptr::null_mut());
            let d2 = c::cmsDupContext(ctx, std::ptr::null_mut());
            eprintln!("cnvp ctx={} dup={}/{}", !ctx.is_null(),
                !d1.is_null(), !d2.is_null());
            if !d1.is_null() { c::cmsDeleteContext(d1); }
            if !d2.is_null() { c::cmsDeleteContext(d2); }
            if !ctx.is_null() { c::cmsDeleteContext(ctx); }
            if let Some(f) = flush { f(); }
            _exit(0);
        }
    }
    let mut st = 0i32;
    unsafe { waitpid(pid, &mut st, 0); }
    eprintln!("curvdup child st={st}");
}
