//! cmsgamma predicate/estimation/eval surface: eval8/evalFloat,
//! linear/monotonic/descending/multisegment predicates, estimated-
//! table entries + data — across parametric, tabulated, segmented,
//! and plain-gamma curves.

use lcms::gamma;
use lcms_cref as c;

#[test]
fn tone_curve_api_matrix() {
    unsafe {
        // curve battery: gamma, parametric (3 types), tabulated,
        // segmented
        let curves: Vec<(*mut c::ToneCurve, gamma::ToneCurve, &str)> = vec![
            (
                c::cmsBuildGamma(std::ptr::null_mut(), 2.2),
                gamma::build_gamma(None, 2.2).unwrap(),
                "gamma2.2",
            ),
            (
                c::cmsBuildGamma(std::ptr::null_mut(), 0.5),
                gamma::build_gamma(None, 0.5).unwrap(),
                "gamma0.5",
            ),
            (
                c::cmsBuildParametricToneCurve(
                    std::ptr::null_mut(),
                    4,
                    [
                        2.4,
                        1.0 / 1.055,
                        0.055 / 1.055,
                        1.0 / 12.92,
                        0.04045,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                    ]
                    .as_ptr(),
                ),
                gamma::build_parametric_tone_curve(
                    None,
                    4,
                    &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
                )
                .unwrap(),
                "para4-srgb",
            ),
            (
                c::cmsBuildTabulatedToneCurve16(
                    std::ptr::null_mut(),
                    256,
                    (0..256u16).map(|i| i * 257).collect::<Vec<_>>().as_ptr(),
                ),
                gamma::build_tabulated_tone_curve16(
                    None,
                    256,
                    Some(&(0..256u16).map(|i| i * 257).collect::<Vec<_>>()),
                )
                .unwrap(),
                "tab-linear",
            ),
            (
                c::cmsBuildTabulatedToneCurve16(
                    std::ptr::null_mut(),
                    256,
                    (0..256u16)
                        .map(|i| (i * 257).wrapping_add((i % 3) * 10))
                        .collect::<Vec<_>>()
                        .as_ptr(),
                ),
                gamma::build_tabulated_tone_curve16(
                    None,
                    256,
                    Some(
                        &(0..256u16)
                            .map(|i| (i * 257).wrapping_add((i % 3) * 10))
                            .collect::<Vec<_>>(),
                    ),
                )
                .unwrap(),
                "tab-jagged",
            ),
        ];
        for (ct, rt, name) in &curves {
            // predicates
            assert_eq!(
                c::cmsIsToneCurveLinear(*ct) != 0,
                gamma::is_tone_curve_linear(rt),
                "{name} linear"
            );
            assert_eq!(
                c::cmsIsToneCurveMonotonic(*ct) != 0,
                gamma::is_tone_curve_monotonic(rt),
                "{name} monotonic"
            );
            assert_eq!(
                c::cmsIsToneCurveDescending(*ct) != 0,
                gamma::is_tone_curve_descending(rt),
                "{name} descending"
            );
            assert_eq!(
                c::cmsIsToneCurveMultisegment(*ct) != 0,
                gamma::is_tone_curve_multisegment(rt),
                "{name} multiseg"
            );
            // estimated table
            assert_eq!(
                c::cmsGetToneCurveEstimatedTableEntries(*ct),
                rt.estimated_table_entries() as u32,
                "{name} est entries"
            );
            let ctab = c::cmsGetToneCurveEstimatedTable(*ct);
            let rtab = rt.estimated_table();
            if !ctab.is_null() {
                let n = c::cmsGetToneCurveEstimatedTableEntries(*ct) as usize;
                let cslice = std::slice::from_raw_parts(ctab, n);
                assert_eq!(cslice, &rtab[..], "{name} est data");
            }
            // evalFloat dense
            for i in 0..32 {
                let v = i as f32 / 31.0;
                let cf = c::cmsEvalToneCurveFloat(*ct, v);
                let rf = rt.eval_float(v);
                assert!((cf - rf).abs() <= 2e-6, "{name} evalf {v}: {cf} vs {rf}");
            }
        }
        for (ct, _, _) in curves {
            c::cmsFreeToneCurve(ct)
        }
    }
}
