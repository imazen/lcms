//! Differential test: `lcms::lut` vs `cmslut.c`.

use lcms::lut::*;
use lcms_cref as c;
use std::ptr;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}

const AT_END: i32 = 1; // cmsAT_END

/// Build a C-side pipeline of [matrix -> curves -> clut16] — the classic LUT chain.
fn c_build_chain(mat: &[f64; 9], off: &[f64; 3], gamma: f64, clut: &[u16], grid: u32) -> c::Handle {
    unsafe {
        let lut = c::cmsPipelineAlloc(ptr::null_mut(), 3, 3);
        assert!(!lut.is_null());

        let s = c::cmsStageAllocMatrix(ptr::null_mut(), 3, 3, mat.as_ptr(), off.as_ptr());
        assert_eq!(c::cmsPipelineInsertStage(lut, AT_END, s), 1);

        let g = c::cmsBuildGamma(ptr::null_mut(), gamma);
        let curves = [g, g, g];
        let s = c::cmsStageAllocToneCurves(ptr::null_mut(), 3, curves.as_ptr());
        assert_eq!(c::cmsPipelineInsertStage(lut, AT_END, s), 1);
        c::cmsFreeToneCurve(g);

        let s = c::cmsStageAllocCLut16bit(ptr::null_mut(), grid, 3, 3, clut.as_ptr());
        assert_eq!(c::cmsPipelineInsertStage(lut, AT_END, s), 1);

        lut
    }
}

fn r_build_chain(mat: &[f64; 9], off: &[f64; 3], gamma: f64, clut: &[u16], grid: u32) -> Pipeline {
    let mut lut = pipeline_alloc(None, 3, 3).unwrap();
    assert!(lut.insert_stage(
        StageLoc::AtEnd,
        stage_alloc_matrix(None, 3, 3, mat, Some(off)).unwrap()
    ));
    let g = lcms::gamma::build_gamma(None, gamma).unwrap();
    assert!(lut.insert_stage(
        StageLoc::AtEnd,
        stage_alloc_tone_curves(None, 3, Some(&[&g, &g, &g])).unwrap()
    ));
    assert!(lut.insert_stage(
        StageLoc::AtEnd,
        stage_alloc_clut16bit(None, grid, 3, 3, Some(clut)).unwrap()
    ));
    lut
}

fn c_eval16(lut: c::Handle, i_n: &[u16; 3]) -> [u16; 3] {
    let mut out = [0u16; 3];
    unsafe { c::cmsPipelineEval16(i_n.as_ptr(), out.as_mut_ptr(), lut) };
    out
}

fn c_eval_float(lut: c::Handle, i_n: &[f32; 3]) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    unsafe { c::cmsPipelineEvalFloat(i_n.as_ptr(), out.as_mut_ptr(), lut) };
    out
}

#[test]
fn quantize_val_sweep() {
    let mut s = 1u64;
    for _ in 0..20_000 {
        let i = (rng(&mut s) % 0xffff) as f64;
        let max = (rng(&mut s) % 0xffff + 2) as u32;
        let c_val = unsafe { c::_cmsQuantizeVal(i, max) };
        let r_val = lcms::fixed::quantize_val(i, max);
        assert_eq!(r_val, c_val, "i={i} max={max}");
    }
}

#[test]
fn pipeline_matrix_curves_clut_eval16() {
    let mut s = 7u64;
    let mat: [f64; 9] = std::array::from_fn(|_| (rng(&mut s) % 20000) as f64 / 10000.0 - 0.5);
    let off: [f64; 3] = std::array::from_fn(|_| (rng(&mut s) % 1000) as f64 / 10000.0);
    let grid = 9u32;
    let clut: Vec<u16> = (0..(grid * grid * grid * 3))
        .map(|_| rng(&mut s) as u16)
        .collect();

    let c_lut = c_build_chain(&mat, &off, 2.2, &clut, grid);
    let r_lut = r_build_chain(&mat, &off, 2.2, &clut, grid);

    for _ in 0..2000 {
        let i_n: [u16; 3] = std::array::from_fn(|_| rng(&mut s) as u16);
        let co = c_eval16(c_lut, &i_n);
        let mut ro = [0u16; 3];
        r_lut.eval16(&i_n, &mut ro);
        assert_eq!(ro, co, "eval16 in={i_n:?}");
    }
    unsafe { c::cmsPipelineFree(c_lut) };
}

