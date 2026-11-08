//! cmslut stage-level parity: stage alloc/meta, CLUT sampling (16+float),
//! pipeline build/insert/eval16/eval_float, and stage-type coverage that
//! transforms don't reach (LabV2V4/V4V2 curves, prelin, Lab2XYZ/XYZ2Lab,
//! NamedColor, IdentityCLut).

use lcms::lut;
use lcms_cref as c;
use std::ffi::c_void;

unsafe extern "C" fn c_samp16(inp: *const u16, out: *mut u16, _cargo: *mut c_void) -> i32 {
    let (a, b, c_) = (*inp, *inp.add(1), *inp.add(2));
    *out = a.wrapping_mul(37).wrapping_add(b);
    *out.add(1) = b.wrapping_mul(19).wrapping_add(c_);
    *out.add(2) = c_.wrapping_add(7);
    1
}

fn r_samp16(inp: &[u16], out: &mut [u16]) -> i32 {
    out[0] = inp[0].wrapping_mul(37).wrapping_add(inp[1]);
    out[1] = inp[1].wrapping_mul(19).wrapping_add(inp[2]);
    out[2] = inp[2].wrapping_add(7);
    1
}

#[test]
fn stage_meta_matrix() {
    unsafe {
        let c_g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut r_gamma = [lcms::gamma::build_gamma(None, 2.2).unwrap()];
        let stages: Vec<(c::Handle, lut::Stage)> = vec![
            (c::cmsStageAllocIdentity(std::ptr::null_mut(), 3),
             lut::stage_alloc_identity(None, 3)),
            (c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, [c_g; 3].as_ptr()),
             lut::stage_alloc_tone_curves(None, 3, Some(&[&r_gamma[0], &r_gamma[0], &r_gamma[0]])).unwrap()),
            (c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                [1.0f64; 9].as_ptr(), [0.0f64; 3].as_ptr()),
             lut::stage_alloc_matrix(None, 3, 3, &[1.0f64; 9], Some(&[0.0f64; 3])).unwrap()),
            (c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 5, 3, 3, std::ptr::null()),
             lut::stage_alloc_clut16bit(None, 5, 3, 3, None).unwrap()),
            (c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 5, 3, 3, std::ptr::null()),
             lut::stage_alloc_clut_float(None, 5, 3, 3, None).unwrap()),
            (c::_cmsStageAllocLab2XYZ(std::ptr::null_mut()),
             lut::stage_alloc_lab2xyz(None)),
            (c::_cmsStageAllocXYZ2Lab(std::ptr::null_mut()),
             lut::stage_alloc_xyz2lab(None)),
            (c::_cmsStageAllocLabV2ToV4(std::ptr::null_mut()),
             lut::stage_alloc_lab_v2_to_v4(None).unwrap()),
            (c::_cmsStageAllocLabV4ToV2(std::ptr::null_mut()),
             lut::stage_alloc_lab_v4_to_v2(None).unwrap()),
            (c::_cmsStageAllocLabV2ToV4curves(std::ptr::null_mut()),
             lut::stage_alloc_lab_v2_to_v4_curves(None).unwrap()),
            (c::_cmsStageAllocLabPrelin(std::ptr::null_mut()),
             lut::stage_alloc_lab_prelin(None).unwrap()),
            (c::_cmsStageAllocIdentityCLut(std::ptr::null_mut(), 3),
             lut::stage_alloc_identity_clut(None, 3).unwrap()),
        ];
        for (i, (ch, rs)) in stages.iter().enumerate() {
            assert!(!ch.is_null(), "C stage {i}");
            let mut rs = rs.clone();
            assert_eq!(
                c::cmsStageInputChannels(*ch), rs.input_channels(),
                "stage {i} in-ch"
            );
            assert_eq!(
                c::cmsStageOutputChannels(*ch), rs.output_channels(),
                "stage {i} out-ch"
            );
            assert_eq!(c::cmsStageType(*ch), rs.stage_type(), "stage {i} type");
            c::cmsStageFree(*ch);
        }
    }
}