#[test]
fn pipeline_eval_float() {
    let mut s = 13u64;
    let mat: [f64; 9] = std::array::from_fn(|_| (rng(&mut s) % 20000) as f64 / 10000.0 - 0.5);
    let off: [f64; 3] = std::array::from_fn(|_| (rng(&mut s) % 1000) as f64 / 10000.0);
    let grid = 7u32;
    let clut_f: Vec<f32> = (0..(grid * grid * grid * 3))
        .map(|_| (rng(&mut s) % 0x10000) as f32 / 0x10000 as f32)
        .collect();

    // C side
    let c_lut = unsafe {
        let lut = c::cmsPipelineAlloc(ptr::null_mut(), 3, 3);
        let st = c::cmsStageAllocMatrix(ptr::null_mut(), 3, 3, mat.as_ptr(), off.as_ptr());
        c::cmsPipelineInsertStage(lut, AT_END, st);
        let st = c::cmsStageAllocCLutFloat(ptr::null_mut(), grid, 3, 3, clut_f.as_ptr());
        c::cmsPipelineInsertStage(lut, AT_END, st);
        lut
    };
    // Rust side
    let mut r_lut = pipeline_alloc(None, 3, 3).unwrap();
    assert!(r_lut.insert_stage(
        StageLoc::AtEnd,
        stage_alloc_matrix(None, 3, 3, &mat, Some(&off)).unwrap()
    ));
    assert!(r_lut.insert_stage(
        StageLoc::AtEnd,
        stage_alloc_clut_float(None, grid, 3, 3, Some(&clut_f)).unwrap()
    ));

    for _ in 0..4000 {
        let i_n: [f32; 3] = std::array::from_fn(|_| {
            (rng(&mut s) % 0x20000) as f32 / 0x10000 as f32 - 0.5 // include out-of-range
        });
        let co = c_eval_float(c_lut, &i_n);
        let mut ro = [0.0f32; 3];
        r_lut.eval_float(&i_n, &mut ro);
        assert_eq!(ro.map(f32::to_bits), co.map(f32::to_bits), "in={i_n:?}");
    }
    unsafe { c::cmsPipelineFree(c_lut) };
}

#[test]
fn lab_xyz_and_normalize_stages() {
    let mut s = 3u64;
    // Pipelines of a single specialized stage each
    let stages: Vec<(&str, c::Handle, Stage)> = unsafe {
        vec![
            (
                "lab2xyz",
                c::_cmsStageAllocLab2XYZ(ptr::null_mut()),
                stage_alloc_lab2xyz(None),
            ),
            (
                "xyz2lab",
                c::_cmsStageAllocXYZ2Lab(ptr::null_mut()),
                stage_alloc_xyz2lab(None),
            ),
            (
                "v2v4",
                c::_cmsStageAllocLabV2ToV4(ptr::null_mut()),
                stage_alloc_lab_v2_to_v4(None).unwrap(),
            ),
            (
                "v4v2",
                c::_cmsStageAllocLabV4ToV2(ptr::null_mut()),
                stage_alloc_lab_v4_to_v2(None).unwrap(),
            ),
            (
                "v2v4curves",
                c::_cmsStageAllocLabV2ToV4curves(ptr::null_mut()),
                stage_alloc_lab_v2_to_v4_curves(None).unwrap(),
            ),
            (
                "prelin",
                c::_cmsStageAllocLabPrelin(ptr::null_mut()),
                stage_alloc_lab_prelin(None).unwrap(),
            ),
            (
                "fromlab",
                c::_cmsStageNormalizeFromLabFloat(ptr::null_mut()),
                stage_normalize_from_lab_float(None).unwrap(),
            ),
            (
                "fromxyz",
                c::_cmsStageNormalizeFromXyzFloat(ptr::null_mut()),
                stage_normalize_from_xyz_float(None).unwrap(),
            ),
            (
                "tolab",
                c::_cmsStageNormalizeToLabFloat(ptr::null_mut()),
                stage_normalize_to_lab_float(None).unwrap(),
            ),
            (
                "toxyz",
                c::_cmsStageNormalizeToXyzFloat(ptr::null_mut()),
                stage_normalize_to_xyz_float(None).unwrap(),
            ),
            (
                "clip",
                c::_cmsStageClipNegatives(ptr::null_mut(), 3),
                stage_clip_negatives(None, 3),
            ),
        ]
    };

    for (name, c_stage, r_stage) in stages {
        let c_lut = unsafe {
            let lut = c::cmsPipelineAlloc(ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(lut, AT_END, c_stage);
            lut
        };
        let mut r_lut = pipeline_alloc(None, 3, 3).unwrap();
        assert!(r_lut.insert_stage(StageLoc::AtEnd, r_stage), "{name}");

        for _ in 0..500 {
            let i_n: [f32; 3] =
                std::array::from_fn(|_| (rng(&mut s) % 0x40000) as f32 / 0x20000 as f32 - 0.5);
            let co = c_eval_float(c_lut, &i_n);
            let mut ro = [0.0f32; 3];
            r_lut.eval_float(&i_n, &mut ro);
            assert_eq!(
                ro.map(f32::to_bits),
                co.map(f32::to_bits),
                "{name} in={i_n:?}"
            );
        }
        unsafe { c::cmsPipelineFree(c_lut) };
    }
}

#[test]
fn identity_and_identity_clut() {
    // Identity stage
    let c_id = unsafe { c::cmsStageAllocIdentity(ptr::null_mut(), 4) };
    let r_id = stage_alloc_identity(None, 4);
    let c_lut = unsafe {
        let lut = c::cmsPipelineAlloc(ptr::null_mut(), 4, 4);
        c::cmsPipelineInsertStage(lut, AT_END, c_id);
        lut
    };
    let mut r_lut = pipeline_alloc(None, 4, 4).unwrap();
    assert!(r_lut.insert_stage(StageLoc::AtEnd, r_id));

    let mut s = 5u64;
    for _ in 0..500 {
        let i_n: [u16; 4] = std::array::from_fn(|_| rng(&mut s) as u16);
        let mut co = [0u16; 4];
        let mut ro = [0u16; 4];
        unsafe { c::cmsPipelineEval16(i_n.as_ptr(), co.as_mut_ptr(), c_lut) };
        r_lut.eval16(&i_n, &mut ro);
        assert_eq!(ro, co);
    }
    unsafe { c::cmsPipelineFree(c_lut) };

    // _cmsStageAllocIdentityCLut — the sampled-identity CLUT
    let c_cl = unsafe { c::_cmsStageAllocIdentityCLut(ptr::null_mut(), 3) };
    assert!(!c_cl.is_null());
    let r_cl = stage_alloc_identity_clut(None, 3).unwrap();
    let c_lut = unsafe {
        let lut = c::cmsPipelineAlloc(ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(lut, AT_END, c_cl);
        lut
    };
    let mut r_lut = pipeline_alloc(None, 3, 3).unwrap();
    assert!(r_lut.insert_stage(StageLoc::AtEnd, r_cl));
    for _ in 0..500 {
        let i_n: [u16; 3] = std::array::from_fn(|_| rng(&mut s) as u16);
        let mut co = [0u16; 3];
        let mut ro = [0u16; 3];
        unsafe { c::cmsPipelineEval16(i_n.as_ptr(), co.as_mut_ptr(), c_lut) };
        r_lut.eval16(&i_n, &mut ro);
        assert_eq!(ro, co);
    }
    unsafe { c::cmsPipelineFree(c_lut) };
}

extern "C" fn ramp_sampler(i_n: *const u16, out: *mut u16, cargo: *mut std::ffi::c_void) -> i32 {
    unsafe {
        let n = *(cargo as *const usize);
        for i in 0..n {
            *out.add(i) = i_n.add(i).read().wrapping_add(1); // +1 ramp
        }
    }
    1
}

#[test]
fn sample_clut_and_slice() {
    // Sample the +1 ramp into a C-side CLUT and a Rust-side CLUT; eval parity.
    let grid = 9u32;
    let mut n3 = 3usize;
    let c_stage = unsafe {
        let st = c::cmsStageAllocCLut16bit(ptr::null_mut(), grid, 3, 3, ptr::null());
        assert_eq!(
            c::cmsStageSampleCLut16bit(st, ramp_sampler, &mut n3 as *mut usize as *mut _, 0),
            1
        );
        st
    };
    let mut r_stage = stage_alloc_clut16bit(None, grid, 3, 3, None).unwrap();
    assert!(stage_sample_clut16bit(
        &mut r_stage,
        &mut |i_n, out| {
            for i in 0..3 {
                out[i] = i_n[i].wrapping_add(1);
            }
            1
        },
        0
    ));

    let c_lut = unsafe {
        let lut = c::cmsPipelineAlloc(ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(lut, AT_END, c_stage);
        lut
    };
    let mut r_lut = pipeline_alloc(None, 3, 3).unwrap();
    assert!(r_lut.insert_stage(StageLoc::AtEnd, r_stage));

    let mut s = 11u64;
    for _ in 0..1000 {
        let i_n: [u16; 3] = std::array::from_fn(|_| rng(&mut s) as u16);
        let mut co = [0u16; 3];
        let mut ro = [0u16; 3];
        unsafe { c::cmsPipelineEval16(i_n.as_ptr(), co.as_mut_ptr(), c_lut) };
        r_lut.eval16(&i_n, &mut ro);
        assert_eq!(ro, co, "in={i_n:?}");
    }
    unsafe { c::cmsPipelineFree(c_lut) };

    // cmsSliceSpace16 — verify the In sequence matches between C and Rust.
    extern "C" fn recorder(i_n: *const u16, _o: *mut u16, cargo: *mut std::ffi::c_void) -> i32 {
        unsafe {
            let v = &mut *(cargo as *mut Vec<u16>);
            for i in 0..3 {
                v.push(*i_n.add(i));
            }
        }
        1
    }
    let mut c_seq: Vec<u16> = Vec::new();
    let mut r_seq: Vec<u16> = Vec::new();
    let dims = [4u32, 3, 5];
    unsafe {
        c::cmsSliceSpace16(
            3,
            dims.as_ptr(),
            recorder,
            &mut c_seq as *mut Vec<u16> as *mut _,
        );
    }
    assert!(slice_space16(3, &dims, &mut |i_n, _out| {
        r_seq.extend_from_slice(&i_n[..3]);
        1
    }));
    assert_eq!(r_seq, c_seq);
}

#[test]
fn pipeline_dup_cat_unlink() {
    let mut s = 17u64;
    let mat: [f64; 9] = std::array::from_fn(|_| (rng(&mut s) % 20000) as f64 / 10000.0);
    let clut: Vec<u16> = (0..27 * 3).map(|_| rng(&mut s) as u16).collect();
    let c_lut = c_build_chain(&mat, &[0.0; 3], 1.8, &clut, 3);
    let r_lut = r_build_chain(&mat, &[0.0; 3], 1.8, &clut, 3);

    // Dup — C: cmsPipelineDup; Rust: pipeline.dup()
    let c_dup = unsafe { c::cmsPipelineDup(c_lut) };
    let r_dup = r_lut.dup().unwrap();
    unsafe {
        assert_eq!(
            c::cmsPipelineStageCount(c_lut),
            c::cmsPipelineStageCount(c_dup)
        );
    }
    assert_eq!(r_lut.stage_count(), r_dup.stage_count());
    for _ in 0..300 {
        let i_n: [u16; 3] = std::array::from_fn(|_| rng(&mut s) as u16);
        let mut co = [0u16; 3];
        let mut ro = [0u16; 3];
        unsafe { c::cmsPipelineEval16(i_n.as_ptr(), co.as_mut_ptr(), c_dup) };
        r_dup.eval16(&i_n, &mut ro);
        assert_eq!(ro, co);
    }

    // Cat — [identity3] cat [chain] vs C same
    let c_l2 = unsafe {
        let lut = c::cmsPipelineAlloc(ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(lut, AT_END, c::cmsStageAllocIdentity(ptr::null_mut(), 3));
        assert_eq!(c::cmsPipelineCat(lut, c_lut), 1);
        lut
    };
    let mut r_l2 = pipeline_alloc(None, 3, 3).unwrap();
    assert!(r_l2.insert_stage(StageLoc::AtEnd, stage_alloc_identity(None, 3)));
    assert!(r_l2.cat(&r_lut));
    unsafe { assert_eq!(c::cmsPipelineStageCount(c_l2), r_l2.stage_count()) };
    for _ in 0..300 {
        let i_n: [u16; 3] = std::array::from_fn(|_| rng(&mut s) as u16);
        let mut co = [0u16; 3];
        let mut ro = [0u16; 3];
        unsafe { c::cmsPipelineEval16(i_n.as_ptr(), co.as_mut_ptr(), c_l2) };
        r_l2.eval16(&i_n, &mut ro);
        assert_eq!(ro, co);
    }

    // Unlink last stage from both — resulting pipelines must agree
    unsafe { c::cmsPipelineUnlinkStage(c_l2, AT_END, ptr::null_mut()) };
    let removed = r_l2.unlink_stage(StageLoc::AtEnd).unwrap();
    assert_eq!(removed.r#type, lcms::signatures::stage::SigCLutElemType);
    unsafe { assert_eq!(c::cmsPipelineStageCount(c_l2), r_l2.stage_count()) };
    for _ in 0..300 {
        let i_n: [u16; 3] = std::array::from_fn(|_| rng(&mut s) as u16);
        let mut co = [0u16; 3];
        let mut ro = [0u16; 3];
        unsafe { c::cmsPipelineEval16(i_n.as_ptr(), co.as_mut_ptr(), c_l2) };
        r_l2.eval16(&i_n, &mut ro);
        assert_eq!(ro, co);
    }

    unsafe {
        c::cmsPipelineFree(c_lut);
        c::cmsPipelineFree(c_dup);
        c::cmsPipelineFree(c_l2);
    }
}

#[test]
fn eval_reverse_float() {
    // Identity CLUT (sampled) on both sides → bit-identical tables → Newton
    // iterations must agree step for step.
    let c_stage = unsafe { c::_cmsStageAllocIdentityCLut(ptr::null_mut(), 3) };
    let c_lut = unsafe {
        let lut = c::cmsPipelineAlloc(ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(lut, AT_END, c_stage);
        lut
    };
    let mut r_lut = pipeline_alloc(None, 3, 3).unwrap();
    assert!(r_lut.insert_stage(StageLoc::AtEnd, stage_alloc_identity_clut(None, 3).unwrap()));

    for t in 0..300 {
        let target: [f32; 4] = [0.2 + (t as f32 / 300.0) * 0.6, 0.4, 0.55, 0.0];
        let mut c_target = target;
        let mut c_res = [0.0f32; 4];
        let mut r_res = [0.0f32; 4];
        let c_ok = unsafe {
            c::cmsPipelineEvalReverseFloat(
                c_target.as_mut_ptr(),
                c_res.as_mut_ptr(),
                ptr::null_mut(),
                c_lut,
            )
        };
        let r_ok = r_lut.eval_reverse_float(&target, &mut r_res, None);
        assert_eq!(r_ok as i32, c_ok);
        if c_ok != 0 {
            assert_eq!(
                r_res[..3].iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                c_res[..3].iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                "target={target:?}"
            );
        }
    }

    // With hint
    for t in 0..100 {
        let target: [f32; 4] = [0.5, 0.3 + (t as f32 / 100.0) * 0.4, 0.7, 0.0];
        let hint = [0.4f32, 0.4, 0.4];
        let mut c_target = target;
        let mut c_hint = hint;
        let mut c_res = [0.0f32; 4];
        let mut r_res = [0.0f32; 4];
        let c_ok = unsafe {
            c::cmsPipelineEvalReverseFloat(
                c_target.as_mut_ptr(),
                c_res.as_mut_ptr(),
                c_hint.as_mut_ptr(),
                c_lut,
            )
        };
        let r_ok = r_lut.eval_reverse_float(&target, &mut r_res, Some(&hint));
        assert_eq!(r_ok as i32, c_ok);
        if c_ok != 0 {
            assert_eq!(
                r_res[..3].iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                c_res[..3].iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                "hint target={target:?}"
            );
        }
    }
    unsafe {
        c::cmsPipelineFree(c_lut);
    }
}