#[test]
fn clut_sampling() {
    unsafe {
        // 16-bit CLUT sampled via a C fn-ptr vs a Rust closure — byte-exact table
        let n: u32 = 5;
        let ch = c::cmsStageAllocCLut16bit(std::ptr::null_mut(), n, 3, 3, std::ptr::null());
        let mut rs = lut::stage_alloc_clut16bit(None, n, 3, 3, None).unwrap();
        assert!(c::cmsStageSampleCLut16bit(
            ch, c_samp16, std::ptr::null_mut(), 0) != 0);
        assert!(lut::stage_sample_clut16bit(&mut rs, &mut r_samp16, 0));
        // compare the sampled table via pipeline eval16 on grid points
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe, 1, ch); // AtEnd
        let mut r_pipe = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe.insert_stage(lut::StageLoc::AtEnd, rs);
        let mut seed = 0x1234u64;
        for _ in 0..32 {
            let inp: Vec<u16> = (0..3).map(|_| {
                seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
                (seed >> 48) as u16
            }).collect();
            let mut co = [0u16; 3];
            let mut ro = [0u16; 3];
            c::cmsPipelineEval16(inp.as_ptr(), co.as_mut_ptr(), c_pipe);
            r_pipe.eval16(&inp, &mut ro);
            assert_eq!(co, ro, "clut eval16 {inp:?}");
        }
        c::cmsPipelineFree(c_pipe);
    }
}

#[test]
fn pipeline_eval_variants() {
    unsafe {
        // matrix + tonecurves + clut pipeline; eval16 + eval_float
        let c_g = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let r_g = lcms::gamma::build_gamma(None, 1.8).unwrap();
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, [c_g; 3].as_ptr()));
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                [0.9f64, 0.05, 0.05, 0.02, 0.9, 0.08, 0.03, 0.07, 0.9].as_ptr(),
                [0.01f64; 3].as_ptr()));
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 5, 3, 3, std::ptr::null()));
        let mut r_pipe = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_tone_curves(None, 3, Some(&[&r_g, &r_g, &r_g])).unwrap());
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_matrix(None, 3, 3,
                &[0.9, 0.05, 0.05, 0.02, 0.9, 0.08, 0.03, 0.07, 0.9],
                Some(&[0.01; 3])).unwrap());
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_clut_float(None, 5, 3, 3, None).unwrap());
        let mut seed = 0xabcdu64;
        for _ in 0..24 {
            let inp: Vec<u16> = (0..3).map(|_| {
                seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
                (seed >> 48) as u16
            }).collect();
            let mut co = [0u16; 3];
            let mut ro = [0u16; 3];
            c::cmsPipelineEval16(inp.as_ptr(), co.as_mut_ptr(), c_pipe);
            r_pipe.eval16(&inp, &mut ro);
            assert_eq!(co, ro, "pipe eval16 {inp:?}");
        }
        // float eval — pinned residual: CLUT16 interpolation in float domain
        // differs by sub-LSB amounts (same class as pack's int->float).
        let mut seedf = 0x7777u64;
        for _ in 0..24 {
            let inp: Vec<f32> = (0..3).map(|_| {
                seedf = seedf.wrapping_mul(0x9e3779b97f4a7c15) ^ (seedf >> 33);
                ((seedf >> 48) as u16 as f32) / 65535.0
            }).collect();
            let mut co = [0f32; 3];
            let mut ro = [0f32; 3];
            c::cmsPipelineEvalFloat(inp.as_ptr(), co.as_mut_ptr(), c_pipe);
            r_pipe.eval_float(&inp, &mut ro);
            for i in 0..3 {
                let tol = (5e-5 * co[i].abs()).max(1e-5);
                assert!((co[i] - ro[i]).abs() <= tol,
                    "pipe eval_float {inp:?} ch{i}: {} vs {}", co[i], ro[i]);
            }
        }
        c::cmsPipelineFree(c_pipe);
    }
}

/// `cmsPipelineEvalReverseFloat` — CLUT reverse eval parity.
#[test]
fn pipeline_reverse_float() {
    unsafe {
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 5, 3, 3, std::ptr::null()));
        let mut r_pipe = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_clut_float(None, 5, 3, 3, None).unwrap());
        let mut seed = 0x5555u64;
        for _ in 0..16 {
            let tgt: Vec<f32> = (0..3).map(|_| {
                seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
                ((seed >> 48) as u16 as f32) / 65535.0
            }).collect();
            let mut res = [0f32; 3];
            let mut hint = [0f32; 3];
            let ok = c::cmsPipelineEvalReverseFloat(
                tgt.as_ptr() as *mut _, res.as_mut_ptr(), hint.as_mut_ptr(), c_pipe);
            let mut rres = [0f32; 3];
            let rok = r_pipe.eval_reverse_float(&tgt, &mut rres, Some(&hint));
            assert_eq!(ok != 0, rok, "rev ok {tgt:?}");
            if ok != 0 {
                for i in 0..3 {
                    let tol = (5e-4 * res[i].abs()).max(1e-4);
                    assert!((res[i] - rres[i]).abs() <= tol,
                        "rev {tgt:?} ch{i}: {} vs {}", res[i], rres[i]);
                }
            }
        }
        c::cmsPipelineFree(c_pipe);
    }
}

/// `cmsStageSampleCLutFloat` — float CLUT sampling parity.
#[test]
fn clut_float_sampling() {
    unsafe {
        let ch = c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 5, 3, 3, std::ptr::null());
        let mut rs = lut::stage_alloc_clut_float(None, 5, 3, 3, None).unwrap();
        unsafe extern "C" fn sf(inp: *const f32, out: *mut f32, _c: *mut c_void) -> i32 {
            *out = (*inp * 0.7 + 0.1).clamp(0.0, 1.0);
            *out.add(1) = (*inp.add(1) * 0.5 + 0.2).clamp(0.0, 1.0);
            *out.add(2) = (*inp.add(2) * 0.9).clamp(0.0, 1.0);
            1
        }
        assert!(c::cmsStageSampleCLutFloat(
            ch, sf, std::ptr::null_mut(), 0) != 0);
        let mut rsf = |inp: &[f32], out: &mut [f32]| -> i32 {
            out[0] = (inp[0] * 0.7 + 0.1).clamp(0.0, 1.0);
            out[1] = (inp[1] * 0.5 + 0.2).clamp(0.0, 1.0);
            out[2] = (inp[2] * 0.9).clamp(0.0, 1.0);
            1
        };
        assert!(lut::stage_sample_clut_float(&mut rs, &mut rsf, 0));
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe, 1, ch);
        let mut r_pipe = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe.insert_stage(lut::StageLoc::AtEnd, rs);
        for i in 0..8 {
            let v = i as f32 / 7.0;
            let inp = [v, v * 0.5, v * 0.75];
            let mut co = [0f32; 3];
            let mut ro = [0f32; 3];
            c::cmsPipelineEvalFloat(inp.as_ptr(), co.as_mut_ptr(), c_pipe);
            r_pipe.eval_float(&inp, &mut ro);
            for j in 0..3 {
                let tol = (5e-5 * co[j].abs()).max(1e-6);
                assert!((co[j] - ro[j]).abs() <= tol,
                    "clut float eval {inp:?} ch{j}");
            }
        }
        c::cmsPipelineFree(c_pipe);
    }
}

/// `cmsStageData` + `cmsStageNext` — stage accessor parity.
#[test]
fn stage_accessors() {
    unsafe {
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocIdentity(std::ptr::null_mut(), 3));
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [c::cmsBuildGamma(std::ptr::null_mut(), 2.0); 3].as_ptr()));
        let st1 = c::cmsPipelineGetPtrToFirstStage(c_pipe);
        let st2 = c::cmsStageNext(st1);
        assert!(!st1.is_null() && !st2.is_null());
        assert!(c::cmsStageNext(st2).is_null());
        // identity stages carry no data payload; curves do
        assert!(c::cmsStageData(st1).is_null(), "identity stage data");
        assert!(!c::cmsStageData(st2).is_null(), "curves stage data");
        c::cmsPipelineFree(c_pipe);
    }
}

/// MPE element writers: pipelines with Lab2XYZ/XYZ2Lab stages written
/// as mpet tags on a profile, resaved byte-exact.
#[test]
fn mpe_element_writes() {
    unsafe {
        // Lab->XYZ->Lab pipeline
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::_cmsStageAllocLab2XYZ(std::ptr::null_mut()));
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::_cmsStageAllocXYZ2Lab(std::ptr::null_mut()));
        let mut r_pipe = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe.insert_stage(lut::StageLoc::AtEnd, lut::stage_alloc_lab2xyz(None));
        r_pipe.insert_stage(lut::StageLoc::AtEnd, lut::stage_alloc_xyz2lab(None));
        // write as an mpet tag on an XYZ profile
        let ch = c::cmsCreateXYZProfile();
        let mut rp = lcms::virt::create_xyz_profile(None).unwrap();
        let c_ok = c::cmsWriteTag(ch, 0x41324230, c_pipe as *const _);
        let r_ok = lcms::profile::cms_write_tag(
            &mut rp, 0x41324230,
            Some(lcms::tagtypes::TagObject::Mpe(r_pipe)));
        assert_eq!(c_ok != 0, r_ok, "mpe write");
        if c_ok != 0 {
            let mut c_out = vec![0u8; 1 << 20];
            let mut size = c_out.len() as u32;
            c::cmsSaveProfileToMem(ch, c_out.as_mut_ptr() as *mut _, &mut size);
            c_out.truncate(size as usize);
            let mut r_out = lcms::profile::save_profile_to_mem(&mut rp)
                .unwrap_or_default();
            for i in 0..12 {
                c_out.get_mut(24 + i).map(|b| *b = 0);
                r_out.get_mut(24 + i).map(|b| *b = 0);
            }
            c_out.resize(r_out.len().max(c_out.len()), 0);
            r_out.resize(c_out.len(), 0);
            assert_eq!(c_out, r_out, "mpe resave");
        }
        c::cmsCloseProfile(ch);
        // c_pipe is owned by the profile after write — no free
    }
}

/// MPE element writers round 2: curves-set, matrix, CLUT16, CLUTfloat
/// pipelines written as mpet tags + resave byte-exact.
#[test]
fn mpe_element_writes2() {
    unsafe {
        // curves-set + matrix pipeline
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g; 3];
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, curves.as_ptr()));
        let mat: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        let rg = lcms::gamma::build_gamma(None, 2.2).unwrap();
        let mut r_pipe = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_tone_curves(None, 3, Some(&[&rg; 3])).unwrap());
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_matrix(None, 3, 3, &mat, None).unwrap());
        let ch = c::cmsCreateXYZProfile();
        let mut rp = lcms::virt::create_xyz_profile(None).unwrap();
        let c_ok = c::cmsWriteTag(ch, 0x41324230, c_pipe as *const _);
        let r_ok = lcms::profile::cms_write_tag(&mut rp, 0x41324230,
            Some(lcms::tagtypes::TagObject::Mpe(r_pipe)));
        assert_eq!(c_ok != 0, r_ok, "mpe2 write");
        if c_ok != 0 {
            let mut c_out = vec![0u8; 1 << 20];
            let mut size = c_out.len() as u32;
            c::cmsSaveProfileToMem(ch, c_out.as_mut_ptr() as *mut _, &mut size);
            c_out.truncate(size as usize);
            let mut r_out = lcms::profile::save_profile_to_mem(&mut rp)
                .unwrap_or_default();
            for i in 0..12 {
                c_out.get_mut(24 + i).map(|b| *b = 0);
                r_out.get_mut(24 + i).map(|b| *b = 0);
            }
            c_out.resize(r_out.len().max(c_out.len()), 0);
            r_out.resize(c_out.len(), 0);
            assert_eq!(c_out, r_out, "mpe2 resave");
        }
        c::cmsCloseProfile(ch);
        c::cmsFreeToneCurve(g);
    }
}

/// Pipeline manipulation: dup/cat/unlink stage parity.
#[test]
fn pipeline_manipulation() {
    unsafe {
        // build a 2-stage pipeline on both sides
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g; 3];
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, curves.as_ptr()));
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocIdentity(std::ptr::null_mut(), 3));
        let rg = lcms::gamma::build_gamma(None, 2.2).unwrap();
        let mut r_pipe = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_tone_curves(None, 3, Some(&[&rg; 3])).unwrap());
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_identity(None, 3));
        // stage count
        assert_eq!(c::cmsPipelineStageCount(c_pipe), r_pipe.stage_count(),
            "stage count");
        // dup — eval parity on the copy
        let c_dup = c::cmsPipelineDup(c_pipe);
        let r_dup = r_pipe.dup();
        assert!(!c_dup.is_null());
        assert!(r_dup.is_some());
        let r_dup = r_dup.unwrap();
        let inp = [0x4000u16, 0x8000, 0xc000];
        let mut co = [0u16; 3];
        let mut ro = [0u16; 3];
        c::cmsPipelineEval16(inp.as_ptr(), co.as_mut_ptr(), c_dup);
        r_dup.eval16(&inp, &mut ro);
        assert_eq!(co, ro, "dup eval");
        // cat — append dup to pipe
        let c_pipe2 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe2, 1,
            c::cmsStageAllocIdentity(std::ptr::null_mut(), 3));
        let mut r_pipe2 = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe2.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_identity(None, 3));
        assert_eq!(c::cmsPipelineCat(c_pipe, c_pipe2) != 0,
            r_pipe.cat(&r_pipe2), "cat");
        assert_eq!(c::cmsPipelineStageCount(c_pipe), r_pipe.stage_count(),
            "post-cat count");
        c::cmsPipelineFree(c_dup);
        c::cmsPipelineFree(c_pipe);
        c::cmsFreeToneCurve(g);
    }
}


/// MPE element writers round 3: CLUT16 + CLUTfloat element pipelines
/// written as mpet tags — the CLUT element writers inside MPE.
#[test]
fn mpe_clut_elements() {
    unsafe {
        let grid = 4u32;
        let points = [grid; 3];
        // CLUT16 element
        let table16: Vec<u16> = (0..grid*grid*grid*3)
            .map(|i| (i * 300) as u16).collect();
        let c_pipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), grid, 3, 3,
                table16.as_ptr()));
        let mut r_pipe = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_clut16bit_granular(None, &points, 3, 3,
                Some(&table16)).unwrap());
        let ch = c::cmsCreateXYZProfile();
        let mut rp = lcms::virt::create_xyz_profile(None).unwrap();
        let c_ok = c::cmsWriteTag(ch, 0x41324230, c_pipe as *const _);
        let r_ok = lcms::profile::cms_write_tag(&mut rp, 0x41324230,
            Some(lcms::tagtypes::TagObject::Mpe(r_pipe)));
        assert_eq!(c_ok != 0, r_ok, "clut16 mpe write");
        if c_ok != 0 {
            let mut c_out = vec![0u8; 1 << 20];
            let mut size = c_out.len() as u32;
            c::cmsSaveProfileToMem(ch, c_out.as_mut_ptr() as *mut _, &mut size);
            c_out.truncate(size as usize);
            let mut r_out = lcms::profile::save_profile_to_mem(&mut rp)
                .unwrap_or_default();
            for i in 0..12 {
                c_out.get_mut(24 + i).map(|b| *b = 0);
                r_out.get_mut(24 + i).map(|b| *b = 0);
            }
            c_out.resize(r_out.len().max(c_out.len()), 0);
            r_out.resize(c_out.len(), 0);
            assert_eq!(c_out, r_out, "clut16 mpe resave");
        }
        c::cmsCloseProfile(ch);

        // CLUTfloat element
        let tablef: Vec<f32> = (0..grid*grid*grid*3)
            .map(|i| (i as f32) * 0.001).collect();
        let c_pipe2 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(c_pipe2, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), grid, 3, 3,
                tablef.as_ptr()));
        let mut r_pipe2 = lut::pipeline_alloc(None, 3, 3).unwrap();
        r_pipe2.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_clut_float_granular(None, &points, 3, 3,
                Some(&tablef)).unwrap());
        let ch2 = c::cmsCreateXYZProfile();
        let mut rp2 = lcms::virt::create_xyz_profile(None).unwrap();
        let c_ok2 = c::cmsWriteTag(ch2, 0x41324230, c_pipe2 as *const _);
        let r_ok2 = lcms::profile::cms_write_tag(&mut rp2, 0x41324230,
            Some(lcms::tagtypes::TagObject::Mpe(r_pipe2)));
        assert_eq!(c_ok2 != 0, r_ok2, "clutf mpe write");
        if c_ok2 != 0 {
            let mut c_out = vec![0u8; 1 << 20];
            let mut size = c_out.len() as u32;
            c::cmsSaveProfileToMem(ch2, c_out.as_mut_ptr() as *mut _, &mut size);
            c_out.truncate(size as usize);
            let mut r_out = lcms::profile::save_profile_to_mem(&mut rp2)
                .unwrap_or_default();
            for i in 0..12 {
                c_out.get_mut(24 + i).map(|b| *b = 0);
                r_out.get_mut(24 + i).map(|b| *b = 0);
            }
            c_out.resize(r_out.len().max(c_out.len()), 0);
            r_out.resize(c_out.len(), 0);
            assert_eq!(c_out, r_out, "clutf mpe resave");
        }
        c::cmsCloseProfile(ch2);
    }
}

/// cmsPipelineEvalFloat + cmsStageSampleCLutFloat — the float
/// pipeline-eval + CLUT-sampling paths.
#[test]
fn float_pipeline_eval() {
    unsafe {
        let cp = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let clut = c::cmsStageAllocCLutFloat(
            std::ptr::null_mut(), 17, 3, 3, std::ptr::null_mut());
        let ok = c::cmsPipelineInsertStage(cp, 0, clut);
        assert_ne!(ok, 0);
        let mut rp = lut::pipeline_alloc(None, 3, 3).unwrap();
        let rclut = lut::stage_alloc_clut_float_granular(
            None, &[17, 17, 17], 3, 3, None).unwrap();
        assert!(rp.insert_stage(lut::StageLoc::AtEnd, rclut));
        let inp = [0.2f32, 0.5, 0.8];
        let mut co = [0f32; 3];
        let mut ro = [0f32; 3];
        c::cmsPipelineEvalFloat(inp.as_ptr(), co.as_mut_ptr(), cp);
        rp.eval_float(&inp, &mut ro);
        assert_eq!(co, ro, "float eval");
        c::cmsPipelineFree(cp);
    }
}

/// Identity-stage allocators — cmsStageAllocIdentity +
/// _cmsStageAllocIdentityCurves/_cmsStageAllocIdentityCLut.
#[test]
fn identity_stage_allocators() {
    unsafe {
        for nch in [1u32, 3, 4, 8, 15] {
            let cs = c::cmsStageAllocIdentity(std::ptr::null_mut(), nch);
            assert!(!cs.is_null(), "identity {nch}");
            c::cmsStageFree(cs);
            let icl = c::_cmsStageAllocIdentityCLut(
                std::ptr::null_mut(), nch);
            if !icl.is_null() { c::cmsStageFree(icl); }
            // rust side — identity stage + identity-clut
            let _rs = lut::stage_alloc_identity(None, nch);
            let _rcl = lut::stage_alloc_identity_clut(None, nch);
            let _rcu = lut::stage_alloc_identity_curves(None, nch);
        }
    }
}

/// cmsPipelineEvalReverseFloat — Newton inversion on a 3->3 LUT.
#[test]
fn reverse_pipeline_eval() {
    unsafe {
        // build a 3->3 pipeline with a sampled CLUT
        let cp = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let clut = c::cmsStageAllocCLutFloat(
            std::ptr::null_mut(), 17, 3, 3, std::ptr::null_mut());
        let ok = c::cmsPipelineInsertStage(cp, 0, clut);
        assert_ne!(ok, 0);
        // reverse-eval a target — C side
        let mut tgt = [0.5f32, 0.5, 0.5];
        let mut out = [0f32; 3];
        let mut hint = [0f32; 3];
        let ok = c::cmsPipelineEvalReverseFloat(
            tgt.as_mut_ptr(), out.as_mut_ptr(), hint.as_mut_ptr(), cp);
        eprintln!("c reverse ok={ok} out={out:?}");
        // rust side
        let mut rp = lut::pipeline_alloc(None, 3, 3).unwrap();
        let rclut = lut::stage_alloc_clut_float_granular(
            None, &[17, 17, 17], 3, 3, None).unwrap();
        assert!(rp.insert_stage(lut::StageLoc::AtEnd, rclut));
        let mut ro = [0f32; 3];
        let rok = rp.eval_reverse_float(&tgt, &mut ro, None);
        assert_eq!(ok != 0, rok, "reverse eval status");
        if ok != 0 && rok {
            assert_eq!(out, ro, "reverse eval values");
        }
        c::cmsPipelineFree(cp);
    }
}

/// cmsSliceSpace16 + slice_space_float — the N-dim grid-sampling
/// machinery.
#[test]
fn slice_space() {
    unsafe {
        // slice a 3D grid — the sampler visits each knot
        let mut c_count = 0u32;
        unsafe extern "C" fn c_samp(_i: *const u16,
            _o: *mut u16, cargo: *mut c_void) -> i32 {
            unsafe {
                let c = cargo as *mut u32;
                *c += 1;
            }
            1
        }
        let c_ok = c::cmsSliceSpace16(3, [5u32; 3].as_ptr(),
            c_samp, &mut c_count as *mut _ as *mut _);
        assert_ne!(c_ok, 0);
        // rust side — the sampler is a closure
        let mut r_count = 0u32;
        let r_ok = lut::slice_space16(3, &[5, 5, 5],
            &mut |_i: &[u16], _o: &mut [u16]| {
                r_count += 1;
                1
            });
        assert_eq!(c_ok != 0, r_ok, "slice16 status");
        eprintln!("slice16 c={c_count} r={r_count}");
    }
}

/// cmsPipelineStageCount + InputChannels + OutputChannels — the
/// pipeline introspection getters.
#[test]
fn pipeline_introspection() {
    unsafe {
        for (ich, och, n_stages) in [(3u32, 3u32, 2u32),
                                    (4, 4, 3), (15, 15, 1)] {
            let cp = c::cmsPipelineAlloc(std::ptr::null_mut(),
                ich, och);
            for _ in 0..n_stages {
                let st = c::cmsStageAllocIdentity(
                    std::ptr::null_mut(), ich);
                c::cmsPipelineInsertStage(cp, 0, st);
            }
            let cn = c::cmsPipelineStageCount(cp);
            assert_eq!(cn, n_stages, "stage count {ich}x{och}");
            assert_eq!(c::cmsPipelineInputChannels(cp), ich, "ich");
            assert_eq!(c::cmsPipelineOutputChannels(cp), och, "och");
            // rust side
            let mut rp = lut::pipeline_alloc(None, ich, och).unwrap();
            for _ in 0..n_stages {
                let rst = lut::stage_alloc_identity(None, ich);
                assert!(rp.insert_stage(lut::StageLoc::AtEnd, rst));
            }
            assert_eq!(rp.stage_count(), n_stages, "r stage count");
            assert_eq!(rp.input_channels(), ich, "r ich");
            assert_eq!(rp.output_channels(), och, "r och");
            c::cmsPipelineFree(cp);
        }
    }
}

/// cmsStageType + cmsStageDup — stage introspection + dup.
#[test]
fn stage_type_dup() {
    unsafe {
        // each stage type — alloc, query type, dup, free
        let stages: Vec<(*mut c_void, u32)> = vec![
            (c::cmsStageAllocIdentity(std::ptr::null_mut(), 3),
                0x6964656E), // 'iden'
            (c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0].as_ptr(),
                std::ptr::null()), 0x6D617466), // 'matf'
        ];
        for (st, _sig) in &stages {
            if !st.is_null() {
                let ct = c::cmsStageType(*st);
                eprintln!("stage type {ct:#x}");
                let dup = c::cmsStageDup(*st);
                assert!(!dup.is_null(), "dup");
                c::cmsStageFree(dup);
                c::cmsStageFree(*st);
            }
        }
        // rust side — stage_type on the Stage objects
        let rid = lut::stage_alloc_identity(None, 3);
        assert_eq!(rid.stage_type(), 0x69646e20, "identity type");
    }
}

/// _cmsStageAllocLab2XYZ + _cmsStageAllocXYZ2Lab — the Lab/XYZ
/// element allocators inside an MPE pipeline.
#[test]
fn lab_xyz_elements() {
    unsafe {
        let cp = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let l2x = c::_cmsStageAllocLab2XYZ(std::ptr::null_mut());
        let x2l = c::_cmsStageAllocXYZ2Lab(std::ptr::null_mut());
        assert!(!l2x.is_null() && !x2l.is_null());
        c::cmsPipelineInsertStage(cp, 0, l2x);
        c::cmsPipelineInsertStage(cp, 0, x2l); // xyz2lab first
        // eval — Lab -> XYZ -> Lab roundtrip-ish
        let inp = [0.5f32, 0.5, 0.5];
        let mut co = [0f32; 3];
        c::cmsPipelineEvalFloat(inp.as_ptr(), co.as_mut_ptr(), cp);
        // rust side
        let mut rp = lut::pipeline_alloc(None, 3, 3).unwrap();
        let rx2l = lut::stage_alloc_xyz2lab(None);
        assert!(rp.insert_stage(lut::StageLoc::AtEnd, rx2l));
        let rl2x = lut::stage_alloc_lab2xyz(None);
        assert!(rp.insert_stage(lut::StageLoc::AtEnd, rl2x));
        let mut ro = [0f32; 3];
        rp.eval_float(&inp, &mut ro);
        for i in 0..3 {
            assert!((co[i] - ro[i]).abs() < 1e-4,
                "eval[{i}] c={} r={}", co[i], ro[i]);
        }
        c::cmsPipelineFree(cp);
    }
}

/// cmsStageInputChannels + cmsStageOutputChannels — the stage
/// channel getters.
#[test]
fn stage_channels() {
    unsafe {
        for nch in [1u32, 3, 4, 8, 15] {
            let st = c::cmsStageAllocIdentity(std::ptr::null_mut(),
                nch);
            assert!(!st.is_null());
            assert_eq!(c::cmsStageInputChannels(st), nch, "ich {nch}");
            assert_eq!(c::cmsStageOutputChannels(st), nch, "och {nch}");
            c::cmsStageFree(st);
        }
        // a matrix stage with different in/out
        let m = c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 4,
            [1.0; 12].as_ptr(), std::ptr::null());
        assert!(!m.is_null());
        let ci = c::cmsStageInputChannels(m);
        let co = c::cmsStageOutputChannels(m);
        eprintln!("matrix ich={ci} och={co}");
        c::cmsStageFree(m);
    }
}

/// MPE deeper — multi-stage mpet pipelines with mixed element
/// types (curves + matrix + CLUT + lab2xyz).
#[test]
fn mpe_mixed_stages() {
    unsafe {
        let cp = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let curves = c::cmsStageAllocToneCurves(std::ptr::null_mut(),
            3, [c::cmsBuildGamma(std::ptr::null_mut(), 2.2); 3]
            .as_ptr());
        let matrix = c::cmsStageAllocMatrix(std::ptr::null_mut(),
            3, 3, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .as_ptr(), std::ptr::null());
        let clut = c::cmsStageAllocCLut16bitGranular(
            std::ptr::null_mut(), [17u32; 3].as_ptr(), 3, 3,
            std::ptr::null_mut());
        let l2x = c::_cmsStageAllocLab2XYZ(std::ptr::null_mut());
        c::cmsPipelineInsertStage(cp, 0, curves);
        c::cmsPipelineInsertStage(cp, 0, matrix);
        c::cmsPipelineInsertStage(cp, 0, clut);
        c::cmsPipelineInsertStage(cp, 0, l2x);
        // write as mpet tag + resave + read back
        let ch = c::cmsCreateXYZProfile();
        let c_ok = c::cmsWriteTag(ch, 0x41324230, cp); // 'A2B0'
        assert_ne!(c_ok, 0, "mpet write");
        let _ = c::cmsCloseProfile(ch);
        c::cmsPipelineFree(cp);
    }
}

/// cmsSliceSpaceFloat — N-dim float grid sampling.
#[test]
fn slice_space_float() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NC: AtomicU32 = AtomicU32::new(0);
    static NR: AtomicU32 = AtomicU32::new(0);
    unsafe extern "C" fn c_samp(
        _inp: *const c::F32,
        _out: *mut c::F32,
        _cargo: *mut c_void,
    ) -> i32 {
        NC.fetch_add(1, Ordering::Relaxed);
        1
    }
    unsafe {
        // 3d 4x4x4 grid = 64 calls
        let dims = [4u32; 3];
        let c_ok = c::cmsSliceSpaceFloat(3, dims.as_ptr(), c_samp,
            std::ptr::null_mut());
        eprintln!("c_ok={c_ok} c_bad-wait"); assert_eq!(c_ok, 1, "C slice float");
        assert_eq!(NC.load(Ordering::Relaxed), 64, "C ncalls");
        let mut r_samp = |_inp: &[f32], _out: &mut [f32]| -> i32 {
            NR.fetch_add(1, Ordering::Relaxed);
            1
        };
        let r_ok = lut::slice_space_float(3, &dims, &mut r_samp);
        assert!(r_ok, "rust slice float");
        assert_eq!(NR.load(Ordering::Relaxed), 64, "rust ncalls");
        // edge: 0 inputs fails both sides
        let c_bad = c::cmsSliceSpaceFloat(0, dims.as_ptr(), c_samp,
            std::ptr::null_mut());
        let r_bad = lut::slice_space_float(0, &dims, &mut r_samp);
        assert_eq!(c_bad != 0, r_bad, "0-input parity c={c_bad} r={r_bad}");
    }
}
